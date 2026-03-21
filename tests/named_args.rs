mod common;

// BC-1: Named args on function calls
// BC-2: Named args on constructor calls

#[test]
fn named_args_simple_function_call() {
    common::check_ok(
        r#"
def greet(name: String) -> String
    name
let x: String = greet(name: "Alice")
"#,
    );
}

#[test]
fn named_args_multi_param_function() {
    common::check_ok(
        r#"
def add(a: Int, b: Int) -> Int
    a + b
let x: Int = add(a: 1, b: 2)
"#,
    );
}

#[test]
fn named_args_order_independent() {
    common::check_ok(
        r#"
def add(a: Int, b: Int) -> Int
    a + b
let x: Int = add(b: 2, a: 1)
"#,
    );
}

#[test]
fn named_args_constructor() {
    common::check_ok(
        r#"
class Point
    x: Int
    y: Int
let p = Point(x: 1, y: 2)
"#,
    );
}

#[test]
fn named_args_constructor_order_independent() {
    common::check_ok(
        r#"
class Point
    x: Int
    y: Int
let p = Point(y: 2, x: 1)
"#,
    );
}

#[test]
fn named_args_builtin_log() {
    common::check_ok(
        r#"
log(message: "hello")
"#,
    );
}

#[test]
fn named_args_builtin_print() {
    common::check_ok(
        r#"
say(message: "hello")
"#,
    );
}

#[test]
fn named_args_builtin_len() {
    common::check_ok(
        r#"
let n: Int = len(value: "hello")
"#,
    );
}

#[test]
fn named_args_builtin_to_string() {
    common::check_ok(
        r#"
let s: String = to_string(value: 42)
"#,
    );
}

#[test]
fn named_args_missing_name_error() {
    // Positional args give a typecheck error hinting at the correct param name
    let err = common::check_err(
        r#"
def greet(name: String) -> String
    name
let x = greet("Alice")
"#,
    );
    assert!(
        err.contains("name: value"),
        "expected hint about 'name' param, got: {}",
        err
    );
}

#[test]
fn named_args_wrong_name_error() {
    let err = common::check_err(
        r#"
def greet(name: String) -> String
    name
let x = greet(nme: "Alice")
"#,
    );
    assert!(
        err.contains("nme"),
        "error should mention wrong arg name: {}",
        err
    );
}

#[test]
fn named_args_duplicate_name_error() {
    common::check_parse_err(
        r#"
def add(a: Int, b: Int) -> Int
    a + b
let x = add(a: 1, a: 2)
"#,
    );
}

#[test]
fn named_args_missing_required_arg_error() {
    let err = common::check_err(
        r#"
def add(a: Int, b: Int) -> Int
    a + b
let x = add(a: 1)
"#,
    );
    assert!(
        err.contains("b") || err.contains("missing"),
        "error should mention missing arg: {}",
        err
    );
}

#[test]
fn named_args_extra_arg_error() {
    let err = common::check_err(
        r#"
def add(a: Int, b: Int) -> Int
    a + b
let x = add(a: 1, b: 2, c: 3)
"#,
    );
    assert!(
        err.contains("c") || err.contains("unknown"),
        "error should mention extra arg: {}",
        err
    );
}

#[test]
fn named_args_type_mismatch() {
    let err = common::check_err(
        r#"
def greet(name: String) -> String
    name
let x = greet(name: 42)
"#,
    );
    assert!(
        err.contains("mismatch") || err.contains("expected") || err.contains("expects"),
        "should report type mismatch: {}",
        err
    );
}

#[test]
fn named_args_generic_function() {
    common::check_ok(
        r#"
def identity(x: T) -> T
    x
let a: Int = identity(x: 42)
let b: String = identity(x: "hello")
"#,
    );
}

#[test]
fn named_args_zero_arg_call() {
    common::check_ok(
        r#"
def nothing() -> Int
    0
let x: Int = nothing()
"#,
    );
}

#[test]
fn named_args_constructor_with_inheritance() {
    common::check_ok(
        r#"
class AppError extends Error
    code: Int
let e = AppError(message: "fail", code: 42)
"#,
    );
}

#[test]
fn named_args_nullable_or() {
    common::check_ok(
        r#"
let x: Int? = nil
let y: Int = x.or(default: 0)
"#,
    );
}

#[test]
fn named_args_nullable_or_else() {
    common::check_ok(
        r#"
let x: Int? = nil
let y: Int = x.or_else(f: 0)
"#,
    );
}
