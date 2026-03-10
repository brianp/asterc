mod common;

// ============================================================
// Phase 1: Enums
// ============================================================

#[test]
fn parse_unit_enum() {
    common::check_ok(
        "\
enum Color
  Red
  Green
  Blue
",
    );
}

#[test]
fn enum_variant_has_enum_type() {
    common::check_ok(
        "\
enum Color
  Red
  Green
  Blue

let c: Color = Color.Red
",
    );
}

#[test]
fn enum_variant_different_variants_same_type() {
    common::check_ok(
        "\
enum Color
  Red
  Green
  Blue

let a: Color = Color.Red
let b: Color = Color.Blue
",
    );
}

#[test]
fn enum_unknown_variant_error() {
    let err = common::check_err(
        "\
enum Color
  Red
  Green
  Blue

let c = Color.Purple
",
    );
    assert!(
        err.contains("no") || err.contains("unknown") || err.contains("Purple"),
        "got: {}",
        err
    );
}

#[test]
fn enum_not_a_class_error() {
    // Can't construct enum like a class — enum names are not constructors
    let err = common::check_err(
        "\
enum Color
  Red
  Green

let c = Color(Red: true)
",
    );
    // Should fail — Color is not callable as a constructor
    assert!(!err.is_empty(), "should produce an error");
}

// ============================================================
// Phase 1D: Built-in Ordering enum
// ============================================================

#[test]
fn ordering_less_has_ordering_type() {
    common::check_ok("let o: Ordering = Ordering.Less");
}

#[test]
fn ordering_equal_has_ordering_type() {
    common::check_ok("let o: Ordering = Ordering.Equal");
}

#[test]
fn ordering_greater_has_ordering_type() {
    common::check_ok("let o: Ordering = Ordering.Greater");
}

#[test]
fn ordering_variants_can_be_compared_with_eq() {
    common::check_ok(
        "\
let a = Ordering.Less
let b = Ordering.Less
let result = a == b
",
    );
}

// ============================================================
// Phase 5A: Ord trait — primitives (no regression)
// ============================================================

#[test]
fn ord_primitives_int() {
    common::check_ok("let x = 1 < 2");
}

#[test]
fn ord_primitives_float() {
    common::check_ok("let x = 1.0 > 2.0");
}

#[test]
fn ord_primitives_string() {
    common::check_ok("let x = \"a\" <= \"b\"");
}

#[test]
fn ord_primitives_all_ops() {
    common::check_ok(
        "\
let a = 1 < 2
let b = 1 > 2
let c = 1 <= 2
let d = 1 >= 2
",
    );
}

// ============================================================
// Phase 5B: Ord trait — user types
// ============================================================

#[test]
fn user_type_with_ord_can_use_less_than() {
    common::check_ok(
        "\
class Priority includes Ord
  level: Int
  def cmp(other: Priority) -> Ordering
    Ordering.Less

let p1 = Priority(level: 1)
let p2 = Priority(level: 2)
let result = p1 < p2
",
    );
}

#[test]
fn user_type_with_ord_all_operators() {
    common::check_ok(
        "\
class Priority includes Ord
  level: Int
  def cmp(other: Priority) -> Ordering
    Ordering.Less

let p1 = Priority(level: 1)
let p2 = Priority(level: 2)
let a = p1 < p2
let b = p1 > p2
let c = p1 <= p2
let d = p1 >= p2
",
    );
}

#[test]
fn user_type_without_ord_cannot_use_less_than() {
    let err = common::check_err(
        "\
class Point
  x: Int
  y: Int

let p1 = Point(x: 1, y: 2)
let p2 = Point(x: 3, y: 4)
let result = p1 < p2
",
    );
    assert!(
        err.contains("does not include Ord") || err.contains("Ord"),
        "got: {}",
        err
    );
}

#[test]
fn ord_different_user_types_error() {
    let err = common::check_err(
        "\
class A includes Ord
  v: Int

class B includes Ord
  v: Int

let a = A(v: 1)
let b = B(v: 2)
let result = a < b
",
    );
    assert!(
        err.contains("Cannot") || err.contains("incompatible"),
        "got: {}",
        err
    );
}

// ============================================================
// Phase 5: Ord includes Eq
// ============================================================

#[test]
fn ord_includes_eq_auto() {
    // Including Ord should auto-include Eq — can use == without explicit Eq
    common::check_ok(
        "\
class Priority includes Ord
  level: Int
  def cmp(other: Priority) -> Ordering
    Ordering.Less

let p1 = Priority(level: 1)
let p2 = Priority(level: 1)
let result = p1 == p2
",
    );
}

// ============================================================
// Phase 5: Auto-derive Ord
// ============================================================

#[test]
fn auto_derive_ord_all_fields_ord() {
    // Class includes Ord without defining cmp() — all fields are primitive (include Ord)
    common::check_ok(
        "\
class Task includes Ord
  priority: Int
  created_at: Int

let t1 = Task(priority: 1, created_at: 100)
let t2 = Task(priority: 2, created_at: 50)
let result = t1 < t2
",
    );
}

#[test]
fn auto_derive_ord_field_without_ord_error() {
    let err = common::check_err(
        "\
class NoOrd
  value: Int

class Wrapper includes Ord
  inner: NoOrd
",
    );
    assert!(
        err.contains("does not include Ord")
            || err.contains("Ord")
            || err.contains("cannot derive"),
        "got: {}",
        err
    );
}

#[test]
fn ord_returns_bool_type() {
    common::check_ok(
        "\
class Priority includes Ord
  level: Int
  def cmp(other: Priority) -> Ordering
    Ordering.Equal

let p1 = Priority(level: 1)
let p2 = Priority(level: 2)
let b: Bool = p1 < p2
",
    );
}
