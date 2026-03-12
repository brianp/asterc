mod common;

// ============================================================
// Feature 1: String Interpolation
// ============================================================

#[test]
fn string_interpolation_simple_variable() {
    common::check_ok(
        "\
let name = \"world\"
let greeting = \"hello {name}\"
",
    );
}

#[test]
fn string_interpolation_int_variable() {
    common::check_ok(
        "\
let x = 42
let msg = \"the answer is {x}\"
",
    );
}

#[test]
fn string_interpolation_expression() {
    common::check_ok(
        "\
let msg = \"1 + 2 = {1 + 2}\"
",
    );
}

#[test]
fn string_interpolation_no_interpolation() {
    common::check_ok(
        "\
let msg = \"no interpolation here\"
",
    );
}

#[test]
fn string_interpolation_literal_value() {
    common::check_ok(
        "\
let msg = \"value: {42}\"
",
    );
}

#[test]
fn string_interpolation_bool() {
    common::check_ok(
        "\
let msg = \"flag is {true}\"
",
    );
}

#[test]
fn string_interpolation_multiple() {
    common::check_ok(
        "\
let a = 1
let b = 2
let msg = \"{a} + {b} = {a + b}\"
",
    );
}

#[test]
fn string_interpolation_result_is_string() {
    common::check_ok(
        "\
let x = 42
let msg: String = \"value: {x}\"
",
    );
}

#[test]
fn string_interpolation_nested_parens() {
    common::check_ok(
        "\
let msg = \"result: {(1 + 2) * 3}\"
",
    );
}

#[test]
fn string_interpolation_escaped_braces() {
    common::check_ok(
        "\
let msg = \"use \\{braces\\} literally\"
",
    );
}

#[test]
fn string_interpolation_non_printable_class_error() {
    let err = common::check_err(
        "\
class Foo
  x: Int

let f = Foo(x: 1)
let msg = \"value: {f}\"
",
    );
    assert!(
        err.contains("Printable"),
        "expected Printable error, got: {}",
        err
    );
}

#[test]
fn string_interpolation_printable_class_ok() {
    common::check_ok(
        "\
class Bar includes Printable
  x: Int

  def to_string() -> String
    return \"Bar\"

let b = Bar(x: 1)
let msg = \"value: {b}\"
",
    );
}

// ============================================================
// Feature 2: Const Bindings
// ============================================================

#[test]
fn const_basic_int() {
    common::check_ok("const MAX = 100\n");
}

#[test]
fn const_basic_string() {
    common::check_ok("const NAME = \"Aster\"\n");
}

#[test]
fn const_basic_float() {
    common::check_ok("const PI = 3.14159\n");
}

#[test]
fn const_basic_bool() {
    common::check_ok("const DEBUG = true\n");
}

#[test]
fn const_with_type_annotation() {
    common::check_ok("const X: Int = 42\n");
}

#[test]
fn const_type_mismatch() {
    let err = common::check_err("const X: Int = \"not an int\"\n");
    assert!(
        err.contains("mismatch") || err.contains("E001"),
        "expected type mismatch error, got: {}",
        err
    );
}

#[test]
fn const_reassignment_error() {
    let err = common::check_err(
        "\
const X = 1
X = 2
",
    );
    assert!(
        err.contains("Cannot reassign const") || err.contains("E026"),
        "expected const reassignment error, got: {}",
        err
    );
}

#[test]
fn const_used_in_expression() {
    common::check_ok(
        "\
const LIMIT = 100
let x = LIMIT + 1
",
    );
}

#[test]
fn const_non_constant_value_error() {
    let err = common::check_err(
        "\
let y = 10
const X = y
",
    );
    assert!(
        err.contains("constant") || err.contains("E026"),
        "expected const expression error, got: {}",
        err
    );
}

#[test]
fn const_with_const_expression() {
    common::check_ok("const X = 1 + 2 * 3\n");
}

#[test]
fn const_negative_value() {
    common::check_ok("const NEG = -42\n");
}

#[test]
fn const_reassignment_in_nested_scope_error() {
    let err = common::check_err(
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

// ============================================================
// Feature 2b: String Interpolation — Float
// ============================================================

#[test]
fn string_interpolation_float_variable() {
    common::check_ok(
        "\
def main() -> String
  let pi: Float = 3.14
  \"pi is {pi}\"
",
    );
}

// ============================================================
// Feature 3: Default Parameter Values
// ============================================================

#[test]
fn default_params_basic() {
    common::check_ok(
        "\
def greet(name: String = \"world\") -> String
  return \"hello \" + name

let x = greet()
let y = greet(name: \"Alice\")
",
    );
}

#[test]
fn default_params_type_mismatch() {
    let err = common::check_err(
        "\
def f(x: Int = \"not an int\") -> Int
  return x
",
    );
    assert!(
        err.contains("Default value") || err.contains("E001"),
        "expected default value type error, got: {}",
        err
    );
}

#[test]
fn default_params_mixed() {
    common::check_ok(
        "\
def f(a: Int, b: Int = 10) -> Int
  return a + b

let x = f(a: 1)
let y = f(a: 1, b: 2)
",
    );
}

#[test]
fn default_params_all_defaulted() {
    common::check_ok(
        "\
def f(a: Int = 1, b: Int = 2) -> Int
  return a + b

let x = f()
let y = f(a: 10)
let z = f(a: 10, b: 20)
",
    );
}

#[test]
fn default_params_non_default_after_default_error() {
    let err = common::check_parse_err(
        "\
def f(a: Int = 1, b: Int) -> Int
  return a + b
",
    );
    assert!(
        err.contains("without default follows"),
        "expected non-default after default error, got: {}",
        err
    );
}

#[test]
fn default_params_call_with_wrong_name_error() {
    let err = common::check_err(
        "\
def f(a: Int, b: Int = 10) -> Int
  return a + b

let x = f(a: 1, c: 2)
",
    );
    assert!(
        err.contains("unknown") || err.contains("Unknown argument"),
        "expected unknown arg error, got: {}",
        err
    );
}

// ============================================================
// Feature 4: Map Literals
// ============================================================

#[test]
fn map_literal_empty() {
    common::check_ok("let m: Map[String, Int] = {}\n");
}

#[test]
fn map_literal_string_keys() {
    common::check_ok("let m = {\"a\": 1, \"b\": 2, \"c\": 3}\n");
}

#[test]
fn map_literal_int_keys() {
    common::check_ok("let m = {1: \"one\", 2: \"two\"}\n");
}

#[test]
fn map_literal_mixed_key_types_error() {
    let err = common::check_err("let m = {\"a\": 1, 2: 3}\n");
    assert!(
        err.contains("key type mismatch") || err.contains("E003"),
        "expected key type mismatch, got: {}",
        err
    );
}

#[test]
fn map_literal_mixed_value_types_error() {
    let err = common::check_err("let m = {\"a\": 1, \"b\": \"two\"}\n");
    assert!(
        err.contains("value type mismatch") || err.contains("E003"),
        "expected value type mismatch, got: {}",
        err
    );
}

#[test]
fn map_literal_type_annotation() {
    common::check_ok("let m: Map[String, Int] = {\"x\": 10}\n");
}
