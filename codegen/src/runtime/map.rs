use super::gc::{OBJ_MAP_HANDLE, gc_alloc_data_block, gc_alloc_inner};
use super::string::string_eq;

// ---------------------------------------------------------------------------
// Map operations — handle-based indirection, linear-scan associative array
// Data block layout: [len: i64][cap: i64][entries: [key_ptr: i64, value: i64]...]
// Each entry is 16 bytes. Keys are heap string pointers, compared by content.
// A map value is a handle (pointer-to-pointer), same as lists.
// ---------------------------------------------------------------------------

/// Dereference a map handle to get the data block pointer.
#[inline]
unsafe fn map_block(handle: *const u8) -> *mut u8 {
    debug_assert!(!handle.is_null(), "map_block: null handle");
    let block = unsafe { *(handle as *const *mut u8) };
    debug_assert!(
        !block.is_null(),
        "map_block: handle points to null data block"
    );
    block
}

/// Create a new map with the given initial capacity. Returns a handle.
#[unsafe(no_mangle)]
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
    debug_assert!(
        !block.is_null(),
        "aster_map_new: gc_alloc_data_block returned null"
    );
    unsafe {
        *(block as *mut i64) = 0; // len = 0
        *((block as *mut i64).add(1)) = cap as i64; // cap
    }
    let handle = gc_alloc_inner(8, OBJ_MAP_HANDLE);
    debug_assert!(
        !handle.is_null(),
        "aster_map_new: gc_alloc_inner returned null"
    );
    unsafe {
        *(handle as *mut *mut u8) = block;
    }
    handle
}

/// Set a key-value pair in the map. Overwrites if key exists, appends otherwise.
/// Handle stays stable; data block may move. Returns the same handle.
#[unsafe(no_mangle)]
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
        debug_assert!(
            len <= cap,
            "aster_map_set: len ({}) exceeds cap ({}) before grow check",
            len,
            cap
        );
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
            debug_assert!(
                !new_block.is_null(),
                "aster_map_set: gc_alloc_data_block returned null on grow"
            );
            debug_assert!(
                len <= new_cap,
                "aster_map_set: len exceeds new_cap after grow"
            );
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
#[unsafe(no_mangle)]
pub extern "C" fn aster_map_get(handle: *const u8, key: i64) -> i64 {
    if handle.is_null() {
        eprintln!("aster_map_get: null map handle");
        std::process::abort();
    }
    unsafe {
        let block = map_block(handle);
        let raw_len = *(block as *const i64);
        debug_assert!(
            raw_len >= 0,
            "aster_map_get: negative map length {}",
            raw_len
        );
        let len = raw_len as usize;
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
#[unsafe(no_mangle)]
pub extern "C" fn aster_map_has_key(handle: *const u8, key: i64) -> i64 {
    if handle.is_null() {
        eprintln!("aster_map_has_key: null map handle");
        std::process::abort();
    }
    unsafe {
        let block = map_block(handle);
        let raw_len = *(block as *const i64);
        debug_assert!(
            raw_len >= 0,
            "aster_map_has_key: negative map length {}",
            raw_len
        );
        let len = raw_len as usize;
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
