use ast::{BinOp, Diagnostic, Expr, MatchPattern, Type, UnaryOp};

use crate::typechecker::TypeChecker;

impl TypeChecker {
    pub fn check_expr(&mut self, expr: &Expr) -> Result<Type, Diagnostic> {
        match expr {
            Expr::Int(..) => Ok(Type::Int),
            Expr::Float(..) => Ok(Type::Float),
            Expr::Str(..) => Ok(Type::String),
            Expr::Bool(..) => Ok(Type::Bool),
            Expr::Nil(_) => Ok(Type::Nil),

            Expr::Ident(name, span) => self.env.get_var(name).ok_or_else(|| {
                let mut diag = Diagnostic::error(format!("Unknown identifier '{}'", name))
                    .with_code("E002")
                    .with_label(*span, "not found in this scope");
                if let Some(suggestion) = self.suggest_similar_name(name) {
                    diag = diag.with_note(format!("did you mean '{}'?", suggestion));
                }
                diag
            }),

            Expr::Lambda {
                params,
                ret_type,
                body,
                generic_params,
                throws,
                ..
            } => self.check_lambda(params, ret_type, body, generic_params, throws),
            Expr::Call { func, args, .. } => self.check_call(func, args),
            Expr::BinaryOp {
                left, op, right, ..
            } => self.check_binary(left, op, right),
            Expr::UnaryOp { op, operand, .. } => self.check_unary(op, operand),
            Expr::Member { object, field, .. } => self.check_member(object, field),
            Expr::ListLiteral(elems, _) => self.check_list_literal(elems),
            Expr::Index { object, index, .. } => self.check_index(object, index),
            Expr::Match {
                scrutinee, arms, ..
            } => self.check_match_expr(scrutinee, arms),
            Expr::AsyncCall { func, args, .. } => self.check_async_call(func, args),
            Expr::Resolve { expr, .. } => self.check_resolve(expr),
            Expr::DetachedCall { func, args, .. } => self.check_detached_call(func, args),
            Expr::Propagate(inner, _) => self.check_propagate(inner),
            Expr::Throw(value, _) => self.check_throw(value),
            Expr::ErrorOr { expr, default, .. } => self.check_error_or(expr, default),
            Expr::ErrorOrElse { expr, handler, .. } => self.check_error_or_else(expr, handler),
            Expr::ErrorCatch { expr, arms, .. } => self.check_error_catch(expr, arms),
            Expr::AsyncScope { body, .. } => self.check_async_scope(body),
        }
    }

    fn check_lambda(
        &mut self,
        params: &[(String, Type)],
        ret_type: &Type,
        body: &[ast::Stmt],
        generic_params: &Option<Vec<String>>,
        throws: &Option<Type>,
    ) -> Result<Type, Diagnostic> {
        // Infer type parameters: collect unknown Custom(name, []) names from params.
        // If generic_params is already set (from [T] syntax on classes), use those.
        // Otherwise, auto-detect from param types.
        let inferred_type_params = if generic_params.is_some() {
            generic_params.clone().unwrap_or_default()
        } else {
            let mut type_param_names: Vec<String> = Vec::new();
            for (_, t) in params {
                self.collect_unknown_type_names(t, &mut type_param_names);
            }
            // Also scan return type for type params already found in params
            // (RFC rule: return types only reference previously declared type parameters)
            type_param_names
        };

        let mut sub = self.child_checker();
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
            if let ast::Stmt::Return(expr, _) = s {
                let ret_val_ty = sub.check_expr(expr)?;
                if *ret_type != Type::Void
                    && ret_val_ty != *ret_type
                    && ret_val_ty != Type::Never
                    && !ret_val_ty.is_error()
                    && !Self::is_nullable_compatible(ret_type, &ret_val_ty)
                {
                    return Err(Diagnostic::error(format!(
                        "Return type mismatch: expected {:?}, got {:?}",
                        ret_type, ret_val_ty
                    ))
                    .with_code("E004")
                    .with_label(expr.span(), format!("expected {:?}", ret_type)));
                }
                last = ret_val_ty;
            } else {
                last = sub.check_stmt(s)?;
            }
        }
        if !is_abstract
            && &last != ret_type
            && *ret_type != Type::Void
            && last != Type::Never
            && !last.is_error()
        {
            if let Type::Nullable(inner) = ret_type {
                if last != **inner && last != Type::Nil {
                    return Err(Diagnostic::error(format!(
                        "Lambda return type mismatch: expected {:?}, got {:?}",
                        ret_type, last
                    ))
                    .with_code("E004"));
                }
            } else {
                return Err(Diagnostic::error(format!(
                    "Lambda return type mismatch: expected {:?}, got {:?}",
                    ret_type, last
                ))
                .with_code("E004"));
            }
        }

        // Build the Function type. Convert inferred type params from Custom to TypeVar.
        let (final_params, final_ret) = if inferred_type_params.is_empty() {
            (param_types, ret_type.clone())
        } else {
            let fp = param_types
                .iter()
                .map(|t| Self::replace_custom_with_typevar(t, &inferred_type_params))
                .collect();
            let fr = Self::replace_custom_with_typevar(ret_type, &inferred_type_params);
            (fp, fr)
        };

        Ok(Type::Function {
            param_names: params.iter().map(|(n, _)| n.clone()).collect(),
            params: final_params,
            ret: Box::new(final_ret),
            throws: throws.clone().map(Box::new),
        })
    }

    /// Collect unknown type names from a type. A Custom(name, []) is "unknown" if
    /// it doesn't correspond to a known class, trait, or enum in the current environment.
    fn collect_unknown_type_names(&self, ty: &Type, out: &mut Vec<String>) {
        match ty {
            Type::Custom(name, args) if args.is_empty() => {
                if !self.is_known_type_name(name) && !out.contains(name) {
                    out.push(name.clone());
                }
            }
            Type::Custom(_, args) => {
                for a in args {
                    self.collect_unknown_type_names(a, out);
                }
            }
            Type::List(inner) | Type::Task(inner) | Type::Nullable(inner) => {
                self.collect_unknown_type_names(inner, out);
            }
            Type::Map(k, v) => {
                self.collect_unknown_type_names(k, out);
                self.collect_unknown_type_names(v, out);
            }
            Type::Function { params, ret, .. } => {
                for p in params {
                    self.collect_unknown_type_names(p, out);
                }
                self.collect_unknown_type_names(ret, out);
            }
            _ => {}
        }
    }

    /// Check if a type name is a known class, trait, enum, or "Self".
    fn is_known_type_name(&self, name: &str) -> bool {
        self.env.get_class(name).is_some()
            || self.env.get_trait(name).is_some()
            || self.env.get_enum(name).is_some()
            || name == "Self"
    }

    /// Replace Custom(name, []) with TypeVar(name) for names in the given type param list.
    fn replace_custom_with_typevar(ty: &Type, type_params: &[String]) -> Type {
        match ty {
            Type::Custom(name, args) if args.is_empty() && type_params.contains(name) => {
                Type::TypeVar(name.clone())
            }
            Type::Custom(name, args) => {
                let new_args = args
                    .iter()
                    .map(|a| Self::replace_custom_with_typevar(a, type_params))
                    .collect();
                Type::Custom(name.clone(), new_args)
            }
            Type::List(inner) => Type::List(Box::new(Self::replace_custom_with_typevar(
                inner,
                type_params,
            ))),
            Type::Map(k, v) => Type::Map(
                Box::new(Self::replace_custom_with_typevar(k, type_params)),
                Box::new(Self::replace_custom_with_typevar(v, type_params)),
            ),
            Type::Task(inner) => Type::Task(Box::new(Self::replace_custom_with_typevar(
                inner,
                type_params,
            ))),
            Type::Nullable(inner) => Type::Nullable(Box::new(Self::replace_custom_with_typevar(
                inner,
                type_params,
            ))),
            Type::Function {
                param_names,
                params,
                ret,
                throws,
            } => Type::Function {
                param_names: param_names.clone(),
                params: params
                    .iter()
                    .map(|p| Self::replace_custom_with_typevar(p, type_params))
                    .collect(),
                ret: Box::new(Self::replace_custom_with_typevar(ret, type_params)),
                throws: throws
                    .as_ref()
                    .map(|t| Box::new(Self::replace_custom_with_typevar(t, type_params))),
            },
            Type::TypeVar(_) => ty.clone(),
            _ => ty.clone(),
        }
    }

    fn check_binary(&mut self, left: &Expr, op: &BinOp, right: &Expr) -> Result<Type, Diagnostic> {
        let lt = self.check_expr(left)?;
        let rt = self.check_expr(right)?;
        if lt.is_error() || rt.is_error() {
            return Ok(Type::Error);
        }
        match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod | BinOp::Pow => {
                if *op == BinOp::Add && lt == Type::String && rt == Type::String {
                    return Ok(Type::String);
                }
                match (&lt, &rt) {
                    (Type::Int, Type::Int) => Ok(Type::Int),
                    (Type::Float, Type::Float) => Ok(Type::Float),
                    (Type::Int, Type::Float) | (Type::Float, Type::Int) => Ok(Type::Float),
                    _ => Err(Diagnostic::error(format!(
                        "Cannot apply {:?} to {:?} and {:?}",
                        op, lt, rt
                    ))
                    .with_code("E001")
                    .with_label(left.span().merge(right.span()), "type mismatch")),
                }
            }
            BinOp::Eq | BinOp::Neq => {
                if matches!(&lt, Type::Function { .. }) || matches!(&rt, Type::Function { .. }) {
                    return Err(Diagnostic::error(format!(
                        "Cannot compare function types with {:?}",
                        op
                    ))
                    .with_code("E019")
                    .with_label(left.span().merge(right.span()), "function comparison"));
                }
                if lt == rt {
                    // Same type — check if user type includes Eq
                    if let Type::Custom(ref class_name, _) = lt
                        && !self.type_includes_eq(&lt)
                    {
                        return Err(Diagnostic::error(format!(
                            "'{}' does not include Eq. Add 'includes Eq' to enable == and != comparisons",
                            class_name
                        ))
                        .with_code("E019")
                        .with_label(left.span().merge(right.span()), "type does not include Eq"));
                    }
                    return Ok(Type::Bool);
                }
                match (&lt, &rt) {
                    (Type::Int, Type::Float) | (Type::Float, Type::Int) => Ok(Type::Bool),
                    _ => Err(Diagnostic::error(format!(
                        "Cannot compare {:?} and {:?} with {:?}",
                        lt, rt, op
                    ))
                    .with_code("E019")
                    .with_label(left.span().merge(right.span()), "incompatible types")),
                }
            }
            BinOp::Lt | BinOp::Gt | BinOp::Lte | BinOp::Gte => match (&lt, &rt) {
                (Type::Int, Type::Int)
                | (Type::Float, Type::Float)
                | (Type::Int, Type::Float)
                | (Type::Float, Type::Int)
                | (Type::String, Type::String) => Ok(Type::Bool),
                (Type::Custom(name_l, _), Type::Custom(name_r, _)) if name_l == name_r => {
                    if self.type_includes_ord(&lt) {
                        Ok(Type::Bool)
                    } else {
                        Err(Diagnostic::error(format!(
                            "'{}' does not include Ord. Add 'includes Ord' to enable ordering comparisons",
                            name_l
                        ))
                        .with_code("E019")
                        .with_label(left.span().merge(right.span()), "type does not include Ord"))
                    }
                }
                _ => Err(Diagnostic::error(format!(
                    "Cannot order {:?} and {:?} with {:?}",
                    lt, rt, op
                ))
                .with_code("E019")
                .with_label(left.span().merge(right.span()), "incompatible types")),
            },
            BinOp::And | BinOp::Or => {
                if lt == Type::Bool && rt == Type::Bool {
                    Ok(Type::Bool)
                } else {
                    Err(Diagnostic::error(format!(
                        "Logical {:?} requires Bool operands, got {:?} and {:?}",
                        op, lt, rt
                    ))
                    .with_code("E020")
                    .with_label(left.span().merge(right.span()), "expected Bool operands"))
                }
            }
        }
    }

    fn check_unary(&mut self, op: &UnaryOp, operand: &Expr) -> Result<Type, Diagnostic> {
        let t = self.check_expr(operand)?;
        if t.is_error() {
            return Ok(Type::Error);
        }
        match op {
            UnaryOp::Neg => match t {
                Type::Int => Ok(Type::Int),
                Type::Float => Ok(Type::Float),
                _ => Err(Diagnostic::error(format!("Cannot negate {:?}", t))
                    .with_code("E001")
                    .with_label(operand.span(), "expected numeric type")),
            },
            UnaryOp::Not => {
                if t == Type::Bool {
                    Ok(Type::Bool)
                } else {
                    Err(Diagnostic::error(format!("Cannot apply 'not' to {:?}", t))
                        .with_code("E001")
                        .with_label(operand.span(), "expected Bool"))
                }
            }
        }
    }

    fn check_list_literal(&mut self, elems: &[Expr]) -> Result<Type, Diagnostic> {
        if elems.is_empty() {
            return Ok(Type::List(Box::new(Type::Nil)));
        }
        let first_ty = self.check_expr(&elems[0])?;
        if first_ty.is_error() {
            return Ok(Type::Error);
        }
        for (i, elem) in elems.iter().enumerate().skip(1) {
            let ty = self.check_expr(elem)?;
            if ty.is_error() {
                return Ok(Type::Error);
            }
            if ty != first_ty {
                return Err(Diagnostic::error(format!(
                    "List element {} has type {:?}, expected {:?} (all elements must have consistent type)",
                    i, ty, first_ty
                ))
                .with_code("E017")
                .with_label(elem.span(), format!("expected {:?}", first_ty)));
            }
        }
        Ok(Type::List(Box::new(first_ty)))
    }

    fn check_index(&mut self, object: &Expr, index: &Expr) -> Result<Type, Diagnostic> {
        let obj_ty = self.check_expr(object)?;
        let idx_ty = self.check_expr(index)?;
        if obj_ty.is_error() || idx_ty.is_error() {
            return Ok(Type::Error);
        }
        if idx_ty != Type::Int {
            return Err(
                Diagnostic::error(format!("List index must be Int, got {:?}", idx_ty))
                    .with_code("E016")
                    .with_label(index.span(), "expected Int"),
            );
        }
        match obj_ty {
            Type::List(inner) => Ok(*inner),
            _ => Err(
                Diagnostic::error(format!("Cannot index into {:?}, expected List", obj_ty))
                    .with_code("E016")
                    .with_label(object.span(), "not a list"),
            ),
        }
    }

    pub(crate) fn check_member(&mut self, object: &Expr, field: &str) -> Result<Type, Diagnostic> {
        use std::collections::HashMap;

        // Check for enum variant access: EnumName.VariantName
        if let Expr::Ident(name, _) = object
            && let Some(enum_info) = self.env.get_enum(name)
        {
            if enum_info.variants.contains(&field.to_string()) {
                return Ok(Type::Custom(name.clone(), Vec::new()));
            }
            return Err(
                Diagnostic::error(format!("Enum '{}' has no variant '{}'", name, field))
                    .with_code("E010")
                    .with_label(
                        object.span(),
                        format!("no variant '{}' on this enum", field),
                    ),
            );
        }

        let obj_ty = self.check_expr(object)?;
        if obj_ty.is_error() {
            return Ok(Type::Error);
        }
        if let Type::Nullable(_) = &obj_ty {
            if field == "or" || field == "or_else" || field == "or_throw" {
                return Ok(Type::Void);
            }
            return Err(Diagnostic::error(format!(
                "Cannot access '{}' on nullable type {:?}. Resolve with .or(), .or_else(), .or_throw(), or match first",
                field, obj_ty
            ))
            .with_code("E018")
            .with_label(object.span(), "nullable type"));
        }
        if let Type::Custom(class_name, type_args) = obj_ty {
            let mut current_class = Some(class_name.clone());
            while let Some(ref cname) = current_class {
                if let Some(info) = self.env.get_class(cname) {
                    let bindings: HashMap<String, Type> = info
                        .generic_params
                        .as_ref()
                        .map(|gp| {
                            gp.iter()
                                .zip(type_args.iter())
                                .map(|(p, t)| (p.clone(), t.clone()))
                                .collect()
                        })
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
                    return Err(Diagnostic::error(format!("Unknown class '{}'", cname))
                        .with_code("E010")
                        .with_label(object.span(), "unknown class"));
                }
            }
            Err(Diagnostic::error(format!(
                "Class '{}' has no field or method '{}'",
                class_name, field
            ))
            .with_code("E010")
            .with_label(object.span(), format!("no member '{}' on this type", field)))
        } else {
            Err(
                Diagnostic::error(format!("Cannot access member '{}' on {:?}", field, obj_ty))
                    .with_code("E010")
                    .with_label(object.span(), "not a class type"),
            )
        }
    }

    fn check_match_expr(
        &mut self,
        scrutinee: &Expr,
        arms: &[(MatchPattern, Expr)],
    ) -> Result<Type, Diagnostic> {
        let scrutinee_ty = self.check_expr(scrutinee)?;
        if scrutinee_ty.is_error() {
            return Ok(Type::Error);
        }
        if arms.is_empty() {
            return Err(Diagnostic::error(
                "Match expression must have at least one arm".to_string(),
            )
            .with_code("E011")
            .with_label(scrutinee.span(), "match has no arms"));
        }
        let mut result_ty: Option<Type> = None;
        for (pattern, value) in arms {
            self.check_match_pattern(pattern, &scrutinee_ty)?;
            let arm_ty = if let MatchPattern::Ident(name, _) = pattern {
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
            if arm_ty == Type::Never || arm_ty.is_error() {
                continue;
            }
            if let Some(ref expected) = result_ty {
                if *expected == Type::Never || expected.is_error() {
                    result_ty = Some(arm_ty);
                } else if arm_ty != *expected {
                    return Err(Diagnostic::error(format!(
                        "Match arm type mismatch: expected {:?}, got {:?}",
                        expected, arm_ty
                    ))
                    .with_code("E001")
                    .with_label(value.span(), format!("expected {:?}", expected)));
                }
            } else {
                result_ty = Some(arm_ty);
            }
        }

        // Exhaustiveness check
        let has_catchall = arms
            .iter()
            .any(|(p, _)| matches!(p, MatchPattern::Wildcard(_) | MatchPattern::Ident(..)));
        if !has_catchall {
            match &scrutinee_ty {
                Type::Bool => {
                    let has_true = arms.iter().any(|(p, _)| {
                        matches!(p, MatchPattern::Literal(e, _) if matches!(**e, Expr::Bool(true, _)))
                    });
                    let has_false = arms.iter().any(|(p, _)| {
                        matches!(p, MatchPattern::Literal(e, _) if matches!(**e, Expr::Bool(false, _)))
                    });
                    if !has_true || !has_false {
                        return Err(Diagnostic::error(
                            "Non-exhaustive match: Bool match must cover both true and false, or include a wildcard".to_string()
                        )
                        .with_code("E011")
                        .with_label(scrutinee.span(), "non-exhaustive patterns"));
                    }
                }
                _ => {
                    return Err(Diagnostic::error(
                        "Non-exhaustive match: must include a wildcard '_' or variable pattern as catch-all".to_string()
                    )
                    .with_code("E011")
                    .with_label(scrutinee.span(), "non-exhaustive patterns"));
                }
            }
        }

        Ok(result_ty.unwrap_or(Type::Void))
    }

    fn check_async_call(
        &mut self,
        func: &Expr,
        args: &[(String, Expr)],
    ) -> Result<Type, Diagnostic> {
        // Bypass throws check: error handling moves to `resolve task!`
        let ret_ty = self.check_call_inner(func, args, true)?;
        Ok(Type::Task(Box::new(ret_ty)))
    }

    fn check_resolve(&mut self, expr: &Expr) -> Result<Type, Diagnostic> {
        let ty = self.check_expr(expr)?;
        if ty.is_error() {
            return Ok(Type::Error);
        }
        match ty {
            Type::Task(_) => {
                // Bare resolve without ! is an error — CancelledError is always possible
                Err(Diagnostic::error(
                    "resolve requires ! — any task can be cancelled (CancelledError). Use resolve expr! to handle CancelledError"
                        .to_string(),
                )
                .with_code("E012")
                .with_label(expr.span(), "add ! to propagate CancelledError"))
            }
            _ => Err(Diagnostic::error(format!(
                "resolve expects a Task[T] expression, got {:?}",
                ty
            ))
            .with_code("E012")
            .with_label(expr.span(), "expected Task[T]")),
        }
    }

    fn check_detached_call(
        &mut self,
        func: &Expr,
        args: &[(String, Expr)],
    ) -> Result<Type, Diagnostic> {
        // Bypass throws check: detached tasks log errors at runtime
        self.check_call_inner(func, args, true)?;
        Ok(Type::Void)
    }

    fn check_async_scope(&mut self, body: &[ast::Stmt]) -> Result<Type, Diagnostic> {
        let mut sub = self.child_checker();
        sub.loop_depth = 0; // async scope cannot break/continue outer loops
        sub.check_body(body)
    }
}
