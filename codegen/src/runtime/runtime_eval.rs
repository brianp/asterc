//! Runtime entry points for nested JIT evaluation.
//!
//! Two entry points:
//! - `aster_runtime_jit_eval`: 1-arg, used by `jit_run` (no context/env)
//! - `aster_runtime_eval_with_ctx`: 3-arg, used by `evaluate()` call sites
//!   with full context snapshot and captured env pointer.

use super::string::aster_string_to_rust;
use crate::host_function_registry;

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
/// On success, returns 0 (evaluate is Void).
/// On failure, constructs an EvalError object, sets the typed error flag,
/// and returns 0. The caller's error-handling code (!, !.or(), !.catch())
/// checks the flag after the call.
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

    let mut context = if context_ptr.is_null() {
        None
    } else {
        let json_str = unsafe { aster_string_to_rust(context_ptr) };
        match serde_json::from_str::<ast::ContextSnapshot>(&json_str) {
            Ok(snapshot) => Some(snapshot),
            Err(e) => {
                set_eval_error("runtime", &format!("failed to deserialize context: {e}"));
                return 0;
            }
        }
    };

    // Populate function pointers from the global host function registry
    if let Some(ref mut snapshot) = context {
        populate_function_pointers(snapshot);
    }

    let env = if env_ptr == 0 { None } else { Some(env_ptr) };

    match crate::eval_pipeline::jit_compile_and_run(&source, "<eval>", context.as_ref(), env) {
        Ok(exit_code) => exit_code,
        Err(e) => {
            set_eval_error(e.kind, &e.message);
            0
        }
    }
}

/// Construct a heap-allocated EvalError object and set the typed error flag.
///
/// The EvalError layout matches the sentinel ClassId registration:
///   offset 0: kind (String pointer)
///   offset 8: message (String pointer)
fn set_eval_error(kind: &str, message: &str) {
    use super::alloc::aster_class_alloc_typed;
    use super::error::aster_error_set_typed;
    use super::string::aster_string_new_from_rust;
    use ast::eval_error::{EVAL_ERROR_CLASS_ID, EVAL_ERROR_PTR_COUNT, EVAL_ERROR_SIZE};

    let obj = aster_class_alloc_typed(EVAL_ERROR_SIZE, EVAL_ERROR_PTR_COUNT);
    unsafe {
        let base = obj as *mut i64;
        *base = aster_string_new_from_rust(kind) as i64;
        *base.add(1) = aster_string_new_from_rust(message) as i64;
    }
    aster_error_set_typed(EVAL_ERROR_CLASS_ID as i64, obj as i64);
}

/// Resolve host function pointers from the global registry into the snapshot.
/// Only resolves class methods (qualified as "ClassName.method_name").
/// Standalone functions from snapshot.functions are not resolved here to
/// avoid conflicts with eval module declarations (e.g. "main").
fn populate_function_pointers(snapshot: &mut ast::ContextSnapshot) {
    if let Some(class_name) = &snapshot.current_class
        && let Some(ci) = &snapshot.class_info
    {
        for method_name in ci.methods.keys() {
            let qualified = format!("{}.{}", class_name, method_name);
            if let Some(ptr) = host_function_registry::lookup(&qualified) {
                snapshot.function_pointers.insert(qualified, ptr as u64);
            }
        }
    }
}
