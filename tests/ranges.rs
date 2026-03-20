mod common;

// ─── Range Type ─────────────────────────────────────────────────────
//
// `1..10` (exclusive), `1..=10` (inclusive)
// Int-only operands, produces Range type, includes Iterable.

// ─── Parsing ────────────────────────────────────────────────────────

#[test]
fn range_exclusive_parses() {
    common::check_ok(
        r#"let r = 1..10
"#,
    );
}

#[test]
fn range_inclusive_parses() {
    common::check_ok(
        r#"let r = 1..=10
"#,
    );
}

#[test]
fn range_with_variables() {
    common::check_ok(
        r#"let a: Int = 1
let b: Int = 10
let r = a..b
"#,
    );
}

#[test]
fn range_with_expressions() {
    common::check_ok(
        r#"let n: Int = 5
let r = 0..n * 2
"#,
    );
}

// ─── Type checking ──────────────────────────────────────────────────

#[test]
fn range_start_must_be_int() {
    let err = common::check_err(
        r#"let r = 1.0..10
"#,
    );
    assert!(err.contains("Int"), "Expected Int type error, got: {}", err);
}

#[test]
fn range_end_must_be_int() {
    let err = common::check_err(
        r#"let r = 1.."hello"
"#,
    );
    assert!(err.contains("Int"), "Expected Int type error, got: {}", err);
}

// ─── For loops ──────────────────────────────────────────────────────

#[test]
fn for_loop_over_exclusive_range() {
    common::check_ok(
        r#"for n in 1..10
  say(message: n)
"#,
    );
}

#[test]
fn for_loop_over_inclusive_range() {
    common::check_ok(
        r#"for n in 1..=10
  say(message: n)
"#,
    );
}

#[test]
fn for_loop_range_variable_is_int() {
    // n should be Int inside the loop body
    common::check_ok(
        r#"for n in 1..10
  let x: Int = n
"#,
    );
}

#[test]
fn for_loop_range_variable_not_string() {
    let err = common::check_err(
        r#"for n in 1..10
  let x: String = n
"#,
    );
    assert!(
        err.contains("String") || err.contains("Int"),
        "Expected type mismatch error, got: {}",
        err
    );
}

// ─── Range.random() ─────────────────────────────────────────────────

#[test]
fn range_random_method_typechecks() {
    common::check_ok(
        r#"let n: Int = (1..100).random()
"#,
    );
}

#[test]
fn range_random_returns_int() {
    let err = common::check_err(
        r#"let s: String = (1..100).random()
"#,
    );
    assert!(
        err.contains("String") || err.contains("Int"),
        "Expected type mismatch, got: {}",
        err
    );
}

// ─── Execution (JIT) ────────────────────────────────────────────────

#[test]
fn run_range_for_inclusive_sum() {
    let output = common::cli(&["run", "examples/executable/range_for.aster"]);
    assert!(output.status.success(), "{}", common::output_text(&output));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        "55",
        "1+2+...+10 should be 55"
    );
}
