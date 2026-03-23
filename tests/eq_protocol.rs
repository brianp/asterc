mod common;

// ============================================================
// Phase 2: Self Type
// ============================================================

#[test]
fn self_type_in_trait_method_parses() {
    common::check_ok(
        "\
trait Eq
  def eq(other: Self) -> Bool
",
    );
}

#[test]
fn self_type_resolves_to_class_type() {
    // Class includes trait with Self — class method uses concrete type, matches
    common::check_ok(
        "\
trait Eq
  def eq(other: Self) -> Bool

class Point includes Eq
  x: Int
  y: Int
  def eq(other: Point) -> Bool
    true
",
    );
}

#[test]
fn self_type_mismatch_in_trait_impl() {
    // Class method uses wrong type instead of Self (resolved to class type)
    let err = common::check_err(
        "\
trait Eq
  def eq(other: Self) -> Bool

class Point includes Eq
  x: Int
  def eq(other: Int) -> Bool
    true
",
    );
    assert!(
        err.contains("signature") || err.contains("mismatch") || err.contains("requires"),
        "got: {}",
        err
    );
}

#[test]
fn self_type_in_return_position() {
    common::check_ok(
        "\
trait Cloneable
  def clone() -> Self
",
    );
}

// ============================================================
// Phase 4A: Eq Protocol — Primitives (no regression)
// ============================================================

#[test]
fn eq_primitives_int() {
    common::check_ok("let x = 1 == 2");
}

#[test]
fn eq_primitives_float() {
    common::check_ok("let x = 1.0 == 2.0");
}

#[test]
fn eq_primitives_string() {
    common::check_ok("let x = \"a\" == \"b\"");
}

#[test]
fn eq_primitives_bool() {
    common::check_ok("let x = true == false");
}

#[test]
fn neq_primitives() {
    common::check_ok("let x = 1 != 2");
}

// ============================================================
// Phase 4B: Eq Protocol — User types with manual Eq
// ============================================================

#[test]
fn user_type_with_eq_can_use_double_equals() {
    common::check_ok(
        "\
trait Eq
  def eq(other: Self) -> Bool

class Point includes Eq
  x: Int
  y: Int
  def eq(other: Point) -> Bool
    true

let p1 = Point(x: 1, y: 2)
let p2 = Point(x: 1, y: 2)
let result = p1 == p2
",
    );
}

#[test]
fn user_type_with_eq_can_use_not_equals() {
    common::check_ok(
        "\
trait Eq
  def eq(other: Self) -> Bool

class Point includes Eq
  x: Int
  y: Int
  def eq(other: Point) -> Bool
    true

let p1 = Point(x: 1, y: 2)
let p2 = Point(x: 3, y: 4)
let result = p1 != p2
",
    );
}

#[test]
fn user_type_without_eq_cannot_use_double_equals() {
    let err = common::check_err(
        "\
class Point
  x: Int
  y: Int

let p1 = Point(x: 1, y: 2)
let p2 = Point(x: 1, y: 2)
let result = p1 == p2
",
    );
    assert!(
        err.contains("does not include Eq") || err.contains("Eq"),
        "got: {}",
        err
    );
}

#[test]
fn user_type_without_eq_cannot_use_not_equals() {
    let err = common::check_err(
        "\
class Point
  x: Int
  y: Int

let p1 = Point(x: 1, y: 2)
let p2 = Point(x: 1, y: 2)
let result = p1 != p2
",
    );
    assert!(
        err.contains("does not include Eq") || err.contains("Eq"),
        "got: {}",
        err
    );
}

#[test]
fn eq_different_user_types_error() {
    let err = common::check_err(
        "\
trait Eq
  def eq(other: Self) -> Bool

class Point includes Eq
  x: Int
  def eq(other: Point) -> Bool
    true

class Circle includes Eq
  r: Int
  def eq(other: Circle) -> Bool
    true

let p = Point(x: 1)
let c = Circle(r: 2)
let result = p == c
",
    );
    assert!(
        err.contains("Cannot compare") || err.contains("mismatch") || err.contains("incompatible"),
        "got: {}",
        err
    );
}

#[test]
fn eq_returns_bool_type() {
    common::check_ok(
        "\
trait Eq
  def eq(other: Self) -> Bool

class Point includes Eq
  x: Int
  def eq(other: Point) -> Bool
    true

let p1 = Point(x: 1)
let p2 = Point(x: 1)
let b: Bool = p1 == p2
",
    );
}

// ============================================================
// Phase 4C: Auto-derive Eq
// ============================================================

#[test]
fn auto_derive_eq_all_fields_eq() {
    // Class includes Eq without defining eq() — all fields are primitive (include Eq)
    common::check_ok(
        "\
class Point includes Eq
  x: Int
  y: Int

let p1 = Point(x: 1, y: 2)
let p2 = Point(x: 1, y: 2)
let result = p1 == p2
",
    );
}

#[test]
fn auto_derive_eq_nested() {
    // Nested auto-derive: Line has Point fields, both include Eq
    common::check_ok(
        "\
class Point includes Eq
  x: Int
  y: Int

class Line includes Eq
  start: Point
  end: Point

let l1 = Line(start: Point(x: 0, y: 0), end: Point(x: 1, y: 1))
let l2 = Line(start: Point(x: 0, y: 0), end: Point(x: 1, y: 1))
let result = l1 == l2
",
    );
}

#[test]
fn auto_derive_eq_field_without_eq_error() {
    // Auto-derive should fail if a field's type doesn't include Eq
    let err = common::check_err(
        "\
class NoEq
  value: Int

class Wrapper includes Eq
  inner: NoEq
",
    );
    assert!(
        err.contains("does not include Eq") || err.contains("Eq") || err.contains("cannot derive"),
        "got: {}",
        err
    );
}

#[test]
fn manual_eq_overrides_auto_derive() {
    // Class defines eq() manually — should use manual, not auto
    common::check_ok(
        "\
trait Eq
  def eq(other: Self) -> Bool

class User includes Eq
  id: Int
  name: String
  def eq(other: User) -> Bool
    true
",
    );
}

// ============================================================
// Phase 4D: Built-in Eq registration
// ============================================================

#[test]
fn builtin_eq_trait_exists() {
    // The built-in Eq trait should be available without defining it
    common::check_ok(
        "\
class Point includes Eq
  x: Int
  y: Int

let p1 = Point(x: 1, y: 2)
let p2 = Point(x: 1, y: 2)
let result = p1 == p2
",
    );
}

#[test]
fn function_types_still_cannot_be_compared() {
    let err = common::check_err(
        "\
def f(x: Int) -> Int
  x
def g(x: Int) -> Int
  x
let result = f == g
",
    );
    assert!(
        err.contains("function") || err.contains("Cannot compare"),
        "got: {}",
        err
    );
}

// ============================================================
// Container types cannot use == or !=
// ============================================================

#[test]
fn list_eq_rejected() {
    let err = common::check_err(
        "\
let a: List[Int] = [1, 2, 3]
let b: List[Int] = [1, 2, 3]
let r = a == b
",
    );
    assert!(
        err.contains("List") && err.contains("do not support"),
        "expected List comparison rejection, got: {}",
        err
    );
}

#[test]
fn list_neq_rejected() {
    let err = common::check_err(
        "\
let a: List[Int] = [1, 2]
let b: List[Int] = [3, 4]
let r = a != b
",
    );
    assert!(
        err.contains("List") && err.contains("do not support"),
        "expected List comparison rejection, got: {}",
        err
    );
}

#[test]
fn map_eq_rejected() {
    let err = common::check_err(
        "\
let a: Map[String, Int] = {\"x\": 1}
let b: Map[String, Int] = {\"x\": 1}
let r = a == b
",
    );
    assert!(
        err.contains("Map") && err.contains("do not support"),
        "expected Map comparison rejection, got: {}",
        err
    );
}

#[test]
fn nullable_eq_rejected() {
    let err = common::check_err(
        "\
let a: Int? = 1
let b: Int? = 1
let r = a == b
",
    );
    assert!(
        err.contains("Nullable") && err.contains("do not support"),
        "expected Nullable comparison rejection, got: {}",
        err
    );
}

#[test]
fn task_eq_rejected() {
    let err = common::check_err(
        "\
def f() -> Int
  42

let a = async f()
let b = async f()
let r = a == b
",
    );
    assert!(
        err.contains("Task") && err.contains("do not support"),
        "expected Task comparison rejection, got: {}",
        err
    );
}
