mod common;

// ============================================================
// Audit fixes — TDD RED tests
// ============================================================

// F2: Nullable(TypeVar) must be substituted in generics
#[test]
fn nullable_typevar_substitution_in_generic() {
    // A generic function returning T? should produce Int? when called with Int
    common::check_ok(
        r#"class Box[T]
  value: T

def maybe_first(xs: List[T]) -> T?
  nil

let items = [1, 2, 3]
let result: Int? = maybe_first(xs: items)
"#,
    );
}

// F3: Calling a throws function without ! must be a type error
#[test]
fn throws_call_without_bang_error() {
    let err = common::check_err(
        r#"class AppError extends Error
  code: Int

def risky() throws AppError -> Int
  42

def safe() -> Int
  risky()
"#,
    );
    assert!(err.contains("throws") || err.contains("error handling") || err.contains("!"));
}

// F3b: Calling throws function WITH ! must still work
#[test]
fn throws_call_with_bang_ok() {
    common::check_ok(
        r#"class AppError extends Error
  code: Int

def risky() throws AppError -> Int
  42

def caller() throws AppError -> Int
  risky()!
"#,
    );
}

// F3c: Calling throws function with !.or() must still work
#[test]
fn throws_call_with_or_ok() {
    common::check_ok(
        r#"class AppError extends Error
  code: Int

def risky() throws AppError -> Int
  42

def safe() -> Int
  risky()!.or(0)
"#,
    );
}

// F4: calling a throwing function without error handling must be an error
#[test]
fn throwing_call_without_error_handling() {
    let err = common::check_err(
        r#"class AppError extends Error
  code: Int

def risky_fetch() throws AppError -> String
  "data"

def main() -> String
  risky_fetch()
"#,
    );
    assert!(err.contains("throws") || err.contains("error handling") || err.contains("!"));
}

// F5: !.catch arms should be validated against declared throws type
#[test]
fn catch_arm_unreachable_type_error() {
    let err = common::check_err(
        r#"class AppError extends Error
  code: Int

class DatabaseError extends Error
  code: Int

def risky() throws AppError -> Int
  42

def caller() throws AppError -> Int
  risky()!.catch
    DatabaseError e -> 0
    _ -> -1
"#,
    );
    assert!(
        err.contains("DatabaseError")
            || err.contains("subtype")
            || err.contains("unreachable")
            || err.contains("not a subtype")
    );
}

// F5b: !.catch with valid subtype should work
#[test]
fn catch_arm_valid_subtype_ok() {
    common::check_ok(
        r#"class AppError extends Error
  code: Int

def risky() throws AppError -> Int
  42

def caller() -> Int
  risky()!.catch
    AppError e -> 0
    _ -> -1
"#,
    );
}

// F6: Inherited fields must be accessible via extends
#[test]
fn inherited_field_access() {
    common::check_ok(
        r#"class Animal
  name: String

class Dog extends Animal
  breed: String

let d = Dog(name: "buddy", breed: "lab")
let n: String = d.name
"#,
    );
}

// F6b: Inherited methods must be accessible via extends
#[test]
fn inherited_method_access() {
    common::check_ok(
        r#"class Base
  x: Int

  def greet() -> String
    "hello"

class Child extends Base
  y: Int

let c = Child(x: 1, y: 2)
let val: String = c.greet()
"#,
    );
}

// F7: Assignment to nullable variable should accept inner type and nil
#[test]
fn nullable_assignment_accepts_inner_type() {
    common::check_ok(
        r#"let x: String? = nil
x = "hello"
"#,
    );
}

#[test]
fn nullable_assignment_accepts_nil() {
    common::check_ok(
        r#"let x: String? = "hello"
x = nil
"#,
    );
}

// F9: Literal match patterns on nullable scrutinee should work for inner type
#[test]
fn literal_match_on_nullable_scrutinee() {
    common::check_ok(
        r#"def check(x: String?) -> Int
  match x
    "hello" => 1
    nil => 0
    _ => -1
"#,
    );
}

// F12: Map[K,V]? should parse
#[test]
fn map_nullable_type_parses() {
    common::check_ok(
        r#"def get_map() -> Map[String, Int]?
  nil
"#,
    );
}

// F13: Task[T]? should parse
#[test]
fn task_nullable_type_parses() {
    common::check_ok(
        r#"def get_task() -> Task[Int]?
  nil
"#,
    );
}

// H1: Deeply nested unary operators should hit depth limit, not stack overflow
#[test]
fn unary_recursion_depth_limit() {
    let result = std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(|| {
            let depth = 500;
            let nots: String = "not ".repeat(depth);
            let src = format!("let x = {}true", nots);
            let tokens = lexer::lex(&src).expect("lex ok");
            let mut parser = parser::Parser::new(tokens);
            let result = parser.parse_module("test");
            assert!(result.is_err(), "Expected recursion depth error");
        })
        .unwrap()
        .join();
    assert!(
        result.is_ok(),
        "Thread panicked — stack overflow instead of depth error"
    );
}

// H1b: Deeply nested statement blocks should hit depth limit
#[test]
fn block_recursion_depth_limit() {
    let result = std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(|| {
            // Build deeply nested if blocks
            let depth = 200;
            let mut src = String::new();
            for _ in 0..depth {
                src.push_str("if true\n");
                // each indent adds two spaces per level — but we need proper indent
            }
            // Actually, build with proper indentation
            let mut src2 = String::new();
            for i in 0..depth {
                let indent = "  ".repeat(i);
                src2.push_str(&format!("{}if true\n", indent));
            }
            let final_indent = "  ".repeat(depth);
            src2.push_str(&format!("{}let x = 1\n", final_indent));
            let tokens = lexer::lex(&src2).expect("lex ok");
            let mut parser = parser::Parser::new(tokens);
            let result = parser.parse_module("test");
            assert!(
                result.is_err(),
                "Expected recursion depth error for nested blocks"
            );
        })
        .unwrap()
        .join();
    assert!(
        result.is_ok(),
        "Thread panicked — stack overflow instead of depth error"
    );
}

// H2: Cyclic extends chains must produce error, not infinite loop
#[test]
fn cyclic_extends_detection() {
    // Self-cycle: class extends itself — detectable because class is registered before extends check
    let err = common::check_err(
        r#"class A extends A
  message: String
"#,
    );
    assert!(
        err.contains("ircular") || err.contains("ycle"),
        "Expected cycle error, got: {}",
        err
    );
}

// Field shadowing: child class cannot redeclare inherited field
#[test]
fn field_shadowing_in_extends_error() {
    let err = common::check_err(
        r#"class Base
  message: String

class Child extends Base
  message: String
  extra: Int
"#,
    );
    assert!(
        err.contains("shadow") || err.contains("redeclar") || err.contains("already"),
        "Expected field shadowing error, got: {}",
        err
    );
}

// M1: File size limit
#[test]
fn file_size_limit_rejects_large_input() {
    // 10MB+ input should be rejected
    let huge = "a".repeat(11 * 1024 * 1024);
    let result = lexer::lex(&huge);
    assert!(result.is_err(), "Huge input should be rejected");
}

// M5: String literal length limit
#[test]
fn string_literal_length_limit() {
    let long_str = format!("let x = \"{}\"", "a".repeat(1_000_001));
    let result = lexer::lex(&long_str);
    assert!(
        result.is_err(),
        "Very long string literal should be rejected"
    );
}

// M6: Number literal digit limit
#[test]
fn number_literal_digit_limit() {
    let long_num = format!("let x = {}", "9".repeat(1001));
    let result = lexer::lex(&long_num);
    assert!(
        result.is_err(),
        "Very long number literal should be rejected"
    );
}

// ─── Audit Round 2: Remaining findings ──────────────────────────────

// DC2: Type::Never should be recognized by from_ident (BUG — currently becomes Custom("Never"))
#[test]
fn never_type_recognized_by_from_ident() {
    // "Never" used as a type annotation should be recognized, not become Custom("Never")
    // This validates that the type system correctly handles Never as a built-in type
    use ast::Type;
    assert_eq!(Type::from_ident("Never"), Type::Never);
}

// Lowercase built-in type names produce a clear error
#[test]
fn lowercase_builtin_type_rejected() {
    for (wrong, right) in [
        ("bool", "Bool"),
        ("int", "Int"),
        ("float", "Float"),
        ("string", "String"),
        ("void", "Void"),
    ] {
        let src = format!("def f() -> {}\n  1\n", wrong);
        let err = common::check_parse_err(&src);
        assert!(
            err.contains(&format!("Did you mean '{}'?", right)),
            "'{}' should suggest '{}', got: {}",
            wrong,
            right,
            err
        );
    }
}

// L2: Unicode homoglyph identifiers should be rejected
#[test]
fn unicode_homoglyph_identifier_rejected() {
    // Cyrillic 'а' (U+0430) looks identical to Latin 'a' but is a different character
    // This could cause confusion and security issues
    let result = lexer::lex("let \u{0430} = 1");
    assert!(result.is_err(), "Cyrillic homoglyph should be rejected");
}

#[test]
fn ascii_identifiers_accepted() {
    // Normal ASCII identifiers should still work
    let result = lexer::lex("let abc = 1");
    assert!(result.is_ok());
}

#[test]
fn underscore_identifiers_accepted() {
    let result = lexer::lex("let _foo = 1");
    assert!(result.is_ok());
}

// DC4-DC6: Visibility — validated by compile-time checks, no runtime test needed
// These will be verified by changing pub to pub(crate) and confirming tests still compile

// D3: Behavior-lock test for bracketed ident parsing (generics, includes)
// Ensures parse_bracketed_idents extraction doesn't change behavior
#[test]
fn generic_class_params_parsed() {
    common::check_ok(
        r#"class Pair[A, B]
  first: A
  second: B
"#,
    );
}

#[test]
fn generic_function_params_parsed() {
    common::check_ok(
        r#"def identity(x: T) -> T
  x
"#,
    );
}

#[test]
fn includes_multiple_traits_parsed() {
    common::check_ok(
        r#"trait Printable
  def to_str() -> String

trait Serializable
  def serialize() -> String

class Widget includes Printable, Serializable
  name: String
  def to_str() -> String
    "widget"
  def serialize() -> String
    "json"
"#,
    );
}

// D5: Behavior-lock for body checking in multiple contexts
#[test]
fn if_condition_must_be_bool() {
    let err = common::check_err(
        r#"if 42
  let x = 1
"#,
    );
    assert!(
        err.contains("Bool"),
        "Expected Bool condition error, got: {}",
        err
    );
}

#[test]
fn while_body_typechecked() {
    common::check_ok(
        r#"let x = 0
while x < 10
  x = x + 1
"#,
    );
}

#[test]
fn for_body_typechecked() {
    common::check_ok(
        r#"let items: List[Int] = [1, 2, 3]
for item in items
  let y = item + 1
"#,
    );
}

// D6/D7: Behavior-lock for nullable and error or/or_else handlers
#[test]
fn nullable_or_returns_inner_type() {
    common::check_ok(
        r#"let x: Int? = 5
let y: Int = x.or(default: 0)
"#,
    );
}

#[test]
fn nullable_or_else_returns_inner_type() {
    common::check_ok(
        r#"let x: Int? = nil
let y: Int = x.or_else(f: 0)
"#,
    );
}

#[test]
fn nullable_or_type_mismatch_error() {
    let err = common::check_err(
        r#"let x: Int? = 5
let y: Int = x.or(default: "not_int")
"#,
    );
    assert!(
        err.contains("mismatch"),
        "Expected type mismatch, got: {}",
        err
    );
}

#[test]
fn error_or_returns_success_type() {
    common::check_ok(
        r#"class AppError extends Error
  code: Int

def risky() throws AppError -> String
  "ok"

def safe() -> String
  risky()!.or("default")
"#,
    );
}

#[test]
fn error_or_else_returns_success_type() {
    common::check_ok(
        r#"class AppError extends Error
  code: Int

def risky() throws AppError -> String
  "ok"

def safe() -> String
  risky()!.or_else(-> "fallback")
"#,
    );
}

#[test]
fn error_or_type_mismatch_error() {
    let err = common::check_err(
        r#"class AppError extends Error
  code: Int

def risky() throws AppError -> String
  "ok"

def safe() -> String
  risky()!.or(42)
"#,
    );
    assert!(
        err.contains("mismatch"),
        "Expected type mismatch, got: {}",
        err
    );
}

// D8: Statement-to-expression fallthrough — expressions work as statements
#[test]
fn expr_as_statement() {
    common::check_ok(
        r#"def f() -> Int
  let x = 1
  x + 2
"#,
    );
}

#[test]
fn match_as_statement() {
    common::check_ok(
        r#"let x = match 1
  1 => "one"
  _ => "other"
"#,
    );
}

// Q5: Nullable handling in call context — lock behavior
#[test]
fn nullable_or_throw_in_throws_context() {
    common::check_ok(
        r#"class AppError extends Error
  code: Int

def risky(x: Int?) throws AppError -> Int
  x.or_throw(error: AppError(message: "null", code: 0))
"#,
    );
}

#[test]
fn nullable_method_call_rejected() {
    let err = common::check_err(
        r#"class Foo
  name: String

let x: Foo? = nil
let y = x.name
"#,
    );
    assert!(
        err.contains("nullable") || err.contains("Resolve") || err.contains("Cannot access"),
        "Expected nullable access error, got: {}",
        err
    );
}

// M2: Quadratic dedent-at-EOF — cache lines().count()
// (Performance fix — already fixed in code, verified by code review)

// L1: unwrap() replaced with proper error handling in lexer
// (Robustness fix — already fixed in code)

// L4: Tab indentation should produce a clear error
#[test]
fn tab_indentation_rejected() {
    let result = lexer::lex("def f() -> Int\n\treturn 1\n");
    assert!(result.is_err(), "Tab indentation should be rejected");
    let err = result.unwrap_err().to_string();
    assert!(err.contains("tab") || err.contains("Tab") || err.contains("indent"));
}
