// ─── Introspection API ──────────────────────────────────────────────
//
// Tests for the 7 built-in introspection methods available on every instance:
//   class_name, fields, methods, ancestors, children, is_a, responds_to
//
// Also tests: Type as a value (comparable, stringifiable),
//             FieldInfo, MethodInfo, ParamInfo built-in types.

// ─── Contract tests: introspection methods exist and return correct types ────

#[test]
fn class_name_returns_type() {
    crate::common::check_ok(
        r#"class User
  name: String

let u = User(name: "Alice")
let t: Type = u.class_name
"#,
    );
}

#[test]
fn fields_returns_list_of_field_info() {
    crate::common::check_ok(
        r#"class User
  name: String
  age: Int

let u = User(name: "Alice", age: 30)
let fs: List[FieldInfo] = u.fields
"#,
    );
}

#[test]
fn methods_returns_list_of_method_info() {
    crate::common::check_ok(
        r#"class User
  name: String

  def greet() -> String
    "hello"

let u = User(name: "Alice")
let ms: List[MethodInfo] = u.methods
"#,
    );
}

#[test]
fn ancestors_returns_list_of_type() {
    crate::common::check_ok(
        r#"class Animal
  name: String

class Dog extends Animal
  breed: String

let d = Dog(name: "Rex", breed: "Lab")
let a: List[Type] = d.ancestors
"#,
    );
}

#[test]
fn children_returns_list_of_type() {
    crate::common::check_ok(
        r#"class Animal
  name: String

class Dog extends Animal
  breed: String

let a = Animal(name: "Rex")
let c: List[Type] = a.children
"#,
    );
}

#[test]
fn is_a_returns_bool() {
    crate::common::check_ok(
        r#"class User
  name: String

let u = User(name: "Alice")
let b: Bool = u.is_a(User)
"#,
    );
}

#[test]
fn responds_to_returns_bool() {
    crate::common::check_ok(
        r#"class User
  name: String

  def greet() -> String
    "hello"

let u = User(name: "Alice")
let b: Bool = u.responds_to("greet")
"#,
    );
}

// ─── Happy path: class_name ─────────────────────────────────────────

#[test]
fn class_name_to_string() {
    crate::common::check_ok(
        r#"class User
  name: String

let u = User(name: "Alice")
let s: String = u.class_name.to_string()
"#,
    );
}

#[test]
fn class_name_equality() {
    crate::common::check_ok(
        r#"class User
  name: String

let u1 = User(name: "Alice")
let u2 = User(name: "Bob")
let same: Bool = u1.class_name == u2.class_name
"#,
    );
}

#[test]
fn class_name_inequality_different_classes() {
    crate::common::check_ok(
        r#"class Dog
  name: String

class Cat
  name: String

let d = Dog(name: "Rex")
let c = Cat(name: "Whiskers")
let diff: Bool = d.class_name != c.class_name
"#,
    );
}

// ─── Happy path: is_a ───────────────────────────────────────────────

#[test]
fn is_a_same_class() {
    crate::common::check_ok(
        r#"class User
  name: String

let u = User(name: "Alice")
let b = u.is_a(User)
"#,
    );
}

#[test]
fn is_a_parent_class() {
    crate::common::check_ok(
        r#"class Animal
  name: String

class Dog extends Animal
  breed: String

let d = Dog(name: "Rex", breed: "Lab")
let b = d.is_a(Animal)
"#,
    );
}

#[test]
fn is_a_transitive_ancestor() {
    crate::common::check_ok(
        r#"class AppError extends Error
  code: Int

class NetworkError extends AppError
  url: String

let e = NetworkError(message: "timeout", code: 500, url: "http://x")
let b = e.is_a(Error)
"#,
    );
}

#[test]
fn is_a_unrelated_class_false() {
    crate::common::check_ok(
        r#"class Dog
  name: String

class Cat
  name: String

let d = Dog(name: "Rex")
let b = d.is_a(Cat)
"#,
    );
}

#[test]
fn is_a_with_builtin_error_hierarchy() {
    crate::common::check_ok(
        r#"class AppError extends Error
  code: Int

let e = AppError(message: "oops", code: 500)
let b1 = e.is_a(Error)
let b2 = e.is_a(Exception)
"#,
    );
}

// ─── Happy path: responds_to ────────────────────────────────────────

#[test]
fn responds_to_own_method() {
    crate::common::check_ok(
        r#"class User
  name: String

  def greet() -> String
    "hello"

let u = User(name: "Alice")
let b = u.responds_to("greet")
"#,
    );
}

#[test]
fn responds_to_field_name() {
    crate::common::check_ok(
        r#"class User
  name: String

let u = User(name: "Alice")
let b = u.responds_to("name")
"#,
    );
}

#[test]
fn responds_to_nonexistent_returns_false_type() {
    // responds_to("nonexistent") should typecheck (returns Bool)
    crate::common::check_ok(
        r#"class User
  name: String

let u = User(name: "Alice")
let b = u.responds_to("nonexistent")
"#,
    );
}

// ─── Happy path: fields ─────────────────────────────────────────────

#[test]
fn fields_on_class_with_fields() {
    crate::common::check_ok(
        r#"class User
  name: String
  age: Int

let u = User(name: "Alice", age: 30)
let fs = u.fields
"#,
    );
}

#[test]
fn fields_on_class_with_single_field() {
    crate::common::check_ok(
        r#"class Wrapper
  value: Int

let e = Wrapper(value: 0)
let fs = e.fields
"#,
    );
}

// ─── Happy path: methods ────────────────────────────────────────────

#[test]
fn methods_on_class_with_methods() {
    crate::common::check_ok(
        r#"class Greeter
  name: String

  def greet() -> String
    "hello"

  def farewell() -> String
    "bye"

let g = Greeter(name: "Alice")
let ms = g.methods
"#,
    );
}

#[test]
fn methods_on_class_without_user_methods() {
    crate::common::check_ok(
        r#"class Point
  x: Int
  y: Int

let p = Point(x: 1, y: 2)
let ms = p.methods
"#,
    );
}

// ─── Happy path: ancestors and children ─────────────────────────────

#[test]
fn ancestors_includes_self_and_parents() {
    crate::common::check_ok(
        r#"class AppError extends Error
  code: Int

class NetworkError extends AppError
  url: String

class TimeoutError extends NetworkError
  duration: Int

let e = TimeoutError(message: "timed out", code: 408, url: "http://x", duration: 30)
let a = e.ancestors
"#,
    );
}

#[test]
fn ancestors_on_root_class() {
    crate::common::check_ok(
        r#"class Standalone
  value: Int

let s = Standalone(value: 42)
let a = s.ancestors
"#,
    );
}

#[test]
fn children_on_parent_class() {
    crate::common::check_ok(
        r#"class Animal
  name: String

class Dog extends Animal
  breed: String

class Cat extends Animal
  color: String

let a = Animal(name: "Rex")
let c = a.children
"#,
    );
}

#[test]
fn children_on_leaf_class() {
    crate::common::check_ok(
        r#"class Animal
  name: String

class Dog extends Animal
  breed: String

let d = Dog(name: "Rex", breed: "Lab")
let c = d.children
"#,
    );
}

// ─── Introspection on primitives ────────────────────────────────────

#[test]
fn int_class_name() {
    crate::common::check_ok(
        r#"let t: Type = 42.class_name
"#,
    );
}

#[test]
fn string_class_name() {
    crate::common::check_ok(
        r#"let t: Type = "hello".class_name
"#,
    );
}

#[test]
fn bool_class_name() {
    crate::common::check_ok(
        r#"let t: Type = true.class_name
"#,
    );
}

#[test]
fn float_class_name() {
    crate::common::check_ok(
        r#"let t: Type = 3.14.class_name
"#,
    );
}

#[test]
fn int_responds_to_abs() {
    crate::common::check_ok(
        r#"let b: Bool = 42.responds_to("abs")
"#,
    );
}

#[test]
fn string_responds_to_length() {
    crate::common::check_ok(
        r#"let b: Bool = "hello".responds_to("length")
"#,
    );
}

#[test]
fn int_fields_empty() {
    crate::common::check_ok(
        r#"let fs: List[FieldInfo] = 42.fields
"#,
    );
}

#[test]
fn int_methods_list() {
    crate::common::check_ok(
        r#"let ms: List[MethodInfo] = 42.methods
"#,
    );
}

#[test]
fn int_is_a_int() {
    crate::common::check_ok(
        r#"let b: Bool = 42.is_a(Int)
"#,
    );
}

#[test]
fn string_is_a_string() {
    crate::common::check_ok(
        r#"let b: Bool = "hello".is_a(String)
"#,
    );
}

#[test]
fn int_is_a_string_false() {
    crate::common::check_ok(
        r#"let b: Bool = 42.is_a(String)
"#,
    );
}

#[test]
fn int_ancestors() {
    crate::common::check_ok(
        r#"let a: List[Type] = 42.ancestors
"#,
    );
}

#[test]
fn int_children() {
    crate::common::check_ok(
        r#"let c: List[Type] = 42.children
"#,
    );
}

// ─── Introspection on List ──────────────────────────────────────────

#[test]
fn list_class_name() {
    crate::common::check_ok(
        r#"let xs = [1, 2, 3]
let t: Type = xs.class_name
"#,
    );
}

#[test]
fn list_responds_to_push() {
    crate::common::check_ok(
        r#"let xs = [1, 2, 3]
let b: Bool = xs.responds_to("push")
"#,
    );
}

// ─── Rejection tests: is_a with non-type argument ───────────────────

#[test]
fn is_a_with_string_arg_error() {
    let err = crate::common::check_err(
        r#"class User
  name: String

let u = User(name: "Alice")
let b = u.is_a("User")
"#,
    );
    assert!(
        err.contains("type") || err.contains("Type") || err.contains("class"),
        "expected type error for is_a with string arg, got: {}",
        err
    );
}

#[test]
fn is_a_with_int_arg_error() {
    let err = crate::common::check_err(
        r#"class User
  name: String

let u = User(name: "Alice")
let b = u.is_a(42)
"#,
    );
    assert!(
        err.contains("type") || err.contains("Type") || err.contains("class"),
        "expected type error for is_a with int arg, got: {}",
        err
    );
}

// ─── Rejection tests: responds_to with non-string argument ──────────

#[test]
fn responds_to_with_int_arg_error() {
    let err = crate::common::check_err(
        r#"class User
  name: String

let u = User(name: "Alice")
let b = u.responds_to(42)
"#,
    );
    assert!(
        err.contains("String") || err.contains("mismatch") || err.contains("argument"),
        "expected type error for responds_to with int arg, got: {}",
        err
    );
}

// ─── Composition: introspection with generics ───────────────────────

#[test]
fn introspection_on_generic_class() {
    crate::common::check_ok(
        r#"class Box[T]
  value: T

let b = Box(value: 42)
let t = b.class_name
let fs = b.fields
"#,
    );
}

// ─── Composition: introspection with traits ─────────────────────────

#[test]
fn introspection_on_class_with_trait() {
    crate::common::check_ok(
        r#"trait Printable
  def to_str() -> String

class User includes Printable
  name: String

  def to_str() -> String
    name

let u = User(name: "Alice")
let ms = u.methods
let b = u.responds_to("to_str")
"#,
    );
}

// ─── Composition: user method shadows introspection ─────────────────

#[test]
fn user_method_shadows_introspection() {
    // A class that defines its own `fields` method should use that, not introspection
    crate::common::check_ok(
        r#"class Custom
  name: String

  def fields() -> String
    "custom fields"

let c = Custom(name: "test")
let s: String = c.fields()
"#,
    );
}

// ─── FieldInfo type access ──────────────────────────────────────────

#[test]
fn field_info_has_name() {
    crate::common::check_ok(
        r#"class User
  name: String

let u = User(name: "Alice")
let fi = u.fields[0]
let n: String = fi.name
"#,
    );
}

#[test]
fn field_info_has_type_name() {
    crate::common::check_ok(
        r#"class User
  name: String

let u = User(name: "Alice")
let fi = u.fields[0]
let t: Type = fi.type_name
"#,
    );
}

#[test]
fn field_info_has_is_public() {
    crate::common::check_ok(
        r#"class User
  name: String

let u = User(name: "Alice")
let fi = u.fields[0]
let p: Bool = fi.is_public
"#,
    );
}

// ─── MethodInfo type access ─────────────────────────────────────────

#[test]
fn method_info_has_name() {
    crate::common::check_ok(
        r#"class Greeter
  def greet() -> String
    "hello"

let g = Greeter()
let mi = g.methods[0]
let n: String = mi.name
"#,
    );
}

#[test]
fn method_info_has_return_type() {
    crate::common::check_ok(
        r#"class Greeter
  def greet() -> String
    "hello"

let g = Greeter()
let mi = g.methods[0]
let t: Type = mi.return_type
"#,
    );
}

#[test]
fn method_info_has_params() {
    crate::common::check_ok(
        r#"class Calc
  def add(x: Int, y: Int) -> Int
    x + y

let c = Calc()
let mi = c.methods[0]
let ps: List[ParamInfo] = mi.params
"#,
    );
}

#[test]
fn method_info_has_is_public() {
    crate::common::check_ok(
        r#"class Greeter
  def greet() -> String
    "hello"

let g = Greeter()
let mi = g.methods[0]
let p: Bool = mi.is_public
"#,
    );
}

// ─── ParamInfo type access ──────────────────────────────────────────

#[test]
fn param_info_has_name() {
    crate::common::check_ok(
        r#"class Calc
  def add(x: Int, y: Int) -> Int
    x + y

let c = Calc()
let pi = c.methods[0].params[0]
let n: String = pi.name
"#,
    );
}

#[test]
fn param_info_has_param_type() {
    crate::common::check_ok(
        r#"class Calc
  def add(x: Int, y: Int) -> Int
    x + y

let c = Calc()
let pi = c.methods[0].params[0]
let t: Type = pi.param_type
"#,
    );
}

#[test]
fn param_info_has_has_default() {
    crate::common::check_ok(
        r#"class Calc
  def add(x: Int, y: Int) -> Int
    x + y

let c = Calc()
let pi = c.methods[0].params[0]
let d: Bool = pi.has_default
"#,
    );
}

// ─── Type value comparison and stringification ──────────────────────

#[test]
fn type_equality_same_class() {
    crate::common::check_ok(
        r#"class User
  name: String

let u1 = User(name: "Alice")
let u2 = User(name: "Bob")
let same = u1.class_name == u2.class_name
"#,
    );
}

#[test]
fn type_inequality_different_classes() {
    crate::common::check_ok(
        r#"class Dog
  name: String

class Cat
  name: String

let d = Dog(name: "Rex")
let c = Cat(name: "Whiskers")
let diff = d.class_name != c.class_name
"#,
    );
}

#[test]
fn type_to_string() {
    crate::common::check_ok(
        r#"class User
  name: String

let u = User(name: "Alice")
let s: String = u.class_name.to_string()
"#,
    );
}

// ─── Introspection with inheritance ─────────────────────────────────

#[test]
fn inherited_fields_visible_in_fields() {
    crate::common::check_ok(
        r#"class AppError extends Error
  code: Int

let e = AppError(message: "oops", code: 404)
let fs = e.fields
"#,
    );
}

#[test]
fn inherited_methods_visible_in_responds_to() {
    crate::common::check_ok(
        r#"class Animal
  def speak() -> String
    "..."

class Dog extends Animal
  def fetch() -> String
    "ball"

let d = Dog()
let b = d.responds_to("speak")
"#,
    );
}

// Runtime tests for introspection are in introspection_runtime.rs
