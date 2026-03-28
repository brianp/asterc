use super::list::{aster_list_new, aster_list_push};
use super::string::{aster_string_new_from_rust, aster_string_to_rust};

/// Return the command-line arguments as a List[String].
#[unsafe(no_mangle)]
pub extern "C" fn aster_sys_args() -> *mut u8 {
    let args: Vec<std::string::String> = std::env::args().collect();
    // ptr_elems = 1 because elements are heap string pointers
    let list = aster_list_new(args.len().max(4) as i64, 1);
    for arg in &args {
        let s = aster_string_new_from_rust(arg);
        aster_list_push(list, s as i64);
    }
    list
}

/// Get an environment variable by key. Returns the value as a String pointer,
/// or a nil-tagged pointer (0) if the variable is not set.
#[unsafe(no_mangle)]
pub extern "C" fn aster_sys_env_get(key_ptr: *mut u8) -> *mut u8 {
    let key = unsafe { aster_string_to_rust(key_ptr) };
    match std::env::var(&key) {
        Ok(val) => aster_string_new_from_rust(&val),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Set an environment variable.
#[unsafe(no_mangle)]
pub extern "C" fn aster_sys_env_set(key_ptr: *mut u8, value_ptr: *mut u8) {
    let key = unsafe { aster_string_to_rust(key_ptr) };
    let value = unsafe { aster_string_to_rust(value_ptr) };
    unsafe { std::env::set_var(&key, &value) };
}

/// Exit the process with the given exit code.
#[unsafe(no_mangle)]
pub extern "C" fn aster_sys_exit(code: i64) {
    std::process::exit(code as i32);
}
