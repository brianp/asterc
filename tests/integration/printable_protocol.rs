// ============================================================
// Printable Protocol — Built-in trait registration
// ============================================================

#[test]
fn builtin_printable_trait_exists() {
    // Printable trait should be available without defining it
    crate::common::check_ok(
        "\
class Point includes Printable
  x: Int
  y: Int
",
    );
}

#[test]
fn printable_unknown_trait_error_still_works() {
    // Non-existent traits should still error
    let err = crate::common::check_err(
        "\
class Point includes FakeProtocol
  x: Int
",
    );
    assert!(
        err.contains("Unknown trait") || err.contains("FakeProtocol"),
        "got: {}",
        err
    );
}

// ============================================================
// Manual to_string implementation
// ============================================================

#[test]
fn manual_to_string_satisfies_printable() {
    crate::common::check_ok(
        "\
class Point includes Printable
  x: Int
  y: Int
  def to_string() -> String
    \"point\"
",
    );
}

#[test]
fn manual_to_string_and_debug() {
    crate::common::check_ok(
        "\
class Point includes Printable
  x: Int
  y: Int
  def to_string() -> String
    \"point\"
  def debug() -> String
    \"Point(debug)\"
",
    );
}

#[test]
fn manual_to_string_wrong_return_type_error() {
    let err = crate::common::check_err(
        "\
class Point includes Printable
  x: Int
  def to_string() -> Int
    42
",
    );
    assert!(
        err.contains("signature") || err.contains("mismatch") || err.contains("requires"),
        "got: {}",
        err
    );
}

// ============================================================
// Auto-derive to_string
// ============================================================

#[test]
fn auto_derive_printable_all_primitives() {
    // All fields are primitive (include Printable) — auto-derive works
    crate::common::check_ok(
        "\
class Point includes Printable
  x: Int
  y: Int
",
    );
}

#[test]
fn auto_derive_printable_string_field() {
    crate::common::check_ok(
        "\
class User includes Printable
  name: String
  age: Int
",
    );
}

#[test]
fn auto_derive_printable_nested() {
    // Nested auto-derive: Line has Point fields, both include Printable
    crate::common::check_ok(
        "\
class Point includes Printable
  x: Int
  y: Int

class Line includes Printable
  start: Point
  end: Point
",
    );
}

#[test]
fn auto_derive_printable_field_without_printable_error() {
    // Auto-derive should fail if a field's type doesn't include Printable
    let err = crate::common::check_err(
        "\
class NoPrint
  value: Int

class Wrapper includes Printable
  inner: NoPrint
",
    );
    assert!(
        err.contains("does not include Printable")
            || err.contains("Printable")
            || err.contains("cannot derive"),
        "got: {}",
        err
    );
}

#[test]
fn auto_derive_printable_error_code_e023() {
    let diag = crate::common::check_err_diagnostic(
        "\
class NoPrint
  value: Int

class Wrapper includes Printable
  inner: NoPrint
",
    );
    assert_eq!(diag.code(), Some("E023"));
}

#[test]
fn manual_to_string_overrides_auto_derive() {
    // Class defines to_string() manually — should use manual, not auto
    crate::common::check_ok(
        "\
class User includes Printable
  id: Int
  name: String
  def to_string() -> String
    \"user\"
",
    );
}

// ============================================================
// Auto-derived debug() defaults to to_string()
// ============================================================

#[test]
fn auto_derive_adds_debug_method() {
    // When to_string is auto-derived, debug() should also be available
    crate::common::check_ok(
        "\
class Point includes Printable
  x: Int
  y: Int

let p = Point(x: 1, y: 2)
let s: String = p.debug()
",
    );
}

#[test]
fn manual_to_string_gets_debug_for_free() {
    // Manual to_string without debug — debug should still be auto-added
    crate::common::check_ok(
        "\
class Point includes Printable
  x: Int
  y: Int
  def to_string() -> String
    \"point\"

let p = Point(x: 1, y: 2)
let s: String = p.debug()
",
    );
}

// ============================================================
// Method calls on Printable types
// ============================================================

#[test]
fn call_to_string_on_printable_type() {
    crate::common::check_ok(
        "\
class Point includes Printable
  x: Int
  y: Int

let p = Point(x: 1, y: 2)
let s: String = p.to_string()
",
    );
}

#[test]
fn call_debug_on_printable_type() {
    crate::common::check_ok(
        "\
class Point includes Printable
  x: Int
  y: Int

let p = Point(x: 1, y: 2)
let s: String = p.debug()
",
    );
}

#[test]
fn call_to_string_on_non_printable_type_error() {
    let err = crate::common::check_err(
        "\
class Point
  x: Int
  y: Int

let p = Point(x: 1, y: 2)
let s = p.to_string()
",
    );
    assert!(
        err.contains("no field or method")
            || err.contains("to_string")
            || err.contains("no member"),
        "got: {}",
        err
    );
}

// ============================================================
// Printable with other protocols
// ============================================================

#[test]
fn printable_with_eq() {
    crate::common::check_ok(
        "\
class Point includes Eq, Printable
  x: Int
  y: Int

let p1 = Point(x: 1, y: 2)
let p2 = Point(x: 1, y: 2)
let eq = p1 == p2
let s = p1.to_string()
",
    );
}

#[test]
fn printable_with_ord() {
    crate::common::check_ok(
        "\
class Score includes Ord, Printable
  value: Int

let s1 = Score(value: 10)
let s2 = Score(value: 20)
let cmp = s1 < s2
let s = s1.to_string()
",
    );
}

// ============================================================
// Bool field (all primitives are Printable)
// ============================================================

#[test]
fn auto_derive_printable_with_bool_field() {
    crate::common::check_ok(
        "\
class Flag includes Printable
  name: String
  active: Bool
",
    );
}

#[test]
fn auto_derive_printable_with_float_field() {
    crate::common::check_ok(
        "\
class Measurement includes Printable
  label: String
  value: Float
",
    );
}
