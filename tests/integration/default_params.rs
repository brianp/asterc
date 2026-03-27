
// ─── Basic default parameter values ─────────────────────────────────

#[test]
fn default_params_basic() {
    crate::common::check_ok(
        "\
def greet(name: String = \"world\") -> String
  return \"hello \" + name

let x = greet()
let y = greet(name: \"Alice\")
",
    );
}

#[test]
fn default_params_mixed() {
    crate::common::check_ok(
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
    crate::common::check_ok(
        "\
def f(a: Int = 1, b: Int = 2) -> Int
  return a + b

let x = f()
let y = f(a: 10)
let z = f(a: 10, b: 20)
",
    );
}

// ─── Default parameter errors ───────────────────────────────────────

#[test]
fn default_params_type_mismatch() {
    let err = crate::common::check_err(
        "\
def f(x: Int = \"not an int\") -> Int
  return x
",
    );
    assert!(
        err.contains("Default value")
            || err.contains("E001")
            || err.contains("mismatch")
            || err.contains("expected Int"),
        "expected default value type error, got: {}",
        err
    );
}

#[test]
fn default_params_non_default_after_default_error() {
    let err = crate::common::check_parse_err(
        "\
def f(a: Int = 1, b: Int) -> Int
  return a + b
",
    );
    assert!(
        err.contains("without default follows")
            || err.contains("without default after parameter with default")
            || err.contains("non-default"),
        "expected non-default after default error, got: {}",
        err
    );
}

#[test]
fn default_params_call_with_wrong_name_error() {
    let err = crate::common::check_err(
        "\
def f(a: Int, b: Int = 10) -> Int
  return a + b

let x = f(a: 1, c: 2)
",
    );
    assert!(
        err.contains("unknown")
            || err.contains("Unknown argument")
            || err.contains("Unknown identifier"),
        "expected unknown arg error, got: {}",
        err
    );
}
