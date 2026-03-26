mod common;

// ─── Int Methods ──────────────────────────────────────────────────────
//
// Built-in methods on Int type:
//   is_even, is_odd, abs, clamp(min:, max:), min(value:), max(value:)

// ─── is_even ──────────────────────────────────────────────────────────

#[test]
fn int_is_even_positive_even() {
    common::check_ok(
        r#"let n = 4
let result: Bool = n.is_even()
"#,
    );
}

#[test]
fn int_is_even_positive_odd() {
    common::check_ok(
        r#"let n = 3
let result: Bool = n.is_even()
"#,
    );
}

#[test]
fn int_is_even_zero() {
    common::check_ok(
        r#"let n = 0
let result: Bool = n.is_even()
"#,
    );
}

#[test]
fn int_is_even_negative() {
    common::check_ok(
        r#"let n = -4
let result: Bool = n.is_even()
"#,
    );
}

// ─── is_odd ───────────────────────────────────────────────────────────

#[test]
fn int_is_odd_positive_odd() {
    common::check_ok(
        r#"let n = 3
let result: Bool = n.is_odd()
"#,
    );
}

#[test]
fn int_is_odd_positive_even() {
    common::check_ok(
        r#"let n = 4
let result: Bool = n.is_odd()
"#,
    );
}

#[test]
fn int_is_odd_zero() {
    common::check_ok(
        r#"let n = 0
let result: Bool = n.is_odd()
"#,
    );
}

#[test]
fn int_is_odd_negative() {
    common::check_ok(
        r#"let n = -3
let result: Bool = n.is_odd()
"#,
    );
}

// ─── Int.abs ──────────────────────────────────────────────────────────

#[test]
fn int_abs_positive() {
    common::check_ok(
        r#"let n = 5
let result: Int = n.abs()
"#,
    );
}

#[test]
fn int_abs_negative() {
    common::check_ok(
        r#"let n = -5
let result: Int = n.abs()
"#,
    );
}

#[test]
fn int_abs_zero() {
    common::check_ok(
        r#"let n = 0
let result: Int = n.abs()
"#,
    );
}

// ─── Int.clamp ────────────────────────────────────────────────────────

#[test]
fn int_clamp_within_range() {
    common::check_ok(
        r#"let n = 5
let result: Int = n.clamp(min: 0, max: 10)
"#,
    );
}

#[test]
fn int_clamp_below_min() {
    common::check_ok(
        r#"let n = -5
let result: Int = n.clamp(min: 0, max: 10)
"#,
    );
}

#[test]
fn int_clamp_above_max() {
    common::check_ok(
        r#"let n = 15
let result: Int = n.clamp(min: 0, max: 10)
"#,
    );
}

#[test]
fn int_clamp_at_min() {
    common::check_ok(
        r#"let n = 0
let result: Int = n.clamp(min: 0, max: 10)
"#,
    );
}

#[test]
fn int_clamp_at_max() {
    common::check_ok(
        r#"let n = 10
let result: Int = n.clamp(min: 0, max: 10)
"#,
    );
}

// ─── Int.min ──────────────────────────────────────────────────────────

#[test]
fn int_min_returns_smaller() {
    common::check_ok(
        r#"let n = 5
let result: Int = n.min(value: 3)
"#,
    );
}

#[test]
fn int_min_returns_self_when_smaller() {
    common::check_ok(
        r#"let n = 2
let result: Int = n.min(value: 10)
"#,
    );
}

#[test]
fn int_min_equal() {
    common::check_ok(
        r#"let n = 5
let result: Int = n.min(value: 5)
"#,
    );
}

// ─── Int.max ──────────────────────────────────────────────────────────

#[test]
fn int_max_returns_larger() {
    common::check_ok(
        r#"let n = 5
let result: Int = n.max(value: 10)
"#,
    );
}

#[test]
fn int_max_returns_self_when_larger() {
    common::check_ok(
        r#"let n = 10
let result: Int = n.max(value: 3)
"#,
    );
}

#[test]
fn int_max_equal() {
    common::check_ok(
        r#"let n = 5
let result: Int = n.max(value: 5)
"#,
    );
}

// ─── Float Methods ────────────────────────────────────────────────────
//
// Built-in methods on Float type:
//   abs, round, floor, ceil, clamp(min:, max:), min(value:), max(value:)

// ─── Float.abs ────────────────────────────────────────────────────────

#[test]
fn float_abs_positive() {
    common::check_ok(
        r#"let n = 3.14
let result: Float = n.abs()
"#,
    );
}

#[test]
fn float_abs_negative() {
    common::check_ok(
        r#"let n = -3.14
let result: Float = n.abs()
"#,
    );
}

#[test]
fn float_abs_zero() {
    common::check_ok(
        r#"let n = 0.0
let result: Float = n.abs()
"#,
    );
}

// ─── Float.round ──────────────────────────────────────────────────────

#[test]
fn float_round_down() {
    common::check_ok(
        r#"let n = 3.2
let result: Int = n.round()
"#,
    );
}

#[test]
fn float_round_up() {
    common::check_ok(
        r#"let n = 3.7
let result: Int = n.round()
"#,
    );
}

#[test]
fn float_round_half() {
    common::check_ok(
        r#"let n = 3.5
let result: Int = n.round()
"#,
    );
}

#[test]
fn float_round_negative() {
    common::check_ok(
        r#"let n = -2.7
let result: Int = n.round()
"#,
    );
}

// ─── Float.floor ──────────────────────────────────────────────────────

#[test]
fn float_floor_positive() {
    common::check_ok(
        r#"let n = 3.7
let result: Int = n.floor()
"#,
    );
}

#[test]
fn float_floor_negative() {
    common::check_ok(
        r#"let n = -2.3
let result: Int = n.floor()
"#,
    );
}

#[test]
fn float_floor_whole() {
    common::check_ok(
        r#"let n = 5.0
let result: Int = n.floor()
"#,
    );
}

// ─── Float.ceil ───────────────────────────────────────────────────────

#[test]
fn float_ceil_positive() {
    common::check_ok(
        r#"let n = 3.2
let result: Int = n.ceil()
"#,
    );
}

#[test]
fn float_ceil_negative() {
    common::check_ok(
        r#"let n = -2.7
let result: Int = n.ceil()
"#,
    );
}

#[test]
fn float_ceil_whole() {
    common::check_ok(
        r#"let n = 5.0
let result: Int = n.ceil()
"#,
    );
}

// ─── Float.clamp ──────────────────────────────────────────────────────

#[test]
fn float_clamp_within_range() {
    common::check_ok(
        r#"let n = 5.0
let result: Float = n.clamp(min: 0.0, max: 10.0)
"#,
    );
}

#[test]
fn float_clamp_below_min() {
    common::check_ok(
        r#"let n = -5.0
let result: Float = n.clamp(min: 0.0, max: 10.0)
"#,
    );
}

#[test]
fn float_clamp_above_max() {
    common::check_ok(
        r#"let n = 15.0
let result: Float = n.clamp(min: 0.0, max: 10.0)
"#,
    );
}

// ─── Float.min ────────────────────────────────────────────────────────

#[test]
fn float_min_returns_smaller() {
    common::check_ok(
        r#"let n = 5.0
let result: Float = n.min(value: 3.0)
"#,
    );
}

#[test]
fn float_min_equal() {
    common::check_ok(
        r#"let n = 5.0
let result: Float = n.min(value: 5.0)
"#,
    );
}

// ─── Float.max ────────────────────────────────────────────────────────

#[test]
fn float_max_returns_larger() {
    common::check_ok(
        r#"let n = 5.0
let result: Float = n.max(value: 10.0)
"#,
    );
}

#[test]
fn float_max_equal() {
    common::check_ok(
        r#"let n = 5.0
let result: Float = n.max(value: 5.0)
"#,
    );
}

// ─── Rejection Tests ──────────────────────────────────────────────────
// Methods that don't exist on the wrong type should be rejected.

#[test]
fn int_rejects_round() {
    let err = common::check_err(
        r#"let n = 5
let result = n.round()
"#,
    );
    assert!(
        err.contains("round"),
        "expected error about 'round', got: {}",
        err
    );
}

#[test]
fn int_rejects_floor() {
    let err = common::check_err(
        r#"let n = 5
let result = n.floor()
"#,
    );
    assert!(
        err.contains("floor"),
        "expected error about 'floor', got: {}",
        err
    );
}

#[test]
fn int_rejects_ceil() {
    let err = common::check_err(
        r#"let n = 5
let result = n.ceil()
"#,
    );
    assert!(
        err.contains("ceil"),
        "expected error about 'ceil', got: {}",
        err
    );
}

#[test]
fn float_rejects_is_even() {
    let err = common::check_err(
        r#"let n = 3.14
let result = n.is_even()
"#,
    );
    assert!(
        err.contains("is_even"),
        "expected error about 'is_even', got: {}",
        err
    );
}

#[test]
fn float_rejects_is_odd() {
    let err = common::check_err(
        r#"let n = 3.14
let result = n.is_odd()
"#,
    );
    assert!(
        err.contains("is_odd"),
        "expected error about 'is_odd', got: {}",
        err
    );
}

#[test]
fn string_rejects_abs() {
    let err = common::check_err(
        r#"let s = "hello"
let result = s.abs()
"#,
    );
    assert!(
        err.contains("abs"),
        "expected error about 'abs', got: {}",
        err
    );
}

#[test]
fn bool_rejects_abs() {
    let err = common::check_err(
        r#"let b = true
let result = b.abs()
"#,
    );
    assert!(
        err.contains("abs"),
        "expected error about 'abs', got: {}",
        err
    );
}

// ─── Type Mismatch Tests ─────────────────────────────────────────────
// Wrong argument types should be rejected.

#[test]
fn int_clamp_rejects_float_args() {
    let err = common::check_err(
        r#"let n = 5
let result = n.clamp(min: 0.0, max: 10.0)
"#,
    );
    assert!(
        !err.is_empty(),
        "expected type error for float args on Int.clamp"
    );
}

#[test]
fn float_clamp_rejects_int_args() {
    let err = common::check_err(
        r#"let n = 5.0
let result = n.clamp(min: 0, max: 10)
"#,
    );
    assert!(
        !err.is_empty(),
        "expected type error for int args on Float.clamp"
    );
}

#[test]
fn int_min_rejects_float_arg() {
    let err = common::check_err(
        r#"let n = 5
let result = n.min(value: 3.0)
"#,
    );
    assert!(
        !err.is_empty(),
        "expected type error for float arg on Int.min"
    );
}

#[test]
fn float_min_rejects_int_arg() {
    let err = common::check_err(
        r#"let n = 5.0
let result = n.min(value: 3)
"#,
    );
    assert!(
        !err.is_empty(),
        "expected type error for int arg on Float.min"
    );
}

// ─── Chaining / Composition Tests ─────────────────────────────────────

#[test]
fn int_abs_then_clamp() {
    common::check_ok(
        r#"let n = -15
let result: Int = n.abs().clamp(min: 0, max: 10)
"#,
    );
}

#[test]
fn float_abs_then_round() {
    common::check_ok(
        r#"let n = -3.7
let result: Int = n.abs().round()
"#,
    );
}

#[test]
fn int_method_on_literal() {
    common::check_ok(
        r#"let result: Bool = 42.is_even()
"#,
    );
}

#[test]
fn int_method_on_expression() {
    common::check_ok(
        r#"let a = 3
let b = -5
let result: Int = (a + b).abs()
"#,
    );
}

#[test]
fn float_method_on_expression() {
    common::check_ok(
        r#"let a = 3.5
let b = -1.2
let result: Int = (a + b).round()
"#,
    );
}
