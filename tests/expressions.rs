mod common;

// ─── Phase 1: Basic Expressions and Operators ───────────────────────

#[test]
fn arithmetic_expression() {
    common::check_ok("let x = 1 + 2 * 3");
}

#[test]
fn float_promotion() {
    common::check_ok("let x = 1 + 2.0");
}

#[test]
fn string_concat() {
    common::check_ok("let x = \"hello\" + \" world\"");
}

#[test]
fn comparison_returns_bool() {
    common::check_ok("let x = 1 < 2");
}

#[test]
fn logical_expression() {
    common::check_ok("let x = true and false or true");
}

#[test]
fn unary_neg() {
    common::check_ok("let x = -5");
}

#[test]
fn unary_not() {
    common::check_ok("let x = not true");
}

#[test]
fn nested_precedence() {
    common::check_ok("let x = (1 + 2) * 3 - 4 / 2");
}

#[test]
fn grouped_expression() {
    common::check_ok("let x = (1 + 2) * (3 + 4)");
}

#[test]
fn if_with_comparison() {
    common::check_ok("if 1 < 2\n  3\n");
}

#[test]
fn function_with_binary_ops() {
    common::check_ok("def add(a: Int, b: Int) -> Int\n  a + b\n");
}

#[test]
fn function_with_return() {
    common::check_ok("def f() -> Int\n  return 42\n");
}

#[test]
fn string_sub_type_error() {
    let err = common::check_err("let x = \"a\" - \"b\"");
    assert!(err.contains("Cannot apply"));
}

#[test]
fn int_and_bool_error() {
    let err = common::check_err("let x = 1 + true");
    assert!(err.contains("Cannot apply"));
}

#[test]
fn not_int_error() {
    let err = common::check_err("let x = not 5");
    assert!(err.contains("Cannot apply 'not'"));
}

#[test]
fn comparison_type_mismatch() {
    let err = common::check_err("let x = 1 == \"a\"");
    assert!(err.contains("Cannot compare"));
}

// ─── Phase 2: Control Flow Integration ──────────────────────────────

#[test]
fn while_loop() {
    common::check_ok("let x = true\nwhile x\n  log(message: \"hi\")\n");
}

#[test]
fn while_non_bool_error() {
    let err = common::check_err("while 1\n  1\n");
    assert!(err.contains("While condition must be Bool"));
}

#[test]
fn for_loop() {
    common::check_ok("let items = [\"hi\", \"there\"]\nfor x in items\n  log(message: x)\n");
}

#[test]
fn if_elif_else() {
    common::check_ok("if true\n  1\nelif false\n  2\nelse\n  3\n");
}

#[test]
fn multiple_elifs() {
    common::check_ok("if true\n  1\nelif false\n  2\nelif true\n  3\nelse\n  4\n");
}

#[test]
fn assignment() {
    common::check_ok("let x = 1\nx = 2\n");
}

#[test]
fn assignment_type_mismatch() {
    let err = common::check_err("let x = 1\nx = \"hello\"\n");
    assert!(err.contains("mismatch") || err.contains("Mismatch"));
}

#[test]
fn break_in_while() {
    common::check_ok("while true\n  break\n");
}

#[test]
fn continue_in_while() {
    common::check_ok("while true\n  continue\n");
}

#[test]
fn log_builtin() {
    common::check_ok("log(message: \"hello\")");
}
