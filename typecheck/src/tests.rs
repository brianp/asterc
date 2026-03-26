use super::typechecker::TypeChecker;
use ast::{BinOp, Expr, Span, Stmt, Type, UnaryOp};

// ─── Helpers ────────────────────────────────────────────────────────

fn s() -> Span {
    Span::dummy()
}

fn binop(left: Expr, op: BinOp, right: Expr) -> Expr {
    Expr::BinaryOp {
        left: Box::new(left),
        op,
        right: Box::new(right),
        span: s(),
    }
}

fn unop(op: UnaryOp, operand: Expr) -> Expr {
    Expr::UnaryOp {
        op,
        operand: Box::new(operand),
        span: s(),
    }
}

fn err_msg(r: Result<Type, ast::Diagnostic>) -> String {
    r.unwrap_err().to_string()
}

fn module_err(src: &str) -> String {
    let tokens = lexer::lex(src).expect("lex ok");
    let mut parser = parser::Parser::new(tokens);
    let module = parser.parse_module("test").expect("parse ok");
    let mut tc = TypeChecker::new();
    tc.check_module(&module).unwrap_err().to_string()
}

fn module_ok(src: &str) -> ast::Module {
    let tokens = lexer::lex(src).expect("lex ok");
    let mut parser = parser::Parser::new(tokens);
    parser.parse_module("test").expect("parse ok")
}

// ─── Core: let, lambda, call, if, class ─────────────────────────────

#[test]
fn let_and_ident_lookup() {
    let mut tc = TypeChecker::new();
    let stmt = Stmt::Let {
        name: "x".into(),
        type_ann: None,
        value: Expr::Int(1, s()),
        is_public: false,
        span: s(),
    };
    assert_eq!(tc.check_stmt(&stmt).unwrap(), Type::Int);
    assert_eq!(tc.env.get_var("x").cloned(), Some(Type::Int));
}

#[test]
fn lambda_type_check() {
    let mut tc = TypeChecker::new();
    let lambda = Expr::Lambda {
        params: vec![("a".into(), Type::Int)],
        ret_type: Type::Int,
        body: vec![Stmt::Expr(Expr::Ident("a".into(), s()), s())],
        generic_params: None,
        throws: None,
        type_constraints: vec![],
        defaults: Box::new(vec![]),
        span: s(),
    };
    let ty = tc.check_expr(&lambda).unwrap();
    match ty {
        Type::Function { params, ret, .. } => {
            assert_eq!(params, vec![Type::Int]);
            assert_eq!(*ret, Type::Int);
        }
        _ => panic!("expected function type"),
    }
}

#[test]
fn call_type_check_and_mismatch() {
    let mut tc = TypeChecker::new();
    let lambda = Expr::Lambda {
        params: vec![("x".into(), Type::Int)],
        ret_type: Type::Int,
        body: vec![Stmt::Expr(Expr::Ident("x".into(), s()), s())],
        generic_params: None,
        throws: None,
        type_constraints: vec![],
        defaults: Box::new(vec![]),
        span: s(),
    };
    tc.check_stmt(&Stmt::Let {
        name: "f".into(),
        type_ann: None,
        value: lambda,
        is_public: false,
        span: s(),
    })
    .unwrap();

    let call = Expr::Call {
        func: Box::new(Expr::Ident("f".into(), s())),
        args: vec![("x".into(), s(), Expr::Int(42, s()))],
        span: s(),
    };
    assert_eq!(tc.check_expr(&call).unwrap(), Type::Int);

    let bad_call = Expr::Call {
        func: Box::new(Expr::Ident("f".into(), s())),
        args: vec![("x".into(), s(), Expr::Str("oops".into(), s()))],
        span: s(),
    };
    let err = err_msg(tc.check_expr(&bad_call));
    assert!(err.contains("expects Int, got String"), "got: {}", err);
}

#[test]
fn if_type_check() {
    let mut tc = TypeChecker::new();
    let stmt = Stmt::If {
        cond: Expr::Bool(true, s()),
        then_body: vec![Stmt::Expr(Expr::Int(1, s()), s())],
        elif_branches: vec![],
        else_body: vec![Stmt::Expr(Expr::Int(2, s()), s())],
        span: s(),
    };
    assert_eq!(tc.check_stmt(&stmt).unwrap(), Type::Int);

    let bad = Stmt::If {
        cond: Expr::Int(1, s()),
        then_body: vec![],
        elif_branches: vec![],
        else_body: vec![],
        span: s(),
    };
    let err = err_msg(tc.check_stmt(&bad));
    assert!(err.contains("If condition must be Bool"));
}

#[test]
fn class_type_check_and_member_access() {
    let mut tc = TypeChecker::new();
    let class_stmt = Stmt::Class {
        name: "Point".into(),
        fields: vec![("x".into(), Type::Int, false)],
        methods: vec![Stmt::Let {
            name: "Point.show".into(),
            type_ann: None,
            value: Expr::Lambda {
                params: vec![],
                ret_type: Type::String,
                body: vec![Stmt::Expr(Expr::Str("ok".into(), s()), s())],
                generic_params: None,
                throws: None,
                type_constraints: vec![],
                defaults: Box::new(vec![]),
                span: s(),
            },
            is_public: false,
            span: s(),
        }],
        is_public: false,
        generic_params: None,
        extends: None,
        includes: None,
        span: s(),
    };
    tc.check_stmt(&class_stmt).unwrap();

    tc.env
        .set_var("p".into(), Type::Custom("Point".into(), Vec::new()));
    let access = Expr::Member {
        object: Box::new(Expr::Ident("p".into(), s())),
        field: "x".into(),
        span: s(),
    };
    assert_eq!(tc.check_expr(&access).unwrap(), Type::Int);

    let access_m = Expr::Member {
        object: Box::new(Expr::Ident("p".into(), s())),
        field: "show".into(),
        span: s(),
    };
    assert!(tc.check_expr(&access_m).is_ok());
}

#[test]
fn unknowns_and_errors() {
    let mut tc = TypeChecker::new();
    let err = err_msg(tc.check_expr(&Expr::Ident("y".into(), s())));
    assert!(err.contains("Unknown identifier"));

    let call = Expr::Call {
        func: Box::new(Expr::Int(1, s())),
        args: vec![],
        span: s(),
    };
    let err = err_msg(tc.check_expr(&call));
    assert!(err.contains("Tried to call non-function"));

    tc.env.set_var("p".into(), Type::Int);
    let access = Expr::Member {
        object: Box::new(Expr::Ident("p".into(), s())),
        field: "foo".into(),
        span: s(),
    };
    let err = err_msg(tc.check_expr(&access));
    assert!(err.contains("Cannot access member"));
}

// ─── BinaryOp: Arithmetic ──────────────────────────────────────────

#[test]
fn binary_add_int_int() {
    let mut tc = TypeChecker::new();
    assert_eq!(
        tc.check_expr(&binop(Expr::Int(1, s()), BinOp::Add, Expr::Int(2, s())))
            .unwrap(),
        Type::Int
    );
}

#[test]
fn binary_add_float_float() {
    let mut tc = TypeChecker::new();
    assert_eq!(
        tc.check_expr(&binop(
            Expr::Float(1.0, s()),
            BinOp::Add,
            Expr::Float(2.0, s())
        ))
        .unwrap(),
        Type::Float
    );
}

#[test]
fn binary_add_int_float_promotes() {
    let mut tc = TypeChecker::new();
    assert_eq!(
        tc.check_expr(&binop(Expr::Int(1, s()), BinOp::Add, Expr::Float(2.0, s())))
            .unwrap(),
        Type::Float
    );
}

#[test]
fn binary_add_float_int_promotes() {
    let mut tc = TypeChecker::new();
    assert_eq!(
        tc.check_expr(&binop(Expr::Float(1.0, s()), BinOp::Add, Expr::Int(2, s())))
            .unwrap(),
        Type::Float
    );
}

#[test]
fn binary_add_string_string() {
    let mut tc = TypeChecker::new();
    assert_eq!(
        tc.check_expr(&binop(
            Expr::Str("a".into(), s()),
            BinOp::Add,
            Expr::Str("b".into(), s())
        ))
        .unwrap(),
        Type::String
    );
}

#[test]
fn task_resolve_rejects_detectable_double_consumption() {
    let src = "\
def fetch() -> Int
  42

def main() -> Int
  let t: Task[Int] = async fetch()
  let first = resolve t!
  let second = resolve t!
  second
";
    let err = module_err(src);
    assert!(err.contains("Task 't' is already consumed"));
}

#[test]
fn plain_call_rejects_suspendability_inferred_from_nested_branch() {
    let src = "\
def fetch() -> Int
  42

def parent() -> Int
  if true
    async fetch()
  7

def main() -> Int
  parent()
";
    let err = module_err(src);
    assert!(err.contains("blocking parent()") || err.contains("async parent()"));
}

#[test]
fn resolve_all_returns_list_of_task_results() {
    let src = "\
def fetch() -> Int
  42

def main() -> List[Int]
  let tasks: List[Task[Int]] = [async fetch()]
  resolve_all(tasks: tasks)!
";
    let module = module_ok(src);
    let mut tc = TypeChecker::new();
    tc.check_module(&module).expect("typecheck ok");
}

#[test]
fn resolve_first_returns_task_result_type() {
    let src = "\
def fetch() -> Int
  42

def main() -> Int
  let tasks: List[Task[Int]] = [async fetch()]
  resolve_first(tasks: tasks)!
";
    let module = module_ok(src);
    let mut tc = TypeChecker::new();
    tc.check_module(&module).expect("typecheck ok");
}

#[test]
fn resolve_all_rejects_non_task_lists() {
    let src = "\
def main() -> List[Int]
  let values: List[Int] = [1, 2]
  resolve_all(tasks: values)!
";
    let err = module_err(src);
    assert!(err.contains("resolve_all"));
    assert!(err.contains("Task"));
}

#[test]
fn resolve_first_rejects_non_task_lists() {
    let src = "\
def main() -> Int
  let values: List[Int] = [1, 2]
  resolve_first(tasks: values)!
";
    let err = module_err(src);
    assert!(err.contains("resolve_first"));
    assert!(err.contains("Task"));
}

#[test]
fn binary_sub_int_int() {
    let mut tc = TypeChecker::new();
    assert_eq!(
        tc.check_expr(&binop(Expr::Int(5, s()), BinOp::Sub, Expr::Int(3, s())))
            .unwrap(),
        Type::Int
    );
}

#[test]
fn binary_mul_float_float() {
    let mut tc = TypeChecker::new();
    assert_eq!(
        tc.check_expr(&binop(
            Expr::Float(2.0, s()),
            BinOp::Mul,
            Expr::Float(3.0, s())
        ))
        .unwrap(),
        Type::Float
    );
}

#[test]
fn binary_div_int_int() {
    let mut tc = TypeChecker::new();
    assert_eq!(
        tc.check_expr(&binop(Expr::Int(10, s()), BinOp::Div, Expr::Int(3, s())))
            .unwrap(),
        Type::Int
    );
}

#[test]
fn binary_mod_int_int() {
    let mut tc = TypeChecker::new();
    assert_eq!(
        tc.check_expr(&binop(Expr::Int(10, s()), BinOp::Mod, Expr::Int(3, s())))
            .unwrap(),
        Type::Int
    );
}

#[test]
fn binary_pow_int_int() {
    let mut tc = TypeChecker::new();
    assert_eq!(
        tc.check_expr(&binop(Expr::Int(2, s()), BinOp::Pow, Expr::Int(3, s())))
            .unwrap(),
        Type::Int
    );
}

#[test]
fn binary_pow_float_int_promotes() {
    let mut tc = TypeChecker::new();
    assert_eq!(
        tc.check_expr(&binop(Expr::Float(2.0, s()), BinOp::Pow, Expr::Int(3, s())))
            .unwrap(),
        Type::Float
    );
}

// ─── Arithmetic errors ─────────────────────────────────────────────

#[test]
fn binary_arithmetic_type_error() {
    let mut tc = TypeChecker::new();
    let err = err_msg(tc.check_expr(&binop(Expr::Int(1, s()), BinOp::Add, Expr::Bool(true, s()))));
    assert!(err.contains("Cannot apply"));
}

#[test]
fn binary_string_mul_error() {
    let mut tc = TypeChecker::new();
    let err = err_msg(tc.check_expr(&binop(
        Expr::Str("a".into(), s()),
        BinOp::Mul,
        Expr::Str("b".into(), s()),
    )));
    assert!(err.contains("Cannot apply"));
}

// ─── Comparison ─────────────────────────────────────────────────────

#[test]
fn binary_eq_int_int() {
    let mut tc = TypeChecker::new();
    assert_eq!(
        tc.check_expr(&binop(Expr::Int(1, s()), BinOp::Eq, Expr::Int(1, s())))
            .unwrap(),
        Type::Bool
    );
}

#[test]
fn binary_eq_string_string() {
    let mut tc = TypeChecker::new();
    assert_eq!(
        tc.check_expr(&binop(
            Expr::Str("a".into(), s()),
            BinOp::Eq,
            Expr::Str("a".into(), s())
        ))
        .unwrap(),
        Type::Bool
    );
}

#[test]
fn binary_neq_bool_bool() {
    let mut tc = TypeChecker::new();
    assert_eq!(
        tc.check_expr(&binop(
            Expr::Bool(true, s()),
            BinOp::Neq,
            Expr::Bool(false, s())
        ))
        .unwrap(),
        Type::Bool
    );
}

#[test]
fn binary_lt_int_int() {
    let mut tc = TypeChecker::new();
    assert_eq!(
        tc.check_expr(&binop(Expr::Int(1, s()), BinOp::Lt, Expr::Int(2, s())))
            .unwrap(),
        Type::Bool
    );
}

#[test]
fn binary_comparison_type_mismatch() {
    let mut tc = TypeChecker::new();
    let err = err_msg(tc.check_expr(&binop(
        Expr::Int(1, s()),
        BinOp::Eq,
        Expr::Str("a".into(), s()),
    )));
    assert!(err.contains("Cannot compare"));
}

// ─── Logical ────────────────────────────────────────────────────────

#[test]
fn binary_and_bool_bool() {
    let mut tc = TypeChecker::new();
    assert_eq!(
        tc.check_expr(&binop(
            Expr::Bool(true, s()),
            BinOp::And,
            Expr::Bool(false, s())
        ))
        .unwrap(),
        Type::Bool
    );
}

#[test]
fn binary_or_bool_bool() {
    let mut tc = TypeChecker::new();
    assert_eq!(
        tc.check_expr(&binop(
            Expr::Bool(true, s()),
            BinOp::Or,
            Expr::Bool(false, s())
        ))
        .unwrap(),
        Type::Bool
    );
}

#[test]
fn binary_and_non_bool_error() {
    let mut tc = TypeChecker::new();
    let err = err_msg(tc.check_expr(&binop(Expr::Int(1, s()), BinOp::And, Expr::Bool(true, s()))));
    assert!(err.contains("Logical"));
}

// ─── Unary ──────────────────────────────────────────────────────────

#[test]
fn unary_neg_int() {
    let mut tc = TypeChecker::new();
    assert_eq!(
        tc.check_expr(&unop(UnaryOp::Neg, Expr::Int(5, s())))
            .unwrap(),
        Type::Int
    );
}

#[test]
fn unary_neg_float() {
    let mut tc = TypeChecker::new();
    assert_eq!(
        tc.check_expr(&unop(UnaryOp::Neg, Expr::Float(1.5, s())))
            .unwrap(),
        Type::Float
    );
}

#[test]
fn unary_neg_string_error() {
    let mut tc = TypeChecker::new();
    let err = err_msg(tc.check_expr(&unop(UnaryOp::Neg, Expr::Str("a".into(), s()))));
    assert!(err.contains("Cannot negate"));
}

#[test]
fn unary_not_bool() {
    let mut tc = TypeChecker::new();
    assert_eq!(
        tc.check_expr(&unop(UnaryOp::Not, Expr::Bool(true, s())))
            .unwrap(),
        Type::Bool
    );
}

#[test]
fn unary_not_int_error() {
    let mut tc = TypeChecker::new();
    let err = err_msg(tc.check_expr(&unop(UnaryOp::Not, Expr::Int(1, s()))));
    assert!(err.contains("Cannot apply 'not'"));
}

// ─── Nested ─────────────────────────────────────────────────────────

// ─── Phase 2: Control Flow ──────────────────────────────────────────

// ─── While ──────────────────────────────────────────────────────────

#[test]
fn while_bool_cond_ok() {
    let mut tc = TypeChecker::new();
    let stmt = Stmt::While {
        cond: Expr::Bool(true, s()),
        body: vec![Stmt::Expr(Expr::Int(1, s()), s())],
        span: s(),
    };
    assert!(tc.check_stmt(&stmt).is_ok());
}

#[test]
fn while_non_bool_cond_error() {
    let mut tc = TypeChecker::new();
    let stmt = Stmt::While {
        cond: Expr::Int(1, s()),
        body: vec![Stmt::Expr(Expr::Int(1, s()), s())],
        span: s(),
    };
    let err = err_msg(tc.check_stmt(&stmt));
    assert!(err.contains("While condition must be Bool"));
}

// ─── For ────────────────────────────────────────────────────────────

#[test]
fn for_typechecks_body() {
    let mut tc = TypeChecker::new();
    tc.env
        .set_var("items".into(), Type::List(Box::new(Type::Int)));
    let stmt = Stmt::For {
        var: "x".into(),
        iter: Expr::Ident("items".into(), s()),
        body: vec![Stmt::Expr(Expr::Int(1, s()), s())],
        span: s(),
    };
    assert!(tc.check_stmt(&stmt).is_ok());
}

// ─── Assignment ─────────────────────────────────────────────────────

#[test]
fn assignment_type_match_ok() {
    let mut tc = TypeChecker::new();
    tc.env.set_var("x".into(), Type::Int);
    let stmt = Stmt::Assignment {
        target: Expr::Ident("x".into(), s()),
        value: Expr::Int(5, s()),
        span: s(),
    };
    assert!(tc.check_stmt(&stmt).is_ok());
}

#[test]
fn assignment_type_mismatch_error() {
    let mut tc = TypeChecker::new();
    tc.env.set_var("x".into(), Type::Int);
    let stmt = Stmt::Assignment {
        target: Expr::Ident("x".into(), s()),
        value: Expr::Str("hello".into(), s()),
        span: s(),
    };
    let err = err_msg(tc.check_stmt(&stmt));
    assert!(err.contains("type mismatch") || err.contains("Type mismatch"));
}

#[test]
fn assignment_to_unknown_var_error() {
    let mut tc = TypeChecker::new();
    let stmt = Stmt::Assignment {
        target: Expr::Ident("x".into(), s()),
        value: Expr::Int(5, s()),
        span: s(),
    };
    let err = err_msg(tc.check_stmt(&stmt));
    assert!(err.contains("Unknown") || err.contains("undefined") || err.contains("undeclared"));
}

// ─── Break / Continue ───────────────────────────────────────────────

#[test]
fn break_outside_loop_error() {
    let mut tc = TypeChecker::new();
    let err = err_msg(tc.check_stmt(&Stmt::Break(s())));
    assert!(err.contains("outside of a loop"));
}

#[test]
fn continue_outside_loop_error() {
    let mut tc = TypeChecker::new();
    let err = err_msg(tc.check_stmt(&Stmt::Continue(s())));
    assert!(err.contains("outside of a loop"));
}

#[test]
fn break_inside_loop_ok() {
    let mut tc = TypeChecker::new();
    tc.loop_depth = 1;
    assert!(tc.check_stmt(&Stmt::Break(s())).is_ok());
}

#[test]
fn continue_inside_loop_ok() {
    let mut tc = TypeChecker::new();
    tc.loop_depth = 1;
    assert!(tc.check_stmt(&Stmt::Continue(s())).is_ok());
}

// ─── Elif ───────────────────────────────────────────────────────────

#[test]
fn if_elif_typechecks() {
    let mut tc = TypeChecker::new();
    let stmt = Stmt::If {
        cond: Expr::Bool(true, s()),
        then_body: vec![Stmt::Expr(Expr::Int(1, s()), s())],
        elif_branches: vec![(
            Expr::Bool(false, s()),
            vec![Stmt::Expr(Expr::Int(2, s()), s())],
        )],
        else_body: vec![Stmt::Expr(Expr::Int(3, s()), s())],
        span: s(),
    };
    assert!(tc.check_stmt(&stmt).is_ok());
}

#[test]
fn elif_non_bool_cond_error() {
    let mut tc = TypeChecker::new();
    let stmt = Stmt::If {
        cond: Expr::Bool(true, s()),
        then_body: vec![Stmt::Expr(Expr::Int(1, s()), s())],
        elif_branches: vec![(Expr::Int(1, s()), vec![Stmt::Expr(Expr::Int(2, s()), s())])],
        else_body: vec![],
        span: s(),
    };
    let err = err_msg(tc.check_stmt(&stmt));
    assert!(err.contains("Bool"));
}

// ─── Nested ─────────────────────────────────────────────────────────

// ─── Phase 3: Collections and Type Annotations ─────────────────────

// ─── Let with type annotation ───────────────────────────────────────

#[test]
fn let_with_matching_type_annotation() {
    let mut tc = TypeChecker::new();
    let stmt = Stmt::Let {
        name: "x".into(),
        type_ann: Some(Type::Int),
        value: Expr::Int(5, s()),
        is_public: false,
        span: s(),
    };
    assert!(tc.check_stmt(&stmt).is_ok());
    assert_eq!(tc.env.get_var("x").cloned(), Some(Type::Int));
}

#[test]
fn let_with_mismatched_type_annotation() {
    let mut tc = TypeChecker::new();
    let stmt = Stmt::Let {
        name: "x".into(),
        type_ann: Some(Type::Int),
        value: Expr::Str("hello".into(), s()),
        is_public: false,
        span: s(),
    };
    let err = err_msg(tc.check_stmt(&stmt));
    assert!(err.contains("annotation") || err.contains("mismatch"));
}

#[test]
fn let_without_type_annotation_still_works() {
    let mut tc = TypeChecker::new();
    let stmt = Stmt::Let {
        name: "x".into(),
        type_ann: None,
        value: Expr::Int(5, s()),
        is_public: false,
        span: s(),
    };
    assert!(tc.check_stmt(&stmt).is_ok());
}

// ─── List literals ──────────────────────────────────────────────────

#[test]
fn list_literal_ints() {
    let mut tc = TypeChecker::new();
    let expr = Expr::ListLiteral(
        vec![Expr::Int(1, s()), Expr::Int(2, s()), Expr::Int(3, s())],
        s(),
    );
    assert_eq!(
        tc.check_expr(&expr).unwrap(),
        Type::List(Box::new(Type::Int))
    );
}

#[test]
fn list_literal_strings() {
    let mut tc = TypeChecker::new();
    let expr = Expr::ListLiteral(
        vec![Expr::Str("a".into(), s()), Expr::Str("b".into(), s())],
        s(),
    );
    assert_eq!(
        tc.check_expr(&expr).unwrap(),
        Type::List(Box::new(Type::String))
    );
}

#[test]
fn list_literal_empty_is_nil_list() {
    let mut tc = TypeChecker::new();
    let expr = Expr::ListLiteral(vec![], s());
    // Empty list without annotation should be List[Nil] or similar
    let ty = tc.check_expr(&expr).unwrap();
    assert!(matches!(ty, Type::List(_)));
}

#[test]
fn list_literal_mixed_types_error() {
    let mut tc = TypeChecker::new();
    let expr = Expr::ListLiteral(vec![Expr::Int(1, s()), Expr::Str("two".into(), s())], s());
    let err = err_msg(tc.check_expr(&expr));
    assert!(err.contains("element") || err.contains("mismatch") || err.contains("consistent"));
}

// ─── Index ──────────────────────────────────────────────────────────

#[test]
fn index_list_of_ints() {
    let mut tc = TypeChecker::new();
    tc.env.set_var("xs".into(), Type::List(Box::new(Type::Int)));
    let expr = Expr::Index {
        object: Box::new(Expr::Ident("xs".into(), s())),
        index: Box::new(Expr::Int(0, s())),
        span: s(),
    };
    assert_eq!(tc.check_expr(&expr).unwrap(), Type::Int);
}

#[test]
fn index_list_of_strings() {
    let mut tc = TypeChecker::new();
    tc.env
        .set_var("xs".into(), Type::List(Box::new(Type::String)));
    let expr = Expr::Index {
        object: Box::new(Expr::Ident("xs".into(), s())),
        index: Box::new(Expr::Int(0, s())),
        span: s(),
    };
    assert_eq!(tc.check_expr(&expr).unwrap(), Type::String);
}

#[test]
fn index_non_int_error() {
    let mut tc = TypeChecker::new();
    tc.env.set_var("xs".into(), Type::List(Box::new(Type::Int)));
    let expr = Expr::Index {
        object: Box::new(Expr::Ident("xs".into(), s())),
        index: Box::new(Expr::Str("bad".into(), s())),
        span: s(),
    };
    let err = err_msg(tc.check_expr(&expr));
    assert!(err.contains("Int") || err.contains("index"));
}

#[test]
fn index_non_list_error() {
    let mut tc = TypeChecker::new();
    tc.env.set_var("x".into(), Type::Int);
    let expr = Expr::Index {
        object: Box::new(Expr::Ident("x".into(), s())),
        index: Box::new(Expr::Int(0, s())),
        span: s(),
    };
    let err = err_msg(tc.check_expr(&expr));
    assert!(err.contains("index") || err.contains("List"));
}

// ─── For-in over List[T] ───────────────────────────────────────────

#[test]
fn for_over_list_binds_element_type() {
    let mut tc = TypeChecker::new();
    tc.env.set_var("xs".into(), Type::List(Box::new(Type::Int)));
    let stmt = Stmt::For {
        var: "x".into(),
        iter: Expr::Ident("xs".into(), s()),
        body: vec![Stmt::Expr(
            binop(Expr::Ident("x".into(), s()), BinOp::Add, Expr::Int(1, s())),
            s(),
        )],
        span: s(),
    };
    assert!(tc.check_stmt(&stmt).is_ok());
}

#[test]
fn for_over_non_list_error() {
    let mut tc = TypeChecker::new();
    tc.env.set_var("x".into(), Type::Int);
    let stmt = Stmt::For {
        var: "i".into(),
        iter: Expr::Ident("x".into(), s()),
        body: vec![Stmt::Expr(Expr::Int(1, s()), s())],
        span: s(),
    };
    let err = err_msg(tc.check_stmt(&stmt));
    assert!(err.contains("iterate") || err.contains("List") || err.contains("Iterable"));
}

// ─── Let with List type annotation ──────────────────────────────────

#[test]
fn let_list_type_annotation_match() {
    let mut tc = TypeChecker::new();
    let stmt = Stmt::Let {
        name: "xs".into(),
        type_ann: Some(Type::List(Box::new(Type::Int))),
        value: Expr::ListLiteral(vec![Expr::Int(1, s()), Expr::Int(2, s())], s()),
        is_public: false,
        span: s(),
    };
    assert!(tc.check_stmt(&stmt).is_ok());
}

#[test]
fn let_list_type_annotation_mismatch() {
    let mut tc = TypeChecker::new();
    let stmt = Stmt::Let {
        name: "xs".into(),
        type_ann: Some(Type::List(Box::new(Type::String))),
        value: Expr::ListLiteral(vec![Expr::Int(1, s())], s()),
        is_public: false,
        span: s(),
    };
    let err = err_msg(tc.check_stmt(&stmt));
    assert!(err.contains("annotation") || err.contains("mismatch"));
}

#[test]
fn let_empty_list_with_annotation_gets_annotated_type() {
    let mut tc = TypeChecker::new();
    let stmt = Stmt::Let {
        name: "xs".into(),
        type_ann: Some(Type::List(Box::new(Type::Int))),
        value: Expr::ListLiteral(vec![], s()),
        is_public: false,
        span: s(),
    };
    assert!(tc.check_stmt(&stmt).is_ok());
    assert_eq!(
        tc.env.get_var("xs").cloned(),
        Some(Type::List(Box::new(Type::Int)))
    );
}

// ─── Phase 4: Builtins ──────────────────────────────────────────────

#[test]
fn builtin_log_accepts_string() {
    let mut tc = TypeChecker::new();
    let call = Expr::Call {
        func: Box::new(Expr::Ident("log".into(), s())),
        args: vec![("message".into(), s(), Expr::Str("hello".into(), s()))],
        span: s(),
    };
    assert_eq!(tc.check_expr(&call).unwrap(), Type::Void);
}

#[test]
fn builtin_say_accepts_string() {
    let mut tc = TypeChecker::new();
    let call = Expr::Call {
        func: Box::new(Expr::Ident("say".into(), s())),
        args: vec![("message".into(), s(), Expr::Str("hello".into(), s()))],
        span: s(),
    };
    assert_eq!(tc.check_expr(&call).unwrap(), Type::Void);
}

#[test]
fn builtin_len_list_returns_int() {
    let mut tc = TypeChecker::new();
    tc.env.set_var("xs".into(), Type::List(Box::new(Type::Int)));
    let call = Expr::Call {
        func: Box::new(Expr::Ident("len".into(), s())),
        args: vec![("value".into(), s(), Expr::Ident("xs".into(), s()))],
        span: s(),
    };
    assert_eq!(tc.check_expr(&call).unwrap(), Type::Int);
}

#[test]
fn builtin_len_string_returns_int() {
    let mut tc = TypeChecker::new();
    let call = Expr::Call {
        func: Box::new(Expr::Ident("len".into(), s())),
        args: vec![("value".into(), s(), Expr::Str("hello".into(), s()))],
        span: s(),
    };
    assert_eq!(tc.check_expr(&call).unwrap(), Type::Int);
}

#[test]
fn builtin_to_string_int_returns_string() {
    let mut tc = TypeChecker::new();
    let call = Expr::Call {
        func: Box::new(Expr::Ident("to_string".into(), s())),
        args: vec![("value".into(), s(), Expr::Int(42, s()))],
        span: s(),
    };
    assert_eq!(tc.check_expr(&call).unwrap(), Type::String);
}

#[test]
fn builtin_to_string_float_returns_string() {
    let mut tc = TypeChecker::new();
    let call = Expr::Call {
        func: Box::new(Expr::Ident("to_string".into(), s())),
        args: vec![("value".into(), s(), Expr::Float(3.125, s()))],
        span: s(),
    };
    assert_eq!(tc.check_expr(&call).unwrap(), Type::String);
}

#[test]
fn builtin_to_string_bool_returns_string() {
    let mut tc = TypeChecker::new();
    let call = Expr::Call {
        func: Box::new(Expr::Ident("to_string".into(), s())),
        args: vec![("value".into(), s(), Expr::Bool(true, s()))],
        span: s(),
    };
    assert_eq!(tc.check_expr(&call).unwrap(), Type::String);
}

// ─── existing ───────────────────────────────────────────────────────

#[test]
fn nested_binary_type_propagation() {
    let mut tc = TypeChecker::new();
    let expr = binop(
        binop(Expr::Int(1, s()), BinOp::Add, Expr::Int(2, s())),
        BinOp::Mul,
        Expr::Int(3, s()),
    );
    assert_eq!(tc.check_expr(&expr).unwrap(), Type::Int);
}

// ─── Phase 5: Generics and Traits ───────────────────────────────────

// ─── Trait definitions ──────────────────────────────────────────────

#[test]
fn trait_definition_registers_trait() {
    let mut tc = TypeChecker::new();
    let trait_stmt = Stmt::Trait {
        name: "Printable".into(),
        methods: vec![Stmt::Let {
            name: "Printable.to_string".into(),
            type_ann: None,
            value: Expr::Lambda {
                params: vec![],
                ret_type: Type::String,
                body: vec![Stmt::Expr(Expr::Str("".into(), s()), s())],
                generic_params: None,
                throws: None,
                type_constraints: vec![],
                defaults: Box::new(vec![]),
                span: s(),
            },
            is_public: false,
            span: s(),
        }],
        is_public: false,
        generic_params: None,
        span: s(),
    };
    assert!(tc.check_stmt(&trait_stmt).is_ok());
    assert!(tc.env.get_trait("Printable").is_some());
}

// ─── Class includes trait ───────────────────────────────────────────

#[test]
fn class_includes_trait_gets_methods() {
    let mut tc = TypeChecker::new();
    // Define trait
    let trait_stmt = Stmt::Trait {
        name: "Printable".into(),
        methods: vec![Stmt::Let {
            name: "Printable.to_string".into(),
            type_ann: None,
            value: Expr::Lambda {
                params: vec![],
                ret_type: Type::String,
                body: vec![Stmt::Expr(Expr::Str("".into(), s()), s())],
                generic_params: None,
                throws: None,
                type_constraints: vec![],
                defaults: Box::new(vec![]),
                span: s(),
            },
            is_public: false,
            span: s(),
        }],
        is_public: false,
        generic_params: None,
        span: s(),
    };
    tc.check_stmt(&trait_stmt).unwrap();

    // Define class that includes it
    let class_stmt = Stmt::Class {
        name: "User".into(),
        fields: vec![("name".into(), Type::String, false)],
        methods: vec![Stmt::Let {
            name: "User.to_string".into(),
            type_ann: None,
            value: Expr::Lambda {
                params: vec![],
                ret_type: Type::String,
                body: vec![Stmt::Expr(Expr::Str("user".into(), s()), s())],
                generic_params: None,
                throws: None,
                type_constraints: vec![],
                defaults: Box::new(vec![]),
                span: s(),
            },
            is_public: false,
            span: s(),
        }],
        is_public: false,
        generic_params: None,
        extends: None,
        includes: Some(vec![("Printable".to_string(), vec![])]),
        span: s(),
    };
    assert!(tc.check_stmt(&class_stmt).is_ok());
}

#[test]
fn class_includes_unknown_trait_error() {
    let mut tc = TypeChecker::new();
    let class_stmt = Stmt::Class {
        name: "User".into(),
        fields: vec![],
        methods: vec![],
        is_public: false,
        generic_params: None,
        extends: None,
        includes: Some(vec![("NonExistent".to_string(), vec![])]),
        span: s(),
    };
    let err = err_msg(tc.check_stmt(&class_stmt));
    assert!(err.contains("Unknown trait") || err.contains("NonExistent"));
}

#[test]
fn class_missing_trait_method_error() {
    let mut tc = TypeChecker::new();
    // Define trait with required (abstract) method - empty body
    // Use "Serializable" instead of "Printable" since Printable is now built-in with auto-derive
    let trait_stmt = Stmt::Trait {
        name: "Serializable".into(),
        methods: vec![Stmt::Let {
            name: "Serializable.serialize".into(),
            type_ann: None,
            value: Expr::Lambda {
                params: vec![],
                ret_type: Type::String,
                body: vec![],
                generic_params: None,
                throws: None,
                type_constraints: vec![],
                defaults: Box::new(vec![]),
                span: s(),
            },
            is_public: false,
            span: s(),
        }],
        is_public: false,
        generic_params: None,
        span: s(),
    };
    tc.check_stmt(&trait_stmt).unwrap();

    // Class without the required method
    let class_stmt = Stmt::Class {
        name: "User".into(),
        fields: vec![("name".into(), Type::String, false)],
        methods: vec![],
        is_public: false,
        generic_params: None,
        extends: None,
        includes: Some(vec![("Serializable".to_string(), vec![])]),
        span: s(),
    };
    let err = err_msg(tc.check_stmt(&class_stmt));
    assert!(err.contains("serialize") || err.contains("missing") || err.contains("implement"));
}

// ─── Generic class type checking ────────────────────────────────────

#[test]
fn generic_class_registers_with_params() {
    let mut tc = TypeChecker::new();
    let class_stmt = Stmt::Class {
        name: "Box".into(),
        fields: vec![("value".into(), Type::TypeVar("T".into(), vec![]), false)],
        methods: vec![],
        is_public: false,
        generic_params: Some(vec!["T".into()]),
        extends: None,
        includes: None,
        span: s(),
    };
    assert!(tc.check_stmt(&class_stmt).is_ok());
    let info = tc.env.get_class("Box").unwrap();
    assert!(info.generic_params.is_some());
    assert_eq!(info.generic_params.as_ref().unwrap(), &["T".to_string()]);
}

// ─── Generic function type checking (BC-5: inline generics) ─────────

#[test]
fn generic_lambda_typechecks_inline() {
    // New style: generic_params=None, Custom types auto-detected as type params
    let mut tc = TypeChecker::new();
    let lambda = Expr::Lambda {
        params: vec![("x".into(), Type::Custom("T".into(), vec![]))],
        ret_type: Type::Custom("T".into(), vec![]),
        body: vec![Stmt::Expr(Expr::Ident("x".into(), s()), s())],
        generic_params: None,
        throws: None,
        type_constraints: vec![],
        defaults: Box::new(vec![]),
        span: s(),
    };
    let ty = tc.check_expr(&lambda).unwrap();
    match ty {
        Type::Function { params, ret, .. } => {
            assert_eq!(params, vec![Type::TypeVar("T".into(), vec![])]);
            assert_eq!(*ret, Type::TypeVar("T".into(), vec![]));
        }
        _ => panic!("expected function type"),
    }
}

#[test]
fn generic_lambda_typechecks_explicit() {
    // Explicit generic_params with TypeVar still works
    let mut tc = TypeChecker::new();
    let lambda = Expr::Lambda {
        params: vec![("x".into(), Type::TypeVar("T".into(), vec![]))],
        ret_type: Type::TypeVar("T".into(), vec![]),
        body: vec![Stmt::Expr(Expr::Ident("x".into(), s()), s())],
        generic_params: Some(vec!["T".into()]),
        throws: None,
        type_constraints: vec![],
        defaults: Box::new(vec![]),
        span: s(),
    };
    let ty = tc.check_expr(&lambda).unwrap();
    match ty {
        Type::Function { params, ret, .. } => {
            assert_eq!(params, vec![Type::TypeVar("T".into(), vec![])]);
            assert_eq!(*ret, Type::TypeVar("T".into(), vec![]));
        }
        _ => panic!("expected function type"),
    }
}

// ─── Fix #1: TypeVar unification at call sites ──────────────────────

#[test]
fn generic_call_unifies_typevar_to_int() {
    let mut tc = TypeChecker::new();
    // def identity(x: T) -> T = x (inline generic)
    let lambda = Expr::Lambda {
        params: vec![("x".into(), Type::Custom("T".into(), vec![]))],
        ret_type: Type::Custom("T".into(), vec![]),
        body: vec![Stmt::Expr(Expr::Ident("x".into(), s()), s())],
        generic_params: None,
        throws: None,
        type_constraints: vec![],
        defaults: Box::new(vec![]),
        span: s(),
    };
    tc.check_stmt(&Stmt::Let {
        name: "identity".into(),
        type_ann: None,
        value: lambda,
        is_public: false,
        span: s(),
    })
    .unwrap();

    // identity(x: 42) should return Int
    let call = Expr::Call {
        func: Box::new(Expr::Ident("identity".into(), s())),
        args: vec![("x".into(), s(), Expr::Int(42, s()))],
        span: s(),
    };
    assert_eq!(tc.check_expr(&call).unwrap(), Type::Int);
}

#[test]
fn generic_call_unifies_typevar_to_string() {
    let mut tc = TypeChecker::new();
    let lambda = Expr::Lambda {
        params: vec![("x".into(), Type::Custom("T".into(), vec![]))],
        ret_type: Type::Custom("T".into(), vec![]),
        body: vec![Stmt::Expr(Expr::Ident("x".into(), s()), s())],
        generic_params: None,
        throws: None,
        type_constraints: vec![],
        defaults: Box::new(vec![]),
        span: s(),
    };
    tc.check_stmt(&Stmt::Let {
        name: "identity".into(),
        type_ann: None,
        value: lambda,
        is_public: false,
        span: s(),
    })
    .unwrap();

    // identity(x: "hello") should return String
    let call = Expr::Call {
        func: Box::new(Expr::Ident("identity".into(), s())),
        args: vec![("x".into(), s(), Expr::Str("hello".into(), s()))],
        span: s(),
    };
    assert_eq!(tc.check_expr(&call).unwrap(), Type::String);
}

#[test]
fn generic_call_multi_params_unifies() {
    let mut tc = TypeChecker::new();
    // def first(a: A, b: B) -> A = a
    let lambda = Expr::Lambda {
        params: vec![
            ("a".into(), Type::Custom("A".into(), vec![])),
            ("b".into(), Type::Custom("B".into(), vec![])),
        ],
        ret_type: Type::Custom("A".into(), vec![]),
        body: vec![Stmt::Expr(Expr::Ident("a".into(), s()), s())],
        generic_params: None,
        throws: None,
        type_constraints: vec![],
        defaults: Box::new(vec![]),
        span: s(),
    };
    tc.check_stmt(&Stmt::Let {
        name: "first".into(),
        type_ann: None,
        value: lambda,
        is_public: false,
        span: s(),
    })
    .unwrap();

    // first(a: 42, b: "hello") should return Int
    let call = Expr::Call {
        func: Box::new(Expr::Ident("first".into(), s())),
        args: vec![
            ("a".into(), s(), Expr::Int(42, s())),
            ("b".into(), s(), Expr::Str("hello".into(), s())),
        ],
        span: s(),
    };
    assert_eq!(tc.check_expr(&call).unwrap(), Type::Int);
}

// ─── Fix #4: Trait method signature comparison ──────────────────────

#[test]
fn class_includes_trait_wrong_signature_error() {
    let mut tc = TypeChecker::new();
    // Trait requires: def display() -> String
    let trait_stmt = Stmt::Trait {
        name: "Displayable".into(),
        methods: vec![Stmt::Let {
            name: "Displayable.display".into(),
            type_ann: None,
            value: Expr::Lambda {
                params: vec![],
                ret_type: Type::String,
                body: vec![],
                generic_params: None,
                throws: None,
                type_constraints: vec![],
                defaults: Box::new(vec![]),
                span: s(),
            },
            is_public: false,
            span: s(),
        }],
        is_public: false,
        generic_params: None,
        span: s(),
    };
    tc.check_stmt(&trait_stmt).unwrap();

    // Class implements display() -> Int (wrong return type)
    let class_stmt = Stmt::Class {
        name: "Item".into(),
        fields: vec![],
        methods: vec![Stmt::Let {
            name: "Item.display".into(),
            type_ann: None,
            value: Expr::Lambda {
                params: vec![],
                ret_type: Type::Int,
                body: vec![Stmt::Expr(Expr::Int(0, s()), s())],
                generic_params: None,
                throws: None,
                type_constraints: vec![],
                defaults: Box::new(vec![]),
                span: s(),
            },
            is_public: false,
            span: s(),
        }],
        is_public: false,
        generic_params: None,
        extends: None,
        includes: Some(vec![("Displayable".to_string(), vec![])]),
        span: s(),
    };
    let err = err_msg(tc.check_stmt(&class_stmt));
    assert!(err.contains("signature") || err.contains("mismatch") || err.contains("display"));
}

// ─── Fix #5: Method lookup qualified/unqualified ────────────────────

#[test]
fn member_access_finds_method_unqualified() {
    let mut tc = TypeChecker::new();
    let class_stmt = Stmt::Class {
        name: "Point".into(),
        fields: vec![("x".into(), Type::Int, false)],
        methods: vec![Stmt::Let {
            name: "Point.show".into(),
            type_ann: None,
            value: Expr::Lambda {
                params: vec![],
                ret_type: Type::String,
                body: vec![Stmt::Expr(Expr::Str("ok".into(), s()), s())],
                generic_params: None,
                throws: None,
                type_constraints: vec![],
                defaults: Box::new(vec![]),
                span: s(),
            },
            is_public: false,
            span: s(),
        }],
        is_public: false,
        generic_params: None,
        extends: None,
        includes: None,
        span: s(),
    };
    tc.check_stmt(&class_stmt).unwrap();

    tc.env
        .set_var("p".into(), Type::Custom("Point".into(), Vec::new()));
    // Access via p.show (unqualified) should work
    let access = Expr::Member {
        object: Box::new(Expr::Ident("p".into(), s())),
        field: "show".into(),
        span: s(),
    };
    assert!(tc.check_expr(&access).is_ok());
}

// ─── Fix #7: Return statement validation ────────────────────────────

#[test]
fn return_type_mismatch_in_function_error() {
    let mut tc = TypeChecker::new();
    // def f() -> Int
    //   return "hello"  # should error — return value doesn't match declared type
    let lambda = Expr::Lambda {
        params: vec![],
        ret_type: Type::Int,
        body: vec![Stmt::Return(Expr::Str("hello".into(), s()), s())],
        generic_params: None,
        throws: None,
        type_constraints: vec![],
        defaults: Box::new(vec![]),
        span: s(),
    };
    let err = err_msg(tc.check_expr(&lambda));
    assert!(err.contains("return") || err.contains("mismatch") || err.contains("Return"));
}

#[test]
fn return_mid_body_type_mismatch_error() {
    let mut tc = TypeChecker::new();
    // def f() -> Int
    //   return "hello"   # mid-body return of wrong type
    //   42               # last expr is correct type
    let lambda = Expr::Lambda {
        params: vec![],
        ret_type: Type::Int,
        body: vec![
            Stmt::Return(Expr::Str("hello".into(), s()), s()),
            Stmt::Expr(Expr::Int(42, s()), s()),
        ],
        generic_params: None,
        throws: None,
        type_constraints: vec![],
        defaults: Box::new(vec![]),
        span: s(),
    };
    let err = err_msg(tc.check_expr(&lambda));
    assert!(err.contains("return") || err.contains("mismatch") || err.contains("Return"));
}

// ─── Fix #6: Child scopes for if/while/for ──────────────────────────

#[test]
fn if_body_variables_dont_leak() {
    let mut tc = TypeChecker::new();
    let stmt = Stmt::If {
        cond: Expr::Bool(true, s()),
        then_body: vec![Stmt::Let {
            name: "inner".into(),
            type_ann: None,
            value: Expr::Int(1, s()),
            is_public: false,
            span: s(),
        }],
        elif_branches: vec![],
        else_body: vec![],
        span: s(),
    };
    tc.check_stmt(&stmt).unwrap();
    // "inner" should NOT be visible in outer scope
    assert_eq!(tc.env.get_var("inner"), None);
}

#[test]
fn while_body_variables_dont_leak() {
    let mut tc = TypeChecker::new();
    let stmt = Stmt::While {
        cond: Expr::Bool(true, s()),
        body: vec![
            Stmt::Let {
                name: "inner".into(),
                type_ann: None,
                value: Expr::Int(1, s()),
                is_public: false,
                span: s(),
            },
            Stmt::Break(s()),
        ],
        span: s(),
    };
    tc.check_stmt(&stmt).unwrap();
    assert_eq!(tc.env.get_var("inner"), None);
}

#[test]
fn for_body_variables_dont_leak() {
    let mut tc = TypeChecker::new();
    tc.env.set_var("xs".into(), Type::List(Box::new(Type::Int)));
    let stmt = Stmt::For {
        var: "x".into(),
        iter: Expr::Ident("xs".into(), s()),
        body: vec![Stmt::Let {
            name: "inner".into(),
            type_ann: None,
            value: Expr::Int(1, s()),
            is_public: false,
            span: s(),
        }],
        span: s(),
    };
    tc.check_stmt(&stmt).unwrap();
    // Both "inner" and loop var "x" should NOT leak
    assert_eq!(tc.env.get_var("inner"), None);
    assert_eq!(tc.env.get_var("x"), None);
}

#[test]
fn return_correct_type_ok() {
    let mut tc = TypeChecker::new();
    // def f() -> Int
    //   return 42  # correct
    let lambda = Expr::Lambda {
        params: vec![],
        ret_type: Type::Int,
        body: vec![Stmt::Return(Expr::Int(42, s()), s())],
        generic_params: None,
        throws: None,
        type_constraints: vec![],
        defaults: Box::new(vec![]),
        span: s(),
    };
    assert!(tc.check_expr(&lambda).is_ok());
}

// ─── Fix #2: Generic body return type validation ────────────────────

#[test]
fn generic_lambda_body_type_matches_typevar() {
    let mut tc = TypeChecker::new();
    // def identity[T](x: T) -> T
    //   x    # body returns the TypeVar param — should be OK
    let lambda = Expr::Lambda {
        params: vec![("x".into(), Type::TypeVar("T".into(), vec![]))],
        ret_type: Type::TypeVar("T".into(), vec![]),
        body: vec![Stmt::Expr(Expr::Ident("x".into(), s()), s())],
        generic_params: Some(vec!["T".into()]),
        throws: None,
        type_constraints: vec![],
        defaults: Box::new(vec![]),
        span: s(),
    };
    assert!(tc.check_expr(&lambda).is_ok());
}

#[test]
fn generic_lambda_body_wrong_typevar_error() {
    let mut tc = TypeChecker::new();
    // def bad[T](x: T) -> T
    //   42    # body returns Int, not TypeVar("T") — should error
    let lambda = Expr::Lambda {
        params: vec![("x".into(), Type::TypeVar("T".into(), vec![]))],
        ret_type: Type::TypeVar("T".into(), vec![]),
        body: vec![Stmt::Expr(Expr::Int(42, s()), s())],
        generic_params: Some(vec!["T".into()]),
        throws: None,
        type_constraints: vec![],
        defaults: Box::new(vec![]),
        span: s(),
    };
    let err = err_msg(tc.check_expr(&lambda));
    assert!(err.contains("mismatch") || err.contains("TypeVar"));
}

// ─── Subtype checking via extends chain ──────────────────────────────

#[test]
fn subtype_assignment_through_extends_chain() {
    let src = "\
class Animal
  def sound() -> Int
    0

class Dog extends Animal
  def sound() -> Int
    1

def greet(a: Animal) -> Int
  a.sound()

def main() -> Int
  let d: Dog = Dog()
  greet(a: d)
";
    let module = module_ok(src);
    let mut tc = TypeChecker::new();
    tc.check_module(&module).expect("typecheck ok");
}

#[test]
fn subtype_transitive_extends_chain() {
    let src = "\
class Base
  def id() -> Int
    0

class Mid extends Base
  def id() -> Int
    1

class Leaf extends Mid
  def id() -> Int
    2

def take_base(b: Base) -> Int
  b.id()

def main() -> Int
  let leaf: Leaf = Leaf()
  take_base(b: leaf)
";
    let module = module_ok(src);
    let mut tc = TypeChecker::new();
    tc.check_module(&module).expect("typecheck ok");
}

#[test]
fn subtype_unrelated_class_rejected() {
    let src = "\
class Cat
  def sound() -> Int
    0

class Dog
  def sound() -> Int
    1

def greet(c: Cat) -> Int
  c.sound()

def main() -> Int
  let d: Dog = Dog()
  greet(c: d)
";
    let err = module_err(src);
    assert!(
        err.contains("expects Cat, got Dog"),
        "Expected type error, got: {err}"
    );
}
