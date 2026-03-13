use cranelift_jit::JITBuilder;

// ---------------------------------------------------------------------------
// Runtime builtins — called from JIT-compiled code via symbol registration
// ---------------------------------------------------------------------------

// Heap-allocated string: pointer to { len: i64, data: [u8] }
// We represent strings as a pair (ptr, len) packed into a struct on the heap.
// For now, the "Ptr" in FIR is a raw pointer to the data, with length stored
// at offset -8 (just before the data pointer).

/// Allocate `size` bytes on the heap, 8-byte aligned.
/// Aborts on zero-size allocations or OOM.
pub extern "C" fn aster_alloc(size: usize) -> *mut u8 {
    if size == 0 {
        // Zero-size alloc is UB per the global allocator contract.
        // Return a dangling but aligned pointer (safe as long as nothing is read/written).
        return std::ptr::NonNull::dangling().as_ptr();
    }
    let layout = match std::alloc::Layout::from_size_align(size, 8) {
        Ok(l) => l,
        Err(_) => {
            eprintln!("aster_alloc: invalid layout (size too large)");
            std::process::abort();
        }
    };
    let ptr = unsafe { std::alloc::alloc(layout) };
    if ptr.is_null() {
        std::alloc::handle_alloc_error(layout);
    }
    ptr
}

/// Print a string (ptr to heap string object).
/// String layout: [len: i64][data: u8...]
pub extern "C" fn aster_print_str(ptr: *const u8) {
    if ptr.is_null() {
        println!("nil");
        return;
    }
    unsafe {
        let raw_len = *(ptr as *const i64);
        if raw_len < 0 {
            println!("<invalid string: negative length>");
            return;
        }
        let len = raw_len as usize;
        let data = ptr.add(8);
        let bytes = std::slice::from_raw_parts(data, len);
        match std::str::from_utf8(bytes) {
            Ok(s) => println!("{}", s),
            Err(_) => println!("{}", String::from_utf8_lossy(bytes)),
        }
    }
}

/// Print an integer.
pub extern "C" fn aster_print_int(val: i64) {
    println!("{}", val);
}

/// Print a float.
pub extern "C" fn aster_print_float(val: f64) {
    println!("{}", val);
}

/// Print a bool.
pub extern "C" fn aster_print_bool(val: i8) {
    println!("{}", if val != 0 { "true" } else { "false" });
}

/// Create a new heap-allocated string from a pointer and length.
/// Returns a pointer to the string object [len: i64][data: u8...].
pub extern "C" fn aster_string_new(data: *const u8, len: usize) -> *mut u8 {
    if len > 0 && data.is_null() {
        eprintln!("aster_string_new: null data pointer with nonzero length");
        std::process::abort();
    }
    gc_alloc_string(data, len)
}

/// Concatenate two heap strings. Returns a new heap string.
pub extern "C" fn aster_string_concat(a: *const u8, b: *const u8) -> *mut u8 {
    unsafe {
        let a_len = if a.is_null() {
            0usize
        } else {
            let raw = *(a as *const i64);
            if raw < 0 { 0usize } else { raw as usize }
        };
        let b_len = if b.is_null() {
            0usize
        } else {
            let raw = *(b as *const i64);
            if raw < 0 { 0usize } else { raw as usize }
        };
        let concat_len = a_len.saturating_add(b_len);
        let result = gc_alloc_string(std::ptr::null(), concat_len);
        // Manually fill data (gc_alloc_string zero-inits since data is null)
        if a_len > 0 {
            std::ptr::copy_nonoverlapping(a.add(8), result.add(8), a_len);
        }
        if b_len > 0 {
            std::ptr::copy_nonoverlapping(b.add(8), result.add(8 + a_len), b_len);
        }
        result
    }
}

/// Get the length of a heap string.
pub extern "C" fn aster_string_len(ptr: *const u8) -> i64 {
    if ptr.is_null() {
        return 0;
    }
    let raw = unsafe { *(ptr as *const i64) };
    if raw < 0 { 0 } else { raw }
}

// ---------------------------------------------------------------------------
// List operations — handle-based indirection for alias safety
//
// A list value is a *handle*: a pointer to an 8-byte cell that holds the
// actual data block pointer. Data block layout: [len: i64][cap: i64][data...]
// All aliases share the same handle, so reallocation updates the handle
// target and every alias sees it.
// ---------------------------------------------------------------------------

/// Deallocate a block previously allocated by aster_alloc.
unsafe fn aster_dealloc(ptr: *mut u8, size: usize) {
    if size == 0 || ptr == std::ptr::NonNull::dangling().as_ptr() {
        return;
    }
    unsafe {
        let layout = std::alloc::Layout::from_size_align_unchecked(size, 8);
        std::alloc::dealloc(ptr, layout);
    }
}

/// Dereference a handle to get the data block pointer.
#[inline]
unsafe fn list_block(handle: *const u8) -> *mut u8 {
    unsafe { *(handle as *const *mut u8) }
}

/// Allocate a new list. Returns a handle (pointer-to-pointer).
pub extern "C" fn aster_list_new(cap: i64) -> *mut u8 {
    let cap = cap.max(4) as usize;
    let alloc_size = match cap.checked_mul(8).and_then(|n| n.checked_add(16)) {
        Some(n) => n,
        None => {
            eprintln!("aster_list_new: size overflow");
            std::process::abort();
        }
    };
    let block = gc_alloc_data_block(alloc_size);
    unsafe {
        *(block as *mut i64) = 0; // len = 0
        *((block as *mut i64).add(1)) = cap as i64; // cap
    }
    let handle = gc_alloc_inner(8, OBJ_LIST_HANDLE);
    unsafe {
        *(handle as *mut *mut u8) = block;
    }
    handle
}

/// Get an element from a list by index. Returns the i64 value at that index.
pub extern "C" fn aster_list_get(handle: *const u8, index: i64) -> i64 {
    if handle.is_null() {
        eprintln!("aster_list_get: null list handle");
        std::process::abort();
    }
    unsafe {
        let block = list_block(handle);
        let len = *(block as *const i64);
        if index < 0 || index >= len {
            eprintln!("list index out of bounds: {} (len {})", index, len);
            std::process::abort();
        }
        let data = (block as *const i64).add(2);
        *data.add(index as usize)
    }
}

/// Set an element in a list by index.
pub extern "C" fn aster_list_set(handle: *mut u8, index: i64, value: i64) {
    if handle.is_null() {
        eprintln!("aster_list_set: null list handle");
        std::process::abort();
    }
    unsafe {
        let block = list_block(handle);
        let len = *(block as *const i64);
        if index < 0 || index >= len {
            eprintln!("list index out of bounds: {} (len {})", index, len);
            std::process::abort();
        }
        let data = (block as *mut i64).add(2);
        *data.add(index as usize) = value;
    }
}

/// Push an element to a list. Handle stays stable; data block may move.
/// Returns the same handle for backward compatibility with codegen.
pub extern "C" fn aster_list_push(handle: *mut u8, value: i64) -> *mut u8 {
    if handle.is_null() {
        eprintln!("aster_list_push: null list handle");
        std::process::abort();
    }
    unsafe {
        let block = list_block(handle);
        let len = *(block as *mut i64);
        let cap = *((block as *mut i64).add(1));
        if len >= cap {
            // Grow: double capacity
            let new_cap = (cap * 2).max(4) as usize;
            let alloc_size = match new_cap.checked_mul(8).and_then(|n| n.checked_add(16)) {
                Some(n) => n,
                None => {
                    eprintln!("aster_list_push: size overflow");
                    std::process::abort();
                }
            };
            let new_block = gc_alloc_data_block(alloc_size);
            std::ptr::copy_nonoverlapping(block, new_block, 16 + (len as usize) * 8);
            *((new_block as *mut i64).add(1)) = new_cap as i64;
            // Old block will be swept by GC (no longer referenced)
            *(handle as *mut *mut u8) = new_block;
            let data = (new_block as *mut i64).add(2);
            *data.add(len as usize) = value;
            *(new_block as *mut i64) = len + 1;
        } else {
            let data = (block as *mut i64).add(2);
            *data.add(len as usize) = value;
            *(block as *mut i64) = len + 1;
        }
    }
    handle
}

/// Get the length of a list.
pub extern "C" fn aster_list_len(handle: *const u8) -> i64 {
    if handle.is_null() {
        return 0;
    }
    unsafe {
        let block = list_block(handle);
        *(block as *const i64)
    }
}

/// Integer exponentiation: base ** exp (exp >= 0).
pub extern "C" fn aster_pow_int(base: i64, exp: i64) -> i64 {
    if exp < 0 {
        return 0; // integer pow with negative exp → 0 (floor)
    }
    let mut result: i64 = 1;
    let mut b = base;
    let mut e = exp as u64;
    while e > 0 {
        if e & 1 == 1 {
            result = result.wrapping_mul(b);
        }
        b = b.wrapping_mul(b);
        e >>= 1;
    }
    result
}

/// Convert an integer to a heap string.
pub extern "C" fn aster_int_to_string(val: i64) -> *mut u8 {
    let s = val.to_string();
    aster_string_new(s.as_ptr(), s.len())
}

/// Convert a float to a heap string.
pub extern "C" fn aster_float_to_string(val: f64) -> *mut u8 {
    let s = val.to_string();
    aster_string_new(s.as_ptr(), s.len())
}

/// Convert a bool to a heap string.
pub extern "C" fn aster_bool_to_string(val: i8) -> *mut u8 {
    let s = if val != 0 { "true" } else { "false" };
    aster_string_new(s.as_ptr(), s.len())
}

/// Allocate a class instance. Size is in bytes.
pub extern "C" fn aster_class_alloc(size: usize) -> *mut u8 {
    gc_alloc_inner(size, OBJ_CLASS)
}

// ---------------------------------------------------------------------------
// Map operations — handle-based indirection, linear-scan associative array
// Data block layout: [len: i64][cap: i64][entries: [key_ptr: i64, value: i64]...]
// Each entry is 16 bytes. Keys are heap string pointers, compared by content.
// A map value is a handle (pointer-to-pointer), same as lists.
// ---------------------------------------------------------------------------

/// Dereference a map handle to get the data block pointer.
#[inline]
unsafe fn map_block(handle: *const u8) -> *mut u8 {
    unsafe { *(handle as *const *mut u8) }
}

/// Create a new map with the given initial capacity. Returns a handle.
pub extern "C" fn aster_map_new(cap: i64) -> *mut u8 {
    let cap = cap.max(4) as usize;
    let alloc_size = match cap.checked_mul(16).and_then(|n| n.checked_add(16)) {
        Some(n) => n,
        None => {
            eprintln!("aster_map_new: size overflow");
            std::process::abort();
        }
    };
    let block = gc_alloc_data_block(alloc_size);
    unsafe {
        *(block as *mut i64) = 0; // len = 0
        *((block as *mut i64).add(1)) = cap as i64; // cap
    }
    let handle = gc_alloc_inner(8, OBJ_MAP_HANDLE);
    unsafe {
        *(handle as *mut *mut u8) = block;
    }
    handle
}

/// Compare two heap strings by content. Returns true if equal.
unsafe fn string_eq(a: *const u8, b: *const u8) -> bool {
    if a == b {
        return true;
    }
    if a.is_null() || b.is_null() {
        return false;
    }
    unsafe {
        let a_raw = *(a as *const i64);
        let b_raw = *(b as *const i64);
        if a_raw < 0 || b_raw < 0 {
            return false;
        }
        let a_len = a_raw as usize;
        let b_len = b_raw as usize;
        if a_len != b_len {
            return false;
        }
        let a_data = a.add(8);
        let b_data = b.add(8);
        std::slice::from_raw_parts(a_data, a_len) == std::slice::from_raw_parts(b_data, b_len)
    }
}

/// Set a key-value pair in the map. Overwrites if key exists, appends otherwise.
/// Handle stays stable; data block may move. Returns the same handle.
pub extern "C" fn aster_map_set(handle: *mut u8, key: i64, value: i64) -> *mut u8 {
    if handle.is_null() {
        eprintln!("aster_map_set: null map handle");
        std::process::abort();
    }
    unsafe {
        let block = map_block(handle);
        let len = *(block as *const i64) as usize;
        let entries = block.add(16) as *mut i64;

        // Linear scan for existing key
        for i in 0..len {
            let entry_key = *entries.add(i * 2);
            if string_eq(entry_key as *const u8, key as *const u8) {
                *entries.add(i * 2 + 1) = value;
                return handle;
            }
        }

        // Key not found — append
        let cap = *((block as *const i64).add(1)) as usize;
        if len >= cap {
            // Grow: double capacity
            let new_cap = (cap * 2).max(4);
            let alloc_size = match new_cap.checked_mul(16).and_then(|n| n.checked_add(16)) {
                Some(n) => n,
                None => {
                    eprintln!("aster_map_set: size overflow");
                    std::process::abort();
                }
            };
            let new_block = gc_alloc_data_block(alloc_size);
            std::ptr::copy_nonoverlapping(block, new_block, 16 + len * 16);
            *((new_block as *mut i64).add(1)) = new_cap as i64;
            // Old block will be swept by GC
            *(handle as *mut *mut u8) = new_block;
            let new_entries = new_block.add(16) as *mut i64;
            *new_entries.add(len * 2) = key;
            *new_entries.add(len * 2 + 1) = value;
            *(new_block as *mut i64) = (len + 1) as i64;
        } else {
            *entries.add(len * 2) = key;
            *entries.add(len * 2 + 1) = value;
            *(block as *mut i64) = (len + 1) as i64;
        }
    }
    handle
}

/// Get a value from the map by key. Returns the value or 0 if not found.
pub extern "C" fn aster_map_get(handle: *const u8, key: i64) -> i64 {
    if handle.is_null() {
        eprintln!("aster_map_get: null map handle");
        std::process::abort();
    }
    unsafe {
        let block = map_block(handle);
        let len = *(block as *const i64) as usize;
        let entries = block.add(16) as *const i64;
        for i in 0..len {
            let entry_key = *entries.add(i * 2);
            if string_eq(entry_key as *const u8, key as *const u8) {
                return *entries.add(i * 2 + 1);
            }
        }
        0 // key not found
    }
}

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

use std::cell::Cell;

/// Object types for tracing.
const OBJ_OPAQUE: u8 = 0; // strings, ints — no child pointers
const OBJ_LIST_HANDLE: u8 = 1; // handle → data block with i64 elements (may be ptrs)
const OBJ_MAP_HANDLE: u8 = 2; // handle → data block with kv entries
const OBJ_CLASS: u8 = 3; // all fields are i64 slots (may be ptrs)
const OBJ_CLOSURE: u8 = 4; // [func_ptr, env_ptr] — env_ptr is traceable
const OBJ_DATA_BLOCK: u8 = 5; // raw data block owned by a handle (not independently traced)

/// Magic bytes in header slots [2..3] to identify valid GC objects.
const GC_MAGIC: [u8; 2] = [0xA5, 0x7E];

const HEADER_SIZE: usize = 16;
const GC_THRESHOLD: usize = 256 * 1024; // 256 KB before first collection
const MAX_GC_ROOTS: i64 = 1024; // defensive upper bound on root count per frame

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
    unsafe { *(header.add(4) as *const u32) }
}

/// Read the next pointer from a header.
#[inline]
unsafe fn obj_next(header: *const u8) -> *mut u8 {
    unsafe { *(header.add(8) as *const *mut u8) }
}

/// Set the next pointer on a header.
#[inline]
unsafe fn obj_set_next(header: *mut u8, next: *mut u8) {
    unsafe {
        *(header.add(8) as *mut *mut u8) = next;
    }
}

/// Get the payload pointer from a header pointer.
#[inline]
fn obj_payload(header: *mut u8) -> *mut u8 {
    unsafe { header.add(HEADER_SIZE) }
}

/// Get the header pointer from a payload pointer.
#[inline]
fn payload_header(payload: *const u8) -> *mut u8 {
    unsafe { (payload as *mut u8).sub(HEADER_SIZE) }
}

/// Check if a raw i64 value looks like a valid GC payload pointer by
/// verifying the magic bytes in the header. This enables conservative
/// tracing of untyped slots (list elements, class fields).
#[inline]
unsafe fn is_gc_payload(val: i64) -> bool {
    if val == 0 {
        return false;
    }
    let payload = val as *const u8;
    let header = unsafe { payload.sub(HEADER_SIZE) };
    // Check magic bytes at offset 2 and 3
    unsafe { *header.add(2) == GC_MAGIC[0] && *header.add(3) == GC_MAGIC[1] }
}

/// Allocate a GC-tracked object. Returns a pointer to the payload (after the header).
fn gc_alloc_inner(payload_size: usize, obj_ty: u8) -> *mut u8 {
    // Check if GC is needed
    BYTES_ALLOCATED.with(|b| {
        let total = b.get() + payload_size + HEADER_SIZE;
        b.set(total);
        GC_NEXT_THRESHOLD.with(|thresh| {
            if total >= thresh.get() {
                gc_collect_inner();
                // Double the threshold
                thresh.set(thresh.get() * 2);
                b.set(0);
            }
        });
    });

    let total_size = payload_size + HEADER_SIZE;
    let ptr = aster_alloc(total_size);

    unsafe {
        // Write header: [mark: u8][type: u8][magic: u8; 2][size: u32][next: *mut u8]
        *ptr = 0; // mark = 0 (white)
        *ptr.add(1) = obj_ty; // object type
        *ptr.add(2) = GC_MAGIC[0];
        *ptr.add(3) = GC_MAGIC[1];
        *(ptr.add(4) as *mut u32) = payload_size as u32; // size (safe: objects < 4GB)
    }

    // Link into heap list
    HEAP_HEAD.with(|head| {
        let old_head = head.get();
        unsafe {
            obj_set_next(ptr, old_head);
        }
        head.set(ptr);
    });

    obj_payload(ptr)
}

/// Mark a single object (by payload pointer) and recursively trace children.
unsafe fn gc_mark(payload: *const u8) {
    if payload.is_null() {
        return;
    }
    // Validate the pointer looks like a GC object
    let header = payload_header(payload);
    unsafe {
        if obj_mark(header) != 0 {
            return; // already marked
        }
        obj_set_mark(header, 1);

        match obj_type(header) {
            OBJ_OPAQUE | OBJ_DATA_BLOCK => {
                // No children to trace
            }
            OBJ_LIST_HANDLE => {
                // Handle points to a data block. Mark it, then trace elements.
                let block = *(payload as *const *const u8);
                if !block.is_null() {
                    let block_header = payload_header(block);
                    if obj_mark(block_header) == 0 {
                        obj_set_mark(block_header, 1);
                    }
                    // List block layout: [cap: i64][len: i64][elements: i64 * cap]
                    let len = *((block as *const i64).add(1)) as usize;
                    let elements = (block as *const i64).add(2);
                    for i in 0..len {
                        let val = *elements.add(i);
                        if is_gc_payload(val) {
                            gc_mark(val as *const u8);
                        }
                    }
                }
            }
            OBJ_MAP_HANDLE => {
                // Handle points to a data block. Mark it, then trace entries.
                let block = *(payload as *const *const u8);
                if !block.is_null() {
                    let block_header = payload_header(block);
                    if obj_mark(block_header) == 0 {
                        obj_set_mark(block_header, 1);
                    }
                    // Map block layout: [cap: i64][len: i64][entries: (key: i64, val: i64) * cap]
                    let len = *((block as *const i64).add(1)) as usize;
                    let entries = (block as *const i64).add(2);
                    for i in 0..len {
                        let key = *entries.add(i * 2);
                        let val = *entries.add(i * 2 + 1);
                        if is_gc_payload(key) {
                            gc_mark(key as *const u8);
                        }
                        if is_gc_payload(val) {
                            gc_mark(val as *const u8);
                        }
                    }
                }
            }
            OBJ_CLASS => {
                // All fields are i64 slots — conservatively trace each using magic check.
                let num_fields = obj_size(header) as usize / 8;
                let fields = payload as *const i64;
                for i in 0..num_fields {
                    let val = *fields.add(i);
                    if is_gc_payload(val) {
                        gc_mark(val as *const u8);
                    }
                }
            }
            OBJ_CLOSURE => {
                // Closure: [func_ptr: i64][env_ptr: i64]
                // env_ptr may be a GC-managed environment
                let env_ptr = *((payload as *const i64).add(1)) as *const u8;
                if !env_ptr.is_null() {
                    gc_mark(env_ptr);
                }
            }
            _ => {}
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
    HEAP_HEAD.with(|head| {
        let mut prev: *mut u8 = std::ptr::null_mut();
        let mut current = head.get();

        while !current.is_null() {
            unsafe {
                let next = obj_next(current);
                if obj_mark(current) == 0 {
                    // Unmarked — free it
                    let total = HEADER_SIZE + obj_size(current) as usize;
                    if !prev.is_null() {
                        obj_set_next(prev, next);
                    } else {
                        head.set(next);
                    }
                    aster_dealloc(current, total);
                } else {
                    // Marked — reset mark for next cycle
                    obj_set_mark(current, 0);
                    prev = current;
                }
                current = next;
            }
        }
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

/// Allocate a GC-tracked string.
fn gc_alloc_string(data: *const u8, len: usize) -> *mut u8 {
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
fn gc_alloc_data_block(size: usize) -> *mut u8 {
    gc_alloc_inner(size, OBJ_DATA_BLOCK)
}

// ---------------------------------------------------------------------------
// Error handling — global error flag
// ---------------------------------------------------------------------------

use std::sync::atomic::{AtomicBool, Ordering};

static ERROR_FLAG: AtomicBool = AtomicBool::new(false);

/// Set the global error flag. Called by `throw`.
pub extern "C" fn aster_error_set() {
    ERROR_FLAG.store(true, Ordering::Release);
}

/// Check and clear the global error flag. Returns 1 if error was set.
pub extern "C" fn aster_error_check() -> i8 {
    ERROR_FLAG.swap(false, Ordering::AcqRel) as i8
}

/// Panic / abort for uncaught errors.
pub extern "C" fn aster_panic() {
    eprintln!("aster: uncaught error");
    std::process::abort();
}

/// Register all runtime builtins with a JIT builder.
pub fn register_runtime_builtins(builder: &mut JITBuilder) {
    let symbols: Vec<(&str, *const u8)> = vec![
        ("aster_alloc", aster_alloc as *const u8),
        ("aster_print_str", aster_print_str as *const u8),
        ("aster_print_int", aster_print_int as *const u8),
        ("aster_print_float", aster_print_float as *const u8),
        ("aster_print_bool", aster_print_bool as *const u8),
        ("aster_string_new", aster_string_new as *const u8),
        ("aster_string_concat", aster_string_concat as *const u8),
        ("aster_string_len", aster_string_len as *const u8),
        ("aster_list_new", aster_list_new as *const u8),
        ("aster_list_get", aster_list_get as *const u8),
        ("aster_list_set", aster_list_set as *const u8),
        ("aster_list_push", aster_list_push as *const u8),
        ("aster_list_len", aster_list_len as *const u8),
        ("aster_class_alloc", aster_class_alloc as *const u8),
        ("aster_pow_int", aster_pow_int as *const u8),
        ("aster_int_to_string", aster_int_to_string as *const u8),
        ("aster_float_to_string", aster_float_to_string as *const u8),
        ("aster_bool_to_string", aster_bool_to_string as *const u8),
        ("aster_map_new", aster_map_new as *const u8),
        ("aster_map_set", aster_map_set as *const u8),
        ("aster_map_get", aster_map_get as *const u8),
        ("aster_error_set", aster_error_set as *const u8),
        ("aster_error_check", aster_error_check as *const u8),
        ("aster_panic", aster_panic as *const u8),
        ("aster_gc_push_roots", aster_gc_push_roots as *const u8),
        ("aster_gc_pop_roots", aster_gc_pop_roots as *const u8),
        ("aster_gc_collect", aster_gc_collect as *const u8),
    ];
    builder.symbols(symbols);
}
