mod common;

// ─── Basic expressions and operators ────────────────────────────────

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

// ─── Control flow integration ───────────────────────────────────────

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

// ─── Condition type enforcement ─────────────────────────────────────

#[test]
fn if_condition_must_be_bool() {
    let err = common::check_err(
        r#"if 42
  let x = 1
"#,
    );
    assert!(
        err.contains("Bool"),
        "Expected Bool condition error, got: {}",
        err
    );
}

#[test]
fn while_body_typechecked() {
    common::check_ok(
        r#"let x = 0
while x < 10
  x = x + 1
"#,
    );
}

#[test]
fn for_body_typechecked() {
    common::check_ok(
        r#"let items: List[Int] = [1, 2, 3]
for item in items
  let y = item + 1
"#,
    );
}

// ─── Expression as statement ────────────────────────────────────────

#[test]
fn expr_as_statement() {
    common::check_ok(
        r#"def f() -> Int
  let x = 1
  x + 2
"#,
    );
}

// ─── Parser recursion depth limits ──────────────────────────────────

#[test]
fn unary_recursion_depth_limit() {
    let result = std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(|| {
            let depth = 500;
            let nots: String = "not ".repeat(depth);
            let src = format!("let x = {}true", nots);
            let tokens = lexer::lex(&src).expect("lex ok");
            let mut parser = parser::Parser::new(tokens);
            let result = parser.parse_module("test");
            assert!(result.is_err(), "Expected recursion depth error");
        })
        .unwrap()
        .join();
    assert!(
        result.is_ok(),
        "Thread panicked — stack overflow instead of depth error"
    );
}

#[test]
fn block_recursion_depth_limit() {
    let result = std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(|| {
            let depth = 200;
            let mut src2 = String::new();
            for i in 0..depth {
                let indent = "  ".repeat(i);
                src2.push_str(&format!("{}if true\n", indent));
            }
            let final_indent = "  ".repeat(depth);
            src2.push_str(&format!("{}let x = 1\n", final_indent));
            let tokens = lexer::lex(&src2).expect("lex ok");
            let mut parser = parser::Parser::new(tokens);
            let result = parser.parse_module("test");
            assert!(
                result.is_err(),
                "Expected recursion depth error for nested blocks"
            );
        })
        .unwrap()
        .join();
    assert!(
        result.is_ok(),
        "Thread panicked — stack overflow instead of depth error"
    );
}
