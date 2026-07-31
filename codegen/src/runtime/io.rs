use crate::green::scheduler;

use super::error::set_message_error;
use super::string::{aster_string_new_from_rust, aster_string_to_rust};
use ast::builtin_errors::IO_ERROR_CLASS_ID;

/// Set a typed `IOError` carrying the underlying std::io error text.
fn set_io_error(err: &std::io::Error) {
    set_message_error(IO_ERROR_CLASS_ID, &err.to_string());
}

// ---------------------------------------------------------------------------
// I/O suspension hooks — Phase 5
// ---------------------------------------------------------------------------

/// Suspend the current green thread until `fd` is readable.
/// Internal plumbing — not exposed to Aster code yet.
#[unsafe(no_mangle)]
pub extern "C" fn aster_io_wait_read(fd: i32) {
    scheduler::io_wait_readable(fd);
}

/// Suspend the current green thread until `fd` is writable.
#[unsafe(no_mangle)]
pub extern "C" fn aster_io_wait_write(fd: i32) {
    scheduler::io_wait_writable(fd);
}

/// Submit a blocking operation (entry(arg)) to the blocking thread pool.
/// The current green thread suspends until the operation completes.
#[unsafe(no_mangle)]
pub extern "C" fn aster_blocking_submit(entry: extern "C" fn(i64) -> i64, arg: i64) {
    scheduler::blocking_submit(Box::new(move || entry(arg)));
}

// --- File I/O ---

/// Maximum file size `file_read` will load (256 MB).
const FILE_READ_MAX_SIZE: u64 = 256 * 1024 * 1024;

/// Read a file's contents as a string. Sets a typed IOError on failure or if
/// the file exceeds `FILE_READ_MAX_SIZE`.
#[unsafe(no_mangle)]
pub extern "C" fn aster_file_read(path_ptr: *mut u8) -> *mut u8 {
    let path = unsafe { aster_string_to_rust(path_ptr) };
    let meta = match std::fs::metadata(&path) {
        Ok(m) => m,
        Err(e) => {
            set_io_error(&e);
            return aster_string_new_from_rust("");
        }
    };
    if meta.len() > FILE_READ_MAX_SIZE {
        set_message_error(
            IO_ERROR_CLASS_ID,
            &format!(
                "file '{path}' is {} bytes, exceeds the {FILE_READ_MAX_SIZE}-byte read limit",
                meta.len()
            ),
        );
        return aster_string_new_from_rust("");
    }
    match std::fs::read_to_string(&path) {
        Ok(content) => aster_string_new_from_rust(&content),
        Err(e) => {
            set_io_error(&e);
            aster_string_new_from_rust("")
        }
    }
}

/// Write content to a file (creates or truncates). Sets a typed IOError on failure.
#[unsafe(no_mangle)]
pub extern "C" fn aster_file_write(path_ptr: *mut u8, content_ptr: *mut u8) {
    let path = unsafe { aster_string_to_rust(path_ptr) };
    let content = unsafe { aster_string_to_rust(content_ptr) };
    if let Err(e) = std::fs::write(&path, &content) {
        set_io_error(&e);
    }
}

/// Append content to a file (creates if missing). Sets a typed IOError on failure.
#[unsafe(no_mangle)]
pub extern "C" fn aster_file_append(path_ptr: *mut u8, content_ptr: *mut u8) {
    use std::io::Write;
    let path = unsafe { aster_string_to_rust(path_ptr) };
    let content = unsafe { aster_string_to_rust(content_ptr) };
    let result = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| f.write_all(content.as_bytes()));
    if let Err(e) = result {
        set_io_error(&e);
    }
}
