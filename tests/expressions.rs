mod common;

// ─── Phase 1: Basic Expressions and Operators ───────────────────────

#[test]
fn integration_arithmetic_expression() {
    common::check_ok("let x = 1 + 2 * 3");
}

#[test]
fn integration_float_promotion() {
    common::check_ok("let x = 1 + 2.0");
}

#[test]
fn integration_string_concat() {
    common::check_ok("let x = \"hello\" + \" world\"");
}

#[test]
fn integration_comparison_returns_bool() {
    common::check_ok("let x = 1 < 2");
}

#[test]
fn integration_logical_expression() {
    common::check_ok("let x = true and false or true");
}

#[test]
fn integration_unary_neg() {
    common::check_ok("let x = -5");
}

#[test]
fn integration_unary_not() {
    common::check_ok("let x = not true");
}

#[test]
fn integration_nested_precedence() {
    common::check_ok("let x = (1 + 2) * 3 - 4 / 2");
}

#[test]
fn integration_grouped_expression() {
    common::check_ok("let x = (1 + 2) * (3 + 4)");
}

#[test]
fn integration_if_with_comparison() {
    common::check_ok("if 1 < 2\n  3\n");
}

#[test]
fn integration_function_with_binary_ops() {
    common::check_ok("def add(a: Int, b: Int) -> Int\n  a + b\n");
}

#[test]
fn integration_function_with_return() {
    common::check_ok("def f() -> Int\n  return 42\n");
}

#[test]
fn integration_string_sub_type_error() {
    let err = common::check_err("let x = \"a\" - \"b\"");
    assert!(err.contains("Cannot apply"));
}

#[test]
fn integration_int_and_bool_error() {
    let err = common::check_err("let x = 1 + true");
    assert!(err.contains("Cannot apply"));
}

#[test]
fn integration_not_int_error() {
    let err = common::check_err("let x = not 5");
    assert!(err.contains("Cannot apply 'not'"));
}

#[test]
fn integration_comparison_type_mismatch() {
    let err = common::check_err("let x = 1 == \"a\"");
    assert!(err.contains("Cannot compare"));
}

// ─── Phase 2: Control Flow Integration ──────────────────────────────

#[test]
fn integration_while_loop() {
    common::check_ok("let x = true\nwhile x\n  log(message: \"hi\")\n");
}

#[test]
fn integration_while_non_bool_error() {
    let err = common::check_err("while 1\n  1\n");
    assert!(err.contains("While condition must be Bool"));
}

#[test]
fn integration_for_loop() {
    common::check_ok("let items = [\"hi\", \"there\"]\nfor x in items\n  log(message: x)\n");
}

#[test]
fn integration_if_elif_else() {
    common::check_ok("if true\n  1\nelif false\n  2\nelse\n  3\n");
}

#[test]
fn integration_multiple_elifs() {
    common::check_ok("if true\n  1\nelif false\n  2\nelif true\n  3\nelse\n  4\n");
}

#[test]
fn integration_assignment() {
    common::check_ok("let x = 1\nx = 2\n");
}

#[test]
fn integration_assignment_type_mismatch() {
    let err = common::check_err("let x = 1\nx = \"hello\"\n");
    assert!(err.contains("mismatch") || err.contains("Mismatch"));
}

#[test]
fn integration_break_in_while() {
    common::check_ok("while true\n  break\n");
}

#[test]
fn integration_continue_in_while() {
    common::check_ok("while true\n  continue\n");
}

#[test]
fn integration_log_builtin() {
    common::check_ok("log(message: \"hello\")");
}
