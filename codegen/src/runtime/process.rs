use super::error::aster_error_set;
use super::list::aster_list_get;
use super::string::{aster_string_new_from_rust, aster_string_to_rust};

/// Run a subprocess. Takes a command string and a List[String] of arguments.
/// Returns a pointer to a 3-field struct: [exit_code: i64, stdout: *mut u8, stderr: *mut u8].
/// Sets error flag if the command fails to spawn.
#[unsafe(no_mangle)]
pub extern "C" fn aster_process_run(cmd_ptr: *mut u8, args_list: *mut u8) -> *mut u8 {
    let cmd = unsafe { aster_string_to_rust(cmd_ptr) };

    // Read args from the list handle
    let len = unsafe {
        let block = *(args_list as *const *const u8);
        *(block as *const i64)
    };
    let mut args = Vec::with_capacity(len as usize);
    for i in 0..len {
        let str_ptr = aster_list_get(args_list, i) as *mut u8;
        let arg = unsafe { aster_string_to_rust(str_ptr) };
        args.push(arg);
    }

    let output = match std::process::Command::new(&cmd).args(&args).output() {
        Ok(o) => o,
        Err(_) => {
            aster_error_set();
            // Return a zeroed result struct (3 fields * 8 bytes)
            let result = super::alloc::aster_class_alloc(3 * 8);
            unsafe {
                let base = result as *mut i64;
                // exit_code = -1
                *base = -1;
                // stdout = empty string
                *base.add(1) = aster_string_new_from_rust("") as i64;
                // stderr = empty string
                *base.add(2) = aster_string_new_from_rust("") as i64;
            }
            return result;
        }
    };

    let exit_code = output.status.code().unwrap_or(-1) as i64;
    let stdout_str = String::from_utf8_lossy(&output.stdout);
    let stderr_str = String::from_utf8_lossy(&output.stderr);

    let result = super::alloc::aster_class_alloc(3 * 8);
    unsafe {
        let base = result as *mut i64;
        *base = exit_code;
        *base.add(1) = aster_string_new_from_rust(&stdout_str) as i64;
        *base.add(2) = aster_string_new_from_rust(&stderr_str) as i64;
    }
    result
}
