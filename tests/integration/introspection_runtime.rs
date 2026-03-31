// ─── Introspection Runtime Tests ─────���───────────────────────────────
//
// End-to-end runtime tests for introspection methods.
// These tests compile and execute programs via the CLI to verify
// introspection works at runtime, not just at the type-checking level.

fn run_introspection_program(src: &str) -> String {
    let dir = crate::common::make_temp_dir("introspect");
    let src_path = dir.join("test.aster");
    std::fs::write(&src_path, src).unwrap();
    let output = crate::common::cli(&["run", src_path.to_str().unwrap()]);
    assert!(
        output.status.success(),
        "program failed: {}",
        crate::common::output_text(&output)
    );
    String::from_utf8_lossy(&output.stdout).to_string()
}

#[test]
fn runtime_class_name_say() {
    let out = run_introspection_program(
        r#"class Dog
  name: String

let d = Dog(name: "Rex")
say(d.class_name.to_string())
"#,
    );
    assert_eq!(out.trim(), "Dog");
}

#[test]
fn runtime_is_a_same_class() {
    let out = run_introspection_program(
        r#"class Dog
  name: String

let d = Dog(name: "Rex")
say(d.is_a(Dog))
"#,
    );
    assert_eq!(out.trim(), "true");
}

#[test]
fn runtime_is_a_parent_class() {
    let out = run_introspection_program(
        r#"class Animal
  name: String

class Dog extends Animal
  breed: String

let d = Dog(name: "Rex", breed: "Lab")
say(d.is_a(Animal))
"#,
    );
    assert_eq!(out.trim(), "true");
}

#[test]
fn runtime_is_a_unrelated_false() {
    let out = run_introspection_program(
        r#"class Dog
  name: String

class Cat
  name: String

let d = Dog(name: "Rex")
say(d.is_a(Cat))
"#,
    );
    assert_eq!(out.trim(), "false");
}

#[test]
fn runtime_is_a_transitive_ancestor() {
    let out = run_introspection_program(
        r#"class AppError extends Error
  code: Int

class NetworkError extends AppError
  url: String

let e = NetworkError(message: "timeout", code: 500, url: "http://x")
say(e.is_a(Error))
say(e.is_a(Exception))
"#,
    );
    assert_eq!(out.trim(), "true\ntrue");
}

#[test]
fn runtime_responds_to_own_method() {
    let out = run_introspection_program(
        r#"class User
  name: String

  def greet() -> String
    "hello"

let u = User(name: "Alice")
say(u.responds_to("greet"))
say(u.responds_to("name"))
say(u.responds_to("nonexistent"))
"#,
    );
    assert_eq!(out.trim(), "true\ntrue\nfalse");
}

#[test]
fn runtime_responds_to_inherited_method() {
    let out = run_introspection_program(
        r#"class Animal
  def speak() -> String
    "..."

class Dog extends Animal
  def fetch() -> String
    "ball"

let d = Dog()
say(d.responds_to("speak"))
say(d.responds_to("fetch"))
"#,
    );
    assert_eq!(out.trim(), "true\ntrue");
}

#[test]
fn runtime_responds_to_builtin_methods() {
    let out = run_introspection_program(
        r#"say(42.responds_to("abs"))
say("hello".responds_to("len"))
say(3.14.responds_to("round"))
say([1,2].responds_to("push"))
"#,
    );
    assert_eq!(out.trim(), "true\ntrue\ntrue\ntrue");
}

#[test]
fn runtime_fields_on_class() {
    let out = run_introspection_program(
        r#"class User
  name: String
  age: Int

let u = User(name: "Alice", age: 30)
let fs = u.fields
say(fs.len())
say(fs[0].name)
say(fs[0].type_name.to_string())
say(fs[1].name)
say(fs[1].type_name.to_string())
"#,
    );
    assert_eq!(out.trim(), "2\nname\nString\nage\nInt");
}

#[test]
fn runtime_fields_on_primitive() {
    let out = run_introspection_program(
        r#"say(42.fields.len())
"#,
    );
    assert_eq!(out.trim(), "0");
}

#[test]
fn runtime_fields_inherited() {
    let out = run_introspection_program(
        r#"class AppError extends Error
  code: Int

let e = AppError(message: "oops", code: 404)
let fs = e.fields
say(fs.len())
say(fs[0].name)
say(fs[1].name)
"#,
    );
    // message from Exception, code from AppError
    assert_eq!(out.trim(), "2\nmessage\ncode");
}

#[test]
fn runtime_methods_on_class() {
    let out = run_introspection_program(
        r#"class Greeter
  def greet() -> String
    "hello"

  def farewell() -> String
    "bye"

let g = Greeter()
let ms = g.methods
say(ms.len())
"#,
    );
    // Should have at least the 2 user-defined methods
    let count: i32 = out.trim().parse().unwrap();
    assert!(count >= 2, "expected at least 2 methods, got {}", count);
}

#[test]
fn runtime_methods_has_params() {
    let out = run_introspection_program(
        r#"class Calc
  def add(x: Int, y: Int) -> Int
    x + y

let c = Calc()
let mi = c.methods[0]
say(mi.name)
say(mi.return_type.to_string())
say(mi.params.len())
say(mi.params[0].name)
say(mi.params[0].param_type.to_string())
"#,
    );
    assert_eq!(out.trim(), "add\nInt\n2\nx\nInt");
}

#[test]
fn runtime_ancestors_root_class() {
    let out = run_introspection_program(
        r#"class Standalone
  value: Int

let s = Standalone(value: 42)
let a = s.ancestors
say(a.len())
say(a[0].to_string())
"#,
    );
    assert_eq!(out.trim(), "1\nStandalone");
}

#[test]
fn runtime_ancestors_with_hierarchy() {
    let out = run_introspection_program(
        r#"class AppError extends Error
  code: Int

class NetworkError extends AppError
  url: String

let e = NetworkError(message: "timeout", code: 500, url: "http://x")
let a = e.ancestors
say(a.len())
say(a[0].to_string())
say(a[1].to_string())
say(a[2].to_string())
say(a[3].to_string())
"#,
    );
    assert_eq!(out.trim(), "4\nNetworkError\nAppError\nError\nException");
}

#[test]
fn runtime_children_on_parent() {
    let out = run_introspection_program(
        r#"class Animal
  name: String

class Dog extends Animal
  breed: String

class Cat extends Animal
  color: String

let a = Animal(name: "Rex")
let c = a.children
say(c.len())
"#,
    );
    let count: i32 = out.trim().parse().unwrap();
    assert_eq!(count, 2, "expected 2 children, got {}", count);
}

#[test]
fn runtime_children_on_leaf() {
    let out = run_introspection_program(
        r#"class Animal
  name: String

class Dog extends Animal
  breed: String

let d = Dog(name: "Rex", breed: "Lab")
say(d.children.len())
"#,
    );
    assert_eq!(out.trim(), "0");
}

#[test]
fn runtime_int_class_name() {
    let out = run_introspection_program(
        r#"say(42.class_name.to_string())
"#,
    );
    assert_eq!(out.trim(), "Int");
}

#[test]
fn runtime_int_is_a() {
    let out = run_introspection_program(
        r#"say(42.is_a(Int))
say(42.is_a(String))
"#,
    );
    assert_eq!(out.trim(), "true\nfalse");
}

#[test]
fn runtime_int_methods() {
    let out = run_introspection_program(
        r#"let ms = 42.methods
say(ms.len() > 0)
"#,
    );
    assert_eq!(out.trim(), "true");
}

#[test]
fn runtime_field_info_is_public() {
    let out = run_introspection_program(
        r#"class User
  name: String

let u = User(name: "Alice")
let fi = u.fields[0]
say(fi.is_public)
"#,
    );
    // Fields default to non-public
    assert_eq!(out.trim(), "false");
}

#[test]
fn runtime_type_equality() {
    let out = run_introspection_program(
        r#"class Dog
  name: String

class Cat
  name: String

let d = Dog(name: "Rex")
let d2 = Dog(name: "Buddy")
let c = Cat(name: "Whiskers")
say(d.class_name == d2.class_name)
say(d.class_name != c.class_name)
"#,
    );
    assert_eq!(out.trim(), "true\ntrue");
}
