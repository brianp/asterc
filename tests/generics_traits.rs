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

// ─── S4: Generic class Self type carries type args ──────────────────

#[test]
fn generic_class_self_carries_type_params() {
    common::check_ok(
        r#"class Box[T]
  value: T

  def get() -> T
    value

let b = Box(value: 42)
let v: Int = b.get()
"#,
    );
}

#[test]
fn generic_class_method_returns_self_with_params() {
    // When a generic class method returns Self, Self should carry the type params.
    // class_type for generic classes should include TypeVar args so Self resolves correctly.
    common::check_ok(
        r#"class Container[T]
  value: T

  def identity() -> Self
    Container(value: value)

let c = Container(value: 42)
let c2 = c.identity()
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

// ─── H1: Symmetric TypeVar unification ─────────────────────────────

#[test]
fn typevar_unification_symmetric() {
    // TypeVar on right side should also unify
    common::check_ok(
        r#"def identity(x: T) -> T
  x

def apply(f: (Int) -> Int, val: Int) -> Int
  f(_0: val)

let result = apply(f: identity, val: 5)
"#,
    );
}

// ─── S3: General subtype polymorphism ──────────────────────────────

#[test]
fn subtype_in_function_arg() {
    common::check_ok(
        r#"class Animal
  name: String

class Dog extends Animal
  breed: String

def greet(a: Animal) -> String
  a.name

let d = Dog(name: "Rex", breed: "Lab")
let g = greet(a: d)
"#,
    );
}

#[test]
fn subtype_in_let_annotation() {
    common::check_ok(
        r#"class Animal
  name: String

class Dog extends Animal
  breed: String

let a: Animal = Dog(name: "Rex", breed: "Lab")
"#,
    );
}

#[test]
fn subtype_deep_chain() {
    common::check_ok(
        r#"class Base
  x: Int

class Middle extends Base
  y: Int

class Leaf extends Middle
  z: Int

def use_base(b: Base) -> Int
  b.x

let leaf = Leaf(x: 1, y: 2, z: 3)
let r = use_base(b: leaf)
"#,
    );
}

#[test]
fn non_subtype_still_rejected() {
    let err = common::check_err(
        r#"class Cat
  name: String

class Dog
  name: String

def greet(c: Cat) -> String
  c.name

let d = Dog(name: "Rex")
let g = greet(c: d)
"#,
    );
    assert!(err.contains("mismatch") || err.contains("Cat") || err.contains("Dog"));
}
