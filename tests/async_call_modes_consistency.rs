use ast::{Expr, Stmt};
use aster_fmt::{config::FormatConfig, format_source};
use lexer::{TokenKind, lex};
use parser::Parser;

#[test]
fn call_modes_stay_aligned_across_frontend() {
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
}
