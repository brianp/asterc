use super::error::aster_error_set;
use super::list::{aster_list_new, aster_list_push};
use super::string::{aster_string_new_from_rust, aster_string_to_rust};

/// Maximum file size for read_file (256 MB).
const FILE_READ_MAX_SIZE: u64 = 256 * 1024 * 1024;

/// Read a file's contents as a string. Sets error flag on failure.
#[unsafe(no_mangle)]
pub extern "C" fn aster_fs_read_file(path_ptr: *mut u8) -> *mut u8 {
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
#[unsafe(no_mangle)]
pub extern "C" fn aster_fs_write_file(path_ptr: *mut u8, content_ptr: *mut u8) {
    let path = unsafe { aster_string_to_rust(path_ptr) };
    let content = unsafe { aster_string_to_rust(content_ptr) };
    if std::fs::write(&path, &content).is_err() {
        aster_error_set();
    }
}

/// Append content to a file (creates if missing). Sets error flag on failure.
#[unsafe(no_mangle)]
pub extern "C" fn aster_fs_append_file(path_ptr: *mut u8, content_ptr: *mut u8) {
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

/// Check whether a path exists. Returns 1 (true) or 0 (false).
#[unsafe(no_mangle)]
pub extern "C" fn aster_fs_exists(path_ptr: *mut u8) -> i8 {
    let path = unsafe { aster_string_to_rust(path_ptr) };
    std::path::Path::new(&path).exists() as i8
}

/// Check whether a path is a directory. Returns 1 (true) or 0 (false).
#[unsafe(no_mangle)]
pub extern "C" fn aster_fs_is_dir(path_ptr: *mut u8) -> i8 {
    let path = unsafe { aster_string_to_rust(path_ptr) };
    std::path::Path::new(&path).is_dir() as i8
}

/// Create a directory (and all parent directories). Sets error flag on failure.
#[unsafe(no_mangle)]
pub extern "C" fn aster_fs_mkdir(path_ptr: *mut u8) {
    let path = unsafe { aster_string_to_rust(path_ptr) };
    if std::fs::create_dir_all(&path).is_err() {
        aster_error_set();
    }
}

/// Remove a file or directory (recursive for directories). Sets error flag on failure.
#[unsafe(no_mangle)]
pub extern "C" fn aster_fs_remove(path_ptr: *mut u8) {
    let path = unsafe { aster_string_to_rust(path_ptr) };
    let p = std::path::Path::new(&path);
    let result = if p.is_dir() {
        std::fs::remove_dir_all(p)
    } else {
        std::fs::remove_file(p)
    };
    if result.is_err() {
        aster_error_set();
    }
}

/// List directory contents. Returns a List[String] of entry names.
/// Sets error flag on failure.
#[unsafe(no_mangle)]
pub extern "C" fn aster_fs_list_dir(path_ptr: *mut u8) -> *mut u8 {
    let path = unsafe { aster_string_to_rust(path_ptr) };
    let entries = match std::fs::read_dir(&path) {
        Ok(rd) => rd,
        Err(_) => {
            aster_error_set();
            return aster_list_new(4, 1);
        }
    };
    let list = aster_list_new(16, 1);
    for entry in entries {
        match entry {
            Ok(e) => {
                let name = e.file_name().to_string_lossy().to_string();
                let s = aster_string_new_from_rust(&name);
                aster_list_push(list, s as i64);
            }
            Err(_) => {
                // Skip entries we can't read
            }
        }
    }
    list
}

/// Copy a file from src to dst. Sets error flag on failure.
#[unsafe(no_mangle)]
pub extern "C" fn aster_fs_copy(src_ptr: *mut u8, dst_ptr: *mut u8) {
    let src = unsafe { aster_string_to_rust(src_ptr) };
    let dst = unsafe { aster_string_to_rust(dst_ptr) };
    if std::fs::copy(&src, &dst).is_err() {
        aster_error_set();
    }
}

/// Rename/move a file or directory from src to dst. Sets error flag on failure.
#[unsafe(no_mangle)]
pub extern "C" fn aster_fs_rename(src_ptr: *mut u8, dst_ptr: *mut u8) {
    let src = unsafe { aster_string_to_rust(src_ptr) };
    let dst = unsafe { aster_string_to_rust(dst_ptr) };
    if std::fs::rename(&src, &dst).is_err() {
        aster_error_set();
    }
}
