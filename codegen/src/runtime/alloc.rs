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

/// Deallocate a block previously allocated by aster_alloc.
pub(super) unsafe fn aster_dealloc(ptr: *mut u8, size: usize) {
    if size == 0 || ptr == std::ptr::NonNull::dangling().as_ptr() {
        return;
    }
    let layout = std::alloc::Layout::from_size_align(size, 8).unwrap_or_else(|_| {
        eprintln!("aster_dealloc: invalid layout (size={size})");
        std::process::abort();
    });
    unsafe {
        std::alloc::dealloc(ptr, layout);
    }
}

/// Allocate a class instance. Size is in bytes.
/// Conservative fallback: marks all fields as potential pointers.
/// Used by enum constructors and any code that doesn't supply a ptr_count.
pub extern "C" fn aster_class_alloc(size: usize) -> *mut u8 {
    use super::gc::{OBJ_CLASS, gc_alloc_inner, payload_header};
    let ptr = gc_alloc_inner(size, OBJ_CLASS);
    // Conservative: treat every slot as a potential pointer.
    let header = payload_header(ptr);
    unsafe {
        *header.add(6) = (size / 8) as u8;
    }
    ptr
}

/// Allocate a class instance with a precise pointer-field count.
/// `ptr_count` is the number of leading fields that are GC-traceable pointers.
/// The GC will only trace the first `ptr_count` slots, skipping value fields.
pub extern "C" fn aster_class_alloc_typed(size: usize, ptr_count: i64) -> *mut u8 {
    use super::gc::{OBJ_CLASS, gc_alloc_inner, payload_header};
    let ptr = gc_alloc_inner(size, OBJ_CLASS);
    let header = payload_header(ptr);
    unsafe {
        *header.add(6) = ptr_count as u8;
    }
    ptr
}

/// Allocate a closure object. Size is in bytes.
/// Stamps the header with OBJ_CLOSURE so the GC only traces
/// the env pointer (slot 1), not the function pointer (slot 0).
pub extern "C" fn aster_closure_alloc(size: usize) -> *mut u8 {
    use super::gc::{OBJ_CLOSURE, gc_alloc_inner};
    gc_alloc_inner(size, OBJ_CLOSURE)
}
