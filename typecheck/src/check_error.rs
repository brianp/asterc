use ast::templates::DiagnosticTemplate;
use ast::templates::type_errors::{ErrorPropagation, TaskAlreadyConsumed};
use ast::{Diagnostic, Expr, Type};

use crate::typechecker::TypeChecker;

impl TypeChecker {
    /// Mark a task ident as consumed for must-consume tracking (non-failable).
    /// Used for returns, argument passing, and task consumption tracking.
    pub(crate) fn mark_task_ident_consumed(&mut self, expr: &Expr) {
        if let Expr::Ident(name, _) = expr
            && self.task_bindings.contains_key(name)
        {
            self.consumed_tasks.insert(name.clone());
        }
    }

    fn mark_task_consumed(&mut self, expr: &Expr) -> Result<(), Diagnostic> {
        let Expr::Ident(name, span) = expr else {
            return Ok(());
        };
        if !matches!(self.env.get_var(name), Some(Type::Task(_))) {
            return Ok(());
        }
        if self.consumed_tasks.insert(name.clone()) {
            return Ok(());
        }
        Err(
            Diagnostic::from_template(DiagnosticTemplate::TaskAlreadyConsumed(
                TaskAlreadyConsumed { name: name.clone() },
            ))
            .with_label(*span, "task handles are single-consumer"),
        )
    }

    pub(crate) fn check_propagate(&mut self, inner: &Expr) -> Result<Type, Diagnostic> {
        // Handle resolve expr! — CancelledError is implicit in task resolution.
        // Tasks are inherently cancellable; the user doesn't need to declare
        // `throws CancelledError` on their function just for resolving tasks.
        if let Expr::Resolve { expr, .. } = inner {
            let ty = self.check_expr(expr)?;
            if ty.is_error() {
                return Ok(Type::Error);
            }
            if let Type::Task(inner_ty) = ty {
                self.mark_task_consumed(expr)?;
                return Ok(*inner_ty);
            } else {
                return Err(
                    Diagnostic::from_template(DiagnosticTemplate::TaskAlreadyConsumed(
                        TaskAlreadyConsumed {
                            name: ty.to_string(),
                        },
                    ))
                    .with_label(expr.span(), "expected Task[T]"),
                );
            }
        }

        if let Expr::Call { func, args, .. } = inner {
            let fn_ty = self.resolve_func_type(func)?;
            if let Type::Function {
                throws: Some(ref err_ty),
                ..
            } = fn_ty
            {
                // CancelledError is implicit — it propagates without the caller
                // needing to declare `throws`. Only user-defined errors require it.
                let is_cancelled =
                    matches!(err_ty.as_ref(), Type::Custom(n, _) if n == "CancelledError");
                if !is_cancelled {
                    let caller_throws = self.throws_type.as_ref().ok_or_else(|| {
                        Diagnostic::from_template(DiagnosticTemplate::ErrorPropagation(ErrorPropagation {
                            message: "Cannot use '!' to propagate errors outside of a function that declares 'throws'".to_string(),
                        }))
                        .with_label(inner.span(), "propagation requires 'throws' declaration")
                    })?;
                    if !self.is_error_subtype(err_ty, caller_throws) {
                        return Err(Diagnostic::from_template(
                            DiagnosticTemplate::ErrorPropagation(ErrorPropagation {
                                message: format!(
                                    "Cannot propagate {} — caller declares 'throws {}'",
                                    err_ty, caller_throws
                                ),
                            }),
                        )
                        .with_label(inner.span(), "incompatible error type"));
                    }
                }
                return self.check_call_inner(func, args, true);
            }
        }
        Err(Diagnostic::from_template(DiagnosticTemplate::ErrorPropagation(ErrorPropagation {
            message: "'!' can only be used on calls to functions that declare 'throws', or on resolve expressions".to_string(),
        }))
        .with_label(inner.span(), "not a throwing call"))
    }

    pub(crate) fn check_error_or(
        &mut self,
        expr: &Expr,
        default: &Expr,
    ) -> Result<Type, Diagnostic> {
        self.check_error_recovery(expr, default, "!.or() default")
    }

    pub(crate) fn check_error_or_else(
        &mut self,
        expr: &Expr,
        handler: &Expr,
    ) -> Result<Type, Diagnostic> {
        self.check_error_recovery(expr, handler, "!.or_else() handler")
    }

    pub(crate) fn check_error_recovery(
        &mut self,
        expr: &Expr,
        fallback: &Expr,
        label: &str,
    ) -> Result<Type, Diagnostic> {
        let ret_ty = self.check_throwing_call_expr(expr)?;
        let fallback_ty = self.check_expr(fallback)?;
        if ret_ty.is_error() || fallback_ty.is_error() {
            return Ok(Type::Error);
        }
        if ret_ty != fallback_ty {
            return Err(
                Diagnostic::from_template(DiagnosticTemplate::ErrorPropagation(ErrorPropagation {
                    message: format!(
                        "{} type mismatch: expected {}, got {}",
                        label, ret_ty, fallback_ty
                    ),
                }))
                .with_label(fallback.span(), format!("expected {}", ret_ty)),
            );
        }
        Ok(ret_ty)
    }

    pub(crate) fn check_error_catch(
        &mut self,
        expr: &Expr,
        arms: &[(ast::ErrorCatchPattern, Expr)],
    ) -> Result<Type, Diagnostic> {
        let throws_ty = if let Expr::Resolve { .. } = expr {
            // resolve can throw CancelledError + any error from the original function.
            // Don't restrict catch arm types — any error type is valid.
            None
        } else if let Expr::Call { func, .. } = expr {
            let fn_ty = self.resolve_func_type(func)?;
            if let Type::Function {
                throws: Some(ref err_ty),
                ..
            } = fn_ty
            {
                Some(err_ty.clone())
            } else {
                None
            }
        } else {
            None
        };
        let ret_ty = self.check_throwing_call_expr(expr)?;
        if arms.is_empty() {
            return Err(
                Diagnostic::from_template(DiagnosticTemplate::ErrorPropagation(ErrorPropagation {
                    message: "!.catch must have at least one arm".to_string(),
                }))
                .with_label(expr.span(), "catch has no arms"),
            );
        }
        let mut result_ty: Option<Type> = None;
        for (pattern, value) in arms {
            let arm_ty = match pattern {
                ast::ErrorCatchPattern::Typed {
                    error_type,
                    var,
                    span,
                } => {
                    if self.env.get_class(error_type).is_none() {
                        return Err(Diagnostic::from_template(
                            DiagnosticTemplate::ErrorPropagation(ErrorPropagation {
                                message: format!(
                                    "Unknown error type '{}' in catch arm",
                                    error_type
                                ),
                            }),
                        )
                        .with_label(*span, "unknown error type"));
                    }
                    if let Some(ref thrown) = throws_ty {
                        let caught = Type::Custom(error_type.clone(), Vec::new());
                        if !self.is_error_subtype(&caught, thrown) {
                            return Err(Diagnostic::from_template(
                                DiagnosticTemplate::ErrorPropagation(ErrorPropagation {
                                    message: format!(
                                        "Catch arm type '{}' is not a subtype of thrown type {}",
                                        error_type, thrown
                                    ),
                                }),
                            )
                            .with_label(*span, "not a subtype of thrown error"));
                        }
                    }
                    let mut sub = self.child_checker();
                    sub.warn_if_shadowed(var, *span);
                    sub.env
                        .set_var(var.clone(), Type::Custom(error_type.clone(), Vec::new()));
                    let result = sub.check_expr(value);
                    self.restore_from_child(sub);
                    result?
                }
                ast::ErrorCatchPattern::Wildcard(_) => self.check_expr(value)?,
            };
            if arm_ty == Type::Never || arm_ty.is_error() {
                continue;
            }
            if let Some(ref expected) = result_ty {
                if arm_ty != *expected && !expected.is_error() {
                    return Err(
                        Diagnostic::from_template(DiagnosticTemplate::ErrorPropagation(
                            ErrorPropagation {
                                message: format!(
                                    "!.catch arm type mismatch: expected {}, got {}",
                                    expected, arm_ty
                                ),
                            },
                        ))
                        .with_label(value.span(), format!("expected {}", expected)),
                    );
                }
            } else {
                result_ty = Some(arm_ty);
            }
        }
        // Verify catch arm type matches success path type
        if let Some(ref catch_ty) = result_ty
            && *catch_ty != ret_ty
            && !catch_ty.is_error()
            && !ret_ty.is_error()
            && *catch_ty != Type::Never
        {
            return Err(
                Diagnostic::from_template(DiagnosticTemplate::ErrorPropagation(ErrorPropagation {
                    message: format!(
                        "!.catch arm type {} does not match success type {}",
                        catch_ty, ret_ty
                    ),
                }))
                .with_label(expr.span(), format!("success path returns {}", ret_ty)),
            );
        }
        Ok(result_ty.unwrap_or(ret_ty))
    }

    pub(crate) fn check_throwing_call_expr(&mut self, expr: &Expr) -> Result<Type, Diagnostic> {
        // Handle resolve expr for error recovery (e.g., resolve task!.or(default))
        if let Expr::Resolve { expr: inner, .. } = expr {
            let ty = self.check_expr(inner)?;
            if ty.is_error() {
                return Ok(Type::Error);
            }
            if let Type::Task(inner_ty) = ty {
                self.mark_task_ident_consumed(inner);
                return Ok(*inner_ty);
            } else {
                return Err(
                    Diagnostic::from_template(DiagnosticTemplate::TaskAlreadyConsumed(
                        TaskAlreadyConsumed {
                            name: ty.to_string(),
                        },
                    ))
                    .with_label(inner.span(), "expected Task[T]"),
                );
            }
        }

        if let Expr::Call { func, args, .. } = expr {
            let fn_ty = self.resolve_func_type(func)?;
            if let Type::Function {
                throws: Some(_), ..
            } = &fn_ty
            {
                return self.check_call_inner(func, args, true);
            }
        }
        Err(Diagnostic::from_template(DiagnosticTemplate::ErrorPropagation(ErrorPropagation {
            message: "!.or(), !.or_else(), and !.catch require a call to a function that declares 'throws' or a resolve expression".to_string(),
        }))
        .with_label(expr.span(), "not a throwing call"))
    }

    pub(crate) fn check_throw(&mut self, value: &Expr) -> Result<Type, Diagnostic> {
        let val_ty = self.check_expr(value)?;
        let throws_ty = self.throws_type.as_ref().ok_or_else(|| {
            Diagnostic::from_template(DiagnosticTemplate::ErrorPropagation(ErrorPropagation {
                message: "Cannot use 'throw' outside of a function that declares 'throws'"
                    .to_string(),
            }))
            .with_label(value.span(), "throw requires 'throws' declaration")
        })?;
        if !self.is_error_subtype(&val_ty, throws_ty) {
            return Err(
                Diagnostic::from_template(DiagnosticTemplate::ErrorPropagation(ErrorPropagation {
                    message: format!(
                        "Cannot throw {} — function declares 'throws {}'",
                        val_ty, throws_ty
                    ),
                }))
                .with_label(value.span(), "incompatible error type"),
            );
        }
        Ok(Type::Never)
    }
}
