//! Process-global registry of host-compiled function pointers.
//!
//! When the host JIT compiles a module, it registers all compiled function
//! addresses here. When a nested JIT evaluation needs to call a host function,
//! it looks up the address from this registry.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

/// Wrapper around `*const u8` to allow Send + Sync for the global registry.
/// SAFETY: JIT-compiled function pointers are valid for the process lifetime
/// because the JITModule is kept alive in the caller's stack frame.
#[derive(Clone, Copy)]
struct FnPtr(*const u8);
unsafe impl Send for FnPtr {}
unsafe impl Sync for FnPtr {}

static HOST_FUNCTIONS: LazyLock<Mutex<HashMap<String, FnPtr>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Register a single host function pointer by name.
pub fn register(name: &str, ptr: *const u8) {
    HOST_FUNCTIONS
        .lock()
        .unwrap()
        .insert(name.to_string(), FnPtr(ptr));
}

/// Register multiple host function pointers at once.
pub fn register_batch(entries: impl Iterator<Item = (String, *const u8)>) {
    let mut table = HOST_FUNCTIONS.lock().unwrap();
    for (name, ptr) in entries {
        table.insert(name, FnPtr(ptr));
    }
}

/// Look up a host function pointer by name.
pub fn lookup(name: &str) -> Option<*const u8> {
    HOST_FUNCTIONS.lock().unwrap().get(name).map(|fp| fp.0)
}
