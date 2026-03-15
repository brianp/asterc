use std::fs;
use std::path::Path;

use ast::{Expr, Stmt};
use aster_fmt::{config::FormatConfig, format_source};
use lexer::{TokenKind, lex};
use parser::Parser;

fn read(path: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path))
        .unwrap_or_else(|err| panic!("failed to read {path}: {err}"))
}

#[test]
fn call_modes_stay_aligned_across_frontend_and_docs() {
    let source = "\
def child() -> Int
  42

def parent() -> Int
  blocking child()

def main() throws CancelledError -> Int
  let spawned = async child()
  detached async child()
  let joined = resolve spawned!
  joined + parent()
";

    let tokens = lex(source).expect("lex ok");
    assert!(tokens.iter().any(|token| token.kind == TokenKind::Async));
    assert!(tokens.iter().any(|token| token.kind == TokenKind::Blocking));
    assert!(tokens.iter().any(|token| token.kind == TokenKind::Resolve));
    assert!(tokens.iter().any(|token| token.kind == TokenKind::Detached));

    let mut parser = Parser::new(tokens);
    let module = parser.parse_module("test").expect("parse ok");
    let main = module
        .body
        .iter()
        .find_map(|stmt| match stmt {
            Stmt::Let { name, value, .. } if name == "main" => Some(value),
            _ => None,
        })
        .expect("main function");
    let Expr::Lambda { body, .. } = main else {
        panic!("main should lower from a lambda");
    };
    assert!(body.iter().any(|stmt| matches!(
        stmt,
        Stmt::Let {
            value: Expr::AsyncCall { .. },
            ..
        }
    )));
    assert!(
        body.iter()
            .any(|stmt| matches!(stmt, Stmt::Expr(Expr::DetachedCall { .. }, _)))
    );
    assert!(body.iter().any(|stmt| matches!(
        stmt,
        Stmt::Let {
            value: Expr::Propagate(inner, _),
            ..
        } if matches!(inner.as_ref(), Expr::Resolve { .. })
    )));

    let formatted = format_source(source, &FormatConfig::default()).expect("format ok");
    assert!(formatted.contains("blocking child()"));
    assert!(formatted.contains("async child()"));
    assert!(formatted.contains("detached async child()"));
    assert!(formatted.contains("resolve spawned!"));

    let typecheck_diagnostics = read("typecheck/src/check_call.rs");
    assert!(typecheck_diagnostics.contains("blocking {name}()"));
    assert!(typecheck_diagnostics.contains("async {name}()"));
    assert!(typecheck_diagnostics.contains("blocking f(...)"));
    assert!(typecheck_diagnostics.contains("async f(...)"));

    for path in [
        "docs/src/content/docs/concurrency/overview.mdx",
        "docs/src/content/docs/concurrency/async-blocking.mdx",
        "docs/design/async-rfc.md",
        "docs/src/content/docs/rfcs/async.mdx",
        "docs/src/content/docs/rfcs/full/async.mdx",
    ] {
        let doc = read(path);
        assert!(
            doc.contains("blocking f()"),
            "{path} should document blocking f()"
        );
        assert!(
            !doc.contains("resolve f()"),
            "{path} still documents the removed resolve f() form"
        );
        assert!(
            doc.contains("resolve task"),
            "{path} should reserve resolve for Task[T] handles"
        );
    }
}
