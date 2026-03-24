mod common;

use ast::{Expr, Stmt};
use aster_fmt::{config::FormatConfig, format_source};
use lexer::{TokenKind, lex};
use parser::Parser;

// ─── Call-site async: basic semantics ───────────────────────────────

#[test]
fn async_call_returns_task() {
    // BC-9: functions are plain def, async at call site returns Task[T]
    common::check_ok(
        r#"def fetch() -> Int
  42

def main() -> Task[Int]
  let t: Task[Int] = async fetch()
  t
"#,
    );
}

#[test]
fn blocking_call_returns_plain_value() {
    common::check_ok(
        r#"def fetch() -> Int
  async fetch_child()
  42

def fetch_child() -> Int
  42

def main() -> Int
  blocking fetch()
"#,
    );
}

#[test]
fn plain_call_to_suspendable_callee_is_compile_error() {
    let err = common::check_err(
        r#"def fetch() -> Int
  async fetch_child()
  42

def fetch_child() -> Int
  7

def main() -> Int
  fetch()
"#,
    );
    assert!(err.contains("blocking fetch()") || err.contains("async fetch()"));
}

// ─── Cross-module suspendability metadata ───────────────────────────

#[test]
fn cross_module_suspendable_metadata_rejects_plain_call() {
    let mut files = std::collections::HashMap::new();
    files.insert(
        "worker".to_string(),
        r#"pub def fetch_child() -> Int
  7

pub def fetch() -> Int
  async fetch_child()
  42
"#
        .to_string(),
    );
    let err = common::check_err_with_files(
        r#"use worker { fetch }

def main() -> Int
  fetch()
"#,
        files,
    );
    assert!(err.contains("blocking fetch()") || err.contains("async fetch()"));
}

#[test]
fn cross_module_nested_suspendability_metadata_rejects_plain_call() {
    let mut files = std::collections::HashMap::new();
    files.insert(
        "worker".to_string(),
        r#"pub def fetch_child() -> Int
  7

pub def fetch() -> Int
  if true
    async fetch_child()
  42
"#
        .to_string(),
    );
    let err = common::check_err_with_files(
        r#"use worker { fetch }

def main() -> Int
  fetch()
"#,
        files,
    );
    assert!(err.contains("blocking fetch()") || err.contains("async fetch()"));
}

// ─── Detached async ─────────────────────────────────────────────────

#[test]
fn detached_async_call() {
    common::check_ok(
        r#"def background_job() -> Void
  log(message: "working")

def main() -> Void
  detached async background_job()
"#,
    );
}

#[test]
fn detached_without_async_error() {
    let err = common::check_parse_err(
        r#"def fetch() -> Int
  42

def main() -> Void
  detached fetch()
"#,
    );
    assert!(err.contains("detached") || err.contains("async"));
}

#[test]
fn detached_async_throwing() {
    // detached async on throwing functions — errors logged at runtime
    common::check_ok(
        r#"class AppError extends Error
  code: Int

def risky() throws AppError -> Void
  log(message: "working")

def main() -> Void
  detached async risky()
"#,
    );
}

// ─── Async in any context (no restriction) ──────────────────────────

#[test]
fn async_call_works_anywhere() {
    common::check_ok(
        r#"def fetch() -> Int
  42

def main() -> Task[Int]
  let t = async fetch()
  t
"#,
    );
}

// ─── Task type annotation ───────────────────────────────────────────

#[test]
fn task_type_annotation() {
    common::check_ok(
        r#"def fetch() -> Int
  42

def main() -> Task[Int]
  let t: Task[Int] = async fetch()
  t
"#,
    );
}

// ─── Resolve semantics ─────────────────────────────────────────────

#[test]
fn resolve_without_bang_error() {
    let err = common::check_err(
        r#"def fetch() -> Int
  42

def main() -> Void
  let t = async fetch()
  resolve t
"#,
    );
    assert!(err.contains("resolve") || err.contains("!") || err.contains("CancelledError"));
}

#[test]
fn resolve_without_bang_still_errors() {
    // resolve without ! is still an error — ! is the propagation operator
    let err = common::check_err(
        r#"def compute() -> Int
  42

def main() -> Int
  let t = async compute()
  resolve t
"#,
    );
    assert!(err.contains("resolve") || err.contains("!"));
}

#[test]
fn resolve_member_access() {
    // resolve works on member access expressions
    common::check_ok(
        r#"class TaskHolder
  task: Task[Int]

def fetch() -> Int
  42

def main() -> Void
  let holder = TaskHolder(task: async fetch())
  let v = resolve holder.task!
"#,
    );
}

#[test]
fn resolve_index_access() {
    // resolve works on index expressions
    common::check_ok(
        r#"def fetch() -> Int
  42

def main() -> Void
  let tasks: List[Task[Int]] = [async fetch()]
  let v = resolve tasks[0]!
"#,
    );
}

// ─── Async + throws composition ─────────────────────────────────────

#[test]
fn async_throwing_function() {
    // async on a throwing function should work — errors handled at resolve
    common::check_ok(
        r#"class AppError extends Error
  code: Int

def risky() throws AppError -> String
  "data"

def main() throws Error -> Void
  let task = async risky()
  let result = resolve task!
"#,
    );
}

#[test]
fn async_throwing_with_catch() {
    // async throwing + resolve with catch
    common::check_ok(
        r#"class AppError extends Error
  code: Int

def risky() throws AppError -> String
  "data"

def main() -> String
  let task = async risky()
  resolve task!.catch
    CancelledError e -> "cancelled"
    AppError e -> e.message
    _ -> "unknown"
"#,
    );
}

#[test]
fn async_throwing_with_or() {
    // async throwing + resolve with or
    common::check_ok(
        r#"class AppError extends Error
  code: Int

def risky() throws AppError -> String
  "data"

def main() -> String
  let task = async risky()
  resolve task!.or("fallback")
"#,
    );
}

// ─── CancelledError implicit in task resolution ─────────────────────

#[test]
fn resolve_task_without_throws_cancelled_error() {
    // resolve task! should work without declaring throws CancelledError
    common::check_ok(
        r#"def compute() -> Int
  42

def main() -> Int
  let t = async compute()
  resolve t!
"#,
    );
}

#[test]
fn resolve_task_without_any_throws_declaration() {
    // Function that only resolves tasks needs no throws at all
    common::check_ok(
        r#"def work_a() -> Int
  10

def work_b() -> Int
  20

def main() -> Int
  let a = async work_a()
  let b = async work_b()
  resolve a! + resolve b!
"#,
    );
}

#[test]
fn explicit_throw_cancelled_error_requires_throws() {
    // If the user explicitly throws CancelledError, they must declare it
    let err = common::check_err(
        r#"def main() -> Void
  throw CancelledError(message: "abort")
"#,
    );
    assert!(err.contains("throws") || err.contains("E013"));
}

// ─── async def is a parse error ─────────────────────────────────────

#[test]
fn async_def_is_parse_error() {
    let err = common::check_parse_err(
        r#"async def fetch() -> Int
  42
"#,
    );
    assert!(err.contains("async def is not supported"));
}

// ─── Frontend consistency: tokens, AST, and formatter agree ─────────

#[test]
fn call_modes_stay_aligned_across_frontend() {
    let source = "\
def child() -> Int
  42

def parent() -> Int
  blocking child()

def main() -> Int
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

// ─── Nullable Task type ─────────────────────────────────────────────

#[test]
fn task_nullable_type_parses() {
    common::check_ok(
        r#"def get_task() -> Task[Int]?
  nil
"#,
    );
}
