mod common;
use common::*;

// =============================================================================
// 1. Variable Capture — Read
// =============================================================================

#[test]
fn closure_reads_outer_variable() {
    // A nested function can read variables from the enclosing scope
    check_ok(
        "\
let scale = 2
def double(x: Int) -> Int
  x * scale
",
    );
}

#[test]
fn closure_reads_outer_variable_in_lambda() {
    // A def-as-let lambda can read variables from enclosing scope
    check_ok(
        "\
let offset = 10
def add_offset(x: Int) -> Int
  x + offset
let result = add_offset(x: 5)
",
    );
}

#[test]
fn closure_reads_multiple_outer_variables() {
    check_ok(
        "\
let a = 1
let b = 2
def sum_with(x: Int) -> Int
  x + a + b
",
    );
}

// =============================================================================
// 2. Variable Capture — Write (Assignment)
// =============================================================================

#[test]
fn closure_writes_outer_variable() {
    // A nested function can write to a variable from the enclosing scope
    check_ok(
        "\
let count = 0
def increment() -> Int
  count = count + 1
  count
",
    );
}

#[test]
fn closure_write_type_mismatch() {
    // Cannot assign wrong type to captured variable
    let err = check_err(
        "\
let count = 0
def bad() -> String
  count = \"hello\"
  \"done\"
",
    );
    assert!(err.contains("mismatch"), "expected type mismatch: {}", err);
}

// =============================================================================
// 3. Inline Lambda Syntax — -> params: body
// =============================================================================

#[test]
fn inline_lambda_single_param() {
    // -> x: expr creates a lambda with one inferred-type param
    check_ok(
        "\
def apply(x: Int, f: Fn(Int) -> Int) -> Int
  f(_0: x)
let result = apply(x: 5, f: -> x: x * 2)
",
    );
}

#[test]
fn inline_lambda_two_params() {
    // -> a, b: expr creates a lambda with two inferred-type params
    check_ok(
        "\
def combine(a: Int, b: Int, f: Fn(Int, Int) -> Int) -> Int
  f(_0: a, _1: b)
let result = combine(a: 3, b: 4, f: -> a, b: a + b)
",
    );
}

#[test]
fn inline_lambda_zero_params() {
    // -> : expr creates a zero-param lambda
    check_ok(
        "\
def run(f: Fn() -> Int) -> Int
  f()
let result = run(f: -> : 42)
",
    );
}

#[test]
fn inline_lambda_captures_outer_var() {
    // Inline lambda captures variable from enclosing scope
    check_ok(
        "\
let scale = 10
def apply(x: Int, f: Fn(Int) -> Int) -> Int
  f(_0: x)
let result = apply(x: 5, f: -> x: x * scale)
",
    );
}

// =============================================================================
// 4. Lambda Type Inference from Call Context
// =============================================================================

#[test]
fn lambda_param_type_inferred_from_function_param() {
    // When passed to a function expecting Fn(Int) -> Int, lambda param types are inferred
    check_ok(
        "\
def apply(x: Int, f: Fn(Int) -> Int) -> Int
  f(_0: x)
let result = apply(x: 5, f: -> x: x + 1)
",
    );
}

#[test]
fn lambda_param_type_inferred_returns_correct_type() {
    // The lambda body type must match the expected return type
    let err = check_err(
        "\
def apply(x: Int, f: Fn(Int) -> Int) -> Int
  f(_0: x)
let result = apply(x: 5, f: -> x: \"hello\")
",
    );
    assert!(
        err.contains("mismatch"),
        "expected return type mismatch: {}",
        err
    );
}

#[test]
fn lambda_param_type_inferred_wrong_arity() {
    // Inline lambda with wrong number of params should fail
    let err = check_err(
        "\
def apply(x: Int, f: Fn(Int) -> Int) -> Int
  f(_0: x)
let result = apply(x: 5, f: -> a, b: a + b)
",
    );
    assert!(
        err.contains("mismatch") || err.contains("arity"),
        "expected arity error: {}",
        err
    );
}

// =============================================================================
// 5. Closure Type = Function Type (Unification)
// =============================================================================

#[test]
fn closure_assigned_to_function_typed_var() {
    // A closure can be stored in a variable with function type annotation
    check_ok(
        "\
let scale = 2
let f: Fn(Int) -> Int = -> x: x * scale
",
    );
}

#[test]
fn nested_def_passed_as_function_arg() {
    // A nested def can be passed where a function type is expected
    check_ok(
        "\
def apply(x: Int, f: Fn(Int) -> Int) -> Int
  f(_0: x)
def process(n: Int) -> Int
  def doubler(x: Int) -> Int
    x * 2
  apply(x: n, f: doubler)
",
    );
}

// =============================================================================
// 6. Multi-level Nesting
// =============================================================================

#[test]
fn double_nested_closure_capture() {
    // Lambda inside lambda can capture from grandparent scope
    check_ok(
        "\
let base = 100
def outer(x: Int) -> Int
  def inner(y: Int) -> Int
    y + base
  inner(y: x)
",
    );
}

// =============================================================================
// 7. Error Cases
// =============================================================================

#[test]
fn inline_lambda_no_context_needs_annotation() {
    // Without expected type context, inline lambda param types can't be inferred
    let err = check_err(
        "\
let f = -> x: x * 2
",
    );
    assert!(
        err.contains("Cannot infer") || err.contains("nfer"),
        "expected inference error: {}",
        err
    );
}

#[test]
fn inline_lambda_body_type_error() {
    // Type error inside inline lambda body is reported
    let err = check_err(
        "\
def apply(x: Int, f: Fn(Int) -> Int) -> Int
  f(_0: x)
let result = apply(x: 5, f: -> x: x + \"bad\")
",
    );
    assert!(
        err.contains("Cannot apply") || err.contains("mismatch"),
        "expected type error in body: {}",
        err
    );
}

// =============================================================================
// 8. Annotated Inline Lambda (explicit types)
// =============================================================================

#[test]
fn inline_lambda_with_type_annotation() {
    // When type annotation is provided on let, lambda types are inferred
    check_ok(
        "\
let f: Fn(Int) -> Int = -> x: x * 2
let result = f(x: 5)
",
    );
}

#[test]
fn lambda_assigned_to_typed_let() {
    // Lambda assigned to a let with explicit function type annotation
    check_ok(
        "\
def main() -> Int
  let f: Fn(Int) -> Int = -> x: x + 1
  f(x: 5)
",
    );
}
