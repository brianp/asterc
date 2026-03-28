use std::collections::HashMap;

// ─── FieldAccessible Trait ─────────────────────────────────────────
//
// Tests for the FieldAccessible trait: auto-generated FieldValue enum,
// field_value(name) -> FieldValue? method, unstable gating, composition
// with inheritance, DynamicReceiver, and introspection.
//
// The auto-generated enum is named ClassNameFieldValue (flat, no dots)
// since the parser does not support dotted type paths in annotations.

// ═══════════════════════════════════════════════════════════════════════
// Contract tests: FieldAccessible exists and requires unstable import
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn field_accessible_requires_unstable_import() {
    // Using FieldAccessible without importing std/unstable should fail (module mode).
    let err = crate::common::check_err_with_files(
        "\
class Person includes FieldAccessible
  name: String
  age: Int
",
        HashMap::new(),
    );
    assert!(
        err.contains("FieldAccessible"),
        "Expected error mentioning FieldAccessible, got: {}",
        err
    );
}

#[test]
fn field_accessible_requires_unstable_flag() {
    // Importing std/unstable without --unstable flag should fail.
    let err = crate::common::check_err_with_files(
        "\
use std/unstable { FieldAccessible }

class Person includes FieldAccessible
  name: String
  age: Int
",
        HashMap::new(),
    );
    assert!(
        err.contains("--unstable"),
        "Expected --unstable error, got: {}",
        err
    );
}

#[test]
fn field_accessible_exported_from_std_unstable() {
    // With --unstable, importing FieldAccessible should succeed.
    crate::common::check_ok_with_files_unstable(
        "\
use std/unstable { FieldAccessible }

class Person includes FieldAccessible
  name: String
  age: Int
",
        HashMap::new(),
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Happy path: field_value method is auto-generated
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn field_value_returns_nullable_for_known_field() {
    crate::common::check_ok_with_files_unstable(
        r#"use std/unstable { FieldAccessible }

class Person includes FieldAccessible
  name: String
  age: Int

let p = Person(name: "Alice", age: 30)
let v = p.field_value(name: "name")
"#,
        HashMap::new(),
    );
}

#[test]
fn field_value_returns_nil_for_unknown_field() {
    crate::common::check_ok_with_files_unstable(
        r#"use std/unstable { FieldAccessible }

class Person includes FieldAccessible
  name: String
  age: Int

let p = Person(name: "Alice", age: 30)
let v = p.field_value(name: "nonexistent")
"#,
        HashMap::new(),
    );
}

#[test]
fn field_value_return_type_is_nullable() {
    // The return type should be PersonFieldValue? (nullable).
    crate::common::check_ok_with_files_unstable(
        r#"use std/unstable { FieldAccessible }

class Person includes FieldAccessible
  name: String
  age: Int

let p = Person(name: "Alice", age: 30)
let v: PersonFieldValue? = p.field_value(name: "name")
"#,
        HashMap::new(),
    );
}

#[test]
fn field_value_works_with_multiple_field_types() {
    crate::common::check_ok_with_files_unstable(
        r#"use std/unstable { FieldAccessible }

class Record includes FieldAccessible
  label: String
  count: Int
  ratio: Float
  active: Bool

let r = Record(label: "test", count: 5, ratio: 3.14, active: true)
let a = r.field_value(name: "label")
let b = r.field_value(name: "count")
let c = r.field_value(name: "ratio")
let d = r.field_value(name: "active")
"#,
        HashMap::new(),
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Auto-generated FieldValue enum: matchable by user code
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn field_value_enum_is_matchable() {
    crate::common::check_ok_with_files_unstable(
        r#"use std/unstable { FieldAccessible }

class Person includes FieldAccessible
  name: String
  age: Int

let p = Person(name: "Alice", age: 30)
let v = p.field_value(name: "name")
let result = match v
  nil => "none"
  _ => "found"
"#,
        HashMap::new(),
    );
}

#[test]
fn field_value_enum_variants_named_after_fields() {
    // Each field produces a variant: PersonFieldValue.Name, PersonFieldValue.Age
    crate::common::check_ok_with_files_unstable(
        r#"use std/unstable { FieldAccessible }

class Person includes FieldAccessible
  name: String
  age: Int

let p = Person(name: "Alice", age: 30)
let v = p.field_value(name: "name")
match v
  PersonFieldValue.Name => "name"
  PersonFieldValue.Age => "age"
  nil => "nil"
"#,
        HashMap::new(),
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Boundary tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn field_accessible_single_field_class() {
    crate::common::check_ok_with_files_unstable(
        r#"use std/unstable { FieldAccessible }

class Wrapper includes FieldAccessible
  value: Int

let w = Wrapper(value: 42)
let v = w.field_value(name: "value")
"#,
        HashMap::new(),
    );
}

#[test]
fn field_accessible_many_fields() {
    crate::common::check_ok_with_files_unstable(
        r#"use std/unstable { FieldAccessible }

class Config includes FieldAccessible
  host: String
  port: Int
  debug: Bool
  timeout: Float
  name: String

let c = Config(host: "localhost", port: 8080, debug: false, timeout: 30.0, name: "app")
let a = c.field_value(name: "host")
let b = c.field_value(name: "port")
let d = c.field_value(name: "debug")
let e = c.field_value(name: "timeout")
let f = c.field_value(name: "name")
"#,
        HashMap::new(),
    );
}

#[test]
fn field_value_empty_string_returns_nil() {
    crate::common::check_ok_with_files_unstable(
        r#"use std/unstable { FieldAccessible }

class Person includes FieldAccessible
  name: String
  age: Int

let p = Person(name: "Alice", age: 30)
let v = p.field_value(name: "")
"#,
        HashMap::new(),
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Error / rejection tests
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn field_accessible_rejected_on_generic_class() {
    let err = crate::common::check_err_with_files_unstable(
        r#"use std/unstable { FieldAccessible }

class Box[T] includes FieldAccessible
  value: T
"#,
        HashMap::new(),
    );
    assert!(
        err.to_lowercase().contains("generic") || err.contains("FieldAccessible"),
        "Expected error about generic classes, got: {}",
        err
    );
}

#[test]
fn field_accessible_rejects_user_defined_field_value() {
    let err = crate::common::check_err_with_files_unstable(
        r#"use std/unstable { FieldAccessible }

class Person includes FieldAccessible
  name: String
  age: Int

  def field_value(name: String) -> String
    "custom"
"#,
        HashMap::new(),
    );
    assert!(
        err.contains("field_value"),
        "Expected error about conflicting field_value definition, got: {}",
        err
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Composition: inheritance
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn field_accessible_includes_inherited_fields() {
    crate::common::check_ok_with_files_unstable(
        r#"use std/unstable { FieldAccessible }

class Base
  message: String

class AppError extends Base includes FieldAccessible
  code: Int

let e = AppError(message: "oops", code: 404)
let v = e.field_value(name: "message")
"#,
        HashMap::new(),
    );
}

#[test]
fn field_accessible_inherited_field_enum_has_all_variants() {
    crate::common::check_ok_with_files_unstable(
        r#"use std/unstable { FieldAccessible }

class Base
  message: String

class AppError extends Base includes FieldAccessible
  code: Int

let e = AppError(message: "oops", code: 404)
match e.field_value(name: "message")
  AppErrorFieldValue.Message => "message"
  AppErrorFieldValue.Code => "code"
  nil => "nil"
"#,
        HashMap::new(),
    );
}

#[test]
fn subclass_can_independently_include_field_accessible() {
    crate::common::check_ok_with_files_unstable(
        r#"use std/unstable { FieldAccessible }

class Parent includes FieldAccessible
  name: String

class Child extends Parent includes FieldAccessible
  age: Int

let p = Parent(name: "Alice")
let c = Child(name: "Bob", age: 10)
let pv = p.field_value(name: "name")
let cv = c.field_value(name: "name")
let cv2 = c.field_value(name: "age")
"#,
        HashMap::new(),
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Composition: with other traits
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn field_accessible_with_eq() {
    crate::common::check_ok_with_files_unstable(
        r#"use std/unstable { FieldAccessible }
use std/cmp { Eq }

class Point includes Eq, FieldAccessible
  x: Int
  y: Int

let a = Point(x: 1, y: 2)
let b = Point(x: 1, y: 2)
let eq = a == b
let v = a.field_value(name: "x")
"#,
        HashMap::new(),
    );
}

#[test]
fn field_accessible_with_printable() {
    crate::common::check_ok_with_files_unstable(
        r#"use std/unstable { FieldAccessible }
use std/fmt { Printable }

class Item includes Printable, FieldAccessible
  label: String
  count: Int

let i = Item(label: "widget", count: 5)
let s = i.to_string()
let v = i.field_value(name: "label")
"#,
        HashMap::new(),
    );
}

#[test]
fn field_accessible_with_dynamic_receiver() {
    crate::common::check_ok_with_files_unstable(
        r#"use std/unstable { FieldAccessible }

class FlexObj includes DynamicReceiver, FieldAccessible
  data: String

  def method_missing(fn_name: String, args: Map[String, String]) -> Void
    data = fn_name

let obj = FlexObj(data: "init")
obj.unknown_method()
let v = obj.field_value(name: "data")
"#,
        HashMap::new(),
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Composition: field_value exposes private fields
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn field_accessible_exposes_private_fields() {
    crate::common::check_ok_with_files_unstable(
        r#"use std/unstable { FieldAccessible }

class Secret includes FieldAccessible
  pub label: String
  password: String

let s = Secret(label: "admin", password: "hunter2")
let v = s.field_value(name: "password")
"#,
        HashMap::new(),
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Integration: field_value usable in functions
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn field_value_callable_from_function() {
    crate::common::check_ok_with_files_unstable(
        r#"use std/unstable { FieldAccessible }

class Person includes FieldAccessible
  name: String
  age: Int

def get_field(p: Person, field_name: String) -> PersonFieldValue?
  p.field_value(name: field_name)

let p = Person(name: "Alice", age: 30)
let v = get_field(p: p, field_name: "name")
"#,
        HashMap::new(),
    );
}

#[test]
fn field_value_type_usable_as_parameter() {
    crate::common::check_ok_with_files_unstable(
        r#"use std/unstable { FieldAccessible }

class Person includes FieldAccessible
  name: String
  age: Int

def describe(v: PersonFieldValue?) -> String
  match v
    nil => "nil"
    _ => "value"

let p = Person(name: "Alice", age: 30)
let result = describe(v: p.field_value(name: "name"))
"#,
        HashMap::new(),
    );
}
