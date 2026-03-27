use super::gc::{OBJ_LIST_HANDLE, OBJ_LIST_HANDLE_NOPTR, gc_alloc_data_block, gc_alloc_inner};
use super::numeric::aster_random_int;
use super::string::string_eq;

// ---------------------------------------------------------------------------
// List operations — handle-based indirection for alias safety
//
// A list value is a *handle*: a pointer to an 8-byte cell that holds the
// actual data block pointer. Data block layout: [len: i64][cap: i64][data...]
// All aliases share the same handle, so reallocation updates the handle
// target and every alias sees it.
// ---------------------------------------------------------------------------

/// Dereference a handle to get the data block pointer.
#[inline]
pub(super) unsafe fn list_block(handle: *const u8) -> *mut u8 {
    unsafe { *(handle as *const *mut u8) }
}

/// Allocate a new list. Returns a handle (pointer-to-pointer).
/// `ptr_elems`: 0 = value-type elements (Int/Float/Bool, GC won't scan them),
///              1 = pointer-type elements (String/Class, GC will trace them).
#[unsafe(no_mangle)]
pub extern "C" fn aster_list_new(cap: i64, ptr_elems: i64) -> *mut u8 {
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
    let obj_ty = if ptr_elems != 0 {
        OBJ_LIST_HANDLE
    } else {
        OBJ_LIST_HANDLE_NOPTR
    };
    let handle = gc_alloc_inner(8, obj_ty);
    unsafe {
        *(handle as *mut *mut u8) = block;
    }
    handle
}

/// Get an element from a list by index. Returns the i64 value at that index.
#[unsafe(no_mangle)]
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

/// Pick a random element from a list.
#[unsafe(no_mangle)]
pub extern "C" fn aster_list_random(handle: *const u8) -> i64 {
    if handle.is_null() {
        eprintln!("aster_list_random: null list handle");
        std::process::abort();
    }
    unsafe {
        let block = list_block(handle);
        let len = *(block as *const i64);
        if len <= 0 {
            eprintln!("aster_list_random: empty list");
            std::process::abort();
        }
        let idx = aster_random_int(len);
        let data = (block as *const i64).add(2);
        *data.add(idx as usize)
    }
}

/// Set an element in a list by index.
#[unsafe(no_mangle)]
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
/// Returns the same handle so callers don't need to track reallocation.
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
pub extern "C" fn aster_list_len(handle: *const u8) -> i64 {
    if handle.is_null() {
        return 0;
    }
    unsafe {
        let block = list_block(handle);
        *(block as *const i64)
    }
}

/// Insert an element at a given index, shifting subsequent elements right.
#[unsafe(no_mangle)]
pub extern "C" fn aster_list_insert(handle: *mut u8, index: i64, value: i64) {
    if handle.is_null() {
        eprintln!("aster_list_insert: null list handle");
        std::process::abort();
    }
    unsafe {
        let block = list_block(handle);
        let len = *(block as *const i64);
        if index < 0 || index > len {
            eprintln!("list insert index out of bounds: {} (len {})", index, len);
            std::process::abort();
        }
        let cap = *((block as *const i64).add(1));
        let block = if len >= cap {
            let new_cap = (cap * 2).max(4) as usize;
            let alloc_size = match new_cap.checked_mul(8).and_then(|n| n.checked_add(16)) {
                Some(n) => n,
                None => {
                    eprintln!("aster_list_insert: size overflow");
                    std::process::abort();
                }
            };
            let new_block = gc_alloc_data_block(alloc_size);
            std::ptr::copy_nonoverlapping(block, new_block, 16 + (len as usize) * 8);
            *((new_block as *mut i64).add(1)) = new_cap as i64;
            *(handle as *mut *mut u8) = new_block;
            new_block
        } else {
            block
        };
        let data = (block as *mut i64).add(2);
        let idx = index as usize;
        let count = (len - index) as usize;
        std::ptr::copy(data.add(idx), data.add(idx + 1), count);
        *data.add(idx) = value;
        *(block as *mut i64) = len + 1;
    }
}

/// Remove an element at a given index, shifting subsequent elements left. Returns the removed value.
#[unsafe(no_mangle)]
pub extern "C" fn aster_list_remove(handle: *mut u8, index: i64) -> i64 {
    if handle.is_null() {
        eprintln!("aster_list_remove: null list handle");
        std::process::abort();
    }
    unsafe {
        let block = list_block(handle);
        let len = *(block as *const i64);
        if index < 0 || index >= len {
            eprintln!("list remove index out of bounds: {} (len {})", index, len);
            std::process::abort();
        }
        let data = (block as *mut i64).add(2);
        let idx = index as usize;
        let removed = *data.add(idx);
        let count = (len - index - 1) as usize;
        std::ptr::copy(data.add(idx + 1), data.add(idx), count);
        *(block as *mut i64) = len - 1;
        removed
    }
}

/// Remove and return the last element. Aborts on empty list.
#[unsafe(no_mangle)]
pub extern "C" fn aster_list_pop(handle: *mut u8) -> i64 {
    if handle.is_null() {
        eprintln!("aster_list_pop: null list handle");
        std::process::abort();
    }
    unsafe {
        let block = list_block(handle);
        let len = *(block as *const i64);
        if len <= 0 {
            eprintln!("aster_list_pop: empty list");
            std::process::abort();
        }
        let data = (block as *const i64).add(2);
        let value = *data.add((len - 1) as usize);
        *(block as *mut i64) = len - 1;
        value
    }
}

/// Check if a list contains a value. `is_string`: 1 = use string equality, 0 = bitwise i64.
#[unsafe(no_mangle)]
pub extern "C" fn aster_list_contains(handle: *const u8, item: i64, is_string: i64) -> i8 {
    if handle.is_null() {
        return 0;
    }
    unsafe {
        let block = list_block(handle);
        let len = *(block as *const i64);
        let data = (block as *const i64).add(2);
        for i in 0..len as usize {
            if is_string != 0 {
                if string_eq(*data.add(i) as *const u8, item as *const u8) {
                    return 1;
                }
            } else if *data.add(i) == item {
                return 1;
            }
        }
    }
    0
}

/// Convert a List[Int] to a heap string like "[1, 2, 3]".
/// Handle layout: [block_ptr] -> block: [len: i64][cap: i64][elems: i64...]
#[unsafe(no_mangle)]
pub extern "C" fn aster_list_to_string(handle: *const u8) -> *mut u8 {
    use super::string::aster_string_new;
    if handle.is_null() {
        let s = "[]";
        return aster_string_new(s.as_ptr(), s.len());
    }
    unsafe {
        let block = list_block(handle);
        let raw_len = *(block as *const i64);
        if raw_len < 0 {
            let s = "<invalid list>";
            return aster_string_new(s.as_ptr(), s.len());
        }
        let len = raw_len as usize;
        let data = (block as *const i64).add(2);
        let mut result = String::with_capacity(len * 4 + 2);
        result.push('[');
        for i in 0..len {
            if i > 0 {
                result.push_str(", ");
            }
            result.push_str(&(*data.add(i)).to_string());
        }
        result.push(']');
        aster_string_new(result.as_ptr(), result.len())
    }
}
