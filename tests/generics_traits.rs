mod common;

// ─── Phase 5: Generics and Traits ───────────────────────────────────

#[test]
fn integration_generic_class() {
    common::check_ok("class Box[T]\n  value: Int\n");
}

#[test]
fn integration_generic_function() {
    common::check_ok("def identity(x: T) -> T\n  x\n");
}

#[test]
fn integration_trait_definition() {
    common::check_ok("trait Printable\n  def to_string() -> String\n");
}

#[test]
fn integration_pub_trait() {
    common::check_ok("pub trait Printable\n  def to_string() -> String\n");
}

#[test]
fn integration_trait_with_default_method() {
    common::check_ok(
        r#"trait Printable
  def to_str() -> String
  def print()
    log(message: "hello")
"#,
    );
}

#[test]
fn integration_class_includes_trait() {
    common::check_ok(
        r#"trait Printable
  def to_str() -> String

class User includes Printable
  name: String
  def to_str() -> String
    "user"
"#,
    );
}

#[test]
fn integration_class_includes_unknown_trait_error() {
    let err = common::check_err("class User includes NonExistent\n  name: String\n");
    assert!(err.contains("Unknown trait") || err.contains("NonExistent"));
}

#[test]
fn integration_class_missing_trait_method_error() {
    let err = common::check_err(
        r#"trait Printable
  def to_str() -> String

class User includes Printable
  name: String
"#,
    );
    assert!(err.contains("to_str") || err.contains("implement") || err.contains("missing"));
}

#[test]
fn integration_class_includes_multiple_traits() {
    common::check_ok(
        r#"trait Printable
  def to_str() -> String

trait Greetable
  def greet() -> String

class User includes Printable, Greetable
  name: String
  def to_str() -> String
    "user"
  def greet() -> String
    "hello"
"#,
    );
}

#[test]
fn integration_generic_class_with_includes() {
    common::check_ok(
        r#"trait Printable
  def to_str() -> String

class Container[T] includes Printable
  value: Int
  def to_str() -> String
    "container"
"#,
    );
}

// ─── Fix #1: Generic function call-site unification ─────────────────

#[test]
fn integration_generic_function_call() {
    common::check_ok("def identity(x: T) -> T\n  x\nlet y = identity(x: 42)\n");
}

#[test]
fn integration_generic_function_call_string() {
    common::check_ok("def identity(x: T) -> T\n  x\nlet y = identity(x: \"hello\")\n");
}

#[test]
fn integration_generic_multi_param_call() {
    common::check_ok("def first(a: A, b: B) -> A\n  a\nlet y = first(a: 1, b: \"hello\")\n");
}

// ─── Fix #4: Trait signature mismatch ───────────────────────────────

#[test]
fn integration_trait_signature_mismatch_error() {
    let err = common::check_err(
        r#"trait Displayable
  def display() -> String

class Item includes Displayable
  def display() -> Int
    0
"#,
    );
    assert!(err.contains("signature") || err.contains("mismatch") || err.contains("display"));
}

// ─── Fix #5: Method access uses unqualified name ────────────────────

#[test]
fn integration_member_method_access() {
    common::check_ok(
        r#"class Greeter
  name: String
  def greet() -> String
    "hello"
"#,
    );
}

// ─── Fix #7: Return type validation ─────────────────────────────────

#[test]
fn integration_return_type_mismatch_error() {
    let err = common::check_err("def f() -> Int\n  return \"hello\"\n");
    assert!(err.contains("return") || err.contains("mismatch") || err.contains("Return"));
}

// ─── Fix #6: Scoping ────────────────────────────────────────────────

#[test]
fn integration_if_scope_doesnt_leak() {
    let err = common::check_err("if true\n  let inner = 1\nlet y = inner\n");
    assert!(err.contains("Unknown") || err.contains("inner"));
}

#[test]
fn integration_while_scope_doesnt_leak() {
    let err = common::check_err("while true\n  let inner = 1\n  break\nlet y = inner\n");
    assert!(err.contains("Unknown") || err.contains("inner"));
}

#[test]
fn integration_for_var_doesnt_leak() {
    let err = common::check_err("let xs = [1, 2]\nfor x in xs\n  let inner = 1\nlet y = x\n");
    assert!(err.contains("Unknown") || err.contains("x"));
}
