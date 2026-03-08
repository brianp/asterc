use ast::{BinOp, Expr, MatchPattern, Type, UnaryOp};

use crate::typechecker::TypeChecker;

impl TypeChecker {
    pub fn check_expr(&mut self, expr: &Expr) -> Result<Type, String> {
        match expr {
            Expr::Int(_) => Ok(Type::Int),
            Expr::Float(_) => Ok(Type::Float),
            Expr::Str(_) => Ok(Type::String),
            Expr::Bool(_) => Ok(Type::Bool),
            Expr::Nil => Ok(Type::Nil),

            Expr::Ident(name) => self
                .env
                .get_var(name)
                .ok_or_else(|| format!("Unknown identifier '{}'", name)),

            Expr::Lambda {
                params,
                ret_type,
                body,
                is_async,
                generic_params,
                throws,
            } => self.check_lambda(params, ret_type, body, *is_async, generic_params, throws),
            Expr::Call { func, args } => self.check_call(func, args),
            Expr::BinaryOp { left, op, right } => self.check_binary(left, op, right),
            Expr::UnaryOp { op, operand } => self.check_unary(op, operand),
            Expr::Member { object, field } => self.check_member(object, field),
            Expr::ListLiteral(elems) => self.check_list_literal(elems),
            Expr::Index { object, index } => self.check_index(object, index),
            Expr::Match { scrutinee, arms } => self.check_match_expr(scrutinee, arms),
            Expr::AsyncCall { func, args } => self.check_async_call(func, args),
            Expr::ResolveCall { func, args } => self.check_resolve_call(func, args),
            Expr::DetachedCall { func, args } => self.check_detached_call(func, args),
            Expr::Propagate(inner) => self.check_propagate(inner),
            Expr::Throw(value) => self.check_throw(value),
            Expr::ErrorOr { expr, default } => self.check_error_or(expr, default),
            Expr::ErrorOrElse { expr, handler } => self.check_error_or_else(expr, handler),
            Expr::ErrorCatch { expr, arms } => self.check_error_catch(expr, arms),
            Expr::AsyncScope { body } => self.check_async_scope(body),
        }
    }

    fn check_lambda(
        &mut self,
        params: &[(String, Type)],
        ret_type: &Type,
        body: &[ast::Stmt],
        is_async: bool,
        _generic_params: &Option<Vec<String>>,
        throws: &Option<Type>,
    ) -> Result<Type, String> {
        let mut sub = self.child_checker();
        sub.is_async_context = is_async;
        sub.throws_type = throws.clone();
        if *ret_type != Type::Void {
            sub.expected_return_type = Some(ret_type.clone());
        }
        let mut param_types = Vec::new();
        for (n, t) in params {
            sub.env.set_var(n.clone(), t.clone());
            param_types.push(t.clone());
        }

        let is_abstract = body.is_empty();

        let mut last = Type::Void;
        for s in body {
            // Validate return statements against declared return type
            if let ast::Stmt::Return(expr) = s {
                let ret_val_ty = sub.check_expr(expr)?;
                if *ret_type != Type::Void && ret_val_ty != *ret_type && ret_val_ty != Type::Never
                    && !Self::is_nullable_compatible(ret_type, &ret_val_ty) {
                    return Err(format!(
                        "Return type mismatch: expected {:?}, got {:?}",
                        ret_type, ret_val_ty
                    ));
                }
                last = ret_val_ty;
            } else {
                last = sub.check_stmt(s)?;
            }
        }
        if !is_abstract && &last != ret_type && *ret_type != Type::Void && last != Type::Never {
            // Allow T or Nil to be returned where T? is expected
            if let Type::Nullable(inner) = ret_type {
                if last != **inner && last != Type::Nil {
                    return Err(format!(
                        "Lambda return type mismatch: expected {:?}, got {:?}",
                        ret_type, last
                    ));
                }
            } else {
                return Err(format!(
                    "Lambda return type mismatch: expected {:?}, got {:?}",
                    ret_type, last
                ));
            }
        }
        Ok(Type::Function {
            params: param_types,
            ret: Box::new(ret_type.clone()),
            is_async,
            throws: throws.clone().map(Box::new),
        })
    }

    fn check_binary(&mut self, left: &Expr, op: &BinOp, right: &Expr) -> Result<Type, String> {
        let lt = self.check_expr(left)?;
        let rt = self.check_expr(right)?;
        match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod | BinOp::Pow => {
                if *op == BinOp::Add && lt == Type::String && rt == Type::String {
                    return Ok(Type::String);
                }
                match (&lt, &rt) {
                    (Type::Int, Type::Int) => Ok(Type::Int),
                    (Type::Float, Type::Float) => Ok(Type::Float),
                    (Type::Int, Type::Float) | (Type::Float, Type::Int) => Ok(Type::Float),
                    _ => Err(format!("Cannot apply {:?} to {:?} and {:?}", op, lt, rt)),
                }
            }
            BinOp::Eq | BinOp::Neq => {
                if lt == rt {
                    if matches!(&lt, Type::Function { .. }) {
                        return Err(format!("Cannot compare function types with {:?}", op));
                    }
                    return Ok(Type::Bool);
                }
                match (&lt, &rt) {
                    (Type::Int, Type::Float) | (Type::Float, Type::Int) => Ok(Type::Bool),
                    _ => Err(format!(
                        "Cannot compare {:?} and {:?} with {:?}",
                        lt, rt, op
                    )),
                }
            }
            BinOp::Lt | BinOp::Gt | BinOp::Lte | BinOp::Gte => {
                match (&lt, &rt) {
                    (Type::Int, Type::Int) | (Type::Float, Type::Float)
                    | (Type::Int, Type::Float) | (Type::Float, Type::Int)
                    | (Type::String, Type::String) => Ok(Type::Bool),
                    _ => Err(format!(
                        "Cannot order {:?} and {:?} with {:?}",
                        lt, rt, op
                    )),
                }
            }
            BinOp::And | BinOp::Or => {
                if lt == Type::Bool && rt == Type::Bool {
                    Ok(Type::Bool)
                } else {
                    Err(format!(
                        "Logical {:?} requires Bool operands, got {:?} and {:?}",
                        op, lt, rt
                    ))
                }
            }
        }
    }

    fn check_unary(&mut self, op: &UnaryOp, operand: &Expr) -> Result<Type, String> {
        let t = self.check_expr(operand)?;
        match op {
            UnaryOp::Neg => match t {
                Type::Int => Ok(Type::Int),
                Type::Float => Ok(Type::Float),
                _ => Err(format!("Cannot negate {:?}", t)),
            },
            UnaryOp::Not => {
                if t == Type::Bool {
                    Ok(Type::Bool)
                } else {
                    Err(format!("Cannot apply 'not' to {:?}", t))
                }
            }
        }
    }

    fn check_list_literal(&mut self, elems: &[Expr]) -> Result<Type, String> {
        if elems.is_empty() {
            return Ok(Type::List(Box::new(Type::Nil)));
        }
        let first_ty = self.check_expr(&elems[0])?;
        for (i, elem) in elems.iter().enumerate().skip(1) {
            let ty = self.check_expr(elem)?;
            if ty != first_ty {
                return Err(format!(
                    "List element {} has type {:?}, expected {:?} (all elements must have consistent type)",
                    i, ty, first_ty
                ));
            }
        }
        Ok(Type::List(Box::new(first_ty)))
    }

    fn check_index(&mut self, object: &Expr, index: &Expr) -> Result<Type, String> {
        let obj_ty = self.check_expr(object)?;
        let idx_ty = self.check_expr(index)?;
        if idx_ty != Type::Int {
            return Err(format!("List index must be Int, got {:?}", idx_ty));
        }
        match obj_ty {
            Type::List(inner) => Ok(*inner),
            _ => Err(format!("Cannot index into {:?}, expected List", obj_ty)),
        }
    }

    pub(crate) fn check_member(&mut self, object: &Expr, field: &str) -> Result<Type, String> {
        use std::collections::HashMap;
        let obj_ty = self.check_expr(object)?;
        // Nullable types only allow .or(), .or_else(), .or_throw() (handled in check_call)
        if let Type::Nullable(_) = &obj_ty {
            // Return a sentinel — actual checking happens in check_call
            // If we get here, it's a non-call member access on nullable, which is fine
            // for the method lookup (the call checker will handle it)
            // But direct field access is not allowed
            if field == "or" || field == "or_else" || field == "or_throw" {
                // Return a placeholder that check_call will handle
                return Ok(Type::Void);
            }
            return Err(format!(
                "Cannot access '{}' on nullable type {:?}. Resolve with .or(), .or_else(), .or_throw(), or match first",
                field, obj_ty
            ));
        }
        if let Type::Custom(class_name, type_args) = obj_ty {
            // Walk the class hierarchy (own class + extends chain) looking for the field/method
            let mut current_class = Some(class_name.clone());
            while let Some(ref cname) = current_class {
                if let Some(info) = self.env.get_class(cname) {
                    let bindings: HashMap<String, Type> = info.generic_params
                        .as_ref()
                        .map(|gp| gp.iter().zip(type_args.iter()).map(|(p, t)| (p.clone(), t.clone())).collect())
                        .unwrap_or_default();
                    if let Some(t) = info.fields.get(field) {
                        let resolved = Self::substitute_typevars(t, &bindings);
                        return Ok(resolved);
                    }
                    if let Some(t) = info.methods.get(field) {
                        let resolved = Self::substitute_typevars(t, &bindings);
                        return Ok(resolved);
                    }
                    current_class = info.extends.clone();
                } else {
                    return Err(format!("Unknown class '{}'", cname));
                }
            }
            Err(format!(
                "Class '{}' has no field or method '{}'",
                class_name, field
            ))
        } else {
            Err(format!("Cannot access member '{}' on {:?}", field, obj_ty))
        }
    }

    fn check_match_expr(
        &mut self,
        scrutinee: &Expr,
        arms: &[(MatchPattern, Expr)],
    ) -> Result<Type, String> {
        let scrutinee_ty = self.check_expr(scrutinee)?;
        if arms.is_empty() {
            return Err("Match expression must have at least one arm".to_string());
        }
        let mut result_ty: Option<Type> = None;
        for (pattern, value) in arms {
            self.check_match_pattern(pattern, &scrutinee_ty)?;
            // Bind ident patterns to the scrutinee type in a child scope
            // For nullable types, narrow T? to T in the non-nil arm
            let arm_ty = if let MatchPattern::Ident(name) = pattern {
                let mut sub = self.child_checker();
                let bind_ty = if let Type::Nullable(inner) = &scrutinee_ty {
                    *inner.clone()
                } else {
                    scrutinee_ty.clone()
                };
                sub.env.set_var(name.clone(), bind_ty);
                sub.check_expr(value)?
            } else {
                self.check_expr(value)?
            };
            // Never (diverging) arms are compatible with any type
            if arm_ty == Type::Never {
                continue;
            }
            if let Some(ref expected) = result_ty {
                if *expected == Type::Never {
                    // Previous arms all diverged; adopt this arm's type
                    result_ty = Some(arm_ty);
                } else if arm_ty != *expected {
                    return Err(format!(
                        "Match arm type mismatch: expected {:?}, got {:?}",
                        expected, arm_ty
                    ));
                }
            } else {
                result_ty = Some(arm_ty);
            }
        }

        // Exhaustiveness check
        let has_catchall = arms.iter().any(|(p, _)| matches!(p, MatchPattern::Wildcard | MatchPattern::Ident(_)));
        if !has_catchall {
            match &scrutinee_ty {
                Type::Bool => {
                    let has_true = arms.iter().any(|(p, _)| matches!(p, MatchPattern::Literal(Expr::Bool(true))));
                    let has_false = arms.iter().any(|(p, _)| matches!(p, MatchPattern::Literal(Expr::Bool(false))));
                    if !has_true || !has_false {
                        return Err("Non-exhaustive match: Bool match must cover both true and false, or include a wildcard".to_string());
                    }
                }
                _ => {
                    return Err("Non-exhaustive match: must include a wildcard '_' or variable pattern as catch-all".to_string());
                }
            }
        }

        Ok(result_ty.unwrap_or(Type::Void))
    }

    fn check_async_call(&mut self, func: &Expr, args: &[Expr]) -> Result<Type, String> {
        // Check that we're in an async context
        if !self.is_in_async_context() {
            return Err("Cannot use 'async' call outside of async context".to_string());
        }
        let ret_ty = self.check_call_inner(func, args, true)?;
        // async f() returns Task[T] where T is the return type
        Ok(Type::Task(Box::new(ret_ty)))
    }

    fn check_resolve_call(&mut self, func: &Expr, args: &[Expr]) -> Result<Type, String> {
        // resolve suspends until the async function completes — bypasses async context check
        self.check_call_inner(func, args, true)
    }

    fn check_detached_call(&mut self, func: &Expr, args: &[Expr]) -> Result<Type, String> {
        // detached async fires and forgets — returns Void
        self.check_call_inner(func, args, true)?;
        Ok(Type::Void)
    }

    fn check_async_scope(&mut self, body: &[ast::Stmt]) -> Result<Type, String> {
        let mut sub = self.child_checker();
        sub.is_async_context = true;
        sub.check_body(body)
    }
}
