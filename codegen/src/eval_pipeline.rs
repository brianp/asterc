use std::cell::RefCell;
use std::fmt;
use std::path::Path;
use std::rc::Rc;

use typecheck::module_loader::{FsResolver, ModuleLoader};
use typecheck::typechecker::TypeChecker;

use crate::jit::CraneliftJIT;

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

/// Compile an Aster source string and execute its `main()` via JIT,
/// returning the i64 exit value.
///
/// No file I/O, no `process::exit`. All errors are returned.
pub fn jit_compile_and_run(source: &str, filename: &str) -> Result<i64, RuntimeEvalError> {
    // 1. Lex
    let tokens = lexer::lex(source).map_err(|diag| RuntimeEvalError {
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
    let root = Path::new(filename)
        .parent()
        .unwrap_or(Path::new("."))
        .to_path_buf();
    let resolver = FsResolver { root };
    let module_loader = ModuleLoader::new(Box::new(resolver));
    let loader = Rc::new(RefCell::new(module_loader));
    let mut checker = TypeChecker::with_loader(loader);
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
        let result = jit_compile_and_run("def main() -> Int\n  42", "test.aster");
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn invalid_syntax_returns_syntax_error() {
        let result = jit_compile_and_run("def main( -> Int\n  42", "test.aster");
        let err = result.unwrap_err();
        assert_eq!(err.kind, "syntax");
    }

    #[test]
    fn type_error_returns_type_error() {
        let result = jit_compile_and_run("def main() -> Int\n  \"not an int\"", "test.aster");
        let err = result.unwrap_err();
        assert_eq!(err.kind, "type");
    }

    #[test]
    fn missing_main_returns_lower_error() {
        let result = jit_compile_and_run("def foo() -> Int\n  42", "test.aster");
        let err = result.unwrap_err();
        assert_eq!(err.kind, "lower");
        assert!(err.message.contains("main()"));
    }

    #[test]
    fn arithmetic_expression() {
        let result = jit_compile_and_run("def main() -> Int\n  10 + 32", "test.aster");
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn zero_return() {
        let result = jit_compile_and_run("def main() -> Int\n  0", "test.aster");
        assert_eq!(result.unwrap(), 0);
    }
}
