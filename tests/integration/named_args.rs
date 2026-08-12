// BC-1: Named args on function calls
// BC-2: Named args on constructor calls

#[test]
fn named_args_simple_function_call() {
    crate::common::check_ok(
        r#"
def greet(name: String) -> String
    name
let x: String = greet(name: "Alice")
"#,
    );
}

#[test]
fn named_args_multi_param_function() {
    crate::common::check_ok(
        r#"
def add(a: Int, b: Int) -> Int
    a + b
let x: Int = add(a: 1, b: 2)
"#,
    );
}

#[test]
fn named_args_order_independent() {
    crate::common::check_ok(
        r#"
def add(a: Int, b: Int) -> Int
    a + b
let x: Int = add(b: 2, a: 1)
"#,
    );
}

#[test]
fn named_args_constructor() {
    crate::common::check_ok(
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
    crate::common::check_ok(
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
    crate::common::check_ok(
        r#"
log(message: "hello")
"#,
    );
}

#[test]
fn named_args_builtin_print() {
    crate::common::check_ok(
        r#"
say(message: "hello")
"#,
    );
}

#[test]
fn named_args_builtin_len() {
    crate::common::check_ok(
        r#"
let n: Int = len(value: "hello")
"#,
    );
}

#[test]
fn named_args_builtin_to_string() {
    crate::common::check_ok(
        r#"
let s: String = to_string(value: 42)
"#,
    );
}

#[test]
fn named_args_multi_param_positional_error() {
    // A callee with 2+ params rejects any positional arg, hinting the param name.
    let err = crate::common::check_err(
        r#"
def add(a: Int, b: Int) -> Int
    a + b
let x = add(1, 2)
"#,
    );
    assert!(
        err.contains("add `a: ` before this")
            || err.contains("positional argument")
            || err.contains("named argument"),
        "expected hint about 'a' param, got: {}",
        err
    );
}

// ── Arity-1 named-argument rule (issue #52) ─────────────────────────

#[test]
fn arity1_positional_function_accepted() {
    // A function declaring exactly one param accepts a lone positional arg.
    crate::common::check_ok(
        r#"
def say_hi(message: String) -> String
    message
let x: String = say_hi("hi")
"#,
    );
}

#[test]
fn arity1_labeled_function_accepted() {
    // The explicit label stays legal at arity 1.
    crate::common::check_ok(
        r#"
def say_hi(message: String) -> String
    message
let x: String = say_hi(message: "hi")
"#,
    );
}

#[test]
fn arity1_positional_method_accepted() {
    // A method declaring exactly one param accepts a lone positional arg.
    crate::common::check_ok(
        r#"
class Greeter
    prefix: String
    def greet(message: String) -> String
        message
let g = Greeter(prefix: "hi")
let x: String = g.greet("hello")
"#,
    );
}

#[test]
fn arity1_positional_constructor_accepted() {
    // A class declaring exactly one field accepts a lone positional arg.
    crate::common::check_ok(
        r#"
class Box
    value: Int
let b = Box(42)
"#,
    );
}

#[test]
fn arity1_labeled_constructor_accepted() {
    // The explicit label stays legal at arity 1 for constructors too.
    crate::common::check_ok(
        r#"
class Box
    value: Int
let b = Box(value: 42)
"#,
    );
}

#[test]
fn arity2_positional_function_rejected() {
    // A function declaring 2+ params rejects positional args.
    let err = crate::common::check_err(
        r#"
def add(a: Int, b: Int) -> Int
    a + b
let x = add(1, 2)
"#,
    );
    assert!(
        err.contains("add `a: ` before this")
            || err.contains("positional argument")
            || err.contains("named argument"),
        "expected rejection hinting first param, got: {}",
        err
    );
}

#[test]
fn arity2_positional_constructor_rejected() {
    // A class declaring 2+ fields rejects positional construction; hint names
    // the first unnamed field.
    let err = crate::common::check_err(
        r#"
class Point
    x: Int
    y: Int
let p = Point(1, 2)
"#,
    );
    assert!(
        err.contains("add `x: ` before this")
            || err.contains("positional argument")
            || err.contains("named argument"),
        "expected rejection hinting first field, got: {}",
        err
    );
}

#[test]
fn arity2_lone_positional_with_defaults_rejected() {
    // Declared param count, not call-site arg count, drives the rule: a 2-param
    // callee rejects a lone positional even when the rest have defaults.
    let err = crate::common::check_err(
        r#"
def config(name: String, timeout: Int = 30) -> String
    name
let x = config("prod")
"#,
    );
    assert!(
        err.contains("add `name: ` before this")
            || err.contains("positional argument")
            || err.contains("named argument"),
        "expected rejection hinting 'name', got: {}",
        err
    );
}

#[test]
fn mixed_positional_and_named_rejected() {
    // A single call mixing positional and named args is rejected.
    let err = crate::common::check_err(
        r#"
def add(a: Int, b: Int) -> Int
    a + b
let x = add(1, b: 2)
"#,
    );
    assert!(
        err.contains("add `a: ` before this")
            || err.contains("positional argument")
            || err.contains("named argument"),
        "expected mixed args rejected, got: {}",
        err
    );
}

#[test]
fn arity1_builtin_positional_still_accepted() {
    // The single-arg builtins keep accepting their lone positional argument.
    crate::common::check_ok(
        r#"
say("hi")
log("hello")
let n: Int = len([1, 2, 3])
let s: String = to_string(42)
"#,
    );
}

#[test]
fn named_args_wrong_name_error() {
    let err = crate::common::check_err(
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
    crate::common::check_parse_err(
        r#"
def add(a: Int, b: Int) -> Int
    a + b
let x = add(a: 1, a: 2)
"#,
    );
}

#[test]
fn named_args_missing_required_arg_error() {
    let err = crate::common::check_err(
        r#"
def add(a: Int, b: Int) -> Int
    a + b
let x = add(a: 1)
"#,
    );
    assert!(
        err.contains("b")
            || err.contains("missing")
            || err.contains("expected 2")
            || err.contains("parameter count"),
        "error should mention missing arg: {}",
        err
    );
}

#[test]
fn named_args_extra_arg_error() {
    let err = crate::common::check_err(
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
    let err = crate::common::check_err(
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
    crate::common::check_ok(
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
    crate::common::check_ok(
        r#"
def nothing() -> Int
    0
let x: Int = nothing()
"#,
    );
}

#[test]
fn named_args_constructor_with_inheritance() {
    crate::common::check_ok(
        r#"
class AppError extends Error
    code: Int
let e = AppError(message: "fail", code: 42)
"#,
    );
}

#[test]
fn named_args_nullable_or() {
    crate::common::check_ok(
        r#"
let x: Int? = nil
let y: Int = x.or(default: 0)
"#,
    );
}

#[test]
fn named_args_nullable_or_else() {
    crate::common::check_ok(
        r#"
let x: Int? = nil
let y: Int = x.or_else(f: 0)
"#,
    );
}
