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
    let layout = std::alloc::Layout::from_size_align(size, 8)
        .expect("aster_alloc: invalid layout (size too large)");
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
    let total = 8usize
        .checked_add(len)
        .expect("aster_string_new: size overflow");
    let ptr = aster_alloc(total);
    unsafe {
        *(ptr as *mut i64) = len as i64;
        if len > 0 {
            if data.is_null() {
                eprintln!("aster_string_new: null data pointer with nonzero length");
                std::process::abort();
            }
            std::ptr::copy_nonoverlapping(data, ptr.add(8), len);
        }
    }
    ptr
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
        let total = a_len
            .checked_add(b_len)
            .and_then(|n| n.checked_add(8))
            .expect("aster_string_concat: size overflow");
        let result = aster_alloc(total);
        *(result as *mut i64) = (a_len + b_len) as i64;
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

/// Allocate a new list. Layout: [len: i64][cap: i64][data: [i64...]]
pub extern "C" fn aster_list_new(cap: i64) -> *mut u8 {
    let cap = cap.max(4) as usize;
    let alloc_size = cap
        .checked_mul(8)
        .and_then(|n| n.checked_add(16))
        .expect("aster_list_new: size overflow");
    let ptr = aster_alloc(alloc_size);
    unsafe {
        *(ptr as *mut i64) = 0; // len = 0
        *((ptr as *mut i64).add(1)) = cap as i64; // cap
    }
    ptr
}

/// Get an element from a list by index. Returns the i64 value at that index.
pub extern "C" fn aster_list_get(list: *const u8, index: i64) -> i64 {
    if list.is_null() {
        eprintln!("aster_list_get: null list pointer");
        std::process::abort();
    }
    unsafe {
        let len = *(list as *const i64);
        if index < 0 || index >= len {
            eprintln!("list index out of bounds: {} (len {})", index, len);
            std::process::abort();
        }
        let data = (list as *const i64).add(2);
        *data.add(index as usize)
    }
}

/// Set an element in a list by index.
pub extern "C" fn aster_list_set(list: *mut u8, index: i64, value: i64) {
    if list.is_null() {
        eprintln!("aster_list_set: null list pointer");
        std::process::abort();
    }
    unsafe {
        let len = *(list as *const i64);
        if index < 0 || index >= len {
            eprintln!("list index out of bounds: {} (len {})", index, len);
            std::process::abort();
        }
        let data = (list as *mut i64).add(2);
        *data.add(index as usize) = value;
    }
}

/// Push an element to a list. May reallocate.
pub extern "C" fn aster_list_push(list: *mut u8, value: i64) -> *mut u8 {
    if list.is_null() {
        eprintln!("aster_list_push: null list pointer");
        std::process::abort();
    }
    unsafe {
        let len = *(list as *mut i64);
        let cap = *((list as *mut i64).add(1));
        if len >= cap {
            // Grow: double capacity
            let new_cap = (cap * 2).max(4) as usize;
            let alloc_size = new_cap
                .checked_mul(8)
                .and_then(|n| n.checked_add(16))
                .expect("aster_list_push: size overflow");
            let new_ptr = aster_alloc(alloc_size);
            std::ptr::copy_nonoverlapping(list, new_ptr, 16 + (len as usize) * 8);
            *((new_ptr as *mut i64).add(1)) = new_cap as i64;
            let data = (new_ptr as *mut i64).add(2);
            *data.add(len as usize) = value;
            *(new_ptr as *mut i64) = len + 1;
            new_ptr
        } else {
            let data = (list as *mut i64).add(2);
            *data.add(len as usize) = value;
            *(list as *mut i64) = len + 1;
            list
        }
    }
}

/// Get the length of a list.
pub extern "C" fn aster_list_len(list: *const u8) -> i64 {
    if list.is_null() {
        return 0;
    }
    unsafe { *(list as *const i64) }
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
    aster_alloc(size)
}

// ---------------------------------------------------------------------------
// Map operations — linear-scan associative array
// Layout: [len: i64][cap: i64][entries: [key_ptr: i64, value: i64]...]
// Each entry is 16 bytes. Keys are heap string pointers, compared by content.
// ---------------------------------------------------------------------------

/// Create a new map with the given initial capacity.
pub extern "C" fn aster_map_new(cap: i64) -> *mut u8 {
    let cap = cap.max(4) as usize;
    let alloc_size = cap
        .checked_mul(16)
        .and_then(|n| n.checked_add(16))
        .expect("aster_map_new: size overflow");
    let ptr = aster_alloc(alloc_size);
    unsafe {
        *(ptr as *mut i64) = 0; // len = 0
        *((ptr as *mut i64).add(1)) = cap as i64; // cap
    }
    ptr
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
        let a_len = *(a as *const i64) as usize;
        let b_len = *(b as *const i64) as usize;
        if a_len != b_len {
            return false;
        }
        let a_data = a.add(8);
        let b_data = b.add(8);
        std::slice::from_raw_parts(a_data, a_len) == std::slice::from_raw_parts(b_data, b_len)
    }
}

/// Set a key-value pair in the map. Overwrites if key exists, appends otherwise.
/// May reallocate. Returns the (possibly new) map pointer.
pub extern "C" fn aster_map_set(map: *mut u8, key: i64, value: i64) -> *mut u8 {
    if map.is_null() {
        eprintln!("aster_map_set: null map pointer");
        std::process::abort();
    }
    unsafe {
        let len = *(map as *const i64) as usize;
        let entries = map.add(16) as *mut i64;

        // Linear scan for existing key
        for i in 0..len {
            let entry_key = *entries.add(i * 2);
            if string_eq(entry_key as *const u8, key as *const u8) {
                *entries.add(i * 2 + 1) = value;
                return map;
            }
        }

        // Key not found — append
        let cap = *((map as *const i64).add(1)) as usize;
        if len >= cap {
            // Grow: double capacity
            let new_cap = (cap * 2).max(4);
            let alloc_size = new_cap
                .checked_mul(16)
                .and_then(|n| n.checked_add(16))
                .expect("aster_map_set: size overflow");
            let new_ptr = aster_alloc(alloc_size);
            std::ptr::copy_nonoverlapping(map, new_ptr, 16 + len * 16);
            *((new_ptr as *mut i64).add(1)) = new_cap as i64;
            let new_entries = new_ptr.add(16) as *mut i64;
            *new_entries.add(len * 2) = key;
            *new_entries.add(len * 2 + 1) = value;
            *(new_ptr as *mut i64) = (len + 1) as i64;
            new_ptr
        } else {
            *entries.add(len * 2) = key;
            *entries.add(len * 2 + 1) = value;
            *(map as *mut i64) = (len + 1) as i64;
            map
        }
    }
}

/// Get a value from the map by key. Returns the value or 0 if not found.
pub extern "C" fn aster_map_get(map: *const u8, key: i64) -> i64 {
    if map.is_null() {
        eprintln!("aster_map_get: null map pointer");
        std::process::abort();
    }
    unsafe {
        let len = *(map as *const i64) as usize;
        let entries = map.add(16) as *const i64;
        for i in 0..len {
            let entry_key = *entries.add(i * 2);
            if string_eq(entry_key as *const u8, key as *const u8) {
                return *entries.add(i * 2 + 1);
            }
        }
        0 // key not found
    }
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
    ];
    builder.symbols(symbols);
}
