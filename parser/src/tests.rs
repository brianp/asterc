use super::*;
use ast::{BinOp, Expr, Module, Stmt, Type, UnaryOp};
use lexer::lex;

fn parse_ok(src: &str) -> Module {
    let tokens = lex(src).expect("lex ok");
    let mut parser = Parser::new(tokens);
    parser.parse_module("test").expect("parse ok")
}

fn parse_err(src: &str) -> String {
    let tokens = lex(src).expect("lex ok");
    let mut parser = Parser::new(tokens);
    parser.parse_module("test").unwrap_err().to_string()
}

fn extract_binop(m: &Module) -> (&Expr, &BinOp, &Expr) {
    match &m.body[0] {
        Stmt::Expr(
            Expr::BinaryOp {
                left, op, right, ..
            },
            _,
        ) => (left.as_ref(), op, right.as_ref()),
        other => panic!("expected BinaryOp expr stmt, got {other:?}"),
    }
}

// ─── Basic statements ───────────────────────────────────────────────

#[test]
fn parses_empty_module() {
    let m = parse_ok("");
    assert_eq!(m.name, "test");
    assert!(m.body.is_empty());
}

#[test]
fn parses_simple_expr_stmt() {
    let m = parse_ok("foo");
    assert_eq!(m.body.len(), 1);
    match &m.body[0] {
        Stmt::Expr(Expr::Ident(s, _), _) => assert_eq!(s, "foo"),
        other => panic!("expected expr ident foo, got {other:?}"),
    }
}

#[test]
fn parses_class_with_field_and_method() {
    let src = r#"
class Point
  x: Int
  def show() -> String
    "ok"
"#;
    let m = parse_ok(src);
    assert_eq!(m.body.len(), 1);
    match &m.body[0] {
        Stmt::Class {
            name,
            fields,
            methods,
            ..
        } => {
            assert_eq!(name, "Point");
            assert_eq!(fields, &[("x".to_string(), Type::Int)]);
            assert_eq!(methods.len(), 1);
            match &methods[0] {
                Stmt::Let { name, value, .. } => {
                    assert_eq!(name, "Point.show");
                    match value {
                        Expr::Lambda {
                            params,
                            ret_type,
                            body,
                            ..
                        } => {
                            assert!(params.is_empty());
                            assert_eq!(*ret_type, Type::String);
                            assert_eq!(body.len(), 1);
                            match &body[0] {
                                Stmt::Expr(Expr::Str(s, _), _) => assert_eq!(s, "ok"),
                                other => panic!("expected string literal expr, got {other:?}"),
                            }
                        }
                        _ => panic!("expected lambda"),
                    }
                }
                _ => panic!("expected let stmt"),
            }
        }
        other => panic!("expected class, got {other:?}"),
    }
}

#[test]
fn async_def_is_parse_error() {
    // BC-9: async def is no longer supported — functions are plain def
    let err = parse_err("async def add(a: Int, b: Int) -> Int\n  a\n");
    assert!(err.contains("async def is not supported"));
}

#[test]
fn parses_blocking_call_expr() {
    let m = parse_ok(
        r#"def fetch() -> Int
  42

blocking fetch()
"#,
    );
    match &m.body[1] {
        Stmt::Expr(Expr::BlockingCall { func, args, .. }, _) => {
            assert!(matches!(func.as_ref(), Expr::Ident(name, _) if name == "fetch"));
            assert!(args.is_empty());
        }
        other => panic!("expected blocking call expr, got {other:?}"),
    }
}

#[test]
fn parses_if_else_stmt() {
    let m = parse_ok("if true\n  foo\nelse\n  bar\n");
    assert_eq!(m.body.len(), 1);
    match &m.body[0] {
        Stmt::If {
            cond,
            then_body,
            else_body,
            ..
        } => {
            assert!(matches!(cond, Expr::Bool(true, _)));
            assert_eq!(then_body.len(), 1);
            assert_eq!(else_body.len(), 1);
        }
        other => panic!("expected if stmt, got {other:?}"),
    }
}

// ─── Error reporting ────────────────────────────────────────────────

#[test]
fn reports_bad_class_name() {
    assert!(parse_err("class (").contains("class name"));
}

#[test]
fn reports_bad_fn_param_type() {
    let err = parse_err("def f(x: 42)\n  1\n");
    assert!(err.contains("type") || err.contains("Type"));
}

#[test]
fn reports_unexpected_expr_token() {
    assert!(parse_err("==").contains("unexpected token"));
}

// ─── Arithmetic ─────────────────────────────────────────────────────

#[test]
fn parses_simple_addition() {
    let m = parse_ok("1 + 2");
    let (l, op, r) = extract_binop(&m);
    assert!(matches!(*l, Expr::Int(1, _)));
    assert_eq!(*op, BinOp::Add);
    assert!(matches!(*r, Expr::Int(2, _)));
}

#[test]
fn parses_simple_subtraction() {
    let m = parse_ok("3 - 1");
    let (_, op, _) = extract_binop(&m);
    assert_eq!(*op, BinOp::Sub);
}

#[test]
fn parses_multiplication() {
    let m = parse_ok("2 * 3");
    let (_, op, _) = extract_binop(&m);
    assert_eq!(*op, BinOp::Mul);
}

#[test]
fn parses_division_and_modulo() {
    let m1 = parse_ok("10 / 3");
    let (_, op, _) = extract_binop(&m1);
    assert_eq!(*op, BinOp::Div);

    let m2 = parse_ok("10 % 3");
    let (_, op2, _) = extract_binop(&m2);
    assert_eq!(*op2, BinOp::Mod);
}

#[test]
fn parses_power_right_associative() {
    let m = parse_ok("2 ** 3 ** 4");
    match &m.body[0] {
        Stmt::Expr(
            Expr::BinaryOp {
                left, op, right, ..
            },
            _,
        ) => {
            assert_eq!(*op, BinOp::Pow);
            assert!(matches!(**left, Expr::Int(2, _)));
            match right.as_ref() {
                Expr::BinaryOp { op: inner_op, .. } => assert_eq!(*inner_op, BinOp::Pow),
                other => panic!("expected nested Pow, got {other:?}"),
            }
        }
        other => panic!("expected BinaryOp, got {other:?}"),
    }
}

// ─── Precedence ─────────────────────────────────────────────────────

#[test]
fn precedence_mul_over_add() {
    let m = parse_ok("1 + 2 * 3");
    let (_, op, right) = extract_binop(&m);
    assert_eq!(*op, BinOp::Add);
    assert!(matches!(right, Expr::BinaryOp { op: BinOp::Mul, .. }));
}

#[test]
fn precedence_add_over_comparison() {
    let m = parse_ok("1 + 2 < 4");
    let (left, op, _) = extract_binop(&m);
    assert_eq!(*op, BinOp::Lt);
    assert!(matches!(left, Expr::BinaryOp { op: BinOp::Add, .. }));
}

#[test]
fn precedence_comparison_over_and() {
    let m = parse_ok("1 < 2 and 3 > 4");
    let (left, op, right) = extract_binop(&m);
    assert_eq!(*op, BinOp::And);
    assert!(matches!(left, Expr::BinaryOp { op: BinOp::Lt, .. }));
    assert!(matches!(right, Expr::BinaryOp { op: BinOp::Gt, .. }));
}

#[test]
fn precedence_and_over_or() {
    let m = parse_ok("true and false or true");
    let (left, op, _) = extract_binop(&m);
    assert_eq!(*op, BinOp::Or);
    assert!(matches!(left, Expr::BinaryOp { op: BinOp::And, .. }));
}

#[test]
fn complex_precedence_chain() {
    let m = parse_ok("1 + 2 * 3 == 7 and true");
    let (_, op, _) = extract_binop(&m);
    assert_eq!(*op, BinOp::And);
}

// ─── Unary ──────────────────────────────────────────────────────────

#[test]
fn parses_unary_neg() {
    let m = parse_ok("-5");
    match &m.body[0] {
        Stmt::Expr(Expr::UnaryOp { op, operand, .. }, _) => {
            assert_eq!(*op, UnaryOp::Neg);
            assert!(matches!(**operand, Expr::Int(5, _)));
        }
        other => panic!("expected UnaryOp, got {other:?}"),
    }
}

#[test]
fn parses_unary_not() {
    let m = parse_ok("not true");
    match &m.body[0] {
        Stmt::Expr(Expr::UnaryOp { op, operand, .. }, _) => {
            assert_eq!(*op, UnaryOp::Not);
            assert!(matches!(**operand, Expr::Bool(true, _)));
        }
        other => panic!("expected UnaryOp, got {other:?}"),
    }
}

#[test]
fn unary_neg_in_expression() {
    let m = parse_ok("-1 + 2");
    let (left, op, _) = extract_binop(&m);
    assert_eq!(*op, BinOp::Add);
    assert!(matches!(
        left,
        Expr::UnaryOp {
            op: UnaryOp::Neg,
            ..
        }
    ));
}

#[test]
fn unary_not_in_expression() {
    let m = parse_ok("not true and false");
    let (left, op, _) = extract_binop(&m);
    assert_eq!(*op, BinOp::And);
    assert!(matches!(
        left,
        Expr::UnaryOp {
            op: UnaryOp::Not,
            ..
        }
    ));
}

#[test]
fn double_negation() {
    let m = parse_ok("- -5");
    match &m.body[0] {
        Stmt::Expr(Expr::UnaryOp { op, operand, .. }, _) => {
            assert_eq!(*op, UnaryOp::Neg);
            assert!(matches!(
                operand.as_ref(),
                Expr::UnaryOp {
                    op: UnaryOp::Neg,
                    ..
                }
            ));
        }
        other => panic!("expected double UnaryOp, got {other:?}"),
    }
}

// ─── Grouping ───────────────────────────────────────────────────────

#[test]
fn grouped_expression_parens() {
    let m = parse_ok("(1 + 2) * 3");
    let (left, op, _) = extract_binop(&m);
    assert_eq!(*op, BinOp::Mul);
    assert!(matches!(left, Expr::BinaryOp { op: BinOp::Add, .. }));
}

#[test]
fn nested_grouped_expressions() {
    let m = parse_ok("((1))");
    match &m.body[0] {
        Stmt::Expr(Expr::Int(1, _), _) => {}
        other => panic!("expected Int(1), got {other:?}"),
    }
}

// ─── Postfix (call, member) ─────────────────────────────────────────

#[test]
fn parses_call_in_expression() {
    let m = parse_ok("f(a: 1, b: 2)");
    match &m.body[0] {
        Stmt::Expr(Expr::Call { func, args, .. }, _) => {
            assert!(matches!(func.as_ref(), Expr::Ident(s, _) if s == "f"));
            assert_eq!(args.len(), 2);
        }
        other => panic!("expected Call, got {other:?}"),
    }
}

#[test]
fn parses_member_in_expression() {
    let m = parse_ok("a.b");
    match &m.body[0] {
        Stmt::Expr(Expr::Member { object, field, .. }, _) => {
            assert!(matches!(object.as_ref(), Expr::Ident(s, _) if s == "a"));
            assert_eq!(field, "b");
        }
        other => panic!("expected Member, got {other:?}"),
    }
}

#[test]
fn parses_chained_member_and_call() {
    let m = parse_ok("a.b().c");
    match &m.body[0] {
        Stmt::Expr(Expr::Member { object, field, .. }, _) => {
            assert_eq!(field, "c");
            match object.as_ref() {
                Expr::Call { func, .. } => {
                    assert!(matches!(func.as_ref(), Expr::Member { .. }));
                }
                other => panic!("expected Call, got {other:?}"),
            }
        }
        other => panic!("expected Member, got {other:?}"),
    }
}

// ─── Return ─────────────────────────────────────────────────────────

#[test]
fn parses_return_expression() {
    let m = parse_ok("def f() -> Int\n  return 42\n");
    match &m.body[0] {
        Stmt::Let { value, .. } => match value {
            Expr::Lambda { body, .. } => match &body[0] {
                Stmt::Return(Expr::Int(42, _), _) => {}
                other => panic!("expected Return(42), got {other:?}"),
            },
            other => panic!("expected Lambda, got {other:?}"),
        },
        other => panic!("expected Let, got {other:?}"),
    }
}

// ─── Comparison ─────────────────────────────────────────────────────

#[test]
fn parses_all_comparison_ops() {
    for (src, expected_op) in [
        ("1 == 2", BinOp::Eq),
        ("1 != 2", BinOp::Neq),
        ("1 < 2", BinOp::Lt),
        ("1 > 2", BinOp::Gt),
        ("1 <= 2", BinOp::Lte),
        ("1 >= 2", BinOp::Gte),
    ] {
        let m = parse_ok(src);
        let (_, op, _) = extract_binop(&m);
        assert_eq!(*op, expected_op, "failed for: {src}");
    }
}

// ─── Combined with statements ───────────────────────────────────────

#[test]
fn let_with_binary_expression() {
    let m = parse_ok("let x = 1 + 2");
    match &m.body[0] {
        Stmt::Let { name, value, .. } => {
            assert_eq!(name, "x");
            assert!(matches!(value, Expr::BinaryOp { op: BinOp::Add, .. }));
        }
        other => panic!("expected Let, got {other:?}"),
    }
}

#[test]
fn if_with_comparison_condition() {
    let m = parse_ok("if 1 < 2\n  3\n");
    match &m.body[0] {
        Stmt::If { cond, .. } => {
            assert!(matches!(cond, Expr::BinaryOp { op: BinOp::Lt, .. }));
        }
        other => panic!("expected If, got {other:?}"),
    }
}

// ─── Phase 2: Control Flow ──────────────────────────────────────────

// ─── While ──────────────────────────────────────────────────────────

#[test]
fn parses_while_loop() {
    let m = parse_ok("while true\n  1\n");
    match &m.body[0] {
        Stmt::While { cond, body, .. } => {
            assert!(matches!(cond, Expr::Bool(true, _)));
            assert_eq!(body.len(), 1);
        }
        other => panic!("expected While, got {other:?}"),
    }
}

#[test]
fn parses_while_with_comparison_cond() {
    let m = parse_ok("while x < 10\n  x\n");
    match &m.body[0] {
        Stmt::While { cond, .. } => {
            assert!(matches!(cond, Expr::BinaryOp { op: BinOp::Lt, .. }));
        }
        other => panic!("expected While, got {other:?}"),
    }
}

#[test]
fn parses_while_multi_body() {
    let m = parse_ok("while true\n  1\n  2\n  3\n");
    match &m.body[0] {
        Stmt::While { body, .. } => assert_eq!(body.len(), 3),
        other => panic!("expected While, got {other:?}"),
    }
}

// ─── For ────────────────────────────────────────────────────────────

#[test]
fn parses_for_in_loop() {
    let m = parse_ok("for x in items\n  x\n");
    match &m.body[0] {
        Stmt::For {
            var, iter, body, ..
        } => {
            assert_eq!(var, "x");
            assert!(matches!(iter, Expr::Ident(s, _) if s == "items"));
            assert_eq!(body.len(), 1);
        }
        other => panic!("expected For, got {other:?}"),
    }
}

#[test]
fn parses_for_multi_body() {
    let m = parse_ok("for i in list\n  1\n  2\n");
    match &m.body[0] {
        Stmt::For { body, .. } => assert_eq!(body.len(), 2),
        other => panic!("expected For, got {other:?}"),
    }
}

// ─── Elif ───────────────────────────────────────────────────────────

#[test]
fn parses_if_elif() {
    let m = parse_ok("if true\n  1\nelif false\n  2\n");
    match &m.body[0] {
        Stmt::If {
            elif_branches,
            else_body,
            ..
        } => {
            assert_eq!(elif_branches.len(), 1);
            assert!(else_body.is_empty());
        }
        other => panic!("expected If, got {other:?}"),
    }
}

#[test]
fn parses_if_elif_else() {
    let m = parse_ok("if true\n  1\nelif false\n  2\nelse\n  3\n");
    match &m.body[0] {
        Stmt::If {
            elif_branches,
            else_body,
            ..
        } => {
            assert_eq!(elif_branches.len(), 1);
            assert_eq!(else_body.len(), 1);
        }
        other => panic!("expected If, got {other:?}"),
    }
}

#[test]
fn parses_multiple_elifs() {
    let m = parse_ok("if true\n  1\nelif false\n  2\nelif true\n  3\nelse\n  4\n");
    match &m.body[0] {
        Stmt::If {
            elif_branches,
            else_body,
            ..
        } => {
            assert_eq!(elif_branches.len(), 2);
            assert_eq!(else_body.len(), 1);
        }
        other => panic!("expected If, got {other:?}"),
    }
}

#[test]
fn parses_elif_with_comparison_cond() {
    let m = parse_ok("if x == 1\n  1\nelif x == 2\n  2\n");
    match &m.body[0] {
        Stmt::If { elif_branches, .. } => {
            let (cond, _) = &elif_branches[0];
            assert!(matches!(cond, Expr::BinaryOp { op: BinOp::Eq, .. }));
        }
        other => panic!("expected If, got {other:?}"),
    }
}

// ─── Assignment ─────────────────────────────────────────────────────

#[test]
fn parses_assignment() {
    let m = parse_ok("x = 5");
    match &m.body[0] {
        Stmt::Assignment { target, value, .. } => {
            assert!(matches!(target, Expr::Ident(s, _) if s == "x"));
            assert!(matches!(value, Expr::Int(5, _)));
        }
        other => panic!("expected Assignment, got {other:?}"),
    }
}

#[test]
fn parses_assignment_with_expression() {
    let m = parse_ok("x = 1 + 2");
    match &m.body[0] {
        Stmt::Assignment { target, value, .. } => {
            assert!(matches!(target, Expr::Ident(s, _) if s == "x"));
            assert!(matches!(value, Expr::BinaryOp { op: BinOp::Add, .. }));
        }
        other => panic!("expected Assignment, got {other:?}"),
    }
}

#[test]
fn parses_member_assignment() {
    let m = parse_ok("a.b = 5");
    match &m.body[0] {
        Stmt::Assignment { target, value, .. } => {
            assert!(matches!(target, Expr::Member { .. }));
            assert!(matches!(value, Expr::Int(5, _)));
        }
        other => panic!("expected Assignment, got {other:?}"),
    }
}

// ─── Break / Continue ───────────────────────────────────────────────

#[test]
fn parses_break_stmt() {
    let m = parse_ok("while true\n  break\n");
    match &m.body[0] {
        Stmt::While { body, .. } => {
            assert!(matches!(&body[0], Stmt::Break(_)));
        }
        other => panic!("expected While, got {other:?}"),
    }
}

#[test]
fn parses_continue_stmt() {
    let m = parse_ok("while true\n  continue\n");
    match &m.body[0] {
        Stmt::While { body, .. } => {
            assert!(matches!(&body[0], Stmt::Continue(_)));
        }
        other => panic!("expected While, got {other:?}"),
    }
}

// ─── If/else ────────────────────────────────────────────────────────

#[test]
fn parses_if_else() {
    let m = parse_ok("if true\n  1\nelse\n  2\n");
    match &m.body[0] {
        Stmt::If {
            elif_branches,
            else_body,
            ..
        } => {
            assert!(elif_branches.is_empty());
            assert_eq!(else_body.len(), 1);
        }
        other => panic!("expected If, got {other:?}"),
    }
}

#[test]
fn parses_if_no_else() {
    let m = parse_ok("if true\n  1\n");
    match &m.body[0] {
        Stmt::If {
            elif_branches,
            else_body,
            ..
        } => {
            assert!(elif_branches.is_empty());
            assert!(else_body.is_empty());
        }
        other => panic!("expected If, got {other:?}"),
    }
}

// ─── Phase 3: Collections and Type Annotations ─────────────────────

// ─── Let with type annotation ───────────────────────────────────────

#[test]
fn parses_let_with_type_annotation() {
    let m = parse_ok("let x: Int = 5");
    match &m.body[0] {
        Stmt::Let {
            name,
            type_ann,
            value,
            ..
        } => {
            assert_eq!(name, "x");
            assert_eq!(*type_ann, Some(Type::Int));
            assert!(matches!(value, Expr::Int(5, _)));
        }
        other => panic!("expected Let, got {other:?}"),
    }
}

#[test]
fn parses_let_without_type_annotation() {
    let m = parse_ok("let x = 5");
    match &m.body[0] {
        Stmt::Let { type_ann, .. } => {
            assert_eq!(*type_ann, None);
        }
        other => panic!("expected Let, got {other:?}"),
    }
}

#[test]
fn parses_let_with_string_type_annotation() {
    let m = parse_ok("let name: String = \"alice\"");
    match &m.body[0] {
        Stmt::Let { type_ann, .. } => {
            assert_eq!(*type_ann, Some(Type::String));
        }
        other => panic!("expected Let, got {other:?}"),
    }
}

#[test]
fn parses_let_with_list_type_annotation() {
    let m = parse_ok("let xs: List[Int] = [1, 2]");
    match &m.body[0] {
        Stmt::Let { type_ann, .. } => {
            assert_eq!(*type_ann, Some(Type::List(Box::new(Type::Int))));
        }
        other => panic!("expected Let, got {other:?}"),
    }
}

// ─── List literals ──────────────────────────────────────────────────

#[test]
fn parses_empty_list() {
    let m = parse_ok("[]");
    match &m.body[0] {
        Stmt::Expr(Expr::ListLiteral(elems, _), _) => {
            assert!(elems.is_empty());
        }
        other => panic!("expected ListLiteral, got {other:?}"),
    }
}

#[test]
fn parses_list_with_elements() {
    let m = parse_ok("[1, 2, 3]");
    match &m.body[0] {
        Stmt::Expr(Expr::ListLiteral(elems, _), _) => {
            assert_eq!(elems.len(), 3);
            assert!(matches!(elems[0], Expr::Int(1, _)));
            assert!(matches!(elems[1], Expr::Int(2, _)));
            assert!(matches!(elems[2], Expr::Int(3, _)));
        }
        other => panic!("expected ListLiteral, got {other:?}"),
    }
}

#[test]
fn parses_list_with_single_element() {
    let m = parse_ok("[42]");
    match &m.body[0] {
        Stmt::Expr(Expr::ListLiteral(elems, _), _) => {
            assert_eq!(elems.len(), 1);
            assert!(matches!(elems[0], Expr::Int(42, _)));
        }
        other => panic!("expected ListLiteral, got {other:?}"),
    }
}

#[test]
fn parses_list_with_expressions() {
    let m = parse_ok("[1 + 2, 3 * 4]");
    match &m.body[0] {
        Stmt::Expr(Expr::ListLiteral(elems, _), _) => {
            assert_eq!(elems.len(), 2);
            assert!(matches!(&elems[0], Expr::BinaryOp { op: BinOp::Add, .. }));
            assert!(matches!(&elems[1], Expr::BinaryOp { op: BinOp::Mul, .. }));
        }
        other => panic!("expected ListLiteral, got {other:?}"),
    }
}

#[test]
fn parses_list_trailing_comma() {
    let m = parse_ok("[1, 2,]");
    match &m.body[0] {
        Stmt::Expr(Expr::ListLiteral(elems, _), _) => {
            assert_eq!(elems.len(), 2);
        }
        other => panic!("expected ListLiteral, got {other:?}"),
    }
}

// ─── Indexing ───────────────────────────────────────────────────────

#[test]
fn parses_index_expression() {
    let m = parse_ok("xs[0]");
    match &m.body[0] {
        Stmt::Expr(Expr::Index { object, index, .. }, _) => {
            assert!(matches!(object.as_ref(), Expr::Ident(s, _) if s == "xs"));
            assert!(matches!(**index, Expr::Int(0, _)));
        }
        other => panic!("expected Index, got {other:?}"),
    }
}

#[test]
fn parses_index_with_expression() {
    let m = parse_ok("xs[i + 1]");
    match &m.body[0] {
        Stmt::Expr(Expr::Index { object, index, .. }, _) => {
            assert!(matches!(object.as_ref(), Expr::Ident(s, _) if s == "xs"));
            assert!(matches!(
                index.as_ref(),
                Expr::BinaryOp { op: BinOp::Add, .. }
            ));
        }
        other => panic!("expected Index, got {other:?}"),
    }
}

#[test]
fn parses_chained_index() {
    let m = parse_ok("xs[0][1]");
    match &m.body[0] {
        Stmt::Expr(Expr::Index { object, index, .. }, _) => {
            assert!(matches!(**index, Expr::Int(1, _)));
            assert!(matches!(object.as_ref(), Expr::Index { .. }));
        }
        other => panic!("expected Index, got {other:?}"),
    }
}

#[test]
fn parses_index_after_call() {
    let m = parse_ok("f()[0]");
    match &m.body[0] {
        Stmt::Expr(Expr::Index { object, .. }, _) => {
            assert!(matches!(object.as_ref(), Expr::Call { .. }));
        }
        other => panic!("expected Index, got {other:?}"),
    }
}

// ─── Phase 4: Modules, Imports, Pub ─────────────────────────────────

// ─── Use (whole module) ─────────────────────────────────────────────

#[test]
fn parses_use_whole_module() {
    let m = parse_ok("use std/http");
    match &m.body[0] {
        Stmt::Use {
            path, names, alias, ..
        } => {
            assert_eq!(path, &["std".to_string(), "http".to_string()]);
            assert!(names.is_none());
            assert!(alias.is_none());
        }
        other => panic!("expected Use, got {other:?}"),
    }
}

#[test]
fn parses_use_single_segment() {
    let m = parse_ok("use io");
    match &m.body[0] {
        Stmt::Use {
            path, names, alias, ..
        } => {
            assert_eq!(path, &["io".to_string()]);
            assert!(names.is_none());
            assert!(alias.is_none());
        }
        other => panic!("expected Use, got {other:?}"),
    }
}

#[test]
fn parses_use_deep_path() {
    let m = parse_ok("use std/net/tcp");
    match &m.body[0] {
        Stmt::Use {
            path, names, alias, ..
        } => {
            assert_eq!(
                path,
                &["std".to_string(), "net".to_string(), "tcp".to_string()]
            );
            assert!(names.is_none());
            assert!(alias.is_none());
        }
        other => panic!("expected Use, got {other:?}"),
    }
}

// ─── Use (selective) ────────────────────────────────────────────────

#[test]
fn parses_use_selective_single() {
    let m = parse_ok("use std/http { Request }");
    match &m.body[0] {
        Stmt::Use {
            path, names, alias, ..
        } => {
            assert_eq!(path, &["std".to_string(), "http".to_string()]);
            assert_eq!(names.as_ref().unwrap(), &["Request".to_string()]);
            assert!(alias.is_none());
        }
        other => panic!("expected Use, got {other:?}"),
    }
}

#[test]
fn parses_use_selective_multiple() {
    let m = parse_ok("use std/http { Request, Response }");
    match &m.body[0] {
        Stmt::Use {
            path, names, alias, ..
        } => {
            assert_eq!(path, &["std".to_string(), "http".to_string()]);
            assert_eq!(
                names.as_ref().unwrap(),
                &["Request".to_string(), "Response".to_string()]
            );
            assert!(alias.is_none());
        }
        other => panic!("expected Use, got {other:?}"),
    }
}

// ─── Use (alias) ────────────────────────────────────────────────────

#[test]
fn parses_use_with_alias() {
    let m = parse_ok("use std/http as h");
    match &m.body[0] {
        Stmt::Use {
            path, names, alias, ..
        } => {
            assert_eq!(path, &["std".to_string(), "http".to_string()]);
            assert!(names.is_none());
            assert_eq!(alias.as_deref(), Some("h"));
        }
        other => panic!("expected Use, got {other:?}"),
    }
}

#[test]
fn parses_use_selective_with_alias() {
    let m = parse_ok("use std/http/Security { CSRF, BasicAuth } as hs");
    match &m.body[0] {
        Stmt::Use {
            path, names, alias, ..
        } => {
            assert_eq!(
                path,
                &[
                    "std".to_string(),
                    "http".to_string(),
                    "Security".to_string()
                ]
            );
            assert_eq!(
                names.as_ref().unwrap(),
                &["CSRF".to_string(), "BasicAuth".to_string()]
            );
            assert_eq!(alias.as_deref(), Some("hs"));
        }
        other => panic!("expected Use, got {other:?}"),
    }
}

// ─── Pub modifier ───────────────────────────────────────────────────

#[test]
fn parses_pub_def() {
    let m = parse_ok("pub def foo() -> Int\n  42\n");
    match &m.body[0] {
        Stmt::Let {
            name, is_public, ..
        } => {
            assert_eq!(name, "foo");
            assert!(*is_public);
        }
        other => panic!("expected Let, got {other:?}"),
    }
}

#[test]
fn parses_private_def_by_default() {
    let m = parse_ok("def foo() -> Int\n  42\n");
    match &m.body[0] {
        Stmt::Let { is_public, .. } => {
            assert!(!(*is_public));
        }
        other => panic!("expected Let, got {other:?}"),
    }
}

#[test]
fn parses_pub_class() {
    let m = parse_ok("pub class Foo\n  x: Int\n");
    match &m.body[0] {
        Stmt::Class {
            name, is_public, ..
        } => {
            assert_eq!(name, "Foo");
            assert!(*is_public);
        }
        other => panic!("expected Class, got {other:?}"),
    }
}

#[test]
fn parses_private_class_by_default() {
    let m = parse_ok("class Foo\n  x: Int\n");
    match &m.body[0] {
        Stmt::Class { is_public, .. } => {
            assert!(!(*is_public));
        }
        other => panic!("expected Class, got {other:?}"),
    }
}

#[test]
fn parses_pub_let() {
    let m = parse_ok("pub let x = 5");
    match &m.body[0] {
        Stmt::Let {
            name, is_public, ..
        } => {
            assert_eq!(name, "x");
            assert!(*is_public);
        }
        other => panic!("expected Let, got {other:?}"),
    }
}

#[test]
fn parses_private_let_by_default() {
    let m = parse_ok("let x = 5");
    match &m.body[0] {
        Stmt::Let { is_public, .. } => {
            assert!(!(*is_public));
        }
        other => panic!("expected Let, got {other:?}"),
    }
}

#[test]
fn pub_async_def_is_parse_error() {
    // BC-9: pub async def is no longer valid
    let err = parse_err("pub async def fetch(url: String) -> String\n  url\n");
    assert!(
        err.contains("def")
            || err.contains("class")
            || err.contains("trait")
            || err.contains("let")
    );
}

// ─── Use + other stmts together ─────────────────────────────────────

#[test]
fn parses_use_then_code() {
    let m = parse_ok("use io\nlet x = 5");
    assert_eq!(m.body.len(), 2);
    assert!(matches!(&m.body[0], Stmt::Use { .. }));
    assert!(matches!(&m.body[1], Stmt::Let { .. }));
}

// ─── Original tests ─────────────────────────────────────────────────

#[test]
fn nested_classes_and_methods() {
    let src = r#"
class Outer
  a: Int
  class Inner
    def go() -> Int
      1
"#;
    let m = parse_ok(src);
    match &m.body[0] {
        Stmt::Class {
            name,
            fields,
            methods,
            ..
        } => {
            assert_eq!(name, "Outer");
            assert_eq!(fields, &[("a".to_string(), Type::Int)]);
            assert!(
                methods
                    .iter()
                    .any(|m| matches!(m, Stmt::Class { name, .. } if name == "Inner")),
            );
            let inner = methods
                .iter()
                .find_map(|m| match m {
                    Stmt::Class { name, methods, .. } if name == "Inner" => Some(methods),
                    _ => None,
                })
                .expect("Inner class present");
            assert!(
                inner
                    .iter()
                    .any(|m| matches!(m, Stmt::Let { name, .. } if name == "Inner.go"))
                    || inner
                        .iter()
                        .any(|m| matches!(m, Stmt::Let { name, .. } if name == "Outer.Inner.go")),
            );
        }
        _ => panic!("expected class Outer"),
    }
}

// ─── Phase 5: Generics and Traits ───────────────────────────────────

// ─── Generic class parsing ──────────────────────────────────────────

#[test]
fn parses_generic_class() {
    let m = parse_ok("class Stack[T]\n  items: List[T]\n");
    match &m.body[0] {
        Stmt::Class {
            name,
            generic_params,
            fields,
            ..
        } => {
            assert_eq!(name, "Stack");
            assert_eq!(generic_params.as_ref().unwrap(), &["T".to_string()]);
            assert_eq!(fields.len(), 1);
        }
        other => panic!("expected Class, got {other:?}"),
    }
}

#[test]
fn parses_generic_class_multiple_params() {
    let m = parse_ok("class Pair[A, B]\n  first: A\n  second: B\n");
    match &m.body[0] {
        Stmt::Class {
            name,
            generic_params,
            ..
        } => {
            assert_eq!(name, "Pair");
            assert_eq!(
                generic_params.as_ref().unwrap(),
                &["A".to_string(), "B".to_string()]
            );
        }
        other => panic!("expected Class, got {other:?}"),
    }
}

#[test]
fn parses_class_without_generics_still_works() {
    let m = parse_ok("class Foo\n  x: Int\n");
    match &m.body[0] {
        Stmt::Class { generic_params, .. } => {
            assert!(generic_params.is_none());
        }
        other => panic!("expected Class, got {other:?}"),
    }
}

// ─── Generic function parsing (BC-5: inline syntax) ────────────────

#[test]
fn parses_inline_generic_function() {
    // New syntax: def identity(x: T) -> T (no [T] bracket)
    // Parser produces Custom("T", []) — typechecker converts to TypeVar
    let m = parse_ok("def identity(x: T) -> T\n  x\n");
    match &m.body[0] {
        Stmt::Let { name, value, .. } => {
            assert_eq!(name, "identity");
            match value {
                Expr::Lambda {
                    generic_params,
                    params,
                    ret_type,
                    ..
                } => {
                    assert!(generic_params.is_none());
                    assert_eq!(params.len(), 1);
                    assert_eq!(params[0].1, Type::Custom("T".into(), vec![]));
                    assert_eq!(*ret_type, Type::Custom("T".into(), vec![]));
                }
                other => panic!("expected Lambda, got {other:?}"),
            }
        }
        other => panic!("expected Let, got {other:?}"),
    }
}

#[test]
fn parses_inline_generic_function_multi_params() {
    let m = parse_ok("def map(x: T, f: T) -> U\n  x\n");
    match &m.body[0] {
        Stmt::Let { value, .. } => match value {
            Expr::Lambda {
                generic_params,
                params,
                ret_type,
                ..
            } => {
                assert!(generic_params.is_none());
                assert_eq!(params[0].1, Type::Custom("T".into(), vec![]));
                assert_eq!(params[1].1, Type::Custom("T".into(), vec![]));
                assert_eq!(*ret_type, Type::Custom("U".into(), vec![]));
            }
            other => panic!("expected Lambda, got {other:?}"),
        },
        other => panic!("expected Let, got {other:?}"),
    }
}

#[test]
fn bracket_generic_on_function_is_parse_error() {
    // Old [T] syntax on functions is now a parse error
    parse_err("def identity[T](x: T) -> T\n  x\n");
}

#[test]
fn parses_function_without_generics_still_works() {
    let m = parse_ok("def foo(x: Int) -> Int\n  x\n");
    match &m.body[0] {
        Stmt::Let { value, .. } => match value {
            Expr::Lambda { generic_params, .. } => {
                assert!(generic_params.is_none());
            }
            other => panic!("expected Lambda, got {other:?}"),
        },
        other => panic!("expected Let, got {other:?}"),
    }
}

// ─── TypeVar in class generic context ───────────────────────────────

#[test]
fn parses_typevar_in_class_generic_context() {
    // Class [T] syntax still works — T becomes TypeVar inside class body
    let m = parse_ok("class Box[T]\n  value: T\n");
    match &m.body[0] {
        Stmt::Class {
            generic_params,
            fields,
            ..
        } => {
            assert_eq!(generic_params.as_ref().unwrap(), &["T".to_string()]);
            assert_eq!(fields[0].1, Type::TypeVar("T".into(), vec![]));
        }
        other => panic!("expected Class, got {other:?}"),
    }
}

// ─── Trait definitions ──────────────────────────────────────────────

#[test]
fn parses_trait_with_required_method() {
    let src = "trait Printable\n  def to_string() -> String\n";
    let m = parse_ok(src);
    match &m.body[0] {
        Stmt::Trait {
            name,
            methods,
            is_public,
            ..
        } => {
            assert_eq!(name, "Printable");
            assert!(!is_public);
            assert_eq!(methods.len(), 1);
        }
        other => panic!("expected Trait, got {other:?}"),
    }
}

#[test]
fn parses_pub_trait() {
    let src = "pub trait Printable\n  def to_string() -> String\n";
    let m = parse_ok(src);
    match &m.body[0] {
        Stmt::Trait {
            name, is_public, ..
        } => {
            assert_eq!(name, "Printable");
            assert!(is_public);
        }
        other => panic!("expected Trait, got {other:?}"),
    }
}

#[test]
fn parses_trait_with_default_method() {
    let src = r#"trait Printable
  def to_string() -> String
  def print()
    log(msg: to_string())
"#;
    let m = parse_ok(src);
    match &m.body[0] {
        Stmt::Trait { name, methods, .. } => {
            assert_eq!(name, "Printable");
            assert_eq!(methods.len(), 2);
        }
        other => panic!("expected Trait, got {other:?}"),
    }
}

// ─── Class includes ─────────────────────────────────────────────────

#[test]
fn parses_class_with_single_include() {
    let src = "class User includes Printable\n  name: String\n";
    let m = parse_ok(src);
    match &m.body[0] {
        Stmt::Class { name, includes, .. } => {
            assert_eq!(name, "User");
            assert_eq!(
                includes.as_ref().unwrap(),
                &[("Printable".to_string(), vec![])]
            );
        }
        other => panic!("expected Class, got {other:?}"),
    }
}

#[test]
fn parses_class_with_multiple_includes() {
    let src = "class User includes Printable, Serializable\n  name: String\n";
    let m = parse_ok(src);
    match &m.body[0] {
        Stmt::Class { includes, .. } => {
            let inc = includes.as_ref().unwrap();
            assert_eq!(
                inc,
                &[
                    ("Printable".to_string(), vec![]),
                    ("Serializable".to_string(), vec![])
                ]
            );
        }
        other => panic!("expected Class, got {other:?}"),
    }
}

#[test]
fn parses_class_without_includes_still_works() {
    let m = parse_ok("class Foo\n  x: Int\n");
    match &m.body[0] {
        Stmt::Class { includes, .. } => {
            assert!(includes.is_none());
        }
        other => panic!("expected Class, got {other:?}"),
    }
}

#[test]
fn parses_pub_class_with_includes() {
    let src = "pub class User includes Printable\n  name: String\n";
    let m = parse_ok(src);
    match &m.body[0] {
        Stmt::Class {
            name,
            is_public,
            includes,
            ..
        } => {
            assert_eq!(name, "User");
            assert!(is_public);
            assert_eq!(
                includes.as_ref().unwrap(),
                &[("Printable".to_string(), vec![])]
            );
        }
        other => panic!("expected Class, got {other:?}"),
    }
}

// ─── Generic class with includes ────────────────────────────────────

#[test]
fn parses_generic_class_with_includes() {
    let src = "class Container[T] includes Printable\n  item: T\n";
    let m = parse_ok(src);
    match &m.body[0] {
        Stmt::Class {
            name,
            generic_params,
            includes,
            ..
        } => {
            assert_eq!(name, "Container");
            assert_eq!(generic_params.as_ref().unwrap(), &["T".to_string()]);
            assert_eq!(
                includes.as_ref().unwrap(),
                &[("Printable".to_string(), vec![])]
            );
        }
        other => panic!("expected Class, got {other:?}"),
    }
}

// ─── Fn type syntax ─────────────────────────────────────────────────

#[test]
fn parses_fn_type_single_param() {
    let src = "def apply(f: Fn(Int) -> Int, x: Int) -> Int\n  f(_0: x)\n";
    let m = parse_ok(src);
    match &m.body[0] {
        Stmt::Let {
            value: Expr::Lambda { params, .. },
            ..
        } => match &params[0].1 {
            Type::Function { params, ret, .. } => {
                assert_eq!(params, &[Type::Int]);
                assert_eq!(ret.as_ref(), &Type::Int);
            }
            other => panic!("expected Function type, got {other:?}"),
        },
        other => panic!("expected Let/Lambda, got {other:?}"),
    }
}

#[test]
fn parses_fn_type_multi_param() {
    let src = "def combine(f: Fn(Int, Int) -> Int) -> Int\n  f(_0: 1, _1: 2)\n";
    let m = parse_ok(src);
    match &m.body[0] {
        Stmt::Let {
            value: Expr::Lambda { params, .. },
            ..
        } => match &params[0].1 {
            Type::Function { params, ret, .. } => {
                assert_eq!(params, &[Type::Int, Type::Int]);
                assert_eq!(ret.as_ref(), &Type::Int);
            }
            other => panic!("expected Function type, got {other:?}"),
        },
        other => panic!("expected Let/Lambda, got {other:?}"),
    }
}

#[test]
fn parses_fn_type_no_params() {
    let src = "def run(f: Fn() -> Int) -> Int\n  f()\n";
    let m = parse_ok(src);
    match &m.body[0] {
        Stmt::Let {
            value: Expr::Lambda { params, .. },
            ..
        } => match &params[0].1 {
            Type::Function { params, ret, .. } => {
                assert!(params.is_empty());
                assert_eq!(ret.as_ref(), &Type::Int);
            }
            other => panic!("expected Function type, got {other:?}"),
        },
        other => panic!("expected Let/Lambda, got {other:?}"),
    }
}

#[test]
fn parses_fn_type_return_position() {
    let src = "def get_handler() -> Fn(Int) -> String\n  -> x: to_string(value: x)\n";
    let m = parse_ok(src);
    match &m.body[0] {
        Stmt::Let {
            value: Expr::Lambda { ret_type, .. },
            ..
        } => match ret_type {
            Type::Function { params, ret, .. } => {
                assert_eq!(params, &[Type::Int]);
                assert_eq!(ret.as_ref(), &Type::String);
            }
            other => panic!("expected Function type, got {other:?}"),
        },
        other => panic!("expected Let/Lambda, got {other:?}"),
    }
}

#[test]
fn bare_paren_fn_type_is_parse_error() {
    let err = parse_err("def apply(f: (Int) -> Int) -> Int\n  f(_0: 1)\n");
    assert!(
        err.contains("Fn") || err.contains("function type"),
        "expected Fn hint in error, got: {err}"
    );
}
