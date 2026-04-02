use std::cell::RefCell;
use std::fmt;
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
/// No file I/O, no `process::exit`. All errors are returned.
pub fn jit_compile_and_run(
    source: &str,
    filename: &str,
    context: Option<&ContextSnapshot>,
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
        let module_loader = ModuleLoader::new(Box::new(resolver));
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

    // 6. JIT compile
    let mut jit = CraneliftJIT::new();
    jit.compile_module(&fir_module)
        .map_err(|e| RuntimeEvalError {
            kind: "codegen",
            message: e,
        })?;

    // 7. Execute
    Ok(jit.call_i64(entry))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_program_returns_42() {
        let result = jit_compile_and_run("def main() -> Int\n  42", "test.aster", None);
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn invalid_syntax_returns_syntax_error() {
        let result = jit_compile_and_run("def main( -> Int\n  42", "test.aster", None);
        let err = result.unwrap_err();
        assert_eq!(err.kind, "syntax");
    }

    #[test]
    fn type_error_returns_type_error() {
        let result = jit_compile_and_run("def main() -> Int\n  \"not an int\"", "test.aster", None);
        let err = result.unwrap_err();
        assert_eq!(err.kind, "type");
    }

    #[test]
    fn missing_main_returns_lower_error() {
        let result = jit_compile_and_run("def foo() -> Int\n  42", "test.aster", None);
        let err = result.unwrap_err();
        assert_eq!(err.kind, "lower");
        assert!(err.message.contains("main()"));
    }

    #[test]
    fn arithmetic_expression() {
        let result = jit_compile_and_run("def main() -> Int\n  10 + 32", "test.aster", None);
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn zero_return() {
        let result = jit_compile_and_run("def main() -> Int\n  0", "test.aster", None);
        assert_eq!(result.unwrap(), 0);
    }

    #[test]
    fn nested_jit_invocation() {
        let src = r#"use std/runtime { jit_run }

def main() -> Int
  jit_run(code: "def main() -> Int\n  7")
"#;
        let result = jit_compile_and_run(src, "test.aster", None);
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
        };

        // Bare call to set_name should typecheck because it's registered as a function
        let result = jit_compile_and_run("set_name(value: \"hello\")", "<eval>", Some(&snapshot));
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
        };

        let result = jit_compile_and_run("nonexistent_method(x: 1)", "<eval>", Some(&snapshot));
        let err = result.unwrap_err();
        assert_eq!(err.kind, "type");
    }

    #[test]
    fn context_snapshot_variables_typecheck() {
        // Variables from snapshot should pass typechecking.
        // FIR lowering will fail ("unbound variable") because Phase 5
        // (runtime value passing) hasn't wired env pointers yet.
        use std::collections::HashMap;

        let snapshot = ContextSnapshot {
            current_class: None,
            class_info: None,
            variables: HashMap::from([("x".into(), ast::Type::Int)]),
            functions: HashMap::new(),
        };

        // Code references the pre-populated variable x
        let result = jit_compile_and_run(
            "say(message: to_string(value: x))",
            "<eval>",
            Some(&snapshot),
        );
        // Typechecking passes but lowering fails (expected until Phase 5)
        let err = result.unwrap_err();
        assert_eq!(err.kind, "lower", "expected lower error, got: {err}");
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
        };

        let result = jit_compile_and_run("let y = compute(n: 42)", "<eval>", Some(&snapshot));
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
        };

        let result = jit_compile_and_run("say(message: \"hello\")", "<eval>", Some(&snapshot));
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

        let result = jit_compile_and_run("def main() -> Int\n  0", "test.aster", None);
        let err = result.unwrap_err();
        assert_eq!(err.kind, "runtime");
        assert!(err.message.contains("nesting depth"));

        super::JIT_DEPTH.store(original, Ordering::Relaxed);
    }
}
