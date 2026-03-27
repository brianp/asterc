use std::cell::Cell;

use super::alloc::{aster_alloc, aster_dealloc};

// ---------------------------------------------------------------------------
// Garbage collector — non-moving mark-and-sweep with shadow stack
//
// Object header: [mark: u8, obj_type: u8, pad: u16, size: u32, next: *mut u8]
// Total: 16 bytes, prepended to every GC-tracked allocation.
//
// Shadow stack: linked list of GcFrame { prev, count, roots[] }.
// Each function pushes a frame on entry and pops on exit. Root slots
// are updated when GC-managed locals are assigned.
// ---------------------------------------------------------------------------

/// Object types for tracing.
pub(super) const OBJ_OPAQUE: u8 = 0; // strings, ints — no child pointers
pub(super) const OBJ_LIST_HANDLE: u8 = 1; // handle → data block with i64 elements (may be ptrs)
pub(super) const OBJ_MAP_HANDLE: u8 = 2; // handle → data block with kv entries
pub(super) const OBJ_CLASS: u8 = 3; // all fields are i64 slots (may be ptrs)
pub(super) const OBJ_CLOSURE: u8 = 4; // [func_ptr, env_ptr] — env_ptr is traceable
pub(super) const OBJ_DATA_BLOCK: u8 = 5; // raw data block owned by a handle (not independently traced)
pub(super) const OBJ_TASK: u8 = 6; // [state, consumed, payload] — payload may be a GC pointer
pub(super) const OBJ_LIST_HANDLE_NOPTR: u8 = 7; // handle → data block with value-type elements (no GC trace)

/// Magic bytes in header slots [2..5] to identify valid GC objects.
/// 4 bytes gives a 1-in-2^32 false positive rate for conservative pointer detection.
pub(super) const GC_MAGIC: [u8; 4] = [0xA5, 0x7E, 0xC3, 0x91];

pub(super) const HEADER_SIZE: usize = 24;
const GC_THRESHOLD: usize = 256 * 1024; // 256 KB before first collection
const MAX_GC_ROOTS: i64 = 1024; // defensive upper bound on root count per frame

// Task states (referenced by gc_mark for OBJ_TASK tracing)
pub(super) const TASK_READY: i64 = 1;
pub(super) const TASK_FAILED: i64 = 2;

thread_local! {
    /// Linked list of all GC-tracked objects (via header.next).
    static HEAP_HEAD: Cell<*mut u8> = const { Cell::new(std::ptr::null_mut()) };
    /// Total bytes allocated since last collection.
    static BYTES_ALLOCATED: Cell<usize> = const { Cell::new(0) };
    /// Threshold for next collection (doubles after each GC).
    static GC_NEXT_THRESHOLD: Cell<usize> = const { Cell::new(GC_THRESHOLD) };
    /// Shadow stack head — linked list of GcFrame pointers.
    static SHADOW_STACK: Cell<*mut u8> = const { Cell::new(std::ptr::null_mut()) };
    /// Guard against reentrant collection.
    static GC_COLLECTING: Cell<bool> = const { Cell::new(false) };
    /// Lowest GC heap payload address ever returned.
    static GC_HEAP_LO: Cell<usize> = const { Cell::new(usize::MAX) };
    /// Highest GC heap payload address ever returned (exclusive: addr + size).
    static GC_HEAP_HI: Cell<usize> = const { Cell::new(0) };
}

/// Read the mark byte from an object header.
#[inline]
unsafe fn obj_mark(header: *const u8) -> u8 {
    unsafe { *header }
}

/// Set the mark byte on an object header.
#[inline]
unsafe fn obj_set_mark(header: *mut u8, mark: u8) {
    unsafe {
        *header = mark;
    }
}

/// Read the object type from a header.
#[inline]
unsafe fn obj_type(header: *const u8) -> u8 {
    unsafe { *header.add(1) }
}

/// Read the payload size from a header.
#[inline]
unsafe fn obj_size(header: *const u8) -> u32 {
    unsafe { *(header.add(8) as *const u32) }
}

/// Get pointer field count from header (stored at offset 6).
/// Only meaningful for OBJ_CLASS objects. Set by aster_class_alloc_typed.
#[inline]
pub(super) unsafe fn obj_ptr_count(header: *const u8) -> u8 {
    unsafe { *header.add(6) }
}

/// Read the next pointer from a header.
#[inline]
unsafe fn obj_next(header: *const u8) -> *mut u8 {
    unsafe { *(header.add(16) as *const *mut u8) }
}

/// Set the next pointer on a header.
#[inline]
unsafe fn obj_set_next(header: *mut u8, next: *mut u8) {
    unsafe {
        *(header.add(16) as *mut *mut u8) = next;
    }
}

/// Get the payload pointer from a header pointer.
#[inline]
pub(super) fn obj_payload(header: *mut u8) -> *mut u8 {
    unsafe { header.add(HEADER_SIZE) }
}

/// Get the header pointer from a payload pointer.
#[inline]
pub(super) fn payload_header(payload: *const u8) -> *mut u8 {
    unsafe { (payload as *mut u8).sub(HEADER_SIZE) }
}

/// Check if a raw i64 value looks like a valid GC payload pointer by
/// verifying the magic bytes in the header. This enables conservative
/// tracing of untyped slots (list elements, class fields).
#[inline]
unsafe fn is_gc_payload(val: i64) -> bool {
    // Reject zero, negative values, and misaligned values.
    if val <= 0 || !(val as u64).is_multiple_of(8) {
        return false;
    }
    let addr = val as usize;
    // Only consider values that fall within the known GC heap address range.
    // This prevents reading from arbitrary addresses (e.g. small integers that
    // happen to be aligned) which would cause segfaults.
    let in_range = GC_HEAP_LO.with(|lo| GC_HEAP_HI.with(|hi| addr >= lo.get() && addr < hi.get()));
    if !in_range {
        return false;
    }
    let payload = val as *const u8;
    let header = unsafe { payload.sub(HEADER_SIZE) };
    // Check magic bytes at offset 2..5
    unsafe {
        *header.add(2) == GC_MAGIC[0]
            && *header.add(3) == GC_MAGIC[1]
            && *header.add(4) == GC_MAGIC[2]
            && *header.add(5) == GC_MAGIC[3]
    }
}

/// Allocate a GC-tracked object. Returns a pointer to the payload (after the header).
pub(super) fn gc_alloc_inner(payload_size: usize, obj_ty: u8) -> *mut u8 {
    // Check if GC is needed
    BYTES_ALLOCATED.with(|b| {
        let total = b.get() + payload_size + HEADER_SIZE;
        b.set(total);
        GC_NEXT_THRESHOLD.with(|thresh| {
            if total >= thresh.get() {
                gc_collect_inner();
                b.set(0);
            }
        });
    });

    assert!(
        payload_size <= u32::MAX as usize,
        "gc_alloc: payload_size {payload_size} exceeds u32::MAX"
    );

    let total_size = payload_size + HEADER_SIZE;
    let ptr = aster_alloc(total_size);

    unsafe {
        // Write header: [mark: u8][type: u8][magic: u8; 4][pad: 2][size: u32][pad: 4][next: *mut u8]
        *ptr = 0; // mark = 0 (white)
        *ptr.add(1) = obj_ty; // object type
        *ptr.add(2) = GC_MAGIC[0];
        *ptr.add(3) = GC_MAGIC[1];
        *ptr.add(4) = GC_MAGIC[2];
        *ptr.add(5) = GC_MAGIC[3];
        *(ptr.add(8) as *mut u32) = payload_size as u32;
    }

    // Link into heap list
    HEAP_HEAD.with(|head| {
        let old_head = head.get();
        unsafe {
            obj_set_next(ptr, old_head);
        }
        head.set(ptr);
    });

    let payload = obj_payload(ptr);

    // Track heap address range for conservative pointer validation
    let addr = payload as usize;
    GC_HEAP_LO.with(|lo| {
        if addr < lo.get() {
            lo.set(addr);
        }
    });
    GC_HEAP_HI.with(|hi| {
        let end = addr + payload_size;
        if end > hi.get() {
            hi.set(end);
        }
    });

    payload
}

/// Mark reachable objects using an iterative worklist instead of recursion.
/// This avoids stack overflow on deeply nested object graphs (e.g. long linked lists).
unsafe fn gc_mark(root: *const u8) {
    if root.is_null() {
        return;
    }
    let mut worklist: Vec<*const u8> = vec![root];

    while let Some(payload) = worklist.pop() {
        let header = payload_header(payload);
        unsafe {
            if obj_mark(header) != 0 {
                continue; // already marked
            }
            obj_set_mark(header, 1);

            match obj_type(header) {
                OBJ_OPAQUE | OBJ_DATA_BLOCK => {}
                OBJ_LIST_HANDLE => {
                    let block = *(payload as *const *const u8);
                    if !block.is_null() {
                        let block_header = payload_header(block);
                        if obj_mark(block_header) == 0 {
                            obj_set_mark(block_header, 1);
                        }
                        let len = *(block as *const i64) as usize;
                        let elements = (block as *const i64).add(2);
                        for i in 0..len {
                            let val = *elements.add(i);
                            if is_gc_payload(val) {
                                worklist.push(val as *const u8);
                            }
                        }
                    }
                }
                OBJ_LIST_HANDLE_NOPTR => {
                    let block = *(payload as *const *const u8);
                    if !block.is_null() {
                        let block_header = payload_header(block);
                        if obj_mark(block_header) == 0 {
                            obj_set_mark(block_header, 1);
                        }
                    }
                }
                OBJ_MAP_HANDLE => {
                    let block = *(payload as *const *const u8);
                    if !block.is_null() {
                        let block_header = payload_header(block);
                        if obj_mark(block_header) == 0 {
                            obj_set_mark(block_header, 1);
                        }
                        let len = *(block as *const i64) as usize;
                        let entries = (block as *const i64).add(2);
                        for i in 0..len {
                            let key = *entries.add(i * 2);
                            let val = *entries.add(i * 2 + 1);
                            if is_gc_payload(key) {
                                worklist.push(key as *const u8);
                            }
                            if is_gc_payload(val) {
                                worklist.push(val as *const u8);
                            }
                        }
                    }
                }
                OBJ_CLASS => {
                    // Precise tracing: only trace the first ptr_count slots.
                    // Pointer fields are sorted to the front of the object layout
                    // by the FIR lowerer; ptr_count is stored at header offset 6
                    // by aster_class_alloc_typed (or conservatively by aster_class_alloc).
                    let ptr_count = obj_ptr_count(header) as usize;
                    let fields = payload as *const i64;
                    for i in 0..ptr_count {
                        let val = *fields.add(i);
                        if is_gc_payload(val) {
                            worklist.push(val as *const u8);
                        }
                    }
                }
                OBJ_CLOSURE => {
                    let env_ptr = *((payload as *const i64).add(1)) as *const u8;
                    if !env_ptr.is_null() {
                        worklist.push(env_ptr);
                    }
                }
                OBJ_TASK => {
                    let task_state = *(payload as *const i64);
                    if task_state == TASK_READY || task_state == TASK_FAILED {
                        let val = *((payload as *const i64).add(2));
                        if is_gc_payload(val) {
                            worklist.push(val as *const u8);
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

/// Run mark-and-sweep collection.
fn gc_collect_inner() {
    // Guard against reentrant collection (e.g., finalizer triggering alloc)
    let already_collecting = GC_COLLECTING.with(|g| {
        if g.get() {
            return true;
        }
        g.set(true);
        false
    });
    if already_collecting {
        return;
    }

    // Mark phase: trace from shadow stack roots
    SHADOW_STACK.with(|ss| {
        let mut frame = ss.get();
        while !frame.is_null() {
            unsafe {
                // GcFrame layout: [prev: *mut u8][count: i64][roots: [i64; count]]
                let count = *((frame as *const i64).add(1)) as usize;
                let roots = (frame as *const i64).add(2);
                for i in 0..count {
                    let root = *roots.add(i);
                    if root != 0 {
                        gc_mark(root as *const u8);
                    }
                }
                frame = *(frame as *const *mut u8); // prev
            }
        }
    });

    // Sweep phase: free unmarked objects, reset marks on survivors
    let mut survived_bytes: usize = 0;
    HEAP_HEAD.with(|head| {
        let mut prev: *mut u8 = std::ptr::null_mut();
        let mut current = head.get();

        while !current.is_null() {
            unsafe {
                let next = obj_next(current);
                let total = HEADER_SIZE + obj_size(current) as usize;
                if obj_mark(current) == 0 {
                    // Unmarked — free it
                    if !prev.is_null() {
                        obj_set_next(prev, next);
                    } else {
                        head.set(next);
                    }
                    aster_dealloc(current, total);
                } else {
                    // Marked — reset mark for next cycle
                    obj_set_mark(current, 0);
                    survived_bytes += total;
                    prev = current;
                }
                current = next;
            }
        }
    });

    // Set next threshold proportional to live set (2x survived, minimum GC_THRESHOLD)
    GC_NEXT_THRESHOLD.with(|thresh| {
        thresh.set((survived_bytes * 2).max(GC_THRESHOLD));
    });

    GC_COLLECTING.with(|g| g.set(false));
}

/// Push a shadow stack frame. Layout: [prev: *mut u8][count: i64][roots: [i64; count]]
/// The frame must live on the caller's stack (passed as a pointer).
pub extern "C" fn aster_gc_push_roots(frame: *mut u8, count: i64) {
    if frame.is_null() || !(0..=MAX_GC_ROOTS).contains(&count) {
        return;
    }
    SHADOW_STACK.with(|ss| {
        let old_top = ss.get();
        unsafe {
            *(frame as *mut *mut u8) = old_top; // prev = old top
            *((frame as *mut i64).add(1)) = count; // count
            // Zero the root slots
            let roots = (frame as *mut i64).add(2);
            for i in 0..count as usize {
                *roots.add(i) = 0;
            }
        }
        ss.set(frame);
    });
}

/// Pop the top shadow stack frame.
pub extern "C" fn aster_gc_pop_roots() {
    SHADOW_STACK.with(|ss| {
        let top = ss.get();
        if !top.is_null() {
            let prev = unsafe { *(top as *const *mut u8) };
            ss.set(prev);
        }
    });
}

/// Force a garbage collection cycle.
pub extern "C" fn aster_gc_collect() {
    gc_collect_inner();
}

// ---------------------------------------------------------------------------
// Shadow stack accessors — used by green thread scheduler to save/restore
// ---------------------------------------------------------------------------

pub(crate) fn shadow_stack_get() -> *mut u8 {
    SHADOW_STACK.with(|ss| ss.get())
}

pub(crate) fn shadow_stack_set(ptr: *mut u8) {
    SHADOW_STACK.with(|ss| ss.set(ptr));
}

/// Allocate a GC-tracked string.
pub(super) fn gc_alloc_string(data: *const u8, len: usize) -> *mut u8 {
    let total_payload = 8 + len; // [len: i64][data: u8...]
    let ptr = gc_alloc_inner(total_payload, OBJ_OPAQUE);
    unsafe {
        *(ptr as *mut i64) = len as i64;
        if len > 0 && !data.is_null() {
            std::ptr::copy_nonoverlapping(data, ptr.add(8), len);
        }
    }
    ptr
}

/// Allocate a GC-tracked data block (used by list/map handles internally).
pub(super) fn gc_alloc_data_block(size: usize) -> *mut u8 {
    gc_alloc_inner(size, OBJ_DATA_BLOCK)
}
