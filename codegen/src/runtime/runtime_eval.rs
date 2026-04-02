//! Runtime entry points for nested JIT evaluation.
//!
//! Two entry points:
//! - `aster_runtime_jit_eval`: 1-arg, used by `jit_run` (no context/env)
//! - `aster_runtime_eval_with_ctx`: 3-arg, used by `evaluate()` call sites
//!   with full context snapshot and captured env pointer.

use super::string::aster_string_to_rust;

/// Compile and execute an Aster source string from within JIT-compiled code.
/// Used by `jit_run` (no context or env).
///
/// # Safety
/// `code_ptr` must be a valid Aster heap string pointer (or null).
#[unsafe(no_mangle)]
pub extern "C" fn aster_runtime_jit_eval(code_ptr: *const u8) -> i64 {
    let source = unsafe { aster_string_to_rust(code_ptr) };
    match crate::eval_pipeline::jit_compile_and_run(&source, "<jit_eval>", None, None) {
        Ok(exit_code) => exit_code,
        Err(e) => {
            eprintln!("jit_run error: {e}");
            -1
        }
    }
}

/// Compile and execute with full context and env pointer.
/// Used by `evaluate()` call sites for context-aware runtime evaluation.
///
/// # Safety
/// - `code_ptr`: valid Aster heap string pointer (or null).
/// - `context_ptr`: valid Aster heap string with JSON `ContextSnapshot` (or null).
/// - `env_ptr`: heap-allocated env struct pointer (or 0/null).
#[unsafe(no_mangle)]
pub extern "C" fn aster_runtime_eval_with_ctx(
    code_ptr: *const u8,
    context_ptr: *const u8,
    env_ptr: i64,
) -> i64 {
    let source = unsafe { aster_string_to_rust(code_ptr) };

    let context = if context_ptr.is_null() {
        None
    } else {
        let json_str = unsafe { aster_string_to_rust(context_ptr) };
        match serde_json::from_str::<ast::ContextSnapshot>(&json_str) {
            Ok(snapshot) => Some(snapshot),
            Err(e) => {
                eprintln!("runtime eval error: failed to deserialize context: {e}");
                return -1;
            }
        }
    };

    let env = if env_ptr == 0 { None } else { Some(env_ptr) };

    match crate::eval_pipeline::jit_compile_and_run(&source, "<eval>", context.as_ref(), env) {
        Ok(exit_code) => exit_code,
        Err(e) => {
            eprintln!("runtime eval error: {e}");
            -1
        }
    }
}
