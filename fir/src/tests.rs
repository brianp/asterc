use crate::exprs::{BinOp, FirExpr, UnaryOp};
use crate::lower::Lowerer;
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
    let mut lowerer = Lowerer::new(tc.env);
    lowerer.lower_module(&module).expect("lower ok");
    lowerer.finish()
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

    // Find the expression (could be Expr or Return)
    let expr = func
        .body
        .iter()
        .find_map(|s| match s {
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
    match &func.body[0] {
        FirStmt::Return(FirExpr::IntLit(42)) => {}
        other => panic!("expected Return(IntLit(42)), got {:?}", other),
    }
}

#[test]
fn lower_unary_negation() {
    let fir = lower_ok("def f() -> Int\n  return -5\n");
    let func = &fir.functions[0];
    match &func.body[0] {
        FirStmt::Return(FirExpr::UnaryOp {
            op: UnaryOp::Neg,
            operand,
            result_ty,
        }) => {
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
    match &func.body[0] {
        FirStmt::Return(FirExpr::UnaryOp {
            op: UnaryOp::Not,
            operand,
            result_ty,
        }) => {
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
    assert!(func.body.len() >= 2);

    // First statement: Let x = 42
    match &func.body[0] {
        FirStmt::Let { ty, value, .. } => {
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
        FirStmt::While { cond, body } => {
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
    match &fir.functions[0].body[0] {
        FirStmt::Return(FirExpr::IntLit(42)) => {}
        other => panic!("expected IntLit(42), got {:?}", other),
    }
}

#[test]
#[allow(clippy::approx_constant)]
fn lower_float_literal() {
    let fir = lower_ok("def f() -> Float\n  return 3.14\n");
    match &fir.functions[0].body[0] {
        FirStmt::Return(FirExpr::FloatLit(f)) => {
            assert!((f - 3.14).abs() < f64::EPSILON);
        }
        other => panic!("expected FloatLit, got {:?}", other),
    }
}

#[test]
fn lower_bool_literals() {
    let fir = lower_ok("def f() -> Bool\n  return true\n");
    match &fir.functions[0].body[0] {
        FirStmt::Return(FirExpr::BoolLit(true)) => {}
        other => panic!("expected BoolLit(true), got {:?}", other),
    }
}

#[test]
fn lower_string_literal() {
    let fir = lower_ok("def f() -> String\n  return \"hello\"\n");
    match &fir.functions[0].body[0] {
        FirStmt::Return(FirExpr::StringLit(s)) => {
            assert_eq!(s, "hello");
        }
        other => panic!("expected StringLit, got {:?}", other),
    }
}

#[test]
fn lower_nil_literal() {
    let fir = lower_ok("def f() -> Void\n  return nil\n");
    match &fir.functions[0].body[0] {
        FirStmt::Return(FirExpr::NilLit) => {}
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
    assert_eq!(func.body.len(), 3); // let, assign, return

    match &func.body[1] {
        FirStmt::Assign {
            target: FirPlace::Local(id),
            ..
        } => {
            // The LocalId should match the one from the let binding
            let let_id = match &func.body[0] {
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
    let mut lowerer = Lowerer::new(env);

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
    // 6 let bindings + 1 expr
    assert!(func.body.len() >= 7);

    // Each let should have a BinaryOp with Bool result type
    for i in 0..6 {
        match &func.body[i] {
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
    // For loop is lowered to If(true, [setup..., While { ... }], [])
    let has_for_desugared = func.body.iter().any(|s| match s {
        FirStmt::If {
            cond: FirExpr::BoolLit(true),
            then_body,
            ..
        } => then_body.iter().any(|s| matches!(s, FirStmt::While { .. })),
        _ => false,
    });
    assert!(
        has_for_desugared,
        "for loop should desugar to If(true, [setup, While]): {:?}",
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
// Milestone 11: Async — task handles (eager execution for now)
// ===========================================================================

#[test]
fn lower_async_call_to_eager_exec() {
    let src = "\
def fetch() -> Int
  42

def main() throws CancelledError -> Int
  let t: Task[Int] = async fetch()
  resolve t!
";
    let fir = lower_ok(src);
    let main_func = fir.functions.iter().find(|f| f.name == "main").unwrap();
    // async should lower to an eager call (no true concurrency yet)
    // resolve should lower to reading the already-computed result
    assert!(!main_func.body.is_empty());
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
