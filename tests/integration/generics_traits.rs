
// ─── Generics and traits ────────────────────────────────────────────

#[test]
fn generic_class() {
    crate::common::check_ok("class Box[T]\n  value: Int\n");
}

#[test]
fn generic_function() {
    crate::common::check_ok("def identity(x: T) -> T\n  x\n");
}

#[test]
fn trait_definition() {
    crate::common::check_ok("trait Printable\n  def to_string() -> String\n");
}

#[test]
fn pub_trait() {
    crate::common::check_ok("pub trait Printable\n  def to_string() -> String\n");
}

#[test]
fn trait_with_default_method() {
    crate::common::check_ok(
        r#"trait Printable
  def to_str() -> String
  def print()
    log(message: "hello")
"#,
    );
}

#[test]
fn class_includes_trait() {
    crate::common::check_ok(
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
fn class_includes_unknown_trait_error() {
    let err = crate::common::check_err("class User includes NonExistent\n  name: String\n");
    assert!(err.contains("Unknown trait") || err.contains("NonExistent"));
}

#[test]
fn class_missing_trait_method_error() {
    let err = crate::common::check_err(
        r#"trait Printable
  def to_str() -> String

class User includes Printable
  name: String
"#,
    );
    assert!(err.contains("to_str") || err.contains("implement") || err.contains("missing"));
}

#[test]
fn class_includes_multiple_traits() {
    crate::common::check_ok(
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
fn generic_class_with_includes() {
    crate::common::check_ok(
        r#"trait Printable
  def to_str() -> String

class Container[T] includes Printable
  value: Int
  def to_str() -> String
    "container"
"#,
    );
}

// ─── Generic class Self type carries type args ─────────────────────

#[test]
fn generic_class_self_carries_type_params() {
    crate::common::check_ok(
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
    crate::common::check_ok(
        r#"class Container[T]
  value: T

  def identity() -> Self
    Container(value: value)

let c = Container(value: 42)
let c2 = c.identity()
"#,
    );
}

// ─── Generic function call-site unification ─────────────────────────

#[test]
fn generic_function_call() {
    crate::common::check_ok("def identity(x: T) -> T\n  x\nlet y = identity(x: 42)\n");
}

#[test]
fn generic_function_call_string() {
    crate::common::check_ok("def identity(x: T) -> T\n  x\nlet y = identity(x: \"hello\")\n");
}

#[test]
fn generic_multi_param_call() {
    crate::common::check_ok("def first(a: A, b: B) -> A\n  a\nlet y = first(a: 1, b: \"hello\")\n");
}

// ─── Trait signature mismatch ────────────────────────────────────────

#[test]
fn trait_signature_mismatch_error() {
    let err = crate::common::check_err(
        r#"trait Displayable
  def display() -> String

class Item includes Displayable
  def display() -> Int
    0
"#,
    );
    assert!(err.contains("signature") || err.contains("mismatch") || err.contains("display"));
}

// ─── Method access uses unqualified name ────────────────────────────

#[test]
fn member_method_access() {
    crate::common::check_ok(
        r#"class Greeter
  name: String
  def greet() -> String
    "hello"
"#,
    );
}

// ─── Return type validation ─────────────────────────────────────────

#[test]
fn return_type_mismatch_error() {
    let err = crate::common::check_err("def f() -> Int\n  return \"hello\"\n");
    assert!(err.contains("return") || err.contains("mismatch") || err.contains("Return"));
}

// ─── Block scoping ─────────────────────────────────────────────────

#[test]
fn if_scope_doesnt_leak() {
    let err = crate::common::check_err("if true\n  let inner = 1\nlet y = inner\n");
    assert!(err.contains("Unknown") || err.contains("inner"));
}

#[test]
fn while_scope_doesnt_leak() {
    let err = crate::common::check_err("while true\n  let inner = 1\n  break\nlet y = inner\n");
    assert!(err.contains("Unknown") || err.contains("inner"));
}

#[test]
fn for_var_doesnt_leak() {
    let err = crate::common::check_err("let xs = [1, 2]\nfor x in xs\n  let inner = 1\nlet y = x\n");
    assert!(err.contains("Unknown") || err.contains("x"));
}

// ─── Symmetric TypeVar unification ──────────────────────────────────

#[test]
fn typevar_unification_symmetric() {
    // TypeVar on right side should also unify
    crate::common::check_ok(
        r#"def identity(x: T) -> T
  x

def apply(f: Fn(Int) -> Int, val: Int) -> Int
  f(_0: val)

let result = apply(f: identity, val: 5)
"#,
    );
}

// ─── General subtype polymorphism ───────────────────────────────────

#[test]
fn subtype_in_function_arg() {
    crate::common::check_ok(
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
    crate::common::check_ok(
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
    crate::common::check_ok(
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
    let err = crate::common::check_err(
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

// ─── Method override in subclass ────────────────────────────────────

#[test]
fn method_override_in_subclass() {
    // Child class redefines parent method with same signature
    crate::common::check_ok(
        "\
class Animal
  name: String
  def speak() -> String
    \"...\"

class Dog extends Animal
  breed: String
  def speak() -> String
    \"woof\"
",
    );
}

// ─── Class inheritance: field and method access ─────────────────────

#[test]
fn inherited_field_access() {
    crate::common::check_ok(
        r#"class Animal
  name: String

class Dog extends Animal
  breed: String

let d = Dog(name: "buddy", breed: "lab")
let n: String = d.name
"#,
    );
}

#[test]
fn inherited_method_access() {
    crate::common::check_ok(
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

// ─── Cyclic and shadowed inheritance ────────────────────────────────

#[test]
fn cyclic_extends_detection() {
    // Self-cycle: class extends itself
    let err = crate::common::check_err(
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

#[test]
fn field_shadowing_in_extends_error() {
    let err = crate::common::check_err(
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

// ─── Generic and trait parsing behavior-locks ───────────────────────

#[test]
fn generic_class_params_parsed() {
    crate::common::check_ok(
        r#"class Pair[A, B]
  first: A
  second: B
"#,
    );
}

#[test]
fn generic_function_params_parsed() {
    crate::common::check_ok(
        r#"def identity(x: T) -> T
  x
"#,
    );
}

#[test]
fn includes_multiple_traits_parsed() {
    crate::common::check_ok(
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

// ─── Subtype polymorphism: return types and bare passing ────────────

#[test]
fn return_subtype_accepted() {
    // Returning a Dog from a function declared -> Animal should work
    crate::common::check_ok(
        r#"class Animal
  name: String

class Dog extends Animal
  breed: String

def test() -> Animal
  return Dog(name: "Rex", breed: "Lab")
"#,
    );
}

#[test]
fn implicit_return_subtype_accepted() {
    crate::common::check_ok(
        r#"class Animal
  name: String

class Dog extends Animal
  breed: String

def test() -> Animal
  Dog(name: "Rex", breed: "Lab")
"#,
    );
}

#[test]
fn return_unrelated_type_rejected() {
    let err = crate::common::check_err(
        r#"class Animal
  name: String

class Car
  model: String

def test() -> Animal
  return Car(model: "Tesla")
"#,
    );
    assert!(
        err.contains("mismatch") || err.contains("expected"),
        "Expected type mismatch error, got: {}",
        err
    );
}

#[test]
fn bare_subtype_compatible_in_let() {
    crate::common::check_ok(
        r#"class Animal
  tag: String

class Dog extends Animal
  breed: String

let a: Animal = Dog(tag: "a", breed: "lab")
say(message: a.tag)
"#,
    );
}

#[test]
fn direct_subtype_passing_accepted() {
    crate::common::check_ok(
        r#"class Animal
  name: String

class Dog extends Animal
  breed: String

def greet(a: Animal) -> String
  a.name

greet(a: Dog(name: "Rex", breed: "Lab"))
"#,
    );
}
