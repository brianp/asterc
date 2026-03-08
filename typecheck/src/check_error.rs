use ast::{Expr, Type};

use crate::typechecker::TypeChecker;

impl TypeChecker {
    pub(crate) fn check_propagate(&mut self, inner: &Expr) -> Result<Type, String> {
        if let Expr::Call { func, args } = inner {
            let fn_ty = self.resolve_func_type(func)?;
            if let Type::Function {
                throws: Some(ref err_ty),
                ..
            } = fn_ty
            {
                let caller_throws = self.throws_type.as_ref().ok_or_else(|| {
                    "Cannot use '!' to propagate errors outside of a function that declares 'throws'".to_string()
                })?;
                if !self.is_error_subtype(err_ty, caller_throws) {
                    return Err(format!(
                        "Cannot propagate {:?} — caller declares 'throws {:?}'",
                        err_ty, caller_throws
                    ));
                }
                return self.check_call_inner_throws_ok(func, args, false);
            }
        }
        Err("'!' can only be used on calls to functions that declare 'throws'".to_string())
    }

    pub(crate) fn check_error_or(&mut self, expr: &Expr, default: &Expr) -> Result<Type, String> {
        self.check_error_recovery(expr, default, "!.or() default")
    }

    pub(crate) fn check_error_or_else(
        &mut self,
        expr: &Expr,
        handler: &Expr,
    ) -> Result<Type, String> {
        self.check_error_recovery(expr, handler, "!.or_else() handler")
    }

    /// Shared logic for `!.or()` and `!.or_else()`: verify `expr` is a call to a
    /// throwing function, check that `fallback` has the same type as the success
    /// type, and return that type. `label` is used in the error message.
    pub(crate) fn check_error_recovery(
        &mut self,
        expr: &Expr,
        fallback: &Expr,
        label: &str,
    ) -> Result<Type, String> {
        let ret_ty = self.check_throwing_call_expr(expr)?;
        let fallback_ty = self.check_expr(fallback)?;
        if ret_ty != fallback_ty {
            return Err(format!(
                "{} type mismatch: expected {:?}, got {:?}",
                label, ret_ty, fallback_ty
            ));
        }
        Ok(ret_ty)
    }

    pub(crate) fn check_error_catch(
        &mut self,
        expr: &Expr,
        arms: &[(ast::ErrorCatchPattern, Expr)],
    ) -> Result<Type, String> {
        // Resolve the throws type for catch arm validation
        let throws_ty = if let Expr::Call { func, .. } = expr {
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
            return Err("!.catch must have at least one arm".to_string());
        }
        let mut result_ty: Option<Type> = None;
        for (pattern, value) in arms {
            let arm_ty = match pattern {
                ast::ErrorCatchPattern::Typed { error_type, var } => {
                    // Verify the error type exists as a class
                    if self.env.get_class(error_type).is_none() {
                        return Err(format!("Unknown error type '{}' in catch arm", error_type));
                    }
                    // Verify the caught type is a subtype of the thrown type
                    if let Some(ref thrown) = throws_ty {
                        let caught = Type::Custom(error_type.clone(), Vec::new());
                        if !self.is_error_subtype(&caught, thrown) {
                            return Err(format!(
                                "Catch arm type '{}' is not a subtype of thrown type {:?}",
                                error_type, thrown
                            ));
                        }
                    }
                    let mut sub = self.child_checker();
                    sub.env
                        .set_var(var.clone(), Type::Custom(error_type.clone(), Vec::new()));
                    sub.check_expr(value)?
                }
                ast::ErrorCatchPattern::Wildcard => self.check_expr(value)?,
            };
            if arm_ty == Type::Never {
                continue; // Diverging arms are compatible with anything
            }
            if let Some(ref expected) = result_ty {
                if arm_ty != *expected {
                    return Err(format!(
                        "!.catch arm type mismatch: expected {:?}, got {:?}",
                        expected, arm_ty
                    ));
                }
            } else {
                result_ty = Some(arm_ty);
            }
        }
        // If all arms diverge, use the success type
        Ok(result_ty.unwrap_or(ret_ty))
    }

    /// Check that an expression is a call to a throwing function and return its success type.
    pub(crate) fn check_throwing_call_expr(&mut self, expr: &Expr) -> Result<Type, String> {
        if let Expr::Call { func, args } = expr {
            let fn_ty = self.resolve_func_type(func)?;
            if let Type::Function {
                throws: Some(_), ..
            } = &fn_ty
            {
                return self.check_call_inner_throws_ok(func, args, false);
            }
        }
        Err(
            "!.or(), !.or_else(), and !.catch require a call to a function that declares 'throws'"
                .to_string(),
        )
    }

    pub(crate) fn check_throw(&mut self, value: &Expr) -> Result<Type, String> {
        let val_ty = self.check_expr(value)?;
        // Must be in a function that declares throws
        let throws_ty = self.throws_type.as_ref().ok_or_else(|| {
            "Cannot use 'throw' outside of a function that declares 'throws'".to_string()
        })?;
        // The thrown value must be compatible with the declared throws type
        // For now: exact match or extends check
        if !self.is_error_subtype(&val_ty, throws_ty) {
            return Err(format!(
                "Cannot throw {:?} — function declares 'throws {:?}'",
                val_ty, throws_ty
            ));
        }
        // throw diverges — return Never since control doesn't continue
        Ok(Type::Never)
    }
}
