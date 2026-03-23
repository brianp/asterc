use std::collections::HashSet;

use crate::exprs::FirExpr;
use crate::module::FirModule;
use crate::stmts::FirStmt;
use crate::types::{FunctionId, LocalId};

/// Validation errors found in a FIR module.
#[derive(Debug)]
pub struct FirError {
    pub function: String,
    pub message: String,
}

impl std::fmt::Display for FirError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "FIR validation error in {}: {}",
            self.function, self.message
        )
    }
}

/// Validate a FIR module's structural invariants before codegen.
///
/// Checks:
/// - No placeholder functions (empty name with non-empty body)
/// - All Call/Spawn/BlockOn/ClosureCreate/GlobalFunc reference valid FunctionIds
/// - All Construct reference valid ClassIds
/// - Entry function (if set) references a valid FunctionId
/// - LocalVar ids are declared before use in each function
pub fn validate(module: &FirModule) -> Vec<FirError> {
    let mut errors = Vec::new();
    let num_functions = module.functions.len();
    let num_classes = module.classes.len();

    // Check entry
    if let Some(entry) = module.entry
        && (entry.0 as usize) >= num_functions
    {
        errors.push(FirError {
            function: "<module>".into(),
            message: format!(
                "entry FunctionId({}) out of range (have {} functions)",
                entry.0, num_functions
            ),
        });
    }

    for func in &module.functions {
        // Skip placeholders (empty name, empty body — from out-of-order insertion)
        if func.name.is_empty() && func.body.is_empty() {
            continue;
        }
        // Placeholder with body is a bug
        if func.name.is_empty() && !func.body.is_empty() {
            errors.push(FirError {
                function: format!("FunctionId({})", func.id.0),
                message: "placeholder function has non-empty body".into(),
            });
            continue;
        }

        // Collect declared locals: params + Let bindings
        let mut declared_locals = HashSet::new();
        for (i, _) in func.params.iter().enumerate() {
            declared_locals.insert(LocalId(i as u32));
        }
        collect_declared_locals(&func.body, &mut declared_locals);

        // Validate expressions in body
        validate_stmts(
            &func.body,
            &func.name,
            num_functions,
            num_classes,
            &declared_locals,
            &mut errors,
        );
    }

    errors
}

fn collect_declared_locals(stmts: &[FirStmt], locals: &mut HashSet<LocalId>) {
    for stmt in stmts {
        match stmt {
            FirStmt::Let { name, .. } => {
                locals.insert(*name);
            }
            FirStmt::If {
                then_body,
                else_body,
                ..
            } => {
                collect_declared_locals(then_body, locals);
                collect_declared_locals(else_body, locals);
            }
            FirStmt::While {
                body, increment, ..
            } => {
                collect_declared_locals(body, locals);
                collect_declared_locals(increment, locals);
            }
            FirStmt::Block(stmts) => {
                collect_declared_locals(stmts, locals);
            }
            _ => {}
        }
    }
}

fn validate_stmts(
    stmts: &[FirStmt],
    func_name: &str,
    num_functions: usize,
    num_classes: usize,
    declared_locals: &HashSet<LocalId>,
    errors: &mut Vec<FirError>,
) {
    for stmt in stmts {
        match stmt {
            FirStmt::Let { value, .. } => {
                validate_expr(
                    value,
                    func_name,
                    num_functions,
                    num_classes,
                    declared_locals,
                    errors,
                );
            }
            FirStmt::Assign { value, target, .. } => {
                validate_expr(
                    value,
                    func_name,
                    num_functions,
                    num_classes,
                    declared_locals,
                    errors,
                );
                match target {
                    crate::stmts::FirPlace::Field { object, .. } => {
                        validate_expr(
                            object,
                            func_name,
                            num_functions,
                            num_classes,
                            declared_locals,
                            errors,
                        );
                    }
                    crate::stmts::FirPlace::Index { list, index } => {
                        validate_expr(
                            list,
                            func_name,
                            num_functions,
                            num_classes,
                            declared_locals,
                            errors,
                        );
                        validate_expr(
                            index,
                            func_name,
                            num_functions,
                            num_classes,
                            declared_locals,
                            errors,
                        );
                    }
                    crate::stmts::FirPlace::MapIndex { map, key } => {
                        validate_expr(
                            map,
                            func_name,
                            num_functions,
                            num_classes,
                            declared_locals,
                            errors,
                        );
                        validate_expr(
                            key,
                            func_name,
                            num_functions,
                            num_classes,
                            declared_locals,
                            errors,
                        );
                    }
                    crate::stmts::FirPlace::Local(_) => {}
                }
            }
            FirStmt::Return(expr) | FirStmt::Expr(expr) => {
                validate_expr(
                    expr,
                    func_name,
                    num_functions,
                    num_classes,
                    declared_locals,
                    errors,
                );
            }
            FirStmt::If {
                cond,
                then_body,
                else_body,
            } => {
                validate_expr(
                    cond,
                    func_name,
                    num_functions,
                    num_classes,
                    declared_locals,
                    errors,
                );
                validate_stmts(
                    then_body,
                    func_name,
                    num_functions,
                    num_classes,
                    declared_locals,
                    errors,
                );
                validate_stmts(
                    else_body,
                    func_name,
                    num_functions,
                    num_classes,
                    declared_locals,
                    errors,
                );
            }
            FirStmt::While {
                cond,
                body,
                increment,
            } => {
                validate_expr(
                    cond,
                    func_name,
                    num_functions,
                    num_classes,
                    declared_locals,
                    errors,
                );
                validate_stmts(
                    body,
                    func_name,
                    num_functions,
                    num_classes,
                    declared_locals,
                    errors,
                );
                validate_stmts(
                    increment,
                    func_name,
                    num_functions,
                    num_classes,
                    declared_locals,
                    errors,
                );
            }
            FirStmt::Block(stmts) => {
                validate_stmts(
                    stmts,
                    func_name,
                    num_functions,
                    num_classes,
                    declared_locals,
                    errors,
                );
            }
            FirStmt::Break | FirStmt::Continue => {}
        }
    }
}

fn check_func_id(
    id: FunctionId,
    func_name: &str,
    num_functions: usize,
    errors: &mut Vec<FirError>,
) {
    if (id.0 as usize) >= num_functions {
        errors.push(FirError {
            function: func_name.into(),
            message: format!(
                "references FunctionId({}) but only {} functions exist",
                id.0, num_functions
            ),
        });
    }
}

fn validate_expr(
    expr: &FirExpr,
    func_name: &str,
    num_functions: usize,
    num_classes: usize,
    declared_locals: &HashSet<LocalId>,
    errors: &mut Vec<FirError>,
) {
    match expr {
        FirExpr::Call { func, args, .. } => {
            check_func_id(*func, func_name, num_functions, errors);
            for arg in args {
                validate_expr(
                    arg,
                    func_name,
                    num_functions,
                    num_classes,
                    declared_locals,
                    errors,
                );
            }
        }
        FirExpr::Spawn { func, args, .. } => {
            check_func_id(*func, func_name, num_functions, errors);
            for arg in args {
                validate_expr(
                    arg,
                    func_name,
                    num_functions,
                    num_classes,
                    declared_locals,
                    errors,
                );
            }
        }
        FirExpr::BlockOn { func, args, .. } => {
            check_func_id(*func, func_name, num_functions, errors);
            for arg in args {
                validate_expr(
                    arg,
                    func_name,
                    num_functions,
                    num_classes,
                    declared_locals,
                    errors,
                );
            }
        }
        FirExpr::ClosureCreate { func, env, .. } => {
            check_func_id(*func, func_name, num_functions, errors);
            validate_expr(
                env,
                func_name,
                num_functions,
                num_classes,
                declared_locals,
                errors,
            );
        }
        FirExpr::GlobalFunc(func) => {
            check_func_id(*func, func_name, num_functions, errors);
        }
        FirExpr::Construct { class, fields, .. } => {
            if (class.0 as usize) >= num_classes {
                errors.push(FirError {
                    function: func_name.into(),
                    message: format!(
                        "references ClassId({}) but only {} classes exist",
                        class.0, num_classes
                    ),
                });
            }
            for f in fields {
                validate_expr(
                    f,
                    func_name,
                    num_functions,
                    num_classes,
                    declared_locals,
                    errors,
                );
            }
        }
        FirExpr::LocalVar(id, _) => {
            if !declared_locals.contains(id) {
                errors.push(FirError {
                    function: func_name.into(),
                    message: format!("references undeclared LocalId({})", id.0),
                });
            }
        }
        // Recurse into sub-expressions
        FirExpr::BinaryOp { left, right, .. } => {
            validate_expr(
                left,
                func_name,
                num_functions,
                num_classes,
                declared_locals,
                errors,
            );
            validate_expr(
                right,
                func_name,
                num_functions,
                num_classes,
                declared_locals,
                errors,
            );
        }
        FirExpr::UnaryOp { operand, .. } => {
            validate_expr(
                operand,
                func_name,
                num_functions,
                num_classes,
                declared_locals,
                errors,
            );
        }
        FirExpr::RuntimeCall { args, .. } => {
            for arg in args {
                validate_expr(
                    arg,
                    func_name,
                    num_functions,
                    num_classes,
                    declared_locals,
                    errors,
                );
            }
        }
        FirExpr::ClosureCall { closure, args, .. } => {
            validate_expr(
                closure,
                func_name,
                num_functions,
                num_classes,
                declared_locals,
                errors,
            );
            for arg in args {
                validate_expr(
                    arg,
                    func_name,
                    num_functions,
                    num_classes,
                    declared_locals,
                    errors,
                );
            }
        }
        FirExpr::FieldGet { object, .. } | FirExpr::FieldSet { object, .. } => {
            validate_expr(
                object,
                func_name,
                num_functions,
                num_classes,
                declared_locals,
                errors,
            );
            if let FirExpr::FieldSet { value, .. } = expr {
                validate_expr(
                    value,
                    func_name,
                    num_functions,
                    num_classes,
                    declared_locals,
                    errors,
                );
            }
        }
        FirExpr::ListNew { elements, .. } => {
            for elem in elements {
                validate_expr(
                    elem,
                    func_name,
                    num_functions,
                    num_classes,
                    declared_locals,
                    errors,
                );
            }
        }
        FirExpr::ListGet { list, index, .. } => {
            validate_expr(
                list,
                func_name,
                num_functions,
                num_classes,
                declared_locals,
                errors,
            );
            validate_expr(
                index,
                func_name,
                num_functions,
                num_classes,
                declared_locals,
                errors,
            );
        }
        FirExpr::ListSet {
            list, index, value, ..
        } => {
            validate_expr(
                list,
                func_name,
                num_functions,
                num_classes,
                declared_locals,
                errors,
            );
            validate_expr(
                index,
                func_name,
                num_functions,
                num_classes,
                declared_locals,
                errors,
            );
            validate_expr(
                value,
                func_name,
                num_functions,
                num_classes,
                declared_locals,
                errors,
            );
        }
        FirExpr::TagWrap { value, .. }
        | FirExpr::TagUnwrap { value, .. }
        | FirExpr::TagCheck { value, .. } => {
            validate_expr(
                value,
                func_name,
                num_functions,
                num_classes,
                declared_locals,
                errors,
            );
        }
        FirExpr::ResolveTask { task, .. }
        | FirExpr::CancelTask { task }
        | FirExpr::WaitCancel { task } => {
            validate_expr(
                task,
                func_name,
                num_functions,
                num_classes,
                declared_locals,
                errors,
            );
        }
        FirExpr::EnvLoad { env, .. } => {
            validate_expr(
                env,
                func_name,
                num_functions,
                num_classes,
                declared_locals,
                errors,
            );
        }
        FirExpr::IntToFloat(inner) | FirExpr::Bitcast { value: inner, .. } => {
            validate_expr(
                inner,
                func_name,
                num_functions,
                num_classes,
                declared_locals,
                errors,
            );
        }
        // Literals have no sub-expressions
        FirExpr::IntLit(_)
        | FirExpr::FloatLit(_)
        | FirExpr::BoolLit(_)
        | FirExpr::StringLit(_)
        | FirExpr::NilLit
        | FirExpr::Safepoint => {}
    }
}
