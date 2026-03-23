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

struct ValidationContext<'a> {
    func_name: &'a str,
    num_functions: usize,
    num_classes: usize,
    declared_locals: &'a HashSet<LocalId>,
    errors: &'a mut Vec<FirError>,
}

impl<'a> ValidationContext<'a> {
    fn check_func_id(&mut self, id: FunctionId) {
        if (id.0 as usize) >= self.num_functions {
            self.errors.push(FirError {
                function: self.func_name.into(),
                message: format!(
                    "references FunctionId({}) but only {} functions exist",
                    id.0, self.num_functions
                ),
            });
        }
    }

    fn validate_stmts(&mut self, stmts: &[FirStmt]) {
        for stmt in stmts {
            match stmt {
                FirStmt::Let { value, .. } => {
                    self.validate_expr(value);
                }
                FirStmt::Assign { value, target, .. } => {
                    self.validate_expr(value);
                    match target {
                        crate::stmts::FirPlace::Field { object, .. } => {
                            self.validate_expr(object);
                        }
                        crate::stmts::FirPlace::Index { list, index } => {
                            self.validate_expr(list);
                            self.validate_expr(index);
                        }
                        crate::stmts::FirPlace::MapIndex { map, key } => {
                            self.validate_expr(map);
                            self.validate_expr(key);
                        }
                        crate::stmts::FirPlace::Local(_) => {}
                    }
                }
                FirStmt::Return(expr) | FirStmt::Expr(expr) => {
                    self.validate_expr(expr);
                }
                FirStmt::If {
                    cond,
                    then_body,
                    else_body,
                } => {
                    self.validate_expr(cond);
                    self.validate_stmts(then_body);
                    self.validate_stmts(else_body);
                }
                FirStmt::While {
                    cond,
                    body,
                    increment,
                } => {
                    self.validate_expr(cond);
                    self.validate_stmts(body);
                    self.validate_stmts(increment);
                }
                FirStmt::Block(stmts) => {
                    self.validate_stmts(stmts);
                }
                FirStmt::Break | FirStmt::Continue => {}
            }
        }
    }

    fn validate_expr(&mut self, expr: &FirExpr) {
        match expr {
            FirExpr::Call { func, args, .. } => {
                self.check_func_id(*func);
                for arg in args {
                    self.validate_expr(arg);
                }
            }
            FirExpr::Spawn { func, args, .. } => {
                self.check_func_id(*func);
                for arg in args {
                    self.validate_expr(arg);
                }
            }
            FirExpr::BlockOn { func, args, .. } => {
                self.check_func_id(*func);
                for arg in args {
                    self.validate_expr(arg);
                }
            }
            FirExpr::ClosureCreate { func, env, .. } => {
                self.check_func_id(*func);
                self.validate_expr(env);
            }
            FirExpr::GlobalFunc(func) => {
                self.check_func_id(*func);
            }
            FirExpr::Construct { class, fields, .. } => {
                if (class.0 as usize) >= self.num_classes {
                    self.errors.push(FirError {
                        function: self.func_name.into(),
                        message: format!(
                            "references ClassId({}) but only {} classes exist",
                            class.0, self.num_classes
                        ),
                    });
                }
                for f in fields {
                    self.validate_expr(f);
                }
            }
            FirExpr::LocalVar(id, _) => {
                if !self.declared_locals.contains(id) {
                    self.errors.push(FirError {
                        function: self.func_name.into(),
                        message: format!("references undeclared LocalId({})", id.0),
                    });
                }
            }
            FirExpr::BinaryOp { left, right, .. } => {
                self.validate_expr(left);
                self.validate_expr(right);
            }
            FirExpr::UnaryOp { operand, .. } => {
                self.validate_expr(operand);
            }
            FirExpr::RuntimeCall { args, .. } => {
                for arg in args {
                    self.validate_expr(arg);
                }
            }
            FirExpr::ClosureCall { closure, args, .. } => {
                self.validate_expr(closure);
                for arg in args {
                    self.validate_expr(arg);
                }
            }
            FirExpr::FieldGet { object, .. } | FirExpr::FieldSet { object, .. } => {
                self.validate_expr(object);
                if let FirExpr::FieldSet { value, .. } = expr {
                    self.validate_expr(value);
                }
            }
            FirExpr::ListNew { elements, .. } => {
                for elem in elements {
                    self.validate_expr(elem);
                }
            }
            FirExpr::ListGet { list, index, .. } => {
                self.validate_expr(list);
                self.validate_expr(index);
            }
            FirExpr::ListSet {
                list, index, value, ..
            } => {
                self.validate_expr(list);
                self.validate_expr(index);
                self.validate_expr(value);
            }
            FirExpr::TagWrap { value, .. }
            | FirExpr::TagUnwrap { value, .. }
            | FirExpr::TagCheck { value, .. } => {
                self.validate_expr(value);
            }
            FirExpr::ResolveTask { task, .. }
            | FirExpr::CancelTask { task }
            | FirExpr::WaitCancel { task } => {
                self.validate_expr(task);
            }
            FirExpr::EnvLoad { env, .. } => {
                self.validate_expr(env);
            }
            FirExpr::IntToFloat(inner) | FirExpr::Bitcast { value: inner, .. } => {
                self.validate_expr(inner);
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

        let mut ctx = ValidationContext {
            func_name: &func.name,
            num_functions,
            num_classes,
            declared_locals: &declared_locals,
            errors: &mut errors,
        };
        ctx.validate_stmts(&func.body);
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
