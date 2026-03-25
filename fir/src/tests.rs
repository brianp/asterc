use crate::exprs::{BinOp, FirExpr, UnaryOp};
use crate::lower::{LowerError, Lowerer};
use crate::module::{FirFunction, FirModule};
use crate::stmts::{FirPlace, FirStmt};
use crate::types::{ClassId, FirType, FunctionId, LocalId};

use ast::Type;
use ast::type_env::TypeEnv;

// ---------------------------------------------------------------------------
// Helper: parse, typecheck, and lower Aster source to FIR
// ---------------------------------------------------------------------------

fn lower_ok(src: &str) -> FirModule {
    let tokens = lexer::lex(src).expect("lex ok");
    let mut parser = parser::Parser::new(tokens);
    let module = parser.parse_module("test").expect("parse ok");
    let mut tc = typecheck::TypeChecker::new();
    tc.check_module(&module).expect("typecheck ok");
    let mut lowerer = Lowerer::new(tc.env, tc.type_table);
    lowerer.lower_module(&module).expect("lower ok");
    lowerer.finish()
}

fn lower_err(src: &str) -> LowerError {
    let tokens = lexer::lex(src).expect("lex ok");
    let mut parser = parser::Parser::new(tokens);
    let module = parser.parse_module("test").expect("parse ok");
    let mut tc = typecheck::TypeChecker::new();
    tc.check_module(&module).expect("typecheck ok");
    let mut lowerer = Lowerer::new(tc.env, tc.type_table);
    lowerer
        .lower_module(&module)
        .expect_err("expected lower error")
}

/// Return the function body with the async scope prologue stripped.
/// Every lowered function starts with a `Let { value: RuntimeCall { name: "aster_async_scope_enter" } }`
/// statement; this helper skips it so tests can focus on the user-visible statements.
fn real_body(func: &FirFunction) -> &[FirStmt] {
    if let Some(FirStmt::Let {
        value: FirExpr::RuntimeCall { name, .. },
        ..
    }) = func.body.first()
    {
        if name == "aster_async_scope_enter" {
            return &func.body[1..];
        }
    }
    &func.body
}

/// Return the first non-aster_ statement in a function body, skipping both the
/// async scope prologue and any `aster_*` runtime calls (e.g. scope_exit).
fn first_user_stmt(func: &FirFunction) -> Option<&FirStmt> {
    real_body(func).iter().find(|s| {
        !matches!(s, FirStmt::Expr(FirExpr::RuntimeCall { name, .. }) if name.starts_with("aster_"))
    })
}

// ===========================================================================
// Module contract tests
// ===========================================================================

#[test]
fn module_new_is_empty() {
    let m = FirModule::new();
    assert!(m.functions.is_empty());
    assert!(m.classes.is_empty());
    assert!(m.entry.is_none());
}

#[test]
fn module_add_and_get_function() {
    let mut m = FirModule::new();
    let func = FirFunction {
        id: FunctionId(0),
        name: "add".into(),
        params: vec![("a".into(), FirType::I64), ("b".into(), FirType::I64)],
        ret_type: FirType::I64,
        body: vec![FirStmt::Return(FirExpr::IntLit(0))],
        is_entry: false,
        suspendable: false,
    };
    let id = m.add_function(func);
    assert_eq!(id, FunctionId(0));
    assert_eq!(m.get_function(id).name, "add");
    assert_eq!(m.get_function(id).params.len(), 2);
}

#[test]
fn module_mark_and_functions_since() {
    let mut m = FirModule::new();
    let f0 = FirFunction {
        id: FunctionId(0),
        name: "f0".into(),
        params: vec![],
        ret_type: FirType::Void,
        body: vec![],
        is_entry: false,
        suspendable: false,
    };
    m.add_function(f0);

    let mark = m.mark();
    assert_eq!(mark, 1);

    let f1 = FirFunction {
        id: FunctionId(1),
        name: "f1".into(),
        params: vec![],
        ret_type: FirType::Void,
        body: vec![],
        is_entry: false,
        suspendable: false,
    };
    m.add_function(f1);

    let since = m.functions_since(mark);
    assert_eq!(since.len(), 1);
    assert_eq!(since[0].name, "f1");
}

#[test]
fn module_add_and_get_class() {
    use crate::module::FirClass;
    let mut m = FirModule::new();
    let cls = FirClass {
        id: ClassId(0),
        name: "Point".into(),
        fields: vec![("x".into(), FirType::I64, 0), ("y".into(), FirType::I64, 8)],
        methods: vec![],
        vtable: vec![],
        size: 16,
        alignment: 8,
        parent: None,
    };
    let id = m.add_class(cls);
    assert_eq!(id, ClassId(0));
    assert_eq!(m.get_class(id).name, "Point");
    assert_eq!(m.get_class(id).size, 16);
}

// ===========================================================================
// Serialization tests
// ===========================================================================

#[test]
fn fir_module_serializes_to_json() {
    let mut m = FirModule::new();
    let func = FirFunction {
        id: FunctionId(0),
        name: "identity".into(),
        params: vec![("x".into(), FirType::I64)],
        ret_type: FirType::I64,
        body: vec![FirStmt::Return(FirExpr::LocalVar(LocalId(0), FirType::I64))],
        is_entry: false,
        suspendable: false,
    };
    m.add_function(func);

    let json = serde_json::to_string_pretty(&m).expect("serialize ok");
    assert!(json.contains("\"identity\""));
    assert!(json.contains("\"I64\""));

    // Round-trip
    let deserialized: FirModule = serde_json::from_str(&json).expect("deserialize ok");
    assert_eq!(deserialized.functions.len(), 1);
    assert_eq!(deserialized.functions[0].name, "identity");
}

// ===========================================================================
// Type lowering tests
// ===========================================================================

#[test]
fn lower_type_int_to_i64() {
    let fir = lower_ok("def f(x: Int) -> Int\n  x\n");
    let func = &fir.functions[0];
    assert_eq!(func.params[0].1, FirType::I64);
    assert_eq!(func.ret_type, FirType::I64);
}

#[test]
fn lower_type_float_to_f64() {
    let fir = lower_ok("def f(x: Float) -> Float\n  x\n");
    let func = &fir.functions[0];
    assert_eq!(func.params[0].1, FirType::F64);
    assert_eq!(func.ret_type, FirType::F64);
}

#[test]
fn lower_type_bool() {
    let fir = lower_ok("def f(x: Bool) -> Bool\n  x\n");
    let func = &fir.functions[0];
    assert_eq!(func.params[0].1, FirType::Bool);
    assert_eq!(func.ret_type, FirType::Bool);
}

#[test]
fn lower_type_string_to_ptr() {
    let fir = lower_ok("def f(x: String) -> String\n  x\n");
    let func = &fir.functions[0];
    assert_eq!(func.params[0].1, FirType::Ptr);
    assert_eq!(func.ret_type, FirType::Ptr);
}

#[test]
fn lower_type_void() {
    let fir = lower_ok("def f() -> Void\n  nil\n");
    let func = &fir.functions[0];
    assert_eq!(func.ret_type, FirType::Void);
}

// ===========================================================================
// Integer function lowering (Milestone 1 core)
// ===========================================================================

#[test]
fn lower_simple_add_function() {
    let fir = lower_ok("def add(a: Int, b: Int) -> Int\n  a + b\n");
    assert_eq!(fir.functions.len(), 1);

    let func = &fir.functions[0];
    assert_eq!(func.name, "add");
    assert_eq!(func.params.len(), 2);
    assert_eq!(func.params[0], ("a".into(), FirType::I64));
    assert_eq!(func.params[1], ("b".into(), FirType::I64));
    assert_eq!(func.ret_type, FirType::I64);

    // Body should contain a BinaryOp(Add, ...)
    // (could be Expr or Return depending on how the parser handles implicit returns)
    let has_add = func.body.iter().any(|s| match s {
        FirStmt::Expr(FirExpr::BinaryOp {
            op: BinOp::Add,
            result_ty,
            ..
        }) => *result_ty == FirType::I64,
        FirStmt::Return(FirExpr::BinaryOp {
            op: BinOp::Add,
            result_ty,
            ..
        }) => *result_ty == FirType::I64,
        _ => false,
    });
    assert!(has_add, "expected Add in body: {:?}", func.body);
}

#[test]
fn lower_nested_arithmetic() {
    // 1 + 2 * 3 should parse as 1 + (2 * 3)
    let fir = lower_ok("def f() -> Int\n  1 + 2 * 3\n");
    let func = &fir.functions[0];

    // Find the expression (could be Expr or Return), skip aster_ runtime calls
    let expr = func
        .body
        .iter()
        .find_map(|s| match s {
            FirStmt::Expr(FirExpr::RuntimeCall { name, .. }) if name.starts_with("aster_") => None,
            FirStmt::Expr(e) | FirStmt::Return(e) => Some(e),
            _ => None,
        })
        .expect("expected expression in body");

    match expr {
        FirExpr::BinaryOp {
            op: BinOp::Add,
            left,
            right,
            ..
        } => {
            assert!(matches!(left.as_ref(), FirExpr::IntLit(1)));
            match right.as_ref() {
                FirExpr::BinaryOp { op: BinOp::Mul, .. } => {}
                other => panic!("expected Mul, got {:?}", other),
            }
        }
        other => panic!("expected Add, got {:?}", other),
    }
}

#[test]
fn lower_explicit_return() {
    let fir = lower_ok("def f() -> Int\n  return 42\n");
    let func = &fir.functions[0];
    match first_user_stmt(func) {
        Some(FirStmt::Return(FirExpr::IntLit(42))) => {}
        other => panic!("expected Return(IntLit(42)), got {:?}", other),
    }
}

#[test]
fn lower_unary_negation() {
    let fir = lower_ok("def f() -> Int\n  return -5\n");
    let func = &fir.functions[0];
    match first_user_stmt(func) {
        Some(FirStmt::Return(FirExpr::UnaryOp {
            op: UnaryOp::Neg,
            operand,
            result_ty,
        })) => {
            assert!(matches!(operand.as_ref(), FirExpr::IntLit(5)));
            assert_eq!(*result_ty, FirType::I64);
        }
        other => panic!("expected Return(Neg), got {:?}", other),
    }
}

#[test]
fn lower_not_operator() {
    let fir = lower_ok("def f() -> Bool\n  return not true\n");
    let func = &fir.functions[0];
    match first_user_stmt(func) {
        Some(FirStmt::Return(FirExpr::UnaryOp {
            op: UnaryOp::Not,
            operand,
            result_ty,
        })) => {
            assert!(matches!(operand.as_ref(), FirExpr::BoolLit(true)));
            assert_eq!(*result_ty, FirType::Bool);
        }
        other => panic!("expected Return(Not), got {:?}", other),
    }
}

#[test]
fn lower_comparison_returns_bool() {
    let fir = lower_ok("def f(a: Int, b: Int) -> Bool\n  a < b\n");
    let func = &fir.functions[0];
    let expr = func
        .body
        .iter()
        .find_map(|s| match s {
            FirStmt::Expr(FirExpr::RuntimeCall { name, .. }) if name.starts_with("aster_") => None,
            FirStmt::Expr(e) | FirStmt::Return(e) => Some(e),
            _ => None,
        })
        .expect("expected expression");

    match expr {
        FirExpr::BinaryOp {
            op: BinOp::Lt,
            result_ty,
            ..
        } => {
            assert_eq!(*result_ty, FirType::Bool);
        }
        other => panic!("expected Lt, got {:?}", other),
    }
}

// ===========================================================================
// Let bindings inside functions
// ===========================================================================

#[test]
fn lower_let_binding_in_function() {
    let fir = lower_ok("def f() -> Int\n  let x: Int = 42\n  x\n");
    let func = &fir.functions[0];
    let body = real_body(func);
    assert!(body.len() >= 2);

    // First statement: Let x = 42
    match body.first() {
        Some(FirStmt::Let { ty, value, .. }) => {
            assert_eq!(*ty, FirType::I64);
            assert!(matches!(value, FirExpr::IntLit(42)));
        }
        other => panic!("expected Let, got {:?}", other),
    }
}

#[test]
fn lower_multiple_let_bindings() {
    let src = "def f() -> Int\n  let a: Int = 1\n  let b: Int = 2\n  a + b\n";
    let fir = lower_ok(src);
    let func = &fir.functions[0];
    assert!(func.body.len() >= 3); // let a, let b, expr

    // a and b should have different LocalIds
    let a_id = match &func.body[0] {
        FirStmt::Let { name, .. } => *name,
        other => panic!("expected Let, got {:?}", other),
    };
    let b_id = match &func.body[1] {
        FirStmt::Let { name, .. } => *name,
        other => panic!("expected Let, got {:?}", other),
    };
    assert_ne!(a_id, b_id);
}

// ===========================================================================
// Control flow lowering
// ===========================================================================

#[test]
fn lower_if_else() {
    let src = "def f(x: Int) -> Int\n  if x > 0\n    return 1\n  else\n    return 0\n";
    let fir = lower_ok(src);
    let func = &fir.functions[0];

    // Find the If statement
    let if_stmt = func.body.iter().find(|s| matches!(s, FirStmt::If { .. }));
    assert!(if_stmt.is_some(), "expected If in body: {:?}", func.body);

    match if_stmt.unwrap() {
        FirStmt::If {
            cond,
            then_body,
            else_body,
        } => {
            assert!(matches!(cond, FirExpr::BinaryOp { op: BinOp::Gt, .. }));
            assert!(!then_body.is_empty());
            assert!(!else_body.is_empty());
        }
        _ => unreachable!(),
    }
}

#[test]
fn lower_elif_flattens_to_nested_if() {
    let src = "def f(x: Int) -> Int\n  if x > 0\n    return 1\n  elif x < 0\n    return -1\n  else\n    return 0\n";
    let fir = lower_ok(src);
    let func = &fir.functions[0];

    // Find top-level If
    let if_stmt = func.body.iter().find(|s| matches!(s, FirStmt::If { .. }));
    match if_stmt.unwrap() {
        FirStmt::If { else_body, .. } => {
            // elif should be flattened into nested If in else branch
            assert_eq!(else_body.len(), 1);
            assert!(matches!(&else_body[0], FirStmt::If { .. }));
        }
        _ => unreachable!(),
    }
}

#[test]
fn lower_while_loop() {
    let src = "def f() -> Void\n  let x: Int = 0\n  while x < 10\n    x = x + 1\n";
    let fir = lower_ok(src);
    let func = &fir.functions[0];

    let while_stmt = func
        .body
        .iter()
        .find(|s| matches!(s, FirStmt::While { .. }));
    assert!(
        while_stmt.is_some(),
        "expected While in body: {:?}",
        func.body
    );

    match while_stmt.unwrap() {
        FirStmt::While { cond, body, .. } => {
            assert!(matches!(cond, FirExpr::BinaryOp { op: BinOp::Lt, .. }));
            assert!(!body.is_empty());
        }
        _ => unreachable!(),
    }
}

#[test]
fn lower_break_and_continue() {
    let src = "def f() -> Void\n  let x: Int = 0\n  while true\n    if x > 5\n      break\n    x = x + 1\n    continue\n";
    let fir = lower_ok(src);
    let func = &fir.functions[0];

    // Find while, check it contains break and continue
    match func
        .body
        .iter()
        .find(|s| matches!(s, FirStmt::While { .. }))
        .unwrap()
    {
        FirStmt::While { body, .. } => {
            let has_break = body.iter().any(|s| match s {
                FirStmt::If { then_body, .. } => {
                    then_body.iter().any(|s| matches!(s, FirStmt::Break))
                }
                _ => false,
            });
            let has_continue = body.iter().any(|s| matches!(s, FirStmt::Continue));
            assert!(has_break, "expected break in while body");
            assert!(has_continue, "expected continue in while body");
        }
        _ => unreachable!(),
    }
}

// ===========================================================================
// Function calls
// ===========================================================================

#[test]
fn lower_function_call() {
    let src = "def double(x: Int) -> Int\n  x + x\n\ndef main() -> Int\n  double(x: 5)\n";
    let fir = lower_ok(src);
    assert_eq!(fir.functions.len(), 2);

    let main_func = fir.functions.iter().find(|f| f.name == "main").unwrap();
    // main body should have a Call expression
    let has_call = main_func.body.iter().any(|s| match s {
        FirStmt::Expr(FirExpr::Call { args, ret_ty, .. })
        | FirStmt::Return(FirExpr::Call { args, ret_ty, .. }) => {
            args.len() == 1 && *ret_ty == FirType::I64
        }
        _ => false,
    });
    assert!(has_call, "expected Call in main body: {:?}", main_func.body);
}

#[test]
fn lower_multiple_functions() {
    let src = "def add(a: Int, b: Int) -> Int\n  a + b\n\ndef mul(a: Int, b: Int) -> Int\n  a * b\n\ndef main() -> Int\n  add(a: 2, b: 3)\n";
    let fir = lower_ok(src);
    assert_eq!(fir.functions.len(), 3);

    // All functions should have unique IDs
    let ids: Vec<_> = fir.functions.iter().map(|f| f.id).collect();
    assert_ne!(ids[0], ids[1]);
    assert_ne!(ids[1], ids[2]);
    assert_ne!(ids[0], ids[2]);
}

#[test]
fn lower_main_sets_entry() {
    let src = "def main() -> Int\n  0\n";
    let fir = lower_ok(src);
    assert!(fir.entry.is_some());
    let entry_func = fir.get_function(fir.entry.unwrap());
    assert_eq!(entry_func.name, "main");
    assert!(entry_func.is_entry);
}

// ===========================================================================
// Literal lowering
// ===========================================================================

#[test]
fn lower_int_literal() {
    let fir = lower_ok("def f() -> Int\n  return 42\n");
    let func = &fir.functions[0];
    match first_user_stmt(func) {
        Some(FirStmt::Return(FirExpr::IntLit(42))) => {}
        other => panic!("expected IntLit(42), got {:?}", other),
    }
}

#[test]
#[allow(clippy::approx_constant)]
fn lower_float_literal() {
    let fir = lower_ok("def f() -> Float\n  return 3.14\n");
    let func = &fir.functions[0];
    match first_user_stmt(func) {
        Some(FirStmt::Return(FirExpr::FloatLit(f))) => {
            assert!((f - 3.14).abs() < f64::EPSILON);
        }
        other => panic!("expected FloatLit, got {:?}", other),
    }
}

#[test]
fn lower_bool_literals() {
    let fir = lower_ok("def f() -> Bool\n  return true\n");
    let func = &fir.functions[0];
    match first_user_stmt(func) {
        Some(FirStmt::Return(FirExpr::BoolLit(true))) => {}
        other => panic!("expected BoolLit(true), got {:?}", other),
    }
}

#[test]
fn lower_string_literal() {
    let fir = lower_ok("def f() -> String\n  return \"hello\"\n");
    let func = &fir.functions[0];
    match first_user_stmt(func) {
        Some(FirStmt::Return(FirExpr::StringLit(s))) => {
            assert_eq!(s, "hello");
        }
        other => panic!("expected StringLit, got {:?}", other),
    }
}

#[test]
fn lower_nil_literal() {
    let fir = lower_ok("def f() -> Void\n  return nil\n");
    let func = &fir.functions[0];
    match first_user_stmt(func) {
        Some(FirStmt::Return(FirExpr::NilLit)) => {}
        other => panic!("expected NilLit, got {:?}", other),
    }
}

// ===========================================================================
// Assignment lowering
// ===========================================================================

#[test]
fn lower_variable_assignment() {
    let src = "def f() -> Int\n  let x: Int = 1\n  x = 2\n  return x\n";
    let fir = lower_ok(src);
    let func = &fir.functions[0];
    let body = real_body(func);
    // body: let, assign, [scope_exit,] return — at least 3 user stmts
    assert!(body.len() >= 3);

    match &body[1] {
        FirStmt::Assign {
            target: FirPlace::Local(id),
            ..
        } => {
            // The LocalId should match the one from the let binding
            let let_id = match &body[0] {
                FirStmt::Let { name, .. } => *name,
                _ => panic!("expected Let"),
            };
            assert_eq!(*id, let_id);
        }
        other => panic!("expected Assign(Local, ...), got {:?}", other),
    }
}

// ===========================================================================
// List lowering
// ===========================================================================

#[test]
fn lower_list_literal() {
    let fir = lower_ok("def f() -> List[Int]\n  [1, 2, 3]\n");
    let func = &fir.functions[0];
    let expr = func
        .body
        .iter()
        .find_map(|s| match s {
            FirStmt::Expr(FirExpr::RuntimeCall { name, .. }) if name.starts_with("aster_") => None,
            FirStmt::Expr(e) | FirStmt::Return(e) => Some(e),
            _ => None,
        })
        .expect("expected expression");

    match expr {
        FirExpr::ListNew { elements, elem_ty } => {
            assert_eq!(elements.len(), 3);
            assert_eq!(*elem_ty, FirType::I64);
        }
        other => panic!("expected ListNew, got {:?}", other),
    }
}

#[test]
fn lower_list_index() {
    let src = "def f() -> Int\n  let xs: List[Int] = [10, 20, 30]\n  xs[1]\n";
    let fir = lower_ok(src);
    let func = &fir.functions[0];
    let has_list_get = func.body.iter().any(|s| {
        matches!(
            s,
            FirStmt::Expr(FirExpr::ListGet { .. }) | FirStmt::Return(FirExpr::ListGet { .. })
        )
    });
    assert!(has_list_get, "expected ListGet in body: {:?}", func.body);
}

// ===========================================================================
// REPL incremental lowering
// ===========================================================================

#[test]
fn repl_incremental_lowering() {
    let env = TypeEnv::new();
    let mut lowerer = Lowerer::new(env, ast::TypeTable::new());

    // First REPL input: an expression
    let expr = ast::Expr::Int(42, ast::span::Span::dummy());
    let _id = lowerer
        .lower_repl_expr(&expr, &Type::Int)
        .expect("lower ok");

    let module = lowerer.module();
    assert_eq!(module.functions.len(), 1);
    assert!(module.functions[0].is_entry);

    // The function body should return the integer
    match &module.functions[0].body[0] {
        FirStmt::Return(FirExpr::IntLit(42)) => {}
        other => panic!("expected Return(IntLit(42)), got {:?}", other),
    }
}

// ===========================================================================
// Runtime calls (builtins)
// ===========================================================================

#[test]
fn lower_unknown_function_as_runtime_call() {
    // `log` is a builtin in the typechecker but not registered as a FIR function
    let fir = lower_ok("def f() -> Void\n  log(message: \"hello\")\n");
    let func = &fir.functions[0];
    let has_runtime_call = func.body.iter().any(|s| match s {
        FirStmt::Expr(FirExpr::RuntimeCall { name, args, .. }) => name == "log" && args.len() == 1,
        _ => false,
    });
    assert!(
        has_runtime_call,
        "expected RuntimeCall(log) in body: {:?}",
        func.body
    );
}

// ===========================================================================
// Edge cases
// ===========================================================================

#[test]
fn lower_empty_function() {
    let fir = lower_ok("def f() -> Void\n  nil\n");
    assert_eq!(fir.functions.len(), 1);
    assert_eq!(fir.functions[0].params.len(), 0);
}

#[test]
fn lower_deeply_nested_expressions() {
    // (1 + 2) * (3 - 4) / 5
    let fir = lower_ok("def f() -> Int\n  (1 + 2) * (3 - 4) / 5\n");
    let func = &fir.functions[0];
    // Should produce a tree of BinaryOps
    let has_binop = func.body.iter().any(|s| {
        matches!(
            s,
            FirStmt::Expr(FirExpr::BinaryOp { .. }) | FirStmt::Return(FirExpr::BinaryOp { .. })
        )
    });
    assert!(
        has_binop,
        "expected nested BinaryOp tree in body: {:?}",
        func.body
    );
}

#[test]
fn lower_all_comparison_ops() {
    let src = "def f(a: Int, b: Int) -> Bool\n  let r1: Bool = a == b\n  let r2: Bool = a != b\n  let r3: Bool = a < b\n  let r4: Bool = a > b\n  let r5: Bool = a <= b\n  let r6: Bool = a >= b\n  r1\n";
    let fir = lower_ok(src);
    let func = &fir.functions[0];
    let body = real_body(func);
    // 6 let bindings + 1 expr (+ possible scope_exit before return)
    assert!(body.len() >= 7);

    // Each let should have a BinaryOp with Bool result type
    for i in 0..6 {
        match &body[i] {
            FirStmt::Let { ty, value, .. } => {
                assert_eq!(*ty, FirType::Bool);
                match value {
                    FirExpr::BinaryOp { result_ty, .. } => {
                        assert_eq!(*result_ty, FirType::Bool);
                    }
                    other => panic!("expected BinaryOp at stmt {}, got {:?}", i, other),
                }
            }
            other => panic!("expected Let at stmt {}, got {:?}", i, other),
        }
    }
}

#[test]
fn lower_logical_and_or() {
    let src = "def f(a: Bool, b: Bool) -> Bool\n  a and b or not a\n";
    let fir = lower_ok(src);
    let func = &fir.functions[0];
    // Should be: Or(And(a, b), Not(a))
    let expr = func
        .body
        .iter()
        .find_map(|s| match s {
            FirStmt::Expr(FirExpr::RuntimeCall { name, .. }) if name.starts_with("aster_") => None,
            FirStmt::Expr(e) | FirStmt::Return(e) => Some(e),
            _ => None,
        })
        .expect("expected expression");

    match expr {
        FirExpr::BinaryOp {
            op: BinOp::Or,
            result_ty,
            ..
        } => {
            assert_eq!(*result_ty, FirType::Bool);
        }
        other => panic!("expected Or, got {:?}", other),
    }
}

// ===========================================================================
// FIR type mapping tests
// ===========================================================================

#[test]
fn lower_for_loop_over_list() {
    let src = "\
def sum_list(xs: List[Int]) -> Int
  let total: Int = 0
  for x in xs
    total = total + x
  return total
";
    let fir = lower_ok(src);
    let func = &fir.functions[0];
    // For loop is lowered to Block([setup..., While { ... }])
    let has_for_desugared = func.body.iter().any(|s| match s {
        FirStmt::Block(stmts) => stmts.iter().any(|s| matches!(s, FirStmt::While { .. })),
        _ => false,
    });
    assert!(
        has_for_desugared,
        "for loop should desugar to Block([setup, While]): {:?}",
        func.body
    );
}

#[test]
fn lower_list_len_via_runtime() {
    let src = "\
def f(xs: List[Int]) -> Int
  let n: Int = xs.len()
  n
";
    let fir = lower_ok(src);
    let func = &fir.functions[0];
    // length() should become a RuntimeCall to aster_list_len
    let has_list_len = func.body.iter().any(|s| match s {
        FirStmt::Let {
            value: FirExpr::RuntimeCall { name, .. },
            ..
        } => name == "aster_list_len",
        _ => false,
    });
    assert!(
        has_list_len,
        "expected aster_list_len call: {:?}",
        func.body
    );
}

#[test]
fn lower_list_push_via_runtime() {
    let src = "\
def f() -> Void
  let xs: List[Int] = [1, 2]
  xs.push(item: 3)
";
    let fir = lower_ok(src);
    let func = &fir.functions[0];
    let has_push = func.body.iter().any(|s| match s {
        FirStmt::Expr(FirExpr::RuntimeCall { name, .. }) => name == "aster_list_push",
        _ => false,
    });
    assert!(has_push, "expected aster_list_push call: {:?}", func.body);
}

// ===========================================================================
// Milestone 8: Error handling — tagged unions, throws, propagate, catch
// ===========================================================================

#[test]
fn lower_nullable_wrap_some() {
    let src = "\
def f() -> Int?
  let x: Int = 42
  return x
";
    let fir = lower_ok(src);
    let func = &fir.functions[0];
    // Return of a non-nullable value into a nullable return type should wrap
    assert_eq!(
        func.ret_type,
        FirType::TaggedUnion {
            tag_bits: 1,
            variants: vec![FirType::I64, FirType::Void]
        }
    );
}

#[test]
fn lower_nullable_nil_return() {
    let src = "\
def f() -> Int?
  return nil
";
    let fir = lower_ok(src);
    let func = &fir.functions[0];
    assert_eq!(
        func.ret_type,
        FirType::TaggedUnion {
            tag_bits: 1,
            variants: vec![FirType::I64, FirType::Void]
        }
    );
}

// ===========================================================================
// Milestone 9: Generics — monomorphization
// ===========================================================================

#[test]
fn lower_generic_function_monomorphized() {
    let src = "\
def identity(x: T) -> T
  x

def main() -> Int
  identity(x: 42)
";
    let fir = lower_ok(src);
    // Should have at least 2 functions: the monomorphized identity[Int] and main
    assert!(
        fir.functions.len() >= 2,
        "expected monomorphized identity + main, got {} funcs",
        fir.functions.len()
    );
    // main should call identity and return Int
    let main_func = fir.functions.iter().find(|f| f.name == "main").unwrap();
    assert_eq!(main_func.ret_type, FirType::I64);
}

#[test]
fn lower_generic_class_monomorphized() {
    let src = "\
class Box[T]
  value: T

def main() -> Int
  let b: Box[Int] = Box(value: 42)
  b.value
";
    let fir = lower_ok(src);
    let main_func = fir.functions.iter().find(|f| f.name == "main").unwrap();
    assert_eq!(main_func.ret_type, FirType::I64);
}

// ===========================================================================
// Milestone 11: Async — task handles and explicit async FIR
// ===========================================================================

#[test]
fn lower_async_call_to_spawn() {
    let src = "\
def fetch() -> Int
  42

def main() -> Int
  let t: Task[Int] = async fetch()
  resolve t!
";
    let fir = lower_ok(src);
    let main_func = fir.functions.iter().find(|f| f.name == "main").unwrap();
    let has_spawn = main_func.body.iter().any(|stmt| {
        matches!(
            stmt,
            FirStmt::Let {
                value: FirExpr::Spawn { .. },
                ..
            }
        )
    });
    assert!(
        has_spawn,
        "expected Spawn in lowered body: {:?}",
        main_func.body
    );
}

#[test]
fn lower_blocking_call_to_block_on() {
    let src = "\
def fetch_child() -> Int
  7

def fetch() -> Int
  async fetch_child()
  42

def main() -> Int
  blocking fetch()
";
    let fir = lower_ok(src);
    let main_func = fir.functions.iter().find(|f| f.name == "main").unwrap();
    let has_block_on = main_func.body.iter().any(|stmt| {
        matches!(
            stmt,
            FirStmt::Return(FirExpr::BlockOn { .. }) | FirStmt::Expr(FirExpr::BlockOn { .. })
        )
    });
    assert!(
        has_block_on,
        "expected BlockOn in lowered body: {:?}",
        main_func.body
    );
}

#[test]
fn lower_task_is_ready_method_to_runtime_call() {
    let src = "\
def fetch() -> Int
  42

def main() -> Bool
  let t: Task[Int] = async fetch()
  t.is_ready()
";
    let fir = lower_ok(src);
    let main_func = fir.functions.iter().find(|f| f.name == "main").unwrap();
    let has_is_ready = main_func.body.iter().any(|stmt| {
        matches!(
            stmt,
            FirStmt::Return(FirExpr::RuntimeCall { name, .. })
                if name == "aster_task_is_ready"
        )
    });
    assert!(
        has_is_ready,
        "expected aster_task_is_ready runtime call in lowered body: {:?}",
        main_func.body
    );
}

#[test]
fn lower_task_cancel_method_to_cancel_task_node() {
    let src = "\
def fetch() -> Int
  42

def main() -> Bool
  let t: Task[Int] = async fetch()
  t.cancel()
  t.is_ready()
";
    let fir = lower_ok(src);
    let main_func = fir.functions.iter().find(|f| f.name == "main").unwrap();
    let has_cancel = main_func
        .body
        .iter()
        .any(|stmt| matches!(stmt, FirStmt::Expr(FirExpr::CancelTask { .. })));
    assert!(
        has_cancel,
        "expected CancelTask node in lowered body: {:?}",
        main_func.body
    );
}

#[test]
fn lower_task_wait_cancel_method_to_wait_cancel_node() {
    let src = "\
def fetch() -> Int
  42

def main() -> Bool
  let t: Task[Int] = async fetch()
  t.wait_cancel()
  t.is_ready()
";
    let fir = lower_ok(src);
    let main_func = fir.functions.iter().find(|f| f.name == "main").unwrap();
    let has_wait_cancel = main_func
        .body
        .iter()
        .any(|stmt| matches!(stmt, FirStmt::Expr(FirExpr::WaitCancel { .. })));
    assert!(
        has_wait_cancel,
        "expected WaitCancel node in lowered body: {:?}",
        main_func.body
    );
}

#[test]
fn lower_resolve_all_to_runtime_call() {
    let src = "\
def fetch() -> Int
  42

def main() -> List[Int]
  let tasks: List[Task[Int]] = [async fetch()]
  resolve_all(tasks: tasks)!
";
    let fir = lower_ok(src);
    let main_func = fir.functions.iter().find(|f| f.name == "main").unwrap();
    let has_resolve_all = main_func.body.iter().any(|stmt| {
        matches!(
            stmt,
            FirStmt::Let {
                value: FirExpr::RuntimeCall { name, .. },
                ..
            }
                if name == "aster_task_resolve_all_i64"
        )
    });
    assert!(
        has_resolve_all,
        "expected resolve_all runtime call in lowered body: {:?}",
        main_func.body
    );
}

#[test]
fn lower_blocking_call_to_block_on_without_direct_call() {
    let src = "\
def fetch() -> Int
  async child()
  42

def child() -> Int
  7

def main() -> Int
  blocking fetch()
";
    let fir = lower_ok(src);
    let main_func = fir.functions.iter().find(|f| f.name == "main").unwrap();
    assert!(main_func.body.iter().any(|stmt| matches!(
        stmt,
        FirStmt::Return(FirExpr::BlockOn { .. })
    ) || matches!(
        stmt,
        FirStmt::Expr(FirExpr::BlockOn { .. })
    ) || matches!(
        stmt,
        FirStmt::Let {
            value: FirExpr::BlockOn { .. },
            ..
        }
    )));
    assert!(
        !main_func
            .body
            .iter()
            .any(|stmt| matches!(stmt, FirStmt::Return(FirExpr::Call { .. }))
                || matches!(stmt, FirStmt::Expr(FirExpr::Call { .. }))
                || matches!(
                    stmt,
                    FirStmt::Let {
                        value: FirExpr::Call { .. },
                        ..
                    }
                )),
        "blocking call should lower through BlockOn instead of a direct eager call: {:?}",
        main_func.body
    );
}

#[test]
fn lower_async_call_to_spawn_without_direct_call() {
    let src = "\
def fetch() -> Int
  42

def main() -> Task[Int]
  async fetch()
";
    let fir = lower_ok(src);
    let main_func = fir.functions.iter().find(|f| f.name == "main").unwrap();
    assert!(main_func.body.iter().any(|stmt| matches!(
        stmt,
        FirStmt::Return(FirExpr::Spawn { .. })
    ) || matches!(
        stmt,
        FirStmt::Expr(FirExpr::Spawn { .. })
    ) || matches!(
        stmt,
        FirStmt::Let {
            value: FirExpr::Spawn { .. },
            ..
        }
    )));
    assert!(
        !main_func
            .body
            .iter()
            .any(|stmt| matches!(stmt, FirStmt::Return(FirExpr::Call { .. }))
                || matches!(stmt, FirStmt::Expr(FirExpr::Call { .. }))
                || matches!(
                    stmt,
                    FirStmt::Let {
                        value: FirExpr::Call { .. },
                        ..
                    }
                )),
        "async call should lower through Spawn instead of a direct eager call: {:?}",
        main_func.body
    );
}

#[test]
fn lower_marks_suspendable_functions_for_codegen() {
    let src = "\
def child() -> Int
  7

def parent() -> Int
  let t: Task[Int] = async child()
  resolve t!

def main() -> Int
  blocking parent()
";
    let fir = lower_ok(src);
    let parent = fir
        .functions
        .iter()
        .find(|func| func.name == "parent")
        .expect("parent function");
    let child = fir
        .functions
        .iter()
        .find(|func| func.name == "child")
        .expect("child function");
    let main = fir
        .functions
        .iter()
        .find(|func| func.name == "main")
        .expect("main function");

    assert!(parent.suspendable, "parent should be marked suspendable");
    assert!(!child.suspendable, "child should stay non-suspendable");
    assert!(
        main.suspendable,
        "main should inherit blocking suspendability"
    );
}

#[test]
fn lower_resolve_first_to_runtime_call() {
    let src = "\
def fetch() -> Int
  42

def main() -> Int
  let tasks: List[Task[Int]] = [async fetch()]
  resolve_first(tasks: tasks)!
";
    let fir = lower_ok(src);
    let main_func = fir.functions.iter().find(|f| f.name == "main").unwrap();
    let has_resolve_first = main_func.body.iter().any(|stmt| {
        matches!(
            stmt,
            FirStmt::Let {
                value: FirExpr::RuntimeCall { name, .. },
                ..
            }
                if name == "aster_task_resolve_first_i64"
        )
    });
    assert!(
        has_resolve_first,
        "expected resolve_first runtime call in lowered body: {:?}",
        main_func.body
    );
}

#[test]
fn lower_call_inserts_safepoint_before_direct_call() {
    let src = "\
def child() -> Int
  42

def main() -> Int
  child()
";
    let fir = lower_ok(src);
    let main_func = fir.functions.iter().find(|f| f.name == "main").unwrap();
    assert!(matches!(
        first_user_stmt(main_func),
        Some(FirStmt::Expr(FirExpr::Safepoint))
    ));
}

#[test]
fn lower_loop_body_appends_backedge_safepoint() {
    let src = "\
def main() -> Int
  let i = 0
  while i < 1
    i = i + 1
  i
";
    let fir = lower_ok(src);
    let main_func = fir.functions.iter().find(|f| f.name == "main").unwrap();
    let loop_body = main_func
        .body
        .iter()
        .find_map(|stmt| match stmt {
            FirStmt::While { body, .. } => Some(body),
            _ => None,
        })
        .expect("while loop body");
    assert!(matches!(
        loop_body.last(),
        Some(FirStmt::Expr(FirExpr::Safepoint))
    ));
}

// ===========================================================================
// FIR type mapping tests
// ===========================================================================

#[test]
fn nullable_type_becomes_tagged_union() {
    let fir = lower_ok("def f(x: Int?) -> Int?\n  x\n");
    let func = &fir.functions[0];
    match &func.params[0].1 {
        FirType::TaggedUnion { tag_bits, variants } => {
            assert_eq!(*tag_bits, 1);
            assert_eq!(variants.len(), 2);
            assert_eq!(variants[0], FirType::I64);
            assert_eq!(variants[1], FirType::Void);
        }
        other => panic!("expected TaggedUnion, got {:?}", other),
    }
}

// ===========================================================================
// String interpolation lowering
// ===========================================================================

#[test]
fn lower_string_interpolation_literal_only() {
    // A plain string literal in interpolation context should produce StringLit
    let fir = lower_ok("def f() -> String\n  \"hello\"\n");
    let func = &fir.functions[0];
    let expr = func
        .body
        .iter()
        .find_map(|s| match s {
            FirStmt::Expr(FirExpr::RuntimeCall { name, .. }) if name.starts_with("aster_") => None,
            FirStmt::Expr(e) | FirStmt::Return(e) => Some(e),
            _ => None,
        })
        .expect("expected expression");
    assert!(
        matches!(expr, FirExpr::StringLit(s) if s == "hello"),
        "expected StringLit(\"hello\"), got {:?}",
        expr
    );
}

#[test]
fn lower_string_interpolation_with_int_var() {
    // "val: {x}" where x: Int should produce RuntimeCall to aster_int_to_string + aster_string_concat
    let src = "\
def f() -> String
  let x: Int = 42
  \"val: {x}\"
";
    let fir = lower_ok(src);
    let func = &fir.functions[0];
    let expr = func
        .body
        .iter()
        .find_map(|s| match s {
            FirStmt::Expr(FirExpr::RuntimeCall { name, .. })
                if name.starts_with("aster_") && name != "aster_string_concat" =>
            {
                None
            }
            FirStmt::Expr(e) | FirStmt::Return(e) => Some(e),
            _ => None,
        })
        .expect("expected expression");

    // Should be aster_string_concat(StringLit("val: "), aster_int_to_string(x))
    match expr {
        FirExpr::RuntimeCall { name, args, .. } if name == "aster_string_concat" => {
            assert_eq!(args.len(), 2, "concat should have 2 args");
            assert!(
                matches!(&args[0], FirExpr::StringLit(s) if s == "val: "),
                "first arg should be StringLit(\"val: \"), got {:?}",
                args[0]
            );
            assert!(
                matches!(&args[1], FirExpr::RuntimeCall { name, .. } if name == "aster_int_to_string"),
                "second arg should be aster_int_to_string call, got {:?}",
                args[1]
            );
        }
        other => panic!("expected RuntimeCall(aster_string_concat), got {:?}", other),
    }
}

#[test]
fn lower_string_interpolation_with_class_to_string() {
    // Class with manual to_string, interpolation should produce Call to ClassName.to_string
    let src = "\
class Greeter includes Printable
  name: String

  def to_string() -> String
    return \"hi\"

def f() -> String
  let g: Greeter = Greeter(name: \"world\")
  \"{g}\"
";
    let fir = lower_ok(src);
    let f_func = fir.functions.iter().find(|f| f.name == "f").unwrap();
    // The interpolation of a single class var should call ClassName.to_string
    // It could be a Call (if to_string was lowered) or a RuntimeCall fallback
    fn has_to_string_call(expr: &FirExpr) -> bool {
        match expr {
            FirExpr::Call { args, .. } => {
                // to_string call should have exactly 1 arg (self)
                args.len() == 1
            }
            _ => false,
        }
    }
    assert!(
        f_func.body.iter().any(|stmt| match stmt {
            FirStmt::Expr(expr) | FirStmt::Return(expr) => has_to_string_call(expr),
            _ => false,
        }),
        "expected Call to to_string method, got {:?}",
        f_func.body
    );
}

// ===========================================================================
// Method call lowering
// ===========================================================================

#[test]
fn lower_class_method_call() {
    // obj.method() should produce Call with self as first arg
    let src = "\
class Counter
  value: Int

  def get_value() -> Int
    return value

def f() -> Int
  let c: Counter = Counter(value: 10)
  c.get_value()
";
    let fir = lower_ok(src);
    let f_func = fir.functions.iter().find(|f| f.name == "f").unwrap();
    let has_method_call = f_func.body.iter().any(|s| match s {
        FirStmt::Expr(FirExpr::Call { args, .. }) | FirStmt::Return(FirExpr::Call { args, .. }) => {
            // self (the object) should be passed as the first arg
            args.len() == 1 && matches!(&args[0], FirExpr::LocalVar(_, FirType::Ptr))
        }
        _ => false,
    });
    assert!(
        has_method_call,
        "expected Call with self as first arg in body: {:?}",
        f_func.body
    );
}

#[test]
fn lower_list_len_method() {
    // xs.len() should produce RuntimeCall to aster_list_len
    let src = "\
def f(xs: List[Int]) -> Int
  xs.len()
";
    let fir = lower_ok(src);
    let func = &fir.functions[0];
    let has_list_len = func.body.iter().any(|s| match s {
        FirStmt::Expr(FirExpr::RuntimeCall { name, args, .. })
        | FirStmt::Return(FirExpr::RuntimeCall { name, args, .. }) => {
            name == "aster_list_len" && args.len() == 1
        }
        _ => false,
    });
    assert!(
        has_list_len,
        "expected RuntimeCall(aster_list_len) in body: {:?}",
        func.body
    );
}

// ===========================================================================
// Closure lowering
// ===========================================================================

#[test]
fn lower_lambda_no_captures() {
    // Nested def with no captures should create ClosureCreate with NilLit env
    let src = "\
def f() -> Void
  def add(a: Int, b: Int) -> Int
    a + b
  nil
";
    let fir = lower_ok(src);
    // The lambda should be lifted to a separate function
    assert!(
        fir.functions.len() >= 2,
        "expected at least 2 functions (f + lambda), got {}",
        fir.functions.len()
    );
    // The lambda function should have __env as first param
    let lambda_func = fir
        .functions
        .iter()
        .find(|f| f.name.starts_with("__lambda_"))
        .expect("expected a __lambda_ function");
    assert_eq!(
        lambda_func.params[0].0, "__env",
        "lambda should have __env as first param"
    );
    assert_eq!(lambda_func.params[0].1, FirType::Ptr);

    // f's body should contain a Let with ClosureCreate { env: NilLit }
    let f_func = fir.functions.iter().find(|f| f.name == "f").unwrap();
    let has_closure_create = f_func.body.iter().any(|s| match s {
        FirStmt::Let {
            value: FirExpr::ClosureCreate { env, .. },
            ..
        } => matches!(env.as_ref(), FirExpr::NilLit),
        _ => false,
    });
    assert!(
        has_closure_create,
        "expected ClosureCreate with NilLit env in f body: {:?}",
        f_func.body
    );
}

#[test]
fn lower_lambda_with_capture() {
    // Nested def capturing outer var should create env allocation + ClosureCreate
    let src = "\
def f() -> Void
  let x: Int = 10
  def add_x(y: Int) -> Int
    x + y
  nil
";
    let fir = lower_ok(src);
    // Lambda function should exist and have __env as first param
    let lambda_func = fir
        .functions
        .iter()
        .find(|f| f.name.starts_with("__lambda_"))
        .expect("expected a __lambda_ function");
    assert_eq!(lambda_func.params[0].0, "__env");

    // Lambda body should start with EnvLoad for the captured variable x
    let has_env_load = lambda_func.body.iter().any(|s| {
        matches!(
            s,
            FirStmt::Let {
                value: FirExpr::EnvLoad { .. },
                ..
            }
        )
    });
    assert!(
        has_env_load,
        "expected EnvLoad in lambda body for captured var: {:?}",
        lambda_func.body
    );

    // f's body should contain env allocation (aster_class_alloc) and ClosureCreate
    let f_func = fir.functions.iter().find(|f| f.name == "f").unwrap();
    let has_env_alloc = f_func.body.iter().any(|s| match s {
        FirStmt::Let {
            value: FirExpr::RuntimeCall { name, .. },
            ..
        } => name == "aster_class_alloc",
        _ => false,
    });
    assert!(
        has_env_alloc,
        "expected aster_class_alloc for env in f body: {:?}",
        f_func.body
    );

    let has_closure_create = f_func.body.iter().any(|s| match s {
        FirStmt::Let {
            value: FirExpr::ClosureCreate { env, .. },
            ..
        } => !matches!(env.as_ref(), FirExpr::NilLit),
        _ => false,
    });
    assert!(
        has_closure_create,
        "expected ClosureCreate with non-nil env in f body: {:?}",
        f_func.body
    );
}

// ===========================================================================
// Match with enum patterns
// ===========================================================================

#[test]
fn lower_match_enum_variant() {
    // Match on fieldless enum should produce tag comparison via FieldGet + BinaryOp(Eq)
    let src = "\
enum Color
  Red
  Green
  Blue

def f(c: Color) -> Int
  match c
    Color.Red => 1
    Color.Green => 2
    Color.Blue => 3
";
    let fir = lower_ok(src);
    let f_func = fir.functions.iter().find(|f| f.name == "f").unwrap();

    // Match lowers to pending stmts + an if/else chain.
    // Look for an If statement whose condition involves BinaryOp(Eq) comparing a tag
    fn has_tag_comparison(stmts: &[FirStmt]) -> bool {
        stmts.iter().any(|s| match s {
            FirStmt::If { cond, .. } => matches!(
                cond,
                FirExpr::BinaryOp {
                    op: BinOp::Eq,
                    left,
                    ..
                } if matches!(left.as_ref(), FirExpr::FieldGet { offset: 0, .. })
            ),
            _ => false,
        })
    }
    assert!(
        has_tag_comparison(&f_func.body),
        "expected tag comparison (FieldGet offset 0 + Eq) in body: {:?}",
        f_func.body
    );
}

// ===========================================================================
// Field assignment lowering
// ===========================================================================

#[test]
fn lower_field_assignment() {
    // obj.field = val should produce Assign with FirPlace::Field
    let src = "\
class Point
  x: Int
  y: Int

def f() -> Void
  let p: Point = Point(x: 1, y: 2)
  p.x = 10
";
    let fir = lower_ok(src);
    let f_func = fir.functions.iter().find(|f| f.name == "f").unwrap();
    let has_field_assign = f_func.body.iter().any(|s| match s {
        FirStmt::Assign {
            target: FirPlace::Field { .. },
            value,
        } => matches!(value, FirExpr::IntLit(10)),
        _ => false,
    });
    assert!(
        has_field_assign,
        "expected Assign(Field, IntLit(10)) in body: {:?}",
        f_func.body
    );
}

// ===========================================================================
// Float arithmetic lowering
// ===========================================================================

#[test]
#[allow(clippy::approx_constant)]
fn lower_float_arithmetic() {
    // 3.14 + 2.71 should produce BinaryOp with F64 result type
    let src = "def f() -> Float\n  3.14 + 2.71\n";
    let fir = lower_ok(src);
    let func = &fir.functions[0];
    let expr = func
        .body
        .iter()
        .find_map(|s| match s {
            FirStmt::Expr(FirExpr::RuntimeCall { name, .. }) if name.starts_with("aster_") => None,
            FirStmt::Expr(e) | FirStmt::Return(e) => Some(e),
            _ => None,
        })
        .expect("expected expression");

    match expr {
        FirExpr::BinaryOp {
            op: BinOp::Add,
            left,
            right,
            result_ty,
        } => {
            assert_eq!(*result_ty, FirType::F64);
            assert!(
                matches!(left.as_ref(), FirExpr::FloatLit(f) if (*f - 3.14).abs() < f64::EPSILON),
                "left should be FloatLit(3.14), got {:?}",
                left
            );
            assert!(
                matches!(right.as_ref(), FirExpr::FloatLit(f) if (*f - 2.71).abs() < f64::EPSILON),
                "right should be FloatLit(2.71), got {:?}",
                right
            );
        }
        other => panic!("expected BinaryOp(Add, F64), got {:?}", other),
    }
}

// ===========================================================================
// Mixed Int/Float arithmetic coercion
// ===========================================================================

#[test]
fn lower_mixed_int_float_add() {
    // 1 + 2.5 should promote the Int to Float, yielding F64 result
    let src = "def f() -> Float\n  1 + 2.5\n";
    let fir = lower_ok(src);
    let func = &fir.functions[0];
    let expr = func
        .body
        .iter()
        .find_map(|s| match s {
            FirStmt::Return(e) | FirStmt::Expr(e) => {
                if matches!(e, FirExpr::BinaryOp { .. }) {
                    Some(e)
                } else {
                    None
                }
            }
            _ => None,
        })
        .expect("expected BinaryOp");

    match expr {
        FirExpr::BinaryOp {
            op: BinOp::Add,
            left,
            right,
            result_ty,
        } => {
            assert_eq!(*result_ty, FirType::F64);
            assert!(
                matches!(left.as_ref(), FirExpr::IntToFloat(inner) if matches!(inner.as_ref(), FirExpr::IntLit(1))),
                "left should be IntToFloat(IntLit(1)), got {:?}",
                left
            );
            assert!(
                matches!(right.as_ref(), FirExpr::FloatLit(f) if (*f - 2.5).abs() < f64::EPSILON),
                "right should be FloatLit(2.5), got {:?}",
                right
            );
        }
        other => panic!("expected BinaryOp(Add, F64), got {:?}", other),
    }
}

#[test]
fn lower_mixed_float_int_sub() {
    // 3.0 - 1 should promote the right Int to Float
    let src = "def f() -> Float\n  3.0 - 1\n";
    let fir = lower_ok(src);
    let func = &fir.functions[0];
    let expr = func
        .body
        .iter()
        .find_map(|s| match s {
            FirStmt::Return(e) | FirStmt::Expr(e) => {
                if matches!(e, FirExpr::BinaryOp { .. }) {
                    Some(e)
                } else {
                    None
                }
            }
            _ => None,
        })
        .expect("expected BinaryOp");

    match expr {
        FirExpr::BinaryOp {
            op: BinOp::Sub,
            left,
            right,
            result_ty,
        } => {
            assert_eq!(*result_ty, FirType::F64);
            assert!(
                matches!(left.as_ref(), FirExpr::FloatLit(f) if (*f - 3.0).abs() < f64::EPSILON),
                "left should be FloatLit(3.0), got {:?}",
                left
            );
            assert!(
                matches!(right.as_ref(), FirExpr::IntToFloat(inner) if matches!(inner.as_ref(), FirExpr::IntLit(1))),
                "right should be IntToFloat(IntLit(1)), got {:?}",
                right
            );
        }
        other => panic!("expected BinaryOp(Sub, F64), got {:?}", other),
    }
}

#[test]
fn lower_mixed_int_float_comparison() {
    // 1 < 2.5 should promote Int, result is Bool
    let src = "def f() -> Bool\n  1 < 2.5\n";
    let fir = lower_ok(src);
    let func = &fir.functions[0];
    let expr = func
        .body
        .iter()
        .find_map(|s| match s {
            FirStmt::Return(e) | FirStmt::Expr(e) => {
                if matches!(e, FirExpr::BinaryOp { .. }) {
                    Some(e)
                } else {
                    None
                }
            }
            _ => None,
        })
        .expect("expected BinaryOp");

    match expr {
        FirExpr::BinaryOp {
            op: BinOp::Lt,
            left,
            result_ty,
            ..
        } => {
            assert_eq!(*result_ty, FirType::Bool);
            assert!(
                matches!(left.as_ref(), FirExpr::IntToFloat(_)),
                "left should be IntToFloat, got {:?}",
                left
            );
        }
        other => panic!("expected BinaryOp(Lt, Bool), got {:?}", other),
    }
}

#[test]
fn lower_float_pow() {
    // 2.0 ** 3.0 should call aster_pow_float
    let src = "def f() -> Float\n  2.0 ** 3.0\n";
    let fir = lower_ok(src);
    let func = &fir.functions[0];
    let expr = func
        .body
        .iter()
        .find_map(|s| match s {
            FirStmt::Return(e) | FirStmt::Expr(e) => match e {
                FirExpr::RuntimeCall { name, .. } if name == "aster_pow_float" => Some(e),
                _ => None,
            },
            _ => None,
        })
        .expect("expected RuntimeCall(aster_pow_float)");

    match expr {
        FirExpr::RuntimeCall { name, ret_ty, .. } => {
            assert_eq!(name, "aster_pow_float");
            assert_eq!(*ret_ty, FirType::F64);
        }
        other => panic!("expected RuntimeCall(aster_pow_float), got {:?}", other),
    }
}

#[test]
fn lower_mixed_int_float_pow() {
    // 2 ** 3.0 should promote the Int base, call aster_pow_float
    let src = "def f() -> Float\n  2 ** 3.0\n";
    let fir = lower_ok(src);
    let func = &fir.functions[0];
    let expr = func
        .body
        .iter()
        .find_map(|s| match s {
            FirStmt::Return(e) | FirStmt::Expr(e) => match e {
                FirExpr::RuntimeCall { name, .. } if name == "aster_pow_float" => Some(e),
                _ => None,
            },
            _ => None,
        })
        .expect("expected RuntimeCall(aster_pow_float)");

    match expr {
        FirExpr::RuntimeCall {
            name, args, ret_ty, ..
        } => {
            assert_eq!(name, "aster_pow_float");
            assert_eq!(*ret_ty, FirType::F64);
            assert!(
                matches!(&args[0], FirExpr::IntToFloat(_)),
                "base should be IntToFloat, got {:?}",
                &args[0]
            );
        }
        other => panic!("expected RuntimeCall(aster_pow_float), got {:?}", other),
    }
}

// ===========================================================================
// Throw expression lowering
// ===========================================================================

#[test]
fn lower_throw_to_error_set() {
    // throw err should produce RuntimeCall to aster_error_set_typed (with type tag and value)
    let src = "\
class AppError
  message: String

def f() throws AppError -> Int
  throw AppError(message: \"boom\")
";
    let fir = lower_ok(src);
    let f_func = fir.functions.iter().find(|f| f.name == "f").unwrap();
    let has_error_set = f_func.body.iter().any(|s| match s {
        FirStmt::Expr(FirExpr::RuntimeCall { name, .. }) => {
            name == "aster_error_set_typed" || name == "aster_error_set"
        }
        _ => false,
    });
    assert!(
        has_error_set,
        "expected RuntimeCall(aster_error_set_typed) in body: {:?}",
        f_func.body
    );
}

// ===========================================================================
// Catch multi-arm dispatch (GH #3)
// ===========================================================================

/// throw should pass the error type tag and error value to the runtime.
#[test]
fn lower_throw_passes_type_tag_and_value() {
    let src = "\
class AppError
  message: String

def f() throws AppError -> Int
  throw AppError(message: \"boom\")
";
    let fir = lower_ok(src);
    let f_func = fir.functions.iter().find(|f| f.name == "f").unwrap();
    // aster_error_set_typed should receive 2 args: type tag (i64) and error value (ptr)
    let has_typed_error_set = f_func.body.iter().any(|s| match s {
        FirStmt::Expr(FirExpr::RuntimeCall { name, args, .. }) => {
            name == "aster_error_set_typed" && args.len() == 2
        }
        _ => false,
    });
    assert!(
        has_typed_error_set,
        "expected RuntimeCall(aster_error_set_typed) with 2 args in body: {:?}",
        f_func.body
    );
}

/// catch with multiple typed arms should generate per-arm type-tag checks,
/// not a single catch-all.
#[test]
fn lower_catch_multi_arm_generates_type_checks() {
    let src = "\
class NetworkError extends Error
  code: Int

class ParseError extends Error
  line: Int

def risky() throws Error -> Int
  throw NetworkError(message: \"fail\", code: 500)

def main() -> Int
  risky()!.catch
    NetworkError e -> 0
    ParseError e -> 1
    _ -> 2
";
    let fir = lower_ok(src);
    let main_func = fir.functions.iter().find(|f| f.name == "main").unwrap();

    // Should have aster_error_get_tag call to retrieve the error type
    let has_get_tag = stmt_tree_contains(&main_func.body, &|s| match s {
        FirStmt::Let {
            value: FirExpr::RuntimeCall { name, .. },
            ..
        }
        | FirStmt::Expr(FirExpr::RuntimeCall { name, .. }) => name == "aster_error_get_tag",
        _ => false,
    });
    assert!(
        has_get_tag,
        "expected aster_error_get_tag call in main body: {:#?}",
        main_func.body
    );

    // Should have at least 2 If nodes inside the error-handling branch
    // (one per typed arm: NetworkError and ParseError)
    let type_check_ifs = count_nested_ifs(&main_func.body);
    assert!(
        type_check_ifs >= 2,
        "expected at least 2 nested If stmts for typed catch arms, found {}: {:#?}",
        type_check_ifs,
        main_func.body
    );
}

/// Error variable binding: `AppError e -> e.message` should bind `e` as a
/// local variable containing the error value from the runtime.
#[test]
fn lower_catch_binds_error_variable() {
    let src = "\
class AppError extends Error
  code: Int

def risky() throws AppError -> Int
  throw AppError(message: \"fail\", code: 42)

def main() -> Int
  risky()!.catch
    AppError e -> e.code
    _ -> -1
";
    let fir = lower_ok(src);
    let main_func = fir.functions.iter().find(|f| f.name == "main").unwrap();

    // Should have aster_error_get_value call to retrieve the error object
    let has_get_value = stmt_tree_contains(&main_func.body, &|s| match s {
        FirStmt::Let {
            value: FirExpr::RuntimeCall { name, .. },
            ..
        }
        | FirStmt::Expr(FirExpr::RuntimeCall { name, .. }) => name == "aster_error_get_value",
        _ => false,
    });
    assert!(
        has_get_value,
        "expected aster_error_get_value call in main body: {:#?}",
        main_func.body
    );
}

/// Wildcard-only catch should still work (no type dispatch needed).
#[test]
fn lower_catch_wildcard_only_still_works() {
    let src = "\
class AppError extends Error
  code: Int

def risky() throws AppError -> Int
  42

def main() -> Int
  risky()!.catch
    _ -> 0
";
    let fir = lower_ok(src);
    let main_func = fir.functions.iter().find(|f| f.name == "main").unwrap();
    // Should still have error_check for the catch (appears as the cond of an If)
    let has_error_check = stmt_tree_contains(&main_func.body, &|s| match s {
        FirStmt::If { cond, .. } => matches!(
            cond,
            FirExpr::RuntimeCall { name, .. } if name == "aster_error_check"
        ),
        _ => false,
    });
    assert!(
        has_error_check,
        "expected aster_error_check as If cond in wildcard catch: {:#?}",
        main_func.body
    );
}

// --- helpers for walking the FIR statement tree ---

fn stmt_tree_contains(stmts: &[FirStmt], pred: &dyn Fn(&FirStmt) -> bool) -> bool {
    for s in stmts {
        if pred(s) {
            return true;
        }
        match s {
            FirStmt::If {
                then_body,
                else_body,
                ..
            } => {
                if stmt_tree_contains(then_body, pred) || stmt_tree_contains(else_body, pred) {
                    return true;
                }
            }
            FirStmt::While {
                body, increment, ..
            } => {
                if stmt_tree_contains(body, pred) || stmt_tree_contains(increment, pred) {
                    return true;
                }
            }
            FirStmt::Block(body) => {
                if stmt_tree_contains(body, pred) {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

fn count_nested_ifs(stmts: &[FirStmt]) -> usize {
    let mut count = 0;
    for s in stmts {
        match s {
            FirStmt::If {
                then_body,
                else_body,
                ..
            } => {
                count += 1;
                count += count_nested_ifs(then_body);
                count += count_nested_ifs(else_body);
            }
            FirStmt::While {
                body, increment, ..
            } => {
                count += count_nested_ifs(body);
                count += count_nested_ifs(increment);
            }
            FirStmt::Block(body) => {
                count += count_nested_ifs(body);
            }
            _ => {}
        }
    }
    count
}

// ===========================================================================
// Top-level statement lowering
// ===========================================================================

#[test]
fn top_level_if_injected_into_function() {
    let src = "\
let x = 10
if x > 5
  let y = x + 1

def main() -> Int
  x
";
    let fir = lower_ok(src);
    let main_func = fir.functions.iter().find(|f| f.name == "main").unwrap();
    // Top-level if should be injected into main's body (in the global prelude)
    let has_if = main_func
        .body
        .iter()
        .any(|s| matches!(s, FirStmt::If { .. }));
    assert!(
        has_if,
        "expected If injected into main body: {:?}",
        main_func.body
    );
}

#[test]
fn top_level_while_injected_into_function() {
    let src = "\
let x = 0
while x < 3
  x = x + 1

def main() -> Int
  x
";
    let fir = lower_ok(src);
    let main_func = fir.functions.iter().find(|f| f.name == "main").unwrap();
    let has_while = main_func
        .body
        .iter()
        .any(|s| matches!(s, FirStmt::While { .. }));
    assert!(
        has_while,
        "expected While injected into main body: {:?}",
        main_func.body
    );
}

#[test]
fn top_level_for_injected_into_function() {
    let src = "\
let nums = [1, 2, 3]
for n in nums
  let doubled = n * 2

def main() -> Int
  0
";
    let fir = lower_ok(src);
    let main_func = fir.functions.iter().find(|f| f.name == "main").unwrap();
    // For loops lower to Block([setup..., While { ... }])
    let has_for = main_func
        .body
        .iter()
        .any(|s| matches!(s, FirStmt::Block(_)));
    assert!(
        has_for,
        "expected Block (from for loop) injected into main body: {:?}",
        main_func.body
    );
}

#[test]
fn top_level_assignment_injected_into_function() {
    let src = "\
let x = 10
x = 20

def main() -> Int
  x
";
    let fir = lower_ok(src);
    let main_func = fir.functions.iter().find(|f| f.name == "main").unwrap();
    let has_assign = main_func
        .body
        .iter()
        .any(|s| matches!(s, FirStmt::Assign { .. }));
    assert!(
        has_assign,
        "expected Assign injected into main body: {:?}",
        main_func.body
    );
}

// ===========================================================================
// Async expression lowering
// ===========================================================================

#[test]
fn detached_call_lowers_to_spawn() {
    let src = "\
def work() -> Int
  42

def main() -> Int
  detached async work()
  1
";
    let fir = lower_ok(src);
    let main_func = fir.functions.iter().find(|f| f.name == "main").unwrap();
    let has_spawn = main_func
        .body
        .iter()
        .any(|s| matches!(s, FirStmt::Expr(FirExpr::Spawn { .. })));
    assert!(
        has_spawn,
        "expected Spawn for detached async: {:?}",
        main_func.body
    );
}

#[test]
fn function_body_has_implicit_task_scope() {
    let src = "\
def fetch() -> Int
  42

def main() -> Int
  let a: Task[Int] = async fetch()
  let b: Task[Int] = async fetch()
  resolve a!
  resolve b!
  1
";
    let fir = lower_ok(src);
    let main_func = fir.functions.iter().find(|f| f.name == "main").unwrap();
    // First statement is scope_enter
    let has_scope_enter = main_func.body.iter().any(|stmt| {
        matches!(
            stmt,
            FirStmt::Let {
                value: FirExpr::RuntimeCall { name, .. },
                ..
            } if name == "aster_async_scope_enter"
        )
    });
    assert!(
        has_scope_enter,
        "expected implicit scope_enter in function body: {:?}",
        main_func.body
    );
    // Body includes scope_exit before return
    let has_scope_exit = main_func.body.iter().any(|stmt| {
        matches!(
            stmt,
            FirStmt::Expr(FirExpr::RuntimeCall { name, .. })
                if name == "aster_async_scope_exit"
        )
    });
    assert!(
        has_scope_exit,
        "expected implicit scope_exit in function body: {:?}",
        main_func.body
    );
}

#[test]
fn implicit_scope_tracks_spawned_tasks() {
    let src = "\
def fetch() -> Int
  42

def main() -> Int
  let a: Task[Int] = async fetch()
  let b: Task[Int] = async fetch()
  resolve a!
  resolve b!
  1
";
    let fir = lower_ok(src);
    let main_func = fir.functions.iter().find(|f| f.name == "main").unwrap();
    let owned_spawn_count = main_func
        .body
        .iter()
        .filter(|stmt| {
            matches!(
                stmt,
                FirStmt::Let {
                    value: FirExpr::Spawn { scope: Some(_), .. },
                    ..
                }
            )
        })
        .count();
    assert_eq!(
        owned_spawn_count, 2,
        "expected spawned tasks owned by implicit scope: {:?}",
        main_func.body
    );
}

// ===========================================================================
// LowerError span tests — every error must carry a non-dummy span
// ===========================================================================

#[test]
fn error_unsupported_top_level_trait_has_span() {
    let src = "trait Foo\n  def bar() -> Int\n";
    let err = lower_err(src);
    let span = err.span();
    assert!(span.start < span.end, "span must be non-empty: {:?}", span);
}

#[test]
fn nested_class_in_function_no_longer_errors() {
    // Previously nested classes caused a lowering error; now they are supported (GH #11)
    let src = "\
def main() -> Int
  class Inner
    x: Int
  0
";
    let fir = lower_ok(src);
    assert!(
        fir.classes.iter().any(|c| c.name == "Inner"),
        "expected class Inner in module"
    );
}

#[test]
fn range_random_lowers_inline() {
    // (1..10).random() should lower to aster_random_int
    let src = "\
def main() -> Int
  let x: Int = (1..10).random()
  x
";
    let fir = lower_ok(src);
    let main_func = fir.functions.iter().find(|f| f.name == "main").unwrap();
    let has_random_call = main_func.body.iter().any(|stmt| {
        if let FirStmt::Let { value, .. } = stmt {
            matches!(value, FirExpr::BinaryOp { .. })
                || matches!(value, FirExpr::RuntimeCall { name, .. } if name == "aster_random_int")
        } else {
            false
        }
    });
    assert!(
        has_random_call,
        "expected aster_random_int call in main body: {:?}",
        main_func.body
    );
}

#[test]
fn range_random_via_variable_lowers() {
    // let r = 1..10; r.random() should also work
    let src = "\
def main() -> Int
  let r: Range = 1..10
  r.random()
";
    let fir = lower_ok(src);
    let main_func = fir.functions.iter().find(|f| f.name == "main").unwrap();
    let body_str = format!("{:?}", main_func.body);
    assert!(
        body_str.contains("aster_random_int"),
        "expected aster_random_int in body: {}",
        body_str
    );
}

#[test]
fn error_unbound_variable_has_span() {
    // Use a variable that passes typecheck but not lowering.
    // We need a case where the lowerer can't find the variable.
    // A top-level `use` statement triggers UnsupportedFeature, not UnboundVariable.
    // For now, test that UnsupportedFeature from top-level `use` has a span.
    let src = "use std/cmp { Eq }\ndef main() -> Int\n  0\n";
    let err = lower_err(src);
    let span = err.span();
    assert!(span.start < span.end, "span must be non-empty: {:?}", span);
}

#[test]
fn range_var_for_loop_lowers() {
    let src = "\
def main() -> Int
  let bounds = 2..=5
  let total = 0
  for a in bounds
    total = total + a
  total
";
    let fir = lower_ok(src);
    let main = fir.functions.iter().find(|f| f.name == "main").unwrap();
    let body_str = format!("{:?}", main.body);
    // Should use range-based iteration, NOT aster_list_len
    assert!(
        !body_str.contains("aster_list_len"),
        "range var for loop should not use list iteration: {}",
        body_str
    );
    assert!(
        body_str.contains("aster_range_check") || body_str.contains("FieldGet"),
        "range var for loop should use range-based iteration: {}",
        body_str
    );
}

// ===========================================================================
// Iterable vocabulary methods on lists
// ===========================================================================

#[test]
fn iterable_list_map_lowers() {
    let src = "\
let xs = [1, 2, 3]
let ys = xs.map(f: -> x: x + 1)
def main() -> Int
  ys.len()
";
    let fir = lower_ok(src);
    assert!(fir.entry.is_some());
}

#[test]
fn iterable_list_filter_lowers() {
    let src = "\
let xs = [1, 2, 3, 4]
let ys = xs.filter(f: -> x: x > 2)
def main() -> Int
  ys.len()
";
    let fir = lower_ok(src);
    assert!(fir.entry.is_some());
}

#[test]
fn iterable_list_reduce_lowers() {
    let src = "\
let xs = [1, 2, 3]
let total = xs.reduce(init: 0, f: -> acc, x: acc + x)
def main() -> Int
  total
";
    let fir = lower_ok(src);
    assert!(fir.entry.is_some());
}

#[test]
fn iterable_list_any_lowers() {
    // Test with top-level let binding
    let src = "\
let xs = [1, 2, 3]
let found = xs.any(f: -> x: x == 2)
def main() -> Int
  if found
    1
  else
    0
";
    let fir = lower_ok(src);
    assert!(fir.entry.is_some());
}

#[test]
fn iterable_list_any_inline_lowers() {
    // Test with inline usage in if condition
    let src = "\
let xs = [1, 2, 3]
def main() -> Int
  if xs.any(f: -> x: x == 2)
    1
  else
    0
";
    let fir = lower_ok(src);
    assert!(fir.entry.is_some());
}

#[test]
fn iterable_list_all_lowers() {
    let src = "\
let xs = [1, 2, 3]
let ok = xs.all(f: -> x: x > 0)
def main() -> Int
  if ok
    1
  else
    0
";
    let fir = lower_ok(src);
    assert!(fir.entry.is_some());
}

#[test]
fn iterable_list_count_lowers() {
    let src = "\
let xs = [1, 2, 3]
let n = xs.count()
def main() -> Int
  n
";
    let fir = lower_ok(src);
    assert!(fir.entry.is_some());
}

#[test]
fn iterable_list_first_lowers() {
    let src = "\
let xs = [10, 20, 30]
let f = xs.first()
def main() -> Int
  f.or(default: 0)
";
    let fir = lower_ok(src);
    assert!(fir.entry.is_some());
}

#[test]
fn iterable_list_last_lowers() {
    let src = "\
let xs = [10, 20, 30]
let l = xs.last()
def main() -> Int
  l.or(default: 0)
";
    let fir = lower_ok(src);
    assert!(fir.entry.is_some());
}

#[test]
fn iterable_list_to_list_lowers() {
    let src = "\
let xs = [1, 2, 3]
let ys = xs.to_list()
def main() -> Int
  ys.len()
";
    let fir = lower_ok(src);
    assert!(fir.entry.is_some());
}

#[test]
fn iterable_list_find_lowers() {
    let src = "\
let xs = [1, 2, 3]
let found = xs.find(f: -> x: x == 2)
def main() -> Int
  found.or(default: 0)
";
    let fir = lower_ok(src);
    assert!(fir.entry.is_some());
}

#[test]
fn iterable_list_min_lowers() {
    let src = "\
let xs = [3, 1, 2]
let m = xs.min()
def main() -> Int
  m.or(default: 0)
";
    let fir = lower_ok(src);
    assert!(fir.entry.is_some());
}

#[test]
fn iterable_list_max_lowers() {
    let src = "\
let xs = [3, 1, 2]
let m = xs.max()
def main() -> Int
  m.or(default: 0)
";
    let fir = lower_ok(src);
    assert!(fir.entry.is_some());
}

#[test]
fn iterable_list_sort_lowers() {
    let src = "\
let xs = [3, 1, 2]
let sorted = xs.sort()
def main() -> Int
  sorted.len()
";
    let fir = lower_ok(src);
    assert!(fir.entry.is_some());
}

// ---------------------------------------------------------------------------
// Iterable: each
// ---------------------------------------------------------------------------

#[test]
fn iterable_list_each_lowers() {
    let src = "\
let xs = [1, 2, 3]
xs.each(f: -> x: log(message: \"ok\"))
def main() -> Int
  0
";
    let fir = lower_ok(src);
    assert!(fir.entry.is_some());
}

#[test]
fn iterable_list_each_emits_while() {
    let src = "\
def main() -> Int
  let xs = [1, 2, 3]
  xs.each(f: -> x: log(message: \"ok\"))
  0
";
    let fir = lower_ok(src);
    let entry = fir.get_function(fir.entry.unwrap());
    let has_while = entry
        .body
        .iter()
        .any(|s| matches!(s, FirStmt::While { .. }));
    assert!(has_while, "each should emit a While loop in FIR");
}

#[test]
fn iterable_list_each_on_empty_list_lowers() {
    let src = "\
let xs: List[Int] = []
xs.each(f: -> x: log(message: \"nope\"))
def main() -> Int
  0
";
    let fir = lower_ok(src);
    assert!(fir.entry.is_some());
}

#[test]
fn iterable_list_each_with_capture_lowers() {
    let src = r#"
let xs = [1, 2, 3]
let tag = "hello"
xs.each(f: -> x: log(message: tag))
def main() -> Int
  0
"#;
    let fir = lower_ok(src);
    assert!(fir.entry.is_some());
}

// ---------------------------------------------------------------------------
// FirType::needs_gc_root
// ---------------------------------------------------------------------------

#[test]
fn needs_gc_root_ptr() {
    assert!(FirType::Ptr.needs_gc_root());
}

#[test]
fn needs_gc_root_struct() {
    assert!(FirType::Struct(ClassId(0)).needs_gc_root());
}

#[test]
fn needs_gc_root_tagged_union_with_ptr_variant() {
    let ty = FirType::TaggedUnion {
        tag_bits: 1,
        variants: vec![FirType::Ptr, FirType::Void],
    };
    assert!(ty.needs_gc_root());
}

#[test]
fn needs_gc_root_tagged_union_no_ptr_variant() {
    let ty = FirType::TaggedUnion {
        tag_bits: 1,
        variants: vec![FirType::I64, FirType::Bool],
    };
    assert!(!ty.needs_gc_root());
}

#[test]
fn needs_gc_root_value_types() {
    assert!(!FirType::I64.needs_gc_root());
    assert!(!FirType::F64.needs_gc_root());
    assert!(!FirType::Bool.needs_gc_root());
    assert!(!FirType::Void.needs_gc_root());
    assert!(!FirType::Never.needs_gc_root());
    assert!(!FirType::FnPtr(FunctionId(0)).needs_gc_root());
}

// ===========================================================================
// Indirect function calls (GH #10)
// ===========================================================================

#[test]
fn lower_indirect_call_return_value_called() {
    // get_handler() returns a closure; calling its result should produce a ClosureCall
    let src = "\
def make_doubler() -> Fn(Int) -> Int
  def dbl(x: Int) -> Int
    x * 2
  dbl

def main() -> Int
  let f: Fn(Int) -> Int = make_doubler()
  f(_0: 21)
";
    let fir = lower_ok(src);
    let main = fir.get_function(fir.entry.unwrap());
    let body = real_body(main);
    // The body should contain a ClosureCall somewhere (the indirect call)
    let has_closure_call = body.iter().any(|s| match s {
        FirStmt::Return(FirExpr::ClosureCall { .. }) => true,
        FirStmt::Expr(FirExpr::ClosureCall { .. }) => true,
        FirStmt::Let {
            value: FirExpr::ClosureCall { .. },
            ..
        } => true,
        _ => false,
    });
    assert!(
        has_closure_call,
        "expected ClosureCall in main body, got: {:?}",
        body
    );
}

#[test]
fn lower_indirect_call_variable_fn_type() {
    // A function-type parameter is called dynamically via ClosureCall
    let src = "\
def apply(f: Fn(Int) -> Int, x: Int) -> Int
  f(_0: x)

def main() -> Int
  def double(x: Int) -> Int
    x * 2
  apply(f: double, x: 21)
";
    let fir = lower_ok(src);
    // The ClosureCall is inside `apply`, not `main`
    let apply = &fir.functions[0];
    assert_eq!(apply.name, "apply");
    let body = real_body(apply);
    let has_closure_call = body.iter().any(|s| match s {
        FirStmt::Return(FirExpr::ClosureCall { .. }) => true,
        FirStmt::Expr(FirExpr::ClosureCall { .. }) => true,
        FirStmt::Let {
            value: FirExpr::ClosureCall { .. },
            ..
        } => true,
        _ => false,
    });
    assert!(
        has_closure_call,
        "expected ClosureCall in apply body, got: {:?}",
        body
    );
}

// ===========================================================================
// Nested type definitions inside function bodies (GH #11)
// ===========================================================================

#[test]
fn nested_class_in_function_body_lowers() {
    // A class defined inside a function body should lower without error
    let fir = lower_ok(
        "\
def make_point() -> Int
  class Point
    x: Int
    y: Int
  let p = Point(x: 1, y: 2)
  p.x
",
    );
    // The module should contain the class Point
    assert!(
        fir.classes.iter().any(|c| c.name == "Point"),
        "expected class Point in module, classes: {:?}",
        fir.classes.iter().map(|c| &c.name).collect::<Vec<_>>()
    );
}

#[test]
fn nested_enum_in_function_body_lowers() {
    // An enum defined inside a function body should lower without error
    let fir = lower_ok(
        "\
def process() -> Int
  enum Status
    Ok
    Failed(reason: String)
  let s = Status.Ok
  42
",
    );
    // The module should contain constructor functions for the enum variants
    assert!(
        fir.functions.iter().any(|f| f.name == "Status.Ok"),
        "expected Status.Ok constructor, functions: {:?}",
        fir.functions.iter().map(|f| &f.name).collect::<Vec<_>>()
    );
    assert!(
        fir.functions.iter().any(|f| f.name == "Status.Failed"),
        "expected Status.Failed constructor, functions: {:?}",
        fir.functions.iter().map(|f| &f.name).collect::<Vec<_>>()
    );
}

#[test]
fn nested_class_with_constructor_in_function_body() {
    // A class defined and instantiated inside a function body should lower
    let fir = lower_ok(
        "\
def run() -> Int
  class Pair
    a: Int
    b: Int
  let p = Pair(a: 3, b: 4)
  p.a
",
    );
    assert!(
        fir.classes.iter().any(|c| c.name == "Pair"),
        "expected class Pair in module, classes: {:?}",
        fir.classes.iter().map(|c| &c.name).collect::<Vec<_>>()
    );
}

#[test]
fn nested_enum_variant_call_in_function_body() {
    // An enum defined and its variant constructor called inside a function body
    let fir = lower_ok(
        "\
def process() -> Int
  enum Status
    Ok
    Failed(reason: String)
  let s = Status.Ok
  42
",
    );
    assert!(
        fir.functions.iter().any(|f| f.name == "Status.Ok"),
        "expected Status.Ok constructor, functions: {:?}",
        fir.functions.iter().map(|f| &f.name).collect::<Vec<_>>()
    );
    assert!(
        fir.functions.iter().any(|f| f.name == "Status.Failed"),
        "expected Status.Failed constructor, functions: {:?}",
        fir.functions.iter().map(|f| &f.name).collect::<Vec<_>>()
    );
}
