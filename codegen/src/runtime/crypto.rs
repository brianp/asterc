use sha2::{Digest, Sha256};

use super::string::{aster_string_new_from_rust, aster_string_to_rust};

/// Compute the SHA-256 hex digest of a string.
#[unsafe(no_mangle)]
pub extern "C" fn aster_crypto_sha256(data_ptr: *mut u8) -> *mut u8 {
    let data = unsafe { aster_string_to_rust(data_ptr) };
    let hash = Sha256::digest(data.as_bytes());
    let mut hex = String::with_capacity(64);
    for byte in hash {
        use std::fmt::Write;
        let _ = write!(hex, "{byte:02x}");
    }
    aster_string_new_from_rust(&hex)
}
