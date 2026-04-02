//! Runtime entry point for nested JIT evaluation.
//!
//! `aster_runtime_jit_eval` is called from JIT-compiled code via the
//! `std/runtime { jit_run }` stdlib import. It takes an Aster heap
//! string pointer containing source code, compiles and executes it
//! through the full JIT pipeline, and returns the i64 exit value.

use super::string::aster_string_to_rust;

/// Compile and execute an Aster source string from within JIT-compiled code.
///
/// # Safety
/// `code_ptr` must be a valid Aster heap string pointer (or null).
#[unsafe(no_mangle)]
pub extern "C" fn aster_runtime_jit_eval(code_ptr: *const u8) -> i64 {
    let source = unsafe { aster_string_to_rust(code_ptr) };
    match crate::eval_pipeline::jit_compile_and_run(&source, "<jit_eval>", None) {
        Ok(exit_code) => exit_code,
        Err(e) => {
            eprintln!("jit_run error: {e}");
            -1
        }
    }
}
