mod common;

// ============================================================
// Audit fixes — TDD RED tests
// ============================================================

// F2: Nullable(TypeVar) must be substituted in generics
#[test]
fn audit_f2_nullable_typevar_substitution() {
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
fn audit_f3_throws_call_without_bang_error() {
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
fn audit_f3b_throws_call_with_bang_ok() {
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
fn audit_f3c_throws_call_with_or_ok() {
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
fn audit_f4_throwing_call_without_bang_error() {
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
fn audit_f5_catch_arm_unreachable_type_error() {
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
fn audit_f5b_catch_arm_valid_subtype_ok() {
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
fn audit_f6_inherited_field_access() {
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
fn audit_f6b_inherited_method_access() {
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
fn audit_f7_nullable_assignment_inner_type() {
    common::check_ok(
        r#"let x: String? = nil
x = "hello"
"#,
    );
}

#[test]
fn audit_f7b_nullable_assignment_nil() {
    common::check_ok(
        r#"let x: String? = "hello"
x = nil
"#,
    );
}

// F9: Literal match patterns on nullable scrutinee should work for inner type
#[test]
fn audit_f9_literal_match_on_nullable() {
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
fn audit_f12_map_nullable_parse() {
    common::check_ok(
        r#"def get_map() -> Map[String, Int]?
  nil
"#,
    );
}

// F13: Task[T]? should parse
#[test]
fn audit_f13_task_nullable_parse() {
    common::check_ok(
        r#"def get_task() -> Task[Int]?
  nil
"#,
    );
}

// H1: Deeply nested unary operators should hit depth limit, not stack overflow
#[test]
fn audit_h1_unary_recursion_depth_limit() {
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
fn audit_h1b_block_recursion_depth_limit() {
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
fn audit_h2_cyclic_extends_detection() {
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
fn audit_field_shadowing_in_extends() {
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
fn audit_m1_file_size_limit() {
    // 10MB+ input should be rejected
    let huge = "a".repeat(11 * 1024 * 1024);
    let result = lexer::lex(&huge);
    assert!(result.is_err(), "Huge input should be rejected");
}

// M5: String literal length limit
#[test]
fn audit_m5_string_length_limit() {
    let long_str = format!("let x = \"{}\"", "a".repeat(1_000_001));
    let result = lexer::lex(&long_str);
    assert!(
        result.is_err(),
        "Very long string literal should be rejected"
    );
}

// M6: Number literal digit limit
#[test]
fn audit_m6_digit_limit() {
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
fn audit_dc2_never_type_from_ident() {
    // "Never" used as a type annotation should be recognized, not become Custom("Never")
    // This validates that the type system correctly handles Never as a built-in type
    use ast::Type;
    assert_eq!(Type::from_ident("Never"), Type::Never);
}

// L2: Unicode homoglyph identifiers should be rejected
#[test]
fn audit_l2_unicode_homoglyph_rejected() {
    // Cyrillic 'а' (U+0430) looks identical to Latin 'a' but is a different character
    // This could cause confusion and security issues
    let result = lexer::lex("let \u{0430} = 1");
    assert!(result.is_err(), "Cyrillic homoglyph should be rejected");
}

#[test]
fn audit_l2_ascii_identifiers_ok() {
    // Normal ASCII identifiers should still work
    let result = lexer::lex("let abc = 1");
    assert!(result.is_ok());
}

#[test]
fn audit_l2_underscore_identifiers_ok() {
    let result = lexer::lex("let _foo = 1");
    assert!(result.is_ok());
}

// DC4-DC6: Visibility — validated by compile-time checks, no runtime test needed
// These will be verified by changing pub to pub(crate) and confirming tests still compile

// D3: Behavior-lock test for bracketed ident parsing (generics, includes)
// Ensures parse_bracketed_idents extraction doesn't change behavior
#[test]
fn audit_d3_generic_class_params_parsed() {
    common::check_ok(
        r#"class Pair[A, B]
  first: A
  second: B
"#,
    );
}

#[test]
fn audit_d3_generic_function_params_parsed() {
    common::check_ok(
        r#"def identity(x: T) -> T
  x
"#,
    );
}

#[test]
fn audit_d3_includes_multiple_traits() {
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
fn audit_d5_if_body_condition_checked() {
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
fn audit_d5_while_body_checked() {
    common::check_ok(
        r#"let x = 0
while x < 10
  x = x + 1
"#,
    );
}

#[test]
fn audit_d5_for_body_checked() {
    common::check_ok(
        r#"let items: List[Int] = [1, 2, 3]
for item in items
  let y = item + 1
"#,
    );
}

// D6/D7: Behavior-lock for nullable and error or/or_else handlers
#[test]
fn audit_d6_nullable_or_returns_inner_type() {
    common::check_ok(
        r#"let x: Int? = 5
let y: Int = x.or(default: 0)
"#,
    );
}

#[test]
fn audit_d6_nullable_or_else_returns_inner_type() {
    common::check_ok(
        r#"let x: Int? = nil
let y: Int = x.or_else(f: -> 0)
"#,
    );
}

#[test]
fn audit_d6_nullable_or_type_mismatch() {
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
fn audit_d7_error_or_returns_success_type() {
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
fn audit_d7_error_or_else_returns_success_type() {
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
fn audit_d7_error_or_type_mismatch() {
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
fn audit_d8_expr_as_statement() {
    common::check_ok(
        r#"def f() -> Int
  let x = 1
  x + 2
"#,
    );
}

#[test]
fn audit_d8_match_as_statement() {
    common::check_ok(
        r#"let x = match 1
  1 => "one"
  _ => "other"
"#,
    );
}

// Q5: Nullable handling in call context — lock behavior
#[test]
fn audit_q5_nullable_or_throw_in_throws_context() {
    common::check_ok(
        r#"class AppError extends Error
  code: Int

def risky(x: Int?) throws AppError -> Int
  x.or_throw(error: AppError(message: "null", code: 0))
"#,
    );
}

#[test]
fn audit_q5_nullable_method_call_rejected() {
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
fn audit_l4_tab_indentation_error() {
    let result = lexer::lex("def f() -> Int\n\treturn 1\n");
    assert!(result.is_err(), "Tab indentation should be rejected");
    let err = result.unwrap_err().to_string();
    assert!(err.contains("tab") || err.contains("Tab") || err.contains("indent"));
}
