
// ─── Basic const declarations ───────────────────────────────────────

#[test]
fn const_basic_int() {
    crate::common::check_ok("const MAX = 100\n");
}

#[test]
fn const_basic_string() {
    crate::common::check_ok("const NAME = \"Aster\"\n");
}

#[test]
fn const_basic_float() {
    crate::common::check_ok("const PI = 3.14159\n");
}

#[test]
fn const_basic_bool() {
    crate::common::check_ok("const DEBUG = true\n");
}

#[test]
fn const_negative_value() {
    crate::common::check_ok("const NEG = -42\n");
}

// ─── Const with type annotations ────────────────────────────────────

#[test]
fn const_with_type_annotation() {
    crate::common::check_ok("const X: Int = 42\n");
}

#[test]
fn const_type_mismatch() {
    let err = crate::common::check_err("const X: Int = \"not an int\"\n");
    assert!(
        err.contains("mismatch") || err.contains("E001"),
        "expected type mismatch error, got: {}",
        err
    );
}

// ─── Const expressions ──────────────────────────────────────────────

#[test]
fn const_with_const_expression() {
    crate::common::check_ok("const X = 1 + 2 * 3\n");
}

#[test]
fn const_used_in_expression() {
    crate::common::check_ok(
        "\
const LIMIT = 100
let x = LIMIT + 1
",
    );
}

#[test]
fn const_non_constant_value_error() {
    let err = crate::common::check_err(
        "\
let y = 10
const X = y
",
    );
    assert!(
        err.contains("constant") || err.contains("const") || err.contains("E026"),
        "expected const expression error, got: {}",
        err
    );
}

// ─── Const reassignment errors ──────────────────────────────────────

#[test]
fn const_reassignment_error() {
    let err = crate::common::check_err(
        "\
const X = 1
X = 2
",
    );
    assert!(
        err.contains("Cannot reassign const")
            || err.contains("cannot be reassigned")
            || err.contains("E026"),
        "expected const reassignment error, got: {}",
        err
    );
}

#[test]
fn const_reassignment_in_nested_scope_error() {
    let err = crate::common::check_err(
        "\
def main() -> Int
  const x = 10
  if true
    x = 20
  x
",
    );
    assert!(
        err.contains("const") || err.contains("reassign") || err.contains("immutable"),
        "expected const reassignment error, got: {}",
        err
    );
}
