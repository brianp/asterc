use super::gc::gc_alloc_string;

// Heap-allocated string: pointer to { len: i64, data: [u8] }
// We represent strings as a pair (ptr, len) packed into a struct on the heap.
// For now, the "Ptr" in FIR is a raw pointer to the data, with length stored
// at offset -8 (just before the data pointer).

/// Create a new heap-allocated string from a pointer and length.
/// Returns a pointer to the string object [len: i64][data: u8...].
#[unsafe(no_mangle)]
pub extern "C" fn aster_string_new(data: *const u8, len: usize) -> *mut u8 {
    if len > 0 && data.is_null() {
        eprintln!("aster_string_new: null data pointer with nonzero length");
        std::process::abort();
    }
    gc_alloc_string(data, len)
}

/// Concatenate two heap strings. Returns a new heap string.
#[unsafe(no_mangle)]
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
        debug_assert!(
            !a.is_null() || a_len == 0,
            "aster_string_concat: null `a` with nonzero length"
        );
        debug_assert!(
            !b.is_null() || b_len == 0,
            "aster_string_concat: null `b` with nonzero length"
        );
        let concat_len = a_len.checked_add(b_len).unwrap_or_else(|| {
            eprintln!("aster_string_concat: combined string length overflow");
            std::process::abort();
        });
        let result = gc_alloc_string(std::ptr::null(), concat_len);
        // Manually fill data (gc_alloc_string zero-inits since data is null)
        if a_len > 0 {
            debug_assert!(
                !a.add(8).is_null(),
                "aster_string_concat: `a` data pointer is null"
            );
            std::ptr::copy_nonoverlapping(a.add(8), result.add(8), a_len);
        }
        if b_len > 0 {
            debug_assert!(
                !b.add(8).is_null(),
                "aster_string_concat: `b` data pointer is null"
            );
            std::ptr::copy_nonoverlapping(b.add(8), result.add(8 + a_len), b_len);
        }
        result
    }
}

/// Get the byte length of a heap string.
#[unsafe(no_mangle)]
pub extern "C" fn aster_string_len(ptr: *const u8) -> i64 {
    if ptr.is_null() {
        return 0;
    }
    let raw = unsafe { *(ptr as *const i64) };
    if raw < 0 { 0 } else { raw }
}

/// Get the character (Unicode scalar) length of a heap string.
#[unsafe(no_mangle)]
pub extern "C" fn aster_string_char_len(ptr: *const u8) -> i64 {
    let s = unsafe { aster_string_to_rust(ptr) };
    s.chars().count() as i64
}

/// Return the byte slice of a heap string without allocating.
/// Caller must ensure `ptr` is a valid Aster heap string pointer.
/// Returns an empty slice for null or zero-length strings.
unsafe fn string_bytes(ptr: *const u8) -> &'static [u8] {
    if ptr.is_null() {
        return &[];
    }
    unsafe {
        let len = *(ptr as *const i64);
        if len <= 0 {
            return &[];
        }
        debug_assert!(
            len as usize <= isize::MAX as usize,
            "string_bytes: length exceeds isize::MAX"
        );
        std::slice::from_raw_parts(ptr.add(8), len as usize)
    }
}

/// Check if a heap string contains a substring.
#[unsafe(no_mangle)]
pub extern "C" fn aster_string_contains(haystack: *const u8, needle: *const u8) -> i8 {
    let h = unsafe { string_bytes(haystack) };
    let n = unsafe { string_bytes(needle) };
    if n.is_empty() {
        return 1;
    }
    if h.windows(n.len()).any(|w| w == n) {
        1
    } else {
        0
    }
}

/// Check if a heap string starts with a prefix.
#[unsafe(no_mangle)]
pub extern "C" fn aster_string_starts_with(s: *const u8, prefix: *const u8) -> i8 {
    let s = unsafe { string_bytes(s) };
    let p = unsafe { string_bytes(prefix) };
    if s.starts_with(p) { 1 } else { 0 }
}

/// Check if a heap string ends with a suffix.
#[unsafe(no_mangle)]
pub extern "C" fn aster_string_ends_with(s: *const u8, suffix: *const u8) -> i8 {
    let s = unsafe { string_bytes(s) };
    let sf = unsafe { string_bytes(suffix) };
    if s.ends_with(sf) { 1 } else { 0 }
}

/// Trim leading and trailing whitespace from a heap string.
#[unsafe(no_mangle)]
pub extern "C" fn aster_string_trim(ptr: *const u8) -> *mut u8 {
    let s = unsafe { aster_string_to_rust(ptr) };
    let trimmed = s.trim();
    aster_string_new(trimmed.as_ptr(), trimmed.len())
}

/// Convert a heap string to uppercase.
#[unsafe(no_mangle)]
pub extern "C" fn aster_string_to_upper(ptr: *const u8) -> *mut u8 {
    let s = unsafe { aster_string_to_rust(ptr) };
    let upper = s.to_uppercase();
    aster_string_new(upper.as_ptr(), upper.len())
}

/// Convert a heap string to lowercase.
#[unsafe(no_mangle)]
pub extern "C" fn aster_string_to_lower(ptr: *const u8) -> *mut u8 {
    let s = unsafe { aster_string_to_rust(ptr) };
    let lower = s.to_lowercase();
    aster_string_new(lower.as_ptr(), lower.len())
}

/// Slice a heap string by character indices [from, to), clamped to bounds.
#[unsafe(no_mangle)]
pub extern "C" fn aster_string_slice(ptr: *const u8, from: i64, to: i64) -> *mut u8 {
    let s = unsafe { aster_string_to_rust(ptr) };
    let char_count = s.chars().count();
    let from = (from.max(0) as usize).min(char_count);
    let to = (to.max(0) as usize).min(char_count);
    if from >= to {
        return aster_string_new(std::ptr::null(), 0);
    }
    // Map character indices to byte offsets
    let byte_start = s
        .char_indices()
        .nth(from)
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    let byte_end = if to == char_count {
        s.len()
    } else {
        s.char_indices().nth(to).map(|(i, _)| i).unwrap_or(s.len())
    };
    let slice = &s[byte_start..byte_end];
    aster_string_new(slice.as_ptr(), slice.len())
}

/// Replace all occurrences of `old` with `new` in a heap string.
#[unsafe(no_mangle)]
pub extern "C" fn aster_string_replace(ptr: *const u8, old: *const u8, new: *const u8) -> *mut u8 {
    let s = unsafe { aster_string_to_rust(ptr) };
    let old_s = unsafe { aster_string_to_rust(old) };
    let new_s = unsafe { aster_string_to_rust(new) };
    let result = s.replace(&old_s, &new_s);
    aster_string_new(result.as_ptr(), result.len())
}

/// Split a heap string by a separator, returning a list of heap strings.
#[unsafe(no_mangle)]
pub extern "C" fn aster_string_split(ptr: *const u8, sep: *const u8) -> *mut u8 {
    use super::alloc::aster_alloc;
    let s = unsafe { aster_string_to_rust(ptr) };
    let sep_s = unsafe { aster_string_to_rust(sep) };
    let parts: Vec<&str> = s.split(&sep_s).collect();
    let count = parts.len();

    // Create a list handle -> block: [len: i64][cap: i64][elems: i64...]
    let block_size = 8 + 8 + count * 8; // len + cap + elements
    let block = aster_alloc(block_size) as *mut i64;
    unsafe {
        *block = count as i64; // len
        *block.add(1) = count as i64; // cap
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

/// Compare two heap strings by content. Returns 1 (equal) or 0 (not equal).
#[unsafe(no_mangle)]
pub extern "C" fn aster_string_eq(a: *const u8, b: *const u8) -> i8 {
    if unsafe { string_eq(a, b) } { 1 } else { 0 }
}

/// Lexicographic comparison of two heap strings.
/// Returns -1, 0, or 1 (like C strcmp).
#[unsafe(no_mangle)]
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
pub(super) unsafe fn string_eq(a: *const u8, b: *const u8) -> bool {
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

/// Extract a Rust String from an Aster heap string pointer.
/// Layout: [len: i64][data: u8...]
pub(super) unsafe fn aster_string_to_rust(ptr: *const u8) -> String {
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
pub(super) fn aster_string_new_from_rust(s: &str) -> *mut u8 {
    aster_string_new(s.as_ptr(), s.len())
}

/// Convert an integer to a heap string.
#[unsafe(no_mangle)]
pub extern "C" fn aster_int_to_string(val: i64) -> *mut u8 {
    let s = val.to_string();
    aster_string_new(s.as_ptr(), s.len())
}

/// Convert a float to a heap string.
#[unsafe(no_mangle)]
pub extern "C" fn aster_float_to_string(val: f64) -> *mut u8 {
    let s = val.to_string();
    aster_string_new(s.as_ptr(), s.len())
}

/// Convert a bool to a heap string.
#[unsafe(no_mangle)]
pub extern "C" fn aster_bool_to_string(val: i8) -> *mut u8 {
    let s = if val != 0 { "true" } else { "false" };
    aster_string_new(s.as_ptr(), s.len())
}
