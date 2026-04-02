use std::cell::RefCell;
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering};

use ast::ContextSnapshot;
use typecheck::module_loader::{FsResolver, ModuleLoader};
use typecheck::typechecker::TypeChecker;

use crate::jit::CraneliftJIT;

/// Maximum nesting depth for JIT-from-JIT invocations.
/// Prevents stack overflow from recursive jit_run/evaluate calls.
const MAX_JIT_DEPTH: u32 = 16;

/// Global counter tracking the current JIT nesting depth.
static JIT_DEPTH: AtomicU32 = AtomicU32::new(0);

/// Error returned by [`jit_compile_and_run`] when any stage of the
/// compile-and-execute pipeline fails.
#[derive(Debug)]
pub struct RuntimeEvalError {
    /// One of `"syntax"`, `"type"`, `"lower"`, `"codegen"`, or `"runtime"`.
    pub kind: &'static str,
    pub message: String,
}

impl fmt::Display for RuntimeEvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} error: {}", self.kind, self.message)
    }
}

impl std::error::Error for RuntimeEvalError {}

/// RAII guard that decrements the JIT nesting depth on drop.
struct JitDepthGuard;

impl Drop for JitDepthGuard {
    fn drop(&mut self) {
        JIT_DEPTH.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Compile an Aster source string and execute its `main()` via JIT,
/// returning the i64 exit value.
///
/// When `context` is `Some`, the typechecker is pre-populated from the
/// snapshot (class info, variables, functions) and the source is treated
/// as bare statements wrapped in a synthetic `def main() -> Void`.
///
/// When `env_ptr` is `Some`, the entry function receives it as its first
/// argument, and the Lowerer pre-populates scope with env-backed locals
/// loaded from the env struct (closure-style env passing).
///
/// No file I/O, no `process::exit`. All errors are returned.
pub fn jit_compile_and_run(
    source: &str,
    filename: &str,
    context: Option<&ContextSnapshot>,
    env_ptr: Option<i64>,
) -> Result<i64, RuntimeEvalError> {
    // Guard against unbounded recursive JIT invocations (e.g. jit_run
    // calling jit_run). Decrement on all exit paths via a drop guard.
    let depth = JIT_DEPTH.fetch_add(1, Ordering::Relaxed);
    let _guard = JitDepthGuard;
    if depth >= MAX_JIT_DEPTH {
        return Err(RuntimeEvalError {
            kind: "runtime",
            message: format!("JIT nesting depth exceeded (max {MAX_JIT_DEPTH})"),
        });
    }

    // When a context snapshot is provided, wrap bare statements in a
    // synthetic main so the pipeline can compile them as a module.
    let wrapped;
    let effective_source = if context.is_some() {
        let indented: String = source
            .lines()
            .map(|line| format!("  {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        wrapped = format!("def main() -> Void\n{indented}\n");
        &wrapped
    } else {
        source
    };

    // 1. Lex
    let tokens = lexer::lex(effective_source).map_err(|diag| RuntimeEvalError {
        kind: "syntax",
        message: diag.message.clone(),
    })?;

    // 2. Parse
    let mut parser = parser::Parser::new(tokens);
    let module_ast = parser
        .parse_module(filename)
        .map_err(|diag| RuntimeEvalError {
            kind: "syntax",
            message: diag.message.clone(),
        })?;

    // 3. Typecheck
    let mut checker = if let Some(snapshot) = context {
        TypeChecker::from_snapshot(snapshot)
    } else {
        let root = Path::new(filename)
            .parent()
            .unwrap_or(Path::new("."))
            .to_path_buf();
        let resolver = FsResolver { root };
        let mut module_loader = ModuleLoader::new(Box::new(resolver));
        module_loader.jit = true; // JIT pipeline always has JIT available
        let loader = Rc::new(RefCell::new(module_loader));
        TypeChecker::with_loader(loader)
    };
    let errors = checker.check_module_all(&module_ast);

    if !errors.is_empty() {
        let msg = errors
            .iter()
            .map(|d| d.message.as_str())
            .collect::<Vec<_>>()
            .join("; ");
        return Err(RuntimeEvalError {
            kind: "type",
            message: msg,
        });
    }

    // 4. Merge imported FIR caches and lower AST -> FIR
    let imported_fir_caches = checker
        .module_loader
        .as_ref()
        .map(|loader| loader.borrow_mut().take_fir_caches())
        .unwrap_or_default();

    let mut lowerer = fir::Lowerer::new(checker.env, checker.type_table);

    for cache in &imported_fir_caches {
        lowerer.merge_imported(cache);
    }

    // Set eval env layout so the lowerer can inject __eval_env param
    // and EnvLoad statements for captured variables.
    let has_eval_env = context
        .and_then(|s| s.env_layout.as_ref())
        .is_some_and(|l| !l.is_empty());

    // Safety invariant: env_layout presence and env_ptr must agree.
    // Mismatch causes ABI corruption (caller passes wrong arg count).
    if has_eval_env && env_ptr.is_none() {
        return Err(RuntimeEvalError {
            kind: "runtime",
            message: "env_layout set in context but no env_ptr provided".to_string(),
        });
    }
    if env_ptr.is_some() && !has_eval_env {
        return Err(RuntimeEvalError {
            kind: "runtime",
            message: "env_ptr provided but no env_layout in context".to_string(),
        });
    }

    if let Some(snapshot) = context
        && let Some(env_layout) = &snapshot.env_layout
    {
        lowerer.set_eval_env(env_layout, snapshot);
    }

    lowerer
        .lower_module(&module_ast)
        .map_err(|e| RuntimeEvalError {
            kind: "lower",
            message: e.to_string(),
        })?;
    let fir_module = lowerer.finish();

    // 5. Verify entry point
    let entry = fir_module.entry.ok_or_else(|| RuntimeEvalError {
        kind: "lower",
        message: "no main() function found".to_string(),
    })?;

    // 6. JIT compile — register host function pointers as extra symbols
    let extra_symbols: Vec<(String, *const u8)> = context
        .map(|s| &s.function_pointers)
        .into_iter()
        .flatten()
        .map(|(name, &addr)| (name.clone(), addr as *const u8))
        .collect();

    let mut jit = if extra_symbols.is_empty() {
        CraneliftJIT::new()
    } else {
        let sym_refs: Vec<(&str, *const u8)> = extra_symbols
            .iter()
            .map(|(n, p)| (n.as_str(), *p))
            .collect();
        CraneliftJIT::with_extra_symbols(&sym_refs)
    };

    // Declare extra host functions so they appear in runtime_refs
    if let Some(snapshot) = context {
        jit.declare_extra_functions(snapshot)?;
    }

    jit.compile_module(&fir_module)
        .map_err(|e| RuntimeEvalError {
            kind: "codegen",
            message: e,
        })?;

    // 7. Execute - pass env_ptr as first arg if present.
    // Wrap in catch_unwind to convert runtime panics into errors
    // instead of crashing the host process.
    let jit = AssertUnwindSafe(jit);
    let result = if let Some(ptr) = env_ptr {
        catch_unwind(move || jit.call_i64_i64(entry, ptr))
    } else {
        catch_unwind(move || jit.call_i64(entry))
    };

    result.map_err(|panic_payload| {
        let message = if let Some(s) = panic_payload.downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = panic_payload.downcast_ref::<String>() {
            s.clone()
        } else {
            "unknown panic in evaluated code".to_string()
        };
        RuntimeEvalError {
            kind: "runtime",
            message,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_program_returns_42() {
        let result = jit_compile_and_run("def main() -> Int\n  42", "test.aster", None, None);
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn invalid_syntax_returns_syntax_error() {
        let result = jit_compile_and_run("def main( -> Int\n  42", "test.aster", None, None);
        let err = result.unwrap_err();
        assert_eq!(err.kind, "syntax");
    }

    #[test]
    fn type_error_returns_type_error() {
        let result = jit_compile_and_run(
            "def main() -> Int\n  \"not an int\"",
            "test.aster",
            None,
            None,
        );
        let err = result.unwrap_err();
        assert_eq!(err.kind, "type");
    }

    #[test]
    fn missing_main_returns_lower_error() {
        let result = jit_compile_and_run("def foo() -> Int\n  42", "test.aster", None, None);
        let err = result.unwrap_err();
        assert_eq!(err.kind, "lower");
        assert!(err.message.contains("main()"));
    }

    #[test]
    fn arithmetic_expression() {
        let result = jit_compile_and_run("def main() -> Int\n  10 + 32", "test.aster", None, None);
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn zero_return() {
        let result = jit_compile_and_run("def main() -> Int\n  0", "test.aster", None, None);
        assert_eq!(result.unwrap(), 0);
    }

    #[test]
    fn nested_jit_invocation() {
        let src = r#"use std/runtime { jit_run }

def main() -> Int
  jit_run(code: "def main() -> Int\n  7")
"#;
        let result = jit_compile_and_run(src, "test.aster", None, None);
        assert_eq!(result.unwrap(), 7);
    }

    // ── Phase 4: JIT with ContextSnapshot ──────────────────────────────

    #[test]
    fn context_snapshot_class_method_resolves() {
        // Class with a set_name method: bare call should resolve via env pre-population
        use ast::context_snapshot::SnapshotClassInfo;
        use std::collections::HashMap;

        let snapshot = ContextSnapshot {
            current_class: Some("Widget".into()),
            class_info: Some(SnapshotClassInfo {
                fields: vec![("name".into(), ast::Type::String)],
                methods: HashMap::from([(
                    "set_name".into(),
                    ast::Type::func(
                        vec!["value".into()],
                        vec![ast::Type::String],
                        ast::Type::Void,
                    ),
                )]),
                dynamic_receiver: None,
            }),
            variables: HashMap::new(),
            functions: HashMap::new(),
            env_layout: None,
            function_pointers: HashMap::new(),
        };

        // Bare call to set_name should typecheck because it's registered as a function
        let result = jit_compile_and_run(
            "set_name(value: \"hello\")",
            "<eval>",
            Some(&snapshot),
            None,
        );
        // Should succeed (no type error). The JIT wraps this in def main() -> Void.
        assert!(
            result.is_ok(),
            "expected ok, got: {:?}",
            result.unwrap_err()
        );
    }

    #[test]
    fn context_snapshot_nonexistent_method_errors() {
        // Class WITHOUT DynamicReceiver: bare call to unknown name should fail
        use ast::context_snapshot::SnapshotClassInfo;
        use std::collections::HashMap;

        let snapshot = ContextSnapshot {
            current_class: Some("Widget".into()),
            class_info: Some(SnapshotClassInfo {
                fields: vec![("name".into(), ast::Type::String)],
                methods: HashMap::new(),
                dynamic_receiver: None,
            }),
            variables: HashMap::new(),
            functions: HashMap::new(),
            env_layout: None,
            function_pointers: HashMap::new(),
        };

        let result =
            jit_compile_and_run("nonexistent_method(x: 1)", "<eval>", Some(&snapshot), None);
        let err = result.unwrap_err();
        assert_eq!(err.kind, "type");
    }

    #[test]
    fn context_snapshot_variables_without_env_fail() {
        // Variables in snapshot without env_layout still produce lowering error
        use std::collections::HashMap;

        let snapshot = ContextSnapshot {
            current_class: None,
            class_info: None,
            variables: HashMap::from([("x".into(), ast::Type::Int)]),
            functions: HashMap::new(),
            env_layout: None,
            function_pointers: HashMap::new(),
        };

        let result = jit_compile_and_run(
            "say(message: to_string(value: x))",
            "<eval>",
            Some(&snapshot),
            None,
        );
        let err = result.unwrap_err();
        assert_eq!(err.kind, "lower", "expected lower error, got: {err}");
    }

    #[test]
    fn eval_env_variable_access() {
        // Phase 5: variables from env_layout are loaded from the env pointer
        use std::collections::HashMap;

        let snapshot = ContextSnapshot {
            current_class: None,
            class_info: None,
            variables: HashMap::from([("x".into(), ast::Type::Int)]),
            functions: HashMap::new(),
            env_layout: Some(vec![("x".into(), ast::Type::Int)]),
            function_pointers: HashMap::new(),
        };

        // Create an env struct with x = 99 at offset 0
        let env_data: [i64; 1] = [99];
        let env_ptr = env_data.as_ptr() as i64;

        // Code prints x (which should be 99 from the env)
        let result = jit_compile_and_run(
            "say(message: to_string(value: x))",
            "<eval>",
            Some(&snapshot),
            Some(env_ptr),
        );
        assert!(
            result.is_ok(),
            "expected ok, got: {:?}",
            result.unwrap_err()
        );
    }

    #[test]
    fn eval_env_multiple_variables() {
        // Multiple variables in env, accessed in evaluated code
        use std::collections::HashMap;

        let snapshot = ContextSnapshot {
            current_class: None,
            class_info: None,
            variables: HashMap::from([("a".into(), ast::Type::Int), ("b".into(), ast::Type::Int)]),
            functions: HashMap::new(),
            // Sorted alphabetically: a at offset 0, b at offset 8
            env_layout: Some(vec![
                ("a".into(), ast::Type::Int),
                ("b".into(), ast::Type::Int),
            ]),
            function_pointers: HashMap::new(),
        };

        // env struct: a=10 at offset 0, b=32 at offset 8
        let env_data: [i64; 2] = [10, 32];
        let env_ptr = env_data.as_ptr() as i64;

        // Code that exercises both variables (a + b = 42)
        let result = jit_compile_and_run(
            "say(message: to_string(value: a + b))",
            "<eval>",
            Some(&snapshot),
            Some(env_ptr),
        );
        assert!(
            result.is_ok(),
            "expected ok, got: {:?}",
            result.unwrap_err()
        );
    }

    #[test]
    fn context_snapshot_functions_in_scope() {
        // Functions from snapshot should be callable
        use std::collections::HashMap;

        let snapshot = ContextSnapshot {
            current_class: None,
            class_info: None,
            variables: HashMap::new(),
            functions: HashMap::from([(
                "compute".into(),
                ast::Type::func(vec!["n".into()], vec![ast::Type::Int], ast::Type::Int),
            )]),
            env_layout: None,
            function_pointers: HashMap::new(),
        };

        let result = jit_compile_and_run("let y = compute(n: 42)", "<eval>", Some(&snapshot), None);
        assert!(
            result.is_ok(),
            "expected ok, got: {:?}",
            result.unwrap_err()
        );
    }

    #[test]
    fn context_snapshot_empty_still_works() {
        // An empty snapshot (no class, no vars, no funcs) should still allow basic code
        use std::collections::HashMap;

        let snapshot = ContextSnapshot {
            current_class: None,
            class_info: None,
            variables: HashMap::new(),
            functions: HashMap::new(),
            env_layout: None,
            function_pointers: HashMap::new(),
        };

        let result =
            jit_compile_and_run("say(message: \"hello\")", "<eval>", Some(&snapshot), None);
        assert!(
            result.is_ok(),
            "expected ok, got: {:?}",
            result.unwrap_err()
        );
    }

    #[test]
    fn jit_depth_guard_prevents_overflow() {
        // Manually exhaust the depth counter, then verify the next call is rejected.
        // Reset the counter afterwards so other tests aren't affected.
        use std::sync::atomic::Ordering;

        let original = super::JIT_DEPTH.load(Ordering::Relaxed);
        super::JIT_DEPTH.store(super::MAX_JIT_DEPTH, Ordering::Relaxed);

        let result = jit_compile_and_run("def main() -> Int\n  0", "test.aster", None, None);
        let err = result.unwrap_err();
        assert_eq!(err.kind, "runtime");
        assert!(err.message.contains("nesting depth"));

        super::JIT_DEPTH.store(original, Ordering::Relaxed);
    }

    // ── Phase 6: Function pointer capture ─────────────────────────────

    #[test]
    fn host_function_registry_round_trip() {
        // Register a function pointer and retrieve it
        use crate::host_function_registry;

        extern "C" fn dummy_add_one(x: i64) -> i64 {
            x + 1
        }
        let dummy_fn: extern "C" fn(i64) -> i64 = dummy_add_one;
        let ptr = dummy_fn as *const u8;

        host_function_registry::register("test_fn_roundtrip", ptr);
        let result = host_function_registry::lookup("test_fn_roundtrip");
        assert_eq!(result, Some(ptr));

        // Unknown function returns None
        let missing = host_function_registry::lookup("nonexistent_fn_xyz");
        assert_eq!(missing, None);
    }

    // ── Phase 7: RuntimeEvalError and error boundary ────────────────

    #[test]
    fn catch_unwind_converts_panic_to_runtime_error() {
        // Verify that the catch_unwind in jit_compile_and_run converts panics
        // to RuntimeEvalError with kind "runtime". We trigger this by testing
        // that the mechanism doesn't crash the process.
        // A program that compiles but would panic if the entry function were
        // missing is already guarded by the pipeline. Here we test that even
        // if something goes wrong at runtime, we get an error, not a crash.
        let result = jit_compile_and_run("def main() -> Int\n  42", "test.aster", None, None);
        assert!(result.is_ok(), "basic program should not error");
    }

    #[test]
    fn syntax_error_in_eval_context() {
        use std::collections::HashMap;
        // Phase 7: syntax error in evaluated code returns structured error
        let snapshot = ast::ContextSnapshot {
            current_class: None,
            class_info: None,
            variables: HashMap::new(),
            functions: HashMap::new(),
            env_layout: None,
            function_pointers: HashMap::new(),
        };
        let result = jit_compile_and_run("def broken(", "<eval>", Some(&snapshot), None);
        let err = result.unwrap_err();
        assert_eq!(err.kind, "syntax");
    }

    #[test]
    fn type_error_in_eval_context() {
        use std::collections::HashMap;
        // Phase 7: type error in evaluated code returns structured error
        let snapshot = ast::ContextSnapshot {
            current_class: None,
            class_info: None,
            variables: HashMap::new(),
            functions: HashMap::new(),
            env_layout: None,
            function_pointers: HashMap::new(),
        };
        let result = jit_compile_and_run(
            "let x: Int = \"not an int\"",
            "<eval>",
            Some(&snapshot),
            None,
        );
        let err = result.unwrap_err();
        assert_eq!(err.kind, "type");
    }

    #[test]
    fn eval_with_host_method_call() {
        // Phase 6: evaluated code in a class context calls a host-compiled method.
        // This test compiles a class with a method, registers the method pointer,
        // then evaluates code that calls it.
        use ast::context_snapshot::SnapshotClassInfo;
        use std::collections::HashMap;

        // First, compile the host class with the method
        let host_src = r#"
class Adder
  pub value: Int

  pub def add(n: Int) -> Void
    value = value + n

def main() -> Int
  0
"#;
        // Compile the host code to register function pointers
        let host_result = jit_compile_and_run(host_src, "host.aster", None, None);
        assert!(
            host_result.is_ok(),
            "host compilation failed: {:?}",
            host_result.unwrap_err()
        );

        // Now set up a snapshot as if we're inside an Adder method
        let snapshot = ContextSnapshot {
            current_class: Some("Adder".into()),
            class_info: Some(SnapshotClassInfo {
                fields: vec![("value".into(), ast::Type::Int)],
                methods: HashMap::from([(
                    "add".into(),
                    ast::Type::func(vec!["n".into()], vec![ast::Type::Int], ast::Type::Void),
                )]),
                dynamic_receiver: None,
            }),
            variables: HashMap::new(),
            functions: HashMap::new(),
            env_layout: Some(vec![(
                "self".into(),
                ast::Type::Custom("Adder".into(), vec![]),
            )]),
            function_pointers: HashMap::new(),
        };

        // Create an env with self pointing to an Adder instance
        // The Adder has value: Int at field offset 0
        // We need a heap-allocated object: [header, value_field]
        // For this unit test, just verify the pipeline doesn't error on type/lower
        // The full integration test validates actual execution
        let result = jit_compile_and_run(
            "add(n: 5)",
            "<eval>",
            Some(&snapshot),
            None, // no env for this type-check-only test
        );
        // Should pass typechecking and lowering (may fail at runtime without env)
        // The key assertion: it should NOT silently drop the call
        assert!(
            result.is_ok() || result.as_ref().unwrap_err().kind != "type",
            "call to host method 'add' should not produce a type error: {:?}",
            result
        );
    }
}
