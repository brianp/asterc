mod common;

// ─── Basic string interpolation ─────────────────────────────────────

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
fn string_interpolation_float_variable() {
    common::check_ok(
        "\
def main() -> String
  let pi: Float = 3.14
  \"pi is {pi}\"
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

// ─── Printable requirement for interpolation ────────────────────────

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
