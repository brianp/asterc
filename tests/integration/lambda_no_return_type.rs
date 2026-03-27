use crate::common::*;

// =============================================================================
// Named `def` functions WITH return type — should STILL work
// =============================================================================

#[test]
fn def_with_return_type_still_valid() {
    check_ok(
        "\
def add(x: Int, y: Int) -> Int
  x + y
",
    );
}

#[test]
fn def_in_class_with_return_type_still_valid() {
    check_ok(
        "\
class Math
  def add(x: Int, y: Int) -> Int
    x + y
",
    );
}

#[test]
fn def_with_throws_and_return_type_still_valid() {
    check_ok(
        "\
def parse(s: String) throws ParseError -> Int
  42
",
    );
}

// =============================================================================
// Named `def` functions WITHOUT return type — infer from body
// =============================================================================

#[test]
fn def_without_return_type_infers_int() {
    // def without `-> Type` should infer return type from body.
    check_ok(
        "\
def square(x: Int)
  x * x
let result: Int = square(x: 4)
",
    );
}

#[test]
fn def_without_return_type_infers_string() {
    check_ok(
        "\
def greet(name: String)
  \"hello\"
let msg: String = greet(name: \"world\")
",
    );
}

#[test]
fn def_without_return_type_void() {
    check_ok(
        "\
def log(msg: String)
  say(message: msg)
",
    );
}

// =============================================================================
// Inline lambdas — `-> x: expr` form — always inferred, no return type syntax
// =============================================================================

#[test]
fn inline_lambda_arrow_form_works() {
    check_ok(
        "\
def apply(x: Int, f: Fn(Int) -> Int) -> Int
  f(_0: x)
let result = apply(x: 5, f: -> x: x * 2)
",
    );
}

#[test]
fn inline_lambda_multi_param_works() {
    check_ok(
        "\
def apply2(a: Int, b: Int, f: Fn(Int, Int) -> Int) -> Int
  f(_0: a, _1: b)
let result = apply2(a: 1, b: 2, f: -> a, b: a + b)
",
    );
}

#[test]
fn inline_lambda_zero_param_works() {
    check_ok(
        "\
def run(f: Fn() -> Int) -> Int
  f()
let result = run(f: -> : 42)
",
    );
}

// =============================================================================
// `def` as lambda value (assigned to let) — return type on the `let`, not lambda
// =============================================================================

#[test]
fn let_typed_with_def_no_return_type() {
    // Type annotation on the `let` binding, return type omitted from `def`.
    // The typechecker should infer the def's return type and match the let annotation.
    check_ok(
        "\
def increment(x: Int)
  x + 1
let result: Int = increment(x: 5)
",
    );
}

// =============================================================================
// Formatter: inline lambda in `def` should NOT emit `-> RetType`
// This tests the formatter's output for lambda expressions.
// =============================================================================

// (These tests will be validated via the formatter test suite in aster-fmt)
