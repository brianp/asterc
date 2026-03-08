mod common;

// ═══════════════════════════════════════════════════════════════════════
// Regression tests for audit mitigations
// ═══════════════════════════════════════════════════════════════════════

// C1: Sentinel variable forgery — user code cannot spoof async context
#[test]
fn regression_sentinel_forgery_blocked() {
    let err = common::check_err(r#"async def fetch() -> Int
  42

def sneaky() -> Int
  let __async_context__ = true
  async fetch()
  0
"#);
    assert!(err.contains("async") || err.contains("context"));
}

// C2: Structural type unification for generic params in compound types
#[test]
fn regression_generic_list_param_unifies() {
    common::check_ok(r#"def first_elem[T](items: List[T]) -> T
  items[0]

let xs = [1, 2, 3]
let y = first_elem(xs)
"#);
}

// H2: Match ident pattern binds the variable
#[test]
fn regression_match_ident_binds_variable() {
    common::check_ok(r#"def describe(n: Int) -> Int
  match n
    x => x + 1
"#);
}

// ═══════════════════════════════════════════════════════════════════════
// Round 2: Post-mitigation audit regression tests
// ═══════════════════════════════════════════════════════════════════════

// R2-1: Nested return must be validated against declared return type
#[test]
fn regression_nested_return_type_mismatch() {
    let err = common::check_err(r#"def f() -> Int
  if true
    return "hello"
  42
"#);
    assert!(err.contains("mismatch") || err.contains("Return") || err.contains("return"));
}

// R2-1b: Nested return with correct type still works
#[test]
fn regression_nested_return_correct_type() {
    common::check_ok(r#"def f() -> Int
  if true
    return 42
  0
"#);
}

// R2-1c: Deeply nested return also validated
#[test]
fn regression_deeply_nested_return_mismatch() {
    let err = common::check_err(r#"def f() -> Int
  while true
    if true
      return "wrong"
    break
  0
"#);
    assert!(err.contains("mismatch") || err.contains("Return") || err.contains("return"));
}

// R2-2: Generic class instantiation produces parameterized type
#[test]
fn regression_generic_class_field_access() {
    common::check_ok(r#"class Box[T]
  value: T

let b = Box(42)
let v = b.value
let x: Int = v
"#);
}

// R2-2b: Different generic instantiations produce different types
#[test]
fn regression_generic_class_type_distinction() {
    let err = common::check_err(r#"class Box[T]
  value: T

let b = Box(42)
let v: String = b.value
"#);
    assert!(err.contains("mismatch") || err.contains("annotation"));
}

// R2-3: Phantom type parameters (unresolvable) produce an error
#[test]
fn regression_phantom_typevar_error() {
    let err = common::check_err(r#"def phantom[T]() -> T
  nil

let x = phantom()
"#);
    assert!(err.contains("infer") || err.contains("type parameter") || err.contains("TypeVar") || err.contains("unresolved"));
}

// R2-4: Higher-order generic function unifies function params structurally
#[test]
fn regression_higher_order_generic_unification() {
    common::check_ok(r#"def apply[T](f: (T) -> T, x: T) -> T
  f(x)

def double(n: Int) -> Int
  n + n

let r = apply(double, 5)
"#);
}

// ═══════════════════════════════════════════════════════════════════════
// Phase 7: Error Handling RFC — extends, throws, throw, !, T?
// ═══════════════════════════════════════════════════════════════════════

// ─── 7A. extends keyword ─────────────────────────────────────────────

#[test]
fn phase7_extends_basic() {
    common::check_ok(r#"class Animal
  name: String

class Dog extends Animal
  breed: String
"#);
}

#[test]
fn phase7_extends_unknown_parent_error() {
    let err = common::check_err(r#"class Dog extends NonExistent
  breed: String
"#);
    assert!(err.contains("extends") || err.contains("unknown") || err.contains("Unknown"));
}

#[test]
fn phase7_extends_with_includes() {
    common::check_ok(r#"trait Printable
  def to_str() -> String

class Animal
  name: String

class Dog extends Animal includes Printable
  breed: String
  def to_str() -> String
    "dog"
"#);
}

// ─── 7B. throws / throw / ! ──────────────────────────────────────────

#[test]
fn phase7_throws_basic() {
    common::check_ok(r#"class AppError
  message: String

def risky() throws AppError -> Int
  42
"#);
}

#[test]
fn phase7_throw_basic() {
    common::check_ok(r#"class AppError
  message: String

def risky() throws AppError -> Int
  throw AppError("boom")
"#);
}

#[test]
fn phase7_throw_outside_throws_error() {
    let err = common::check_err(r#"class AppError
  message: String

def safe() -> Int
  throw AppError("boom")
"#);
    assert!(err.contains("throw") || err.contains("throws"));
}

#[test]
fn phase7_bang_propagation() {
    common::check_ok(r#"class AppError
  message: String

def risky() throws AppError -> Int
  42

def caller() throws AppError -> Int
  let x = risky()!
  x + 1
"#);
}

#[test]
fn phase7_bang_outside_throws_error() {
    let err = common::check_err(r#"class AppError
  message: String

def risky() throws AppError -> Int
  42

def safe() -> Int
  let x = risky()!
  x
"#);
    assert!(err.contains("throws") || err.contains("propagate"));
}

#[test]
fn phase7_throw_with_extends_hierarchy() {
    common::check_ok(r#"class AppError
  message: String

class NetworkError extends AppError
  url: String

def fetch() throws AppError -> String
  throw NetworkError("fail", "http://x")
"#);
}

#[test]
fn phase7_bang_propagation_with_extends() {
    common::check_ok(r#"class AppError
  message: String

class NetworkError extends AppError
  url: String

def fetch() throws NetworkError -> String
  "data"

def caller() throws AppError -> String
  fetch()!
"#);
}

#[test]
fn phase7_throws_void_return() {
    common::check_ok(r#"class AppError
  message: String

def side_effect() throws AppError
  throw AppError("boom")
"#);
}

// ─── 7C. !.or(), !.or_else(), !.catch ────────────────────────────────

#[test]
fn phase7_bang_or_basic() {
    common::check_ok(r#"class AppError
  message: String

def risky() throws AppError -> Int
  42

def safe() -> Int
  risky()!.or(0)
"#);
}

#[test]
fn phase7_bang_or_type_mismatch_error() {
    let err = common::check_err(r#"class AppError
  message: String

def risky() throws AppError -> Int
  42

def safe() -> Int
  risky()!.or("fallback")
"#);
    assert!(err.contains("mismatch") || err.contains("expected") || err.contains("or"));
}

#[test]
fn phase7_bang_or_else_basic() {
    common::check_ok(r#"class AppError
  message: String

def risky() throws AppError -> Int
  42

def fallback() -> Int
  0

def safe() -> Int
  risky()!.or_else(-> fallback())
"#);
}

#[test]
fn phase7_bang_catch_basic() {
    common::check_ok(r#"class AppError
  message: String

class NetworkError extends AppError
  url: String

def fetch() throws NetworkError -> String
  "data"

def safe() -> String
  fetch()!.catch
    NetworkError e -> "fallback"
    _ -> "default"
"#);
}

#[test]
fn phase7_bang_catch_binds_var() {
    common::check_ok(r#"class AppError
  message: String

def risky() throws AppError -> String
  "data"

def safe() -> String
  risky()!.catch
    AppError e -> e.message
    _ -> "unknown"
"#);
}

#[test]
fn phase7_bang_catch_no_throws_needed() {
    // !.catch handles all errors — caller doesn't need throws
    common::check_ok(r#"class AppError
  message: String

def risky() throws AppError -> Int
  42

def safe() -> Int
  risky()!.catch
    _ -> 0
"#);
}

// ─── 7D. T? nullable type ────────────────────────────────────────────

#[test]
fn phase7_nullable_type_annotation() {
    common::check_ok(r#"let x: String? = nil
"#);
}

#[test]
fn phase7_nullable_auto_wrap() {
    common::check_ok(r#"let x: String? = "hello"
"#);
}

#[test]
fn phase7_nullable_or() {
    common::check_ok(r#"let x: String? = nil
let y: String = x.or("default")
"#);
}

#[test]
fn phase7_nullable_or_else() {
    common::check_ok(r#"let x: String? = nil
let y: String = x.or_else(-> "computed")
"#);
}

#[test]
fn phase7_nullable_or_throw() {
    common::check_ok(r#"class AppError
  message: String

def get_value() throws AppError -> String
  let x: String? = nil
  x.or_throw(AppError("missing"))
"#);
}

#[test]
fn phase7_nullable_match() {
    common::check_ok(r#"let x: String? = "hello"
let y = match x
  nil => "absent"
  v => v
"#);
}

#[test]
fn phase7_nullable_no_method_access_error() {
    let err = common::check_err(r#"let x: String? = "hello"
let y = len(x)
"#);
    assert!(err.contains("nullable") || err.contains("Nullable") || err.contains("resolve") || err.contains("String?"));
}

#[test]
fn phase7_nullable_nil_to_non_nullable_error() {
    let err = common::check_err(r#"let x: String = nil
"#);
    assert!(err.contains("nil") || err.contains("Nil") || err.contains("nullable") || err.contains("mismatch"));
}

#[test]
fn phase7_nullable_no_double_nullable_error() {
    let err = common::check_parse_err(r#"let x: String?? = nil
"#);
    assert!(err.contains("??") || err.contains("nested") || err.contains("double") || err.contains("Nullable"));
}

#[test]
fn phase7_nullable_field() {
    common::check_ok(r#"class User
  name: String
  bio: String?
"#);
}

#[test]
fn phase7_nullable_return_type() {
    common::check_ok(r#"def find(id: Int) -> String?
  nil
"#);
}

#[test]
fn phase7_nullable_return_value() {
    common::check_ok(r#"def find(id: Int) -> String?
  "found"
"#);
}

// ─── 7E. Remove old error constructs ─────────────────────────────────

// R2-9: Deeply nested expressions produce a clear error, not a stack overflow
#[test]
fn regression_recursion_depth_limit() {
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
            let err = result.unwrap_err();
            assert!(err.contains("depth") || err.contains("nesting") || err.contains("Nesting"));
        })
        .unwrap()
        .join();
    assert!(result.is_ok(), "Thread panicked");
}
