use cranelift_jit::JITBuilder;
use std::sync::Mutex;

use crate::green::scheduler;
use crate::green::thread::GreenThread;

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

/// Say a string (ptr to heap string object).
/// String layout: [len: i64][data: u8...]
pub extern "C" fn aster_say_str(ptr: *const u8) {
    if ptr.is_null() {
        println!("nil");
        return;
    }
    const MAX_STRING_LENGTH: usize = 1_000_000;
    unsafe {
        let raw_len = *(ptr as *const i64);
        if raw_len < 0 || raw_len as usize > MAX_STRING_LENGTH {
            println!("<invalid string: length {} out of bounds>", raw_len);
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

/// Say an integer.
pub extern "C" fn aster_say_int(val: i64) {
    println!("{}", val);
}

/// Say a float.
pub extern "C" fn aster_say_float(val: f64) {
    println!("{}", val);
}

/// Say a bool.
pub extern "C" fn aster_say_bool(val: i8) {
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
        let concat_len = a_len.checked_add(b_len).unwrap_or_else(|| {
            eprintln!("aster_string_concat: combined string length overflow");
            std::process::abort();
        });
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

/// Get the byte length of a heap string.
pub extern "C" fn aster_string_len(ptr: *const u8) -> i64 {
    if ptr.is_null() {
        return 0;
    }
    let raw = unsafe { *(ptr as *const i64) };
    if raw < 0 { 0 } else { raw }
}

/// Get the character (Unicode scalar) length of a heap string.
pub extern "C" fn aster_string_char_len(ptr: *const u8) -> i64 {
    let s = unsafe { aster_string_to_rust(ptr) };
    s.chars().count() as i64
}

/// Check if a heap string contains a substring.
pub extern "C" fn aster_string_contains(haystack: *const u8, needle: *const u8) -> i8 {
    let h = unsafe { aster_string_to_rust(haystack) };
    let n = unsafe { aster_string_to_rust(needle) };
    if h.contains(&n) { 1 } else { 0 }
}

/// Check if a heap string starts with a prefix.
pub extern "C" fn aster_string_starts_with(s: *const u8, prefix: *const u8) -> i8 {
    let s = unsafe { aster_string_to_rust(s) };
    let p = unsafe { aster_string_to_rust(prefix) };
    if s.starts_with(&p) { 1 } else { 0 }
}

/// Check if a heap string ends with a suffix.
pub extern "C" fn aster_string_ends_with(s: *const u8, suffix: *const u8) -> i8 {
    let s = unsafe { aster_string_to_rust(s) };
    let sf = unsafe { aster_string_to_rust(suffix) };
    if s.ends_with(&sf) { 1 } else { 0 }
}

/// Trim leading and trailing whitespace from a heap string.
pub extern "C" fn aster_string_trim(ptr: *const u8) -> *mut u8 {
    let s = unsafe { aster_string_to_rust(ptr) };
    let trimmed = s.trim();
    aster_string_new(trimmed.as_ptr(), trimmed.len())
}

/// Convert a heap string to uppercase.
pub extern "C" fn aster_string_to_upper(ptr: *const u8) -> *mut u8 {
    let s = unsafe { aster_string_to_rust(ptr) };
    let upper = s.to_uppercase();
    aster_string_new(upper.as_ptr(), upper.len())
}

/// Convert a heap string to lowercase.
pub extern "C" fn aster_string_to_lower(ptr: *const u8) -> *mut u8 {
    let s = unsafe { aster_string_to_rust(ptr) };
    let lower = s.to_lowercase();
    aster_string_new(lower.as_ptr(), lower.len())
}

/// Slice a heap string by character indices [from, to), clamped to bounds.
pub extern "C" fn aster_string_slice(ptr: *const u8, from: i64, to: i64) -> *mut u8 {
    let s = unsafe { aster_string_to_rust(ptr) };
    let char_count = s.chars().count();
    let from = (from.max(0) as usize).min(char_count);
    let to = (to.max(0) as usize).min(char_count);
    if from >= to {
        return aster_string_new(std::ptr::null(), 0);
    }
    // Map character indices to byte offsets
    let byte_start = s.char_indices().nth(from).map(|(i, _)| i).unwrap_or(s.len());
    let byte_end = if to == char_count {
        s.len()
    } else {
        s.char_indices().nth(to).map(|(i, _)| i).unwrap_or(s.len())
    };
    let slice = &s[byte_start..byte_end];
    aster_string_new(slice.as_ptr(), slice.len())
}

/// Replace all occurrences of `old` with `new` in a heap string.
pub extern "C" fn aster_string_replace(
    ptr: *const u8,
    old: *const u8,
    new: *const u8,
) -> *mut u8 {
    let s = unsafe { aster_string_to_rust(ptr) };
    let old_s = unsafe { aster_string_to_rust(old) };
    let new_s = unsafe { aster_string_to_rust(new) };
    let result = s.replace(&old_s, &new_s);
    aster_string_new(result.as_ptr(), result.len())
}

/// Split a heap string by a separator, returning a list of heap strings.
pub extern "C" fn aster_string_split(ptr: *const u8, sep: *const u8) -> *mut u8 {
    let s = unsafe { aster_string_to_rust(ptr) };
    let sep_s = unsafe { aster_string_to_rust(sep) };
    let parts: Vec<&str> = s.split(&sep_s).collect();
    let count = parts.len();

    // Create a list handle -> block: [len: i64][cap: i64][elems: i64...]
    let block_size = 8 + 8 + count * 8; // len + cap + elements
    let block = aster_alloc(block_size) as *mut i64;
    unsafe {
        *block = count as i64;         // len
        *block.add(1) = count as i64;  // cap
        for (i, part) in parts.iter().enumerate() {
            let heap_str = aster_string_new(part.as_ptr(), part.len());
            *block.add(2 + i) = heap_str as i64;
        }
    }
    // Wrap in a handle (pointer to block pointer)
    let handle = aster_alloc(8) as *mut *mut i64;
    unsafe {
        *handle = block;
    }
    handle as *mut u8
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
    let layout = std::alloc::Layout::from_size_align(size, 8).unwrap_or_else(|_| {
        eprintln!("aster_dealloc: invalid layout (size={size})");
        std::process::abort();
    });
    unsafe {
        std::alloc::dealloc(ptr, layout);
    }
}

/// Dereference a handle to get the data block pointer.
#[inline]
unsafe fn list_block(handle: *const u8) -> *mut u8 {
    unsafe { *(handle as *const *mut u8) }
}

/// Allocate a new list. Returns a handle (pointer-to-pointer).
/// `ptr_elems`: 0 = value-type elements (Int/Float/Bool — GC won't scan them),
///              1 = pointer-type elements (String/Class — GC will trace them).
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

/// Checked integer addition. Aborts on overflow.
/// This is an interim measure until BigInt promotion is implemented (see bigint-rfc.md).
pub extern "C" fn aster_int_add(a: i64, b: i64) -> i64 {
    match a.checked_add(b) {
        Some(result) => result,
        None => {
            eprintln!("integer overflow: {} + {} exceeds 64-bit range", a, b);
            std::process::abort();
        }
    }
}

/// Checked integer subtraction. Aborts on overflow.
/// This is an interim measure until BigInt promotion is implemented (see bigint-rfc.md).
pub extern "C" fn aster_int_sub(a: i64, b: i64) -> i64 {
    match a.checked_sub(b) {
        Some(result) => result,
        None => {
            eprintln!("integer overflow: {} - {} exceeds 64-bit range", a, b);
            std::process::abort();
        }
    }
}

/// Checked integer multiplication. Aborts on overflow.
/// This is an interim measure until BigInt promotion is implemented (see bigint-rfc.md).
pub extern "C" fn aster_int_mul(a: i64, b: i64) -> i64 {
    match a.checked_mul(b) {
        Some(result) => result,
        None => {
            eprintln!("integer overflow: {} * {} exceeds 64-bit range", a, b);
            std::process::abort();
        }
    }
}

/// Float exponentiation: base ** exp.
pub extern "C" fn aster_pow_float(base: f64, exp: f64) -> f64 {
    base.powf(exp)
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

/// Convert a List[Int] to a heap string like "[1, 2, 3]".
/// Handle layout: [block_ptr] -> block: [len: i64][cap: i64][elems: i64...]
pub extern "C" fn aster_list_to_string(handle: *const u8) -> *mut u8 {
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

/// Allocate a class instance. Size is in bytes.
/// Conservative fallback: marks all fields as potential pointers.
/// Used by enum constructors and any code that doesn't supply a ptr_count.
pub extern "C" fn aster_class_alloc(size: usize) -> *mut u8 {
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
    gc_alloc_inner(size, OBJ_CLOSURE)
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

/// Compare two heap strings by content. Returns 1 (equal) or 0 (not equal).
pub extern "C" fn aster_string_eq(a: *const u8, b: *const u8) -> i8 {
    if unsafe { string_eq(a, b) } { 1 } else { 0 }
}

/// Lexicographic comparison of two heap strings.
/// Returns -1, 0, or 1 (like C strcmp).
pub extern "C" fn aster_string_compare(a: *const u8, b: *const u8) -> i64 {
    unsafe {
        let (a_data, a_len) = if a.is_null() {
            (std::ptr::null::<u8>(), 0usize)
        } else {
            let raw = *(a as *const i64);
            let len = if raw < 0 { 0 } else { raw as usize };
            (a.add(8), len)
        };
        let (b_data, b_len) = if b.is_null() {
            (std::ptr::null::<u8>(), 0usize)
        } else {
            let raw = *(b as *const i64);
            let len = if raw < 0 { 0 } else { raw as usize };
            (b.add(8), len)
        };
        let a_slice = if a_len == 0 {
            &[]
        } else {
            std::slice::from_raw_parts(a_data, a_len)
        };
        let b_slice = if b_len == 0 {
            &[]
        } else {
            std::slice::from_raw_parts(b_data, b_len)
        };
        match a_slice.cmp(b_slice) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        }
    }
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

/// Check whether a key exists in the map. Returns 1 if found, 0 otherwise.
pub extern "C" fn aster_map_has_key(handle: *const u8, key: i64) -> i64 {
    if handle.is_null() {
        eprintln!("aster_map_has_key: null map handle");
        std::process::abort();
    }
    unsafe {
        let block = map_block(handle);
        let len = *(block as *const i64) as usize;
        let entries = block.add(16) as *const i64;
        for i in 0..len {
            let entry_key = *entries.add(i * 2);
            if string_eq(entry_key as *const u8, key as *const u8) {
                return 1;
            }
        }
        0
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
const OBJ_TASK: u8 = 6; // [state, consumed, payload] — payload may be a GC pointer
const OBJ_LIST_HANDLE_NOPTR: u8 = 7; // handle → data block with value-type elements (no GC trace)

/// Magic bytes in header slots [2..5] to identify valid GC objects.
/// 4 bytes gives a 1-in-2^32 false positive rate for conservative pointer detection.
const GC_MAGIC: [u8; 4] = [0xA5, 0x7E, 0xC3, 0x91];

const HEADER_SIZE: usize = 24;
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
unsafe fn obj_ptr_count(header: *const u8) -> u8 {
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
fn gc_alloc_inner(payload_size: usize, obj_ty: u8) -> *mut u8 {
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
// Error handling — per-thread error flag (saved/restored per green thread)
// ---------------------------------------------------------------------------

const TASK_READY: i64 = 1;
const TASK_FAILED: i64 = 2;

thread_local! {
    static ERROR_FLAG: Cell<bool> = const { Cell::new(false) };
    static ERROR_TYPE_TAG: Cell<i64> = const { Cell::new(0) };
    static ERROR_VALUE: Cell<i64> = const { Cell::new(0) };
}

pub extern "C" fn aster_error_set() {
    ERROR_FLAG.set(true);
}

/// Set error flag with a type tag and the error object pointer.
pub extern "C" fn aster_error_set_typed(type_tag: i64, value: i64) {
    ERROR_FLAG.set(true);
    ERROR_TYPE_TAG.set(type_tag);
    ERROR_VALUE.set(value);
}

pub extern "C" fn aster_error_check() -> i8 {
    let was = ERROR_FLAG.get();
    ERROR_FLAG.set(false);
    was as i8
}

/// Return the type tag of the current error (valid after error_check returns true).
pub extern "C" fn aster_error_get_tag() -> i64 {
    ERROR_TYPE_TAG.get()
}

/// Return the error object pointer (valid after error_check returns true).
pub extern "C" fn aster_error_get_value() -> i64 {
    ERROR_VALUE.get()
}

pub(crate) fn error_flag_get() -> bool {
    ERROR_FLAG.get()
}

pub(crate) fn error_flag_set(val: bool) {
    ERROR_FLAG.set(val);
}

pub extern "C" fn aster_safepoint() {
    scheduler::safepoint();
}

pub extern "C" fn aster_panic() {
    eprintln!("aster: uncaught error");
    std::process::abort();
}

// ---------------------------------------------------------------------------
// Async scope
// ---------------------------------------------------------------------------

struct AsyncScopeState {
    tasks: Vec<*mut GreenThread>,
}

struct AsyncScopeHandle {
    state: Mutex<AsyncScopeState>,
}

fn live_scope(scope: *const u8) -> Option<&'static AsyncScopeHandle> {
    if scope.is_null() {
        None
    } else {
        Some(unsafe { &*(scope as *const AsyncScopeHandle) })
    }
}

fn register_task_with_scope(scope: *mut u8, task: *mut GreenThread) {
    if let Some(scope) = live_scope(scope) {
        // Mark the task as scoped so consume_thread_result defers freeing to scope exit.
        let thread = unsafe { &*task };
        thread.state.lock().unwrap().scoped = true;
        let mut state = scope.state.lock().unwrap();
        state.tasks.push(task);
    }
}

pub extern "C" fn aster_async_scope_enter() -> *mut u8 {
    Box::into_raw(Box::new(AsyncScopeHandle {
        state: Mutex::new(AsyncScopeState { tasks: Vec::new() }),
    })) as *mut u8
}

pub extern "C" fn aster_async_scope_exit(scope: *mut u8) {
    if scope.is_null() {
        return;
    }
    let scope_handle = unsafe { &*(scope as *const AsyncScopeHandle) };
    let tasks = {
        let mut state = scope_handle.state.lock().unwrap();
        std::mem::take(&mut state.tasks)
    };
    for &task in &tasks {
        scheduler::cancel_thread(task);
    }
    for &task in &tasks {
        scheduler::wait_cancel_thread(task);
    }
    // Free all scoped task structs. Scoped tasks defer freeing to scope exit,
    // so even consumed tasks still need their struct freed here.
    for task in tasks {
        scheduler::free_scoped_thread(task);
    }
    // Free the scope handle itself
    unsafe { drop(Box::from_raw(scope as *mut AsyncScopeHandle)) };
}

// ---------------------------------------------------------------------------
// Task spawn / resolve / cancel — backed by green threads
// ---------------------------------------------------------------------------

pub extern "C" fn aster_task_spawn(entry: usize, args: *mut u8, scope: *mut u8) -> *mut u8 {
    let thread = scheduler::spawn_green_thread(entry, args as usize);
    register_task_with_scope(scope, thread);
    thread as *mut u8
}

pub extern "C" fn aster_task_block_on(entry: usize, args: *mut u8) -> i64 {
    let task = aster_task_spawn(entry, args, std::ptr::null_mut());
    scheduler::consume_thread_result(task as *mut GreenThread)
}

pub extern "C" fn aster_task_from_i64(value: i64, failed: i8) -> *mut u8 {
    scheduler::allocate_terminal_thread(value, failed != 0) as *mut u8
}

pub extern "C" fn aster_task_from_f64(value: f64, failed: i8) -> *mut u8 {
    scheduler::allocate_terminal_thread(value.to_bits() as i64, failed != 0) as *mut u8
}

pub extern "C" fn aster_task_from_i8(value: i8, failed: i8) -> *mut u8 {
    scheduler::allocate_terminal_thread(value as i64, failed != 0) as *mut u8
}

pub extern "C" fn aster_task_is_ready(task: *const u8) -> i8 {
    if task.is_null() {
        return 0;
    }
    scheduler::is_thread_ready(task as *const GreenThread) as i8
}

pub extern "C" fn aster_task_cancel(task: *mut u8) -> i64 {
    if !task.is_null() {
        scheduler::cancel_thread(task as *mut GreenThread);
    }
    0
}

pub extern "C" fn aster_task_wait_cancel(task: *mut u8) -> i64 {
    if !task.is_null() {
        scheduler::wait_cancel_thread(task as *mut GreenThread);
    }
    0
}

pub extern "C" fn aster_task_resolve_i64(task: *mut u8) -> i64 {
    if task.is_null() {
        aster_error_set();
        return 0;
    }
    scheduler::consume_thread_result(task as *mut GreenThread)
}

pub extern "C" fn aster_task_resolve_f64(task: *mut u8) -> f64 {
    if task.is_null() {
        aster_error_set();
        return 0.0;
    }
    let bits = scheduler::consume_thread_result(task as *mut GreenThread) as u64;
    f64::from_bits(bits)
}

pub extern "C" fn aster_task_resolve_i8(task: *mut u8) -> i8 {
    if task.is_null() {
        aster_error_set();
        return 0;
    }
    scheduler::consume_thread_result(task as *mut GreenThread) as i8
}

pub extern "C" fn aster_task_resolve_all_i64(tasks: *mut u8) -> *mut u8 {
    if tasks.is_null() {
        aster_error_set();
        return std::ptr::null_mut();
    }
    let len = aster_list_len(tasks);
    let out = aster_list_new(len, 0);
    for index in 0..len {
        let task = aster_list_get(tasks, index) as *mut u8;
        let value = aster_task_resolve_i64(task);
        if error_flag_get() {
            return out;
        }
        aster_list_push(out, value);
    }
    out
}

pub extern "C" fn aster_task_resolve_first_i64(tasks: *mut u8) -> i64 {
    if tasks.is_null() {
        aster_error_set();
        return 0;
    }
    let len = aster_list_len(tasks);
    if len == 0 {
        aster_error_set();
        return 0;
    }
    let task_handles: Vec<*mut u8> = (0..len)
        .map(|index| aster_list_get(tasks, index) as *mut u8)
        .collect();
    loop {
        for (winner_index, &task) in task_handles.iter().enumerate() {
            if task.is_null() {
                continue;
            }
            if scheduler::is_thread_ready(task as *const GreenThread) {
                for (index, other) in task_handles.iter().enumerate() {
                    if index != winner_index && !other.is_null() {
                        scheduler::cancel_thread(*other as *mut GreenThread);
                    }
                }
                return aster_task_resolve_i64(task);
            }
        }
        // Yield to scheduler if on a worker, otherwise OS yield
        if scheduler::is_worker_thread() {
            scheduler::safepoint();
        } else {
            std::thread::yield_now();
        }
    }
}

// ---------------------------------------------------------------------------
// I/O suspension hooks — Phase 5
// ---------------------------------------------------------------------------

/// Suspend the current green thread until `fd` is readable.
/// Internal plumbing — not exposed to Aster code yet.
pub extern "C" fn aster_io_wait_read(fd: i32) {
    scheduler::io_wait_readable(fd);
}

/// Suspend the current green thread until `fd` is writable.
pub extern "C" fn aster_io_wait_write(fd: i32) {
    scheduler::io_wait_writable(fd);
}

/// Submit a blocking operation (entry(arg)) to the blocking thread pool.
/// The current green thread suspends until the operation completes.
pub extern "C" fn aster_blocking_submit(entry: extern "C" fn(i64) -> i64, arg: i64) {
    scheduler::blocking_submit(Box::new(move || entry(arg)));
}

// ---------------------------------------------------------------------------
// Mutex — Phase 7
// ---------------------------------------------------------------------------

/// Internal representation of a green-thread-aware mutex.
struct AsterMutex {
    inner: Mutex<AsterMutexState>,
}

struct AsterMutexState {
    locked: bool,
    owner: usize,
    value: i64,
    wait_queue: Vec<*mut GreenThread>,
}

/// Allocate a new Mutex wrapping the given value.
pub extern "C" fn aster_mutex_new(value: i64) -> *mut u8 {
    let m = Box::new(AsterMutex {
        inner: Mutex::new(AsterMutexState {
            locked: false,
            owner: 0,
            value,
            wait_queue: Vec::new(),
        }),
    });
    Box::into_raw(m) as *mut u8
}

/// Acquire the mutex. If contended, suspend the current green thread.
/// Returns the inner value.
pub extern "C" fn aster_mutex_lock(mutex: *mut u8) -> i64 {
    if mutex.is_null() {
        aster_error_set();
        return 0;
    }
    let m = unsafe { &*(mutex as *const AsterMutex) };
    let mut state = m.inner.lock().unwrap();
    if !state.locked {
        state.locked = true;
        state.owner = scheduler::current_thread_id();
        return state.value;
    }
    // Contended — suspend on the wait queue
    let current = scheduler::current_green_thread();
    if !current.is_null() {
        state.wait_queue.push(current);
        drop(state);
        scheduler::suspend_for_mutex();
        // Re-read value after wakeup — we now own the lock
        let mut state = m.inner.lock().unwrap();
        state.locked = true;
        state.owner = scheduler::current_thread_id();
        return state.value;
    }
    // Fallback for non-green-thread context: spin with timeout
    drop(state);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        std::thread::yield_now();
        let mut state = m.inner.lock().unwrap();
        if !state.locked {
            state.locked = true;
            state.owner = scheduler::current_thread_id();
            return state.value;
        }
        drop(state);
        if std::time::Instant::now() >= deadline {
            aster_error_set();
            return 0;
        }
    }
}

/// Release the mutex and store the updated value. Wakes the first waiter.
pub extern "C" fn aster_mutex_unlock(mutex: *mut u8, value: i64) {
    if mutex.is_null() {
        return;
    }
    let m = unsafe { &*(mutex as *const AsterMutex) };
    let mut state = m.inner.lock().unwrap();
    state.value = value;
    if let Some(waiter) = state.wait_queue.pop() {
        // Transfer ownership to the waiter
        state.owner = 0; // waiter will set on resume
        drop(state);
        scheduler::wake_thread(waiter);
    } else {
        state.locked = false;
        state.owner = 0;
    }
}

/// Read the current value without locking (for debug/inspection only).
pub extern "C" fn aster_mutex_get_value(mutex: *mut u8) -> i64 {
    if mutex.is_null() {
        return 0;
    }
    let m = unsafe { &*(mutex as *const AsterMutex) };
    let state = m.inner.lock().unwrap();
    state.value
}

// ---------------------------------------------------------------------------
// Channel — Phase 8
// ---------------------------------------------------------------------------

struct AsterChannel {
    inner: Mutex<AsterChannelState>,
}

struct AsterChannelState {
    buffer: std::collections::VecDeque<i64>,
    capacity: usize,
    closed: bool,
    send_waiters: Vec<(*mut GreenThread, i64)>,
    recv_waiters: Vec<*mut GreenThread>,
}

/// Create a new channel with the given capacity (0 = unbounded, default 64).
pub extern "C" fn aster_channel_new(capacity: i64) -> *mut u8 {
    let cap = if capacity <= 0 { 64 } else { capacity as usize };
    let ch = Box::new(AsterChannel {
        inner: Mutex::new(AsterChannelState {
            buffer: std::collections::VecDeque::with_capacity(cap),
            capacity: cap,
            closed: false,
            send_waiters: Vec::new(),
            recv_waiters: Vec::new(),
        }),
    });
    Box::into_raw(ch) as *mut u8
}

/// Non-blocking send. Drops the value silently if buffer is full or channel is closed.
pub extern "C" fn aster_channel_send(ch: *mut u8, value: i64) {
    if ch.is_null() {
        return;
    }
    let c = unsafe { &*(ch as *const AsterChannel) };
    let mut state = c.inner.lock().unwrap();
    if state.closed {
        return;
    }
    // Direct delivery only when buffer is empty (preserves FIFO ordering)
    if state.buffer.is_empty()
        && let Some(waiter) = state.recv_waiters.pop()
    {
        drop(state);
        scheduler::wake_thread_with_value(waiter, value);
        return;
    }
    if state.buffer.len() < state.capacity {
        state.buffer.push_back(value);
        // Wake a receiver now that there's data in the buffer
        if let Some(waiter) = state.recv_waiters.pop() {
            drop(state);
            scheduler::wake_thread(waiter);
        }
    }
    // else: drop silently (fire-and-forget send semantics)
}

/// Blocking send. Suspends if buffer is full.
pub extern "C" fn aster_channel_wait_send(ch: *mut u8, value: i64) {
    if ch.is_null() {
        aster_error_set();
        return;
    }
    let c = unsafe { &*(ch as *const AsterChannel) };
    let mut state = c.inner.lock().unwrap();
    if state.closed {
        aster_error_set();
        return;
    }
    // Direct delivery only when buffer is empty (preserves FIFO)
    if state.buffer.is_empty()
        && let Some(waiter) = state.recv_waiters.pop()
    {
        drop(state);
        scheduler::wake_thread_with_value(waiter, value);
        return;
    }
    if state.buffer.len() < state.capacity {
        state.buffer.push_back(value);
        if let Some(waiter) = state.recv_waiters.pop() {
            drop(state);
            scheduler::wake_thread(waiter);
        }
        return;
    }
    // Buffer full — suspend
    let current = scheduler::current_green_thread();
    if !current.is_null() {
        state.send_waiters.push((current, value));
        drop(state);
        scheduler::suspend_for_channel_send();
    } else {
        // Fallback for non-green-thread context: spin with timeout
        drop(state);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            std::thread::yield_now();
            let mut state = c.inner.lock().unwrap();
            if state.buffer.len() < state.capacity || state.closed {
                if !state.closed {
                    state.buffer.push_back(value);
                }
                break;
            }
            drop(state);
            if std::time::Instant::now() >= deadline {
                aster_error_set();
                break;
            }
        }
    }
}

/// Try-send. Sets error flag if buffer full or closed.
pub extern "C" fn aster_channel_try_send(ch: *mut u8, value: i64) {
    if ch.is_null() {
        aster_error_set();
        return;
    }
    let c = unsafe { &*(ch as *const AsterChannel) };
    let mut state = c.inner.lock().unwrap();
    if state.closed {
        aster_error_set();
        return;
    }
    // Direct delivery only when buffer is empty (preserves FIFO)
    if state.buffer.is_empty()
        && let Some(waiter) = state.recv_waiters.pop()
    {
        drop(state);
        scheduler::wake_thread_with_value(waiter, value);
        return;
    }
    if state.buffer.len() < state.capacity {
        state.buffer.push_back(value);
    } else {
        aster_error_set();
    }
}

/// Non-blocking receive. Returns 0 and sets error_flag=false if empty (nil semantics).
/// Returns value if available.
pub extern "C" fn aster_channel_receive(ch: *mut u8) -> i64 {
    if ch.is_null() {
        return 0;
    }
    let c = unsafe { &*(ch as *const AsterChannel) };
    let mut state = c.inner.lock().unwrap();
    if let Some(value) = state.buffer.pop_front() {
        // Wake a send waiter if one is pending
        if let Some((waiter, send_val)) = state.send_waiters.pop() {
            state.buffer.push_back(send_val);
            drop(state);
            scheduler::wake_thread(waiter);
        }
        return value;
    }
    0 // nil
}

/// Blocking receive. Suspends if buffer is empty.
pub extern "C" fn aster_channel_wait_receive(ch: *mut u8) -> i64 {
    if ch.is_null() {
        aster_error_set();
        return 0;
    }
    let c = unsafe { &*(ch as *const AsterChannel) };
    let mut state = c.inner.lock().unwrap();
    if let Some(value) = state.buffer.pop_front() {
        if let Some((waiter, send_val)) = state.send_waiters.pop() {
            state.buffer.push_back(send_val);
            drop(state);
            scheduler::wake_thread(waiter);
        }
        return value;
    }
    if state.closed {
        aster_error_set();
        return 0;
    }
    // Empty — suspend
    let current = scheduler::current_green_thread();
    if !current.is_null() {
        state.recv_waiters.push(current);
        drop(state);
        return scheduler::suspend_for_channel_receive();
    }
    // Fallback: spin with timeout
    drop(state);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        std::thread::yield_now();
        let mut state = c.inner.lock().unwrap();
        if let Some(value) = state.buffer.pop_front() {
            return value;
        }
        if state.closed {
            aster_error_set();
            return 0;
        }
        drop(state);
        if std::time::Instant::now() >= deadline {
            aster_error_set();
            return 0;
        }
    }
}

/// Try-receive. Sets error flag if empty or closed.
pub extern "C" fn aster_channel_try_receive(ch: *mut u8) -> i64 {
    if ch.is_null() {
        aster_error_set();
        return 0;
    }
    let c = unsafe { &*(ch as *const AsterChannel) };
    let mut state = c.inner.lock().unwrap();
    if let Some(value) = state.buffer.pop_front() {
        if let Some((waiter, send_val)) = state.send_waiters.pop() {
            state.buffer.push_back(send_val);
            drop(state);
            scheduler::wake_thread(waiter);
        }
        return value;
    }
    aster_error_set();
    0
}

/// Close the channel. Wake all waiters with errors.
pub extern "C" fn aster_channel_close(ch: *mut u8) {
    if ch.is_null() {
        return;
    }
    let c = unsafe { &*(ch as *const AsterChannel) };
    let mut state = c.inner.lock().unwrap();
    state.closed = true;
    let send_waiters: Vec<_> = state.send_waiters.drain(..).collect();
    let recv_waiters: Vec<_> = state.recv_waiters.drain(..).collect();
    drop(state);
    for (waiter, _) in send_waiters {
        scheduler::wake_thread_with_error(waiter);
    }
    for waiter in recv_waiters {
        scheduler::wake_thread_with_error(waiter);
    }
}

// --- File I/O ---

/// Extract a Rust String from an Aster heap string pointer.
/// Layout: [len: i64][data: u8...]
unsafe fn aster_string_to_rust(ptr: *const u8) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe {
        let len = *(ptr as *const i64);
        if len <= 0 {
            return String::new();
        }
        let data = ptr.add(8);
        let bytes = std::slice::from_raw_parts(data, len as usize);
        String::from_utf8_lossy(bytes).into_owned()
    }
}

/// Create a new Aster heap string from a Rust string.
fn aster_string_new_from_rust(s: &str) -> *mut u8 {
    aster_string_new(s.as_ptr(), s.len())
}

/// Maximum file size `file_read` will load (256 MB).
const FILE_READ_MAX_SIZE: u64 = 256 * 1024 * 1024;

/// Read a file's contents as a string. Sets error flag on failure or if the
/// file exceeds `FILE_READ_MAX_SIZE`.
pub extern "C" fn aster_file_read(path_ptr: *mut u8) -> *mut u8 {
    let path = unsafe { aster_string_to_rust(path_ptr) };
    let meta = match std::fs::metadata(&path) {
        Ok(m) => m,
        Err(_) => {
            aster_error_set();
            return aster_string_new_from_rust("");
        }
    };
    if meta.len() > FILE_READ_MAX_SIZE {
        aster_error_set();
        return aster_string_new_from_rust("");
    }
    match std::fs::read_to_string(&path) {
        Ok(content) => aster_string_new_from_rust(&content),
        Err(_) => {
            aster_error_set();
            aster_string_new_from_rust("")
        }
    }
}

/// Write content to a file (creates or truncates). Sets error flag on failure.
pub extern "C" fn aster_file_write(path_ptr: *mut u8, content_ptr: *mut u8) {
    let path = unsafe { aster_string_to_rust(path_ptr) };
    let content = unsafe { aster_string_to_rust(content_ptr) };
    if std::fs::write(&path, &content).is_err() {
        aster_error_set();
    }
}

/// Append content to a file (creates if missing). Sets error flag on failure.
pub extern "C" fn aster_file_append(path_ptr: *mut u8, content_ptr: *mut u8) {
    use std::io::Write;
    let path = unsafe { aster_string_to_rust(path_ptr) };
    let content = unsafe { aster_string_to_rust(content_ptr) };
    let result = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| f.write_all(content.as_bytes()));
    if result.is_err() {
        aster_error_set();
    }
}

// ---------------------------------------------------------------------------
// Range
// ---------------------------------------------------------------------------

/// Create a Range struct: [start: i64, end: i64, inclusive: i8]
pub extern "C" fn aster_range_new(start: i64, end: i64, inclusive: i8) -> *mut u8 {
    let ptr = aster_class_alloc(24); // 8 + 8 + 8 (padded)
    unsafe {
        *(ptr as *mut i64) = start;
        *((ptr as *mut i64).add(1)) = end;
        *((ptr as *mut i64).add(2)) = inclusive as i64;
    }
    ptr
}

/// Check if a loop variable is still within range bounds.
pub extern "C" fn aster_range_check(val: i64, end: i64, inclusive: i8) -> i8 {
    if inclusive != 0 {
        (val <= end) as i8
    } else {
        (val < end) as i8
    }
}

// ---------------------------------------------------------------------------
// Random
// ---------------------------------------------------------------------------

/// Random integer in [0, max).
/// Uses rejection sampling to avoid modulo bias.
pub extern "C" fn aster_random_int(max: i64) -> i64 {
    if max <= 0 {
        return 0;
    }
    let umax = max as u64;
    // Rejection threshold: largest multiple of umax that fits in u64.
    // Values at or above this threshold would introduce modulo bias.
    let threshold = u64::MAX - u64::MAX % umax;
    loop {
        let mut buf = [0u8; 8];
        if getrandom::getrandom(&mut buf).is_err() {
            eprintln!("aster_random_int: getrandom failed");
            std::process::abort();
        }
        let val = u64::from_le_bytes(buf);
        if val < threshold {
            return (val % umax) as i64;
        }
    }
}

/// Random float in [0.0, max).
pub extern "C" fn aster_random_float(max: f64) -> f64 {
    let mut buf = [0u8; 8];
    getrandom::getrandom(&mut buf).unwrap_or_else(|_| {
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        buf = (t as u64).to_le_bytes();
    });
    let val = u64::from_le_bytes(buf);
    // Convert to [0.0, 1.0) then scale
    let unit = (val >> 11) as f64 / (1u64 << 53) as f64;
    unit * max
}

/// Random boolean.
pub extern "C" fn aster_random_bool() -> i8 {
    let mut buf = [0u8; 1];
    getrandom::getrandom(&mut buf).unwrap_or_else(|_| {
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        buf = [(t & 1) as u8];
    });
    (buf[0] & 1) as i8
}

pub fn runtime_builtin_symbols() -> Vec<(&'static str, *const u8)> {
    vec![
        ("aster_alloc", aster_alloc as *const u8),
        ("aster_say_str", aster_say_str as *const u8),
        ("aster_say_int", aster_say_int as *const u8),
        ("aster_say_float", aster_say_float as *const u8),
        ("aster_say_bool", aster_say_bool as *const u8),
        ("aster_string_new", aster_string_new as *const u8),
        ("aster_string_concat", aster_string_concat as *const u8),
        ("aster_string_len", aster_string_len as *const u8),
        ("aster_string_eq", aster_string_eq as *const u8),
        ("aster_string_compare", aster_string_compare as *const u8),
        (
            "aster_string_char_len",
            aster_string_char_len as *const u8,
        ),
        (
            "aster_string_contains",
            aster_string_contains as *const u8,
        ),
        (
            "aster_string_starts_with",
            aster_string_starts_with as *const u8,
        ),
        (
            "aster_string_ends_with",
            aster_string_ends_with as *const u8,
        ),
        ("aster_string_trim", aster_string_trim as *const u8),
        (
            "aster_string_to_upper",
            aster_string_to_upper as *const u8,
        ),
        (
            "aster_string_to_lower",
            aster_string_to_lower as *const u8,
        ),
        ("aster_string_slice", aster_string_slice as *const u8),
        ("aster_string_replace", aster_string_replace as *const u8),
        ("aster_string_split", aster_string_split as *const u8),
        ("aster_list_new", aster_list_new as *const u8),
        ("aster_list_get", aster_list_get as *const u8),
        ("aster_list_random", aster_list_random as *const u8),
        ("aster_list_set", aster_list_set as *const u8),
        ("aster_list_push", aster_list_push as *const u8),
        ("aster_list_len", aster_list_len as *const u8),
        ("aster_class_alloc", aster_class_alloc as *const u8),
        (
            "aster_class_alloc_typed",
            aster_class_alloc_typed as *const u8,
        ),
        ("aster_closure_alloc", aster_closure_alloc as *const u8),
        ("aster_int_add", aster_int_add as *const u8),
        ("aster_int_sub", aster_int_sub as *const u8),
        ("aster_int_mul", aster_int_mul as *const u8),
        ("aster_pow_int", aster_pow_int as *const u8),
        ("aster_pow_float", aster_pow_float as *const u8),
        ("aster_int_to_string", aster_int_to_string as *const u8),
        ("aster_float_to_string", aster_float_to_string as *const u8),
        ("aster_bool_to_string", aster_bool_to_string as *const u8),
        ("aster_list_to_string", aster_list_to_string as *const u8),
        ("aster_map_new", aster_map_new as *const u8),
        ("aster_map_set", aster_map_set as *const u8),
        ("aster_map_get", aster_map_get as *const u8),
        ("aster_map_has_key", aster_map_has_key as *const u8),
        ("aster_error_set", aster_error_set as *const u8),
        ("aster_error_set_typed", aster_error_set_typed as *const u8),
        ("aster_error_check", aster_error_check as *const u8),
        ("aster_error_get_tag", aster_error_get_tag as *const u8),
        ("aster_error_get_value", aster_error_get_value as *const u8),
        ("aster_safepoint", aster_safepoint as *const u8),
        ("aster_panic", aster_panic as *const u8),
        (
            "aster_async_scope_enter",
            aster_async_scope_enter as *const u8,
        ),
        (
            "aster_async_scope_exit",
            aster_async_scope_exit as *const u8,
        ),
        ("aster_task_spawn", aster_task_spawn as *const u8),
        ("aster_task_block_on", aster_task_block_on as *const u8),
        ("aster_task_from_i64", aster_task_from_i64 as *const u8),
        ("aster_task_from_f64", aster_task_from_f64 as *const u8),
        ("aster_task_from_i8", aster_task_from_i8 as *const u8),
        ("aster_task_is_ready", aster_task_is_ready as *const u8),
        ("aster_task_cancel", aster_task_cancel as *const u8),
        (
            "aster_task_wait_cancel",
            aster_task_wait_cancel as *const u8,
        ),
        (
            "aster_task_resolve_i64",
            aster_task_resolve_i64 as *const u8,
        ),
        (
            "aster_task_resolve_f64",
            aster_task_resolve_f64 as *const u8,
        ),
        ("aster_task_resolve_i8", aster_task_resolve_i8 as *const u8),
        (
            "aster_task_resolve_all_i64",
            aster_task_resolve_all_i64 as *const u8,
        ),
        (
            "aster_task_resolve_first_i64",
            aster_task_resolve_first_i64 as *const u8,
        ),
        ("aster_gc_push_roots", aster_gc_push_roots as *const u8),
        ("aster_gc_pop_roots", aster_gc_pop_roots as *const u8),
        ("aster_gc_collect", aster_gc_collect as *const u8),
        ("aster_io_wait_read", aster_io_wait_read as *const u8),
        ("aster_io_wait_write", aster_io_wait_write as *const u8),
        ("aster_blocking_submit", aster_blocking_submit as *const u8),
        ("aster_mutex_new", aster_mutex_new as *const u8),
        ("aster_mutex_lock", aster_mutex_lock as *const u8),
        ("aster_mutex_unlock", aster_mutex_unlock as *const u8),
        ("aster_mutex_get_value", aster_mutex_get_value as *const u8),
        ("aster_channel_new", aster_channel_new as *const u8),
        ("aster_channel_send", aster_channel_send as *const u8),
        (
            "aster_channel_wait_send",
            aster_channel_wait_send as *const u8,
        ),
        (
            "aster_channel_try_send",
            aster_channel_try_send as *const u8,
        ),
        ("aster_channel_receive", aster_channel_receive as *const u8),
        (
            "aster_channel_wait_receive",
            aster_channel_wait_receive as *const u8,
        ),
        (
            "aster_channel_try_receive",
            aster_channel_try_receive as *const u8,
        ),
        ("aster_channel_close", aster_channel_close as *const u8),
        // File I/O
        ("aster_file_read", aster_file_read as *const u8),
        ("aster_file_write", aster_file_write as *const u8),
        ("aster_file_append", aster_file_append as *const u8),
        // Range
        ("aster_range_new", aster_range_new as *const u8),
        ("aster_range_check", aster_range_check as *const u8),
        // Random
        ("aster_random_int", aster_random_int as *const u8),
        ("aster_random_float", aster_random_float as *const u8),
        ("aster_random_bool", aster_random_bool as *const u8),
    ]
}

/// Register all runtime builtins with a JIT builder.
pub fn register_runtime_builtins(builder: &mut JITBuilder) {
    builder.symbols(runtime_builtin_symbols());
}
