mod common;

// ═══════════════════════════════════════════════════════════════════════
// Regression tests for audit mitigations
// ═══════════════════════════════════════════════════════════════════════

// C1: BC-9 removed async context restrictions — async calls work anywhere
// This test verifies that async f() works from any context (no spoofing needed)
#[test]
fn async_call_works_anywhere() {
    common::check_ok(
        r#"def fetch() -> Int
  42

def caller() -> Void
  let t = async fetch()
"#,
    );
}

// C2: Structural type unification for generic params in compound types
#[test]
fn generic_list_param_unifies() {
    common::check_ok(
        r#"def first_elem(items: List[T]) -> T
  items[0]

let xs = [1, 2, 3]
let y = first_elem(items: xs)
"#,
    );
}

// H2: Match ident pattern binds the variable
#[test]
fn match_ident_binds_variable() {
    common::check_ok(
        r#"def describe(n: Int) -> Int
  match n
    x => x + 1
"#,
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Round 2: Post-mitigation audit regression tests
// ═══════════════════════════════════════════════════════════════════════

// R2-1: Nested return must be validated against declared return type
#[test]
fn nested_return_type_mismatch() {
    let err = common::check_err(
        r#"def f() -> Int
  if true
    return "hello"
  42
"#,
    );
    assert!(err.contains("mismatch") || err.contains("Return") || err.contains("return"));
}

// R2-1b: Nested return with correct type still works
#[test]
fn nested_return_correct_type() {
    common::check_ok(
        r#"def f() -> Int
  if true
    return 42
  0
"#,
    );
}

// R2-1c: Deeply nested return also validated
#[test]
fn deeply_nested_return_mismatch() {
    let err = common::check_err(
        r#"def f() -> Int
  while true
    if true
      return "wrong"
    break
  0
"#,
    );
    assert!(err.contains("mismatch") || err.contains("Return") || err.contains("return"));
}

// R2-2: Generic class instantiation produces parameterized type
#[test]
fn generic_class_field_access() {
    common::check_ok(
        r#"class Box[T]
  value: T

let b = Box(value: 42)
let v = b.value
let x: Int = v
"#,
    );
}

// R2-2b: Different generic instantiations produce different types
#[test]
fn generic_class_type_distinction() {
    let err = common::check_err(
        r#"class Box[T]
  value: T

let b = Box(value: 42)
let v: String = b.value
"#,
    );
    assert!(err.contains("mismatch") || err.contains("annotation"));
}

// R2-3: Phantom type parameters (unresolvable) produce an error
#[test]
fn phantom_typevar_error() {
    // T only in return type (not in params) is not a type parameter —
    // it's an unknown type, causing a return type mismatch
    let err = common::check_err(
        r#"def phantom() -> T
  nil

let x = phantom()
"#,
    );
    assert!(
        err.contains("mismatch") || err.contains("Custom"),
        "got: {}",
        err
    );
}

// R2-4: Higher-order generic function unifies function params structurally
#[test]
fn higher_order_generic_unification() {
    common::check_ok(
        r#"def apply(f: (T) -> T, x: T) -> T
  f(_0: x)

def double(n: Int) -> Int
  n + n

let r = apply(f: double, x: 5)
"#,
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Phase 7: Error Handling RFC — extends, throws, throw, !, T?
// ═══════════════════════════════════════════════════════════════════════

// ─── 7A. extends keyword ─────────────────────────────────────────────

#[test]
fn extends_basic_inheritance() {
    common::check_ok(
        r#"class Animal
  name: String

class Dog extends Animal
  breed: String
"#,
    );
}

#[test]
fn extends_unknown_parent_error() {
    let err = common::check_err(
        r#"class Dog extends NonExistent
  breed: String
"#,
    );
    assert!(err.contains("extends") || err.contains("unknown") || err.contains("Unknown"));
}

#[test]
fn extends_with_includes() {
    common::check_ok(
        r#"trait Printable
  def to_str() -> String

class Animal
  name: String

class Dog extends Animal includes Printable
  breed: String
  def to_str() -> String
    "dog"
"#,
    );
}

// ─── 7B. throws / throw / ! ──────────────────────────────────────────

#[test]
fn throws_declaration_basic() {
    common::check_ok(
        r#"class AppError
  message: String

def risky() throws AppError -> Int
  42
"#,
    );
}

#[test]
fn throw_statement_basic() {
    common::check_ok(
        r#"class AppError
  message: String

def risky() throws AppError -> Int
  throw AppError(message: "boom")
"#,
    );
}

#[test]
fn throw_outside_throws_fn_error() {
    let err = common::check_err(
        r#"class AppError
  message: String

def safe() -> Int
  throw AppError(message: "boom")
"#,
    );
    assert!(err.contains("throw") || err.contains("throws"));
}

#[test]
fn bang_propagation_basic() {
    common::check_ok(
        r#"class AppError
  message: String

def risky() throws AppError -> Int
  42

def caller() throws AppError -> Int
  let x = risky()!
  x + 1
"#,
    );
}

#[test]
fn bang_outside_throws_fn_error() {
    let err = common::check_err(
        r#"class AppError
  message: String

def risky() throws AppError -> Int
  42

def safe() -> Int
  let x = risky()!
  x
"#,
    );
    assert!(err.contains("throws") || err.contains("propagate"));
}

#[test]
fn throw_with_extends_hierarchy() {
    common::check_ok(
        r#"class AppError
  message: String

class NetworkError extends AppError
  url: String

def fetch() throws AppError -> String
  throw NetworkError(message: "fail", url: "http://x")
"#,
    );
}

#[test]
fn bang_propagation_with_extends() {
    common::check_ok(
        r#"class AppError
  message: String

class NetworkError extends AppError
  url: String

def fetch() throws NetworkError -> String
  "data"

def caller() throws AppError -> String
  fetch()!
"#,
    );
}

#[test]
fn throws_void_return() {
    common::check_ok(
        r#"class AppError
  message: String

def side_effect() throws AppError
  throw AppError(message: "boom")
"#,
    );
}

// ─── 7C. !.or(), !.or_else(), !.catch ────────────────────────────────

#[test]
fn bang_or_fallback_basic() {
    common::check_ok(
        r#"class AppError
  message: String

def risky() throws AppError -> Int
  42

def safe() -> Int
  risky()!.or(0)
"#,
    );
}

#[test]
fn bang_or_type_mismatch_error() {
    let err = common::check_err(
        r#"class AppError
  message: String

def risky() throws AppError -> Int
  42

def safe() -> Int
  risky()!.or("fallback")
"#,
    );
    assert!(err.contains("mismatch") || err.contains("expected") || err.contains("or"));
}

#[test]
fn bang_or_else_basic() {
    common::check_ok(
        r#"class AppError
  message: String

def risky() throws AppError -> Int
  42

def fallback() -> Int
  0

def safe() -> Int
  risky()!.or_else(-> fallback())
"#,
    );
}

#[test]
fn bang_catch_basic() {
    common::check_ok(
        r#"class AppError
  message: String

class NetworkError extends AppError
  url: String

def fetch() throws NetworkError -> String
  "data"

def safe() -> String
  fetch()!.catch
    NetworkError e -> "fallback"
    _ -> "default"
"#,
    );
}

#[test]
fn bang_catch_binds_error_var() {
    common::check_ok(
        r#"class AppError
  message: String

def risky() throws AppError -> String
  "data"

def safe() -> String
  risky()!.catch
    AppError e -> e.message
    _ -> "unknown"
"#,
    );
}

#[test]
fn bang_catch_no_throws_needed() {
    // !.catch handles all errors — caller doesn't need throws
    common::check_ok(
        r#"class AppError
  message: String

def risky() throws AppError -> Int
  42

def safe() -> Int
  risky()!.catch
    _ -> 0
"#,
    );
}

// ─── 7D. T? nullable type ────────────────────────────────────────────

#[test]
fn nullable_type_annotation() {
    common::check_ok(
        r#"let x: String? = nil
"#,
    );
}

#[test]
fn nullable_auto_wrap() {
    common::check_ok(
        r#"let x: String? = "hello"
"#,
    );
}

#[test]
fn nullable_or() {
    common::check_ok(
        r#"let x: String? = nil
let y: String = x.or(default: "default")
"#,
    );
}

#[test]
fn nullable_or_else() {
    common::check_ok(
        r#"let x: String? = nil
let y: String = x.or_else(f: -> "computed")
"#,
    );
}

#[test]
fn nullable_or_throw() {
    common::check_ok(
        r#"class AppError
  message: String

def get_value() throws AppError -> String
  let x: String? = nil
  x.or_throw(error: AppError(message: "missing"))
"#,
    );
}

#[test]
fn nullable_match() {
    common::check_ok(
        r#"let x: String? = "hello"
let y = match x
  nil => "absent"
  v => v
"#,
    );
}

#[test]
fn nullable_no_method_access_error() {
    let err = common::check_err(
        r#"let x: String? = "hello"
let y = len(value: x)
"#,
    );
    assert!(
        err.contains("nullable")
            || err.contains("Nullable")
            || err.contains("resolve")
            || err.contains("String?")
    );
}

#[test]
fn nullable_nil_to_non_nullable_error() {
    let err = common::check_err(
        r#"let x: String = nil
"#,
    );
    assert!(
        err.contains("nil")
            || err.contains("Nil")
            || err.contains("nullable")
            || err.contains("mismatch")
    );
}

#[test]
fn nullable_no_double_nullable_error() {
    let err = common::check_parse_err(
        r#"let x: String?? = nil
"#,
    );
    assert!(
        err.contains("??")
            || err.contains("nested")
            || err.contains("double")
            || err.contains("Nullable")
    );
}

#[test]
fn nullable_field() {
    common::check_ok(
        r#"class User
  name: String
  bio: String?
"#,
    );
}

#[test]
fn nullable_return_type() {
    common::check_ok(
        r#"def find(id: Int) -> String?
  nil
"#,
    );
}

#[test]
fn nullable_return_value() {
    common::check_ok(
        r#"def find(id: Int) -> String?
  "found"
"#,
    );
}

// ─── 7D+. Catch with multiple error types ─────────────────────────────

#[test]
fn catch_multiple_error_types() {
    common::check_ok(
        r#"class NetworkError extends Error
  code: Int

class ParseError extends Error
  line: Int

def risky() throws Error -> Int
  throw NetworkError(message: "fail", code: 500)

def main() -> Int
  risky()!.catch
    NetworkError e -> 0
    ParseError e -> 1
    Error e -> 2
"#,
    );
}

// ─── 7E. Remove old error constructs ─────────────────────────────────

// R2-9: Deeply nested expressions produce a clear error, not a stack overflow
#[test]
fn recursion_depth_limit() {
    // Run in a thread with a larger stack to avoid test-runner stack overflow
    // while still verifying the parser's own depth limit works.
    let result = std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(|| {
            let depth = 300;
            let open: String = "(".repeat(depth);
            let close: String = ")".repeat(depth);
            let src = format!("let x = {}1{}", open, close);
            let tokens = lexer::lex(&src).expect("lex ok");
            let mut parser = parser::Parser::new(tokens);
            let result = parser.parse_module("test");
            assert!(result.is_err(), "Expected recursion depth error");
            let err = result.unwrap_err().to_string();
            assert!(err.contains("depth") || err.contains("nesting") || err.contains("Nesting"));
        })
        .unwrap()
        .join();
    assert!(result.is_ok(), "Thread panicked");
}

// ─── Soundness: S2 — catch arm type must match success path type ───

#[test]
fn soundness_catch_arm_type_must_match_success_type() {
    // risky() returns Int on success, but catch arms return String — must be an error
    let err = common::check_err(
        r#"class AppError extends Error
  code: Int

def risky() throws AppError -> Int
  42

def caller() throws Error -> String
  risky()!.catch
    AppError e -> "oops"
    _ -> "default"
"#,
    );
    assert!(
        err.contains("does not match") || err.contains("mismatch") || err.contains("E013"),
        "Expected catch arm type mismatch error, got: {}",
        err
    );
}

#[test]
fn soundness_catch_arm_same_type_as_success_is_ok() {
    // catch arms return Int, same as success path — should be fine
    common::check_ok(
        r#"class AppError extends Error
  code: Int

def risky() throws AppError -> Int
  42

def caller() throws Error -> Int
  risky()!.catch
    AppError e -> 0
    _ -> -1
"#,
    );
}
