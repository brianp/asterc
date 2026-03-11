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
                type_constraints,
                ..
            } => self.check_lambda(
                params,
                ret_type,
                body,
                generic_params,
                throws,
                type_constraints,
            ),
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

    /// Check a lambda expression with an optional expected function type for inference.
    /// If `expected` is Some(Type::Function { .. }), inferred param types are resolved from it.
    pub(crate) fn check_lambda_with_expected(
        &mut self,
        expr: &Expr,
        expected: Option<&Type>,
    ) -> Result<Type, Diagnostic> {
        if let Expr::Lambda {
            params,
            ret_type,
            body,
            generic_params,
            throws,
            type_constraints,
            ..
        } = expr
        {
            let has_inferred =
                params.iter().any(|(_, t)| *t == Type::Inferred) || *ret_type == Type::Inferred;

            if has_inferred {
                if let Some(Type::Function {
                    params: expected_params,
                    ret: expected_ret,
                    throws: expected_throws,
                    ..
                }) = expected
                {
                    // Resolve inferred types from the expected function type
                    let resolved_params: Vec<(String, Type)> = params
                        .iter()
                        .enumerate()
                        .map(|(i, (name, ty))| {
                            if *ty == Type::Inferred {
                                let resolved =
                                    expected_params.get(i).cloned().unwrap_or(Type::Void);
                                (name.clone(), resolved)
                            } else {
                                (name.clone(), ty.clone())
                            }
                        })
                        .collect();

                    if resolved_params.len() != expected_params.len() {
                        return Err(Diagnostic::error(format!(
                            "Lambda arity mismatch: expected {} params, got {}",
                            expected_params.len(),
                            resolved_params.len()
                        ))
                        .with_code("E006")
                        .with_label(expr.span(), "wrong number of parameters"));
                    }

                    let resolved_ret = if *ret_type == Type::Inferred {
                        // TypeVars resolve during unification, not lambda body checking
                        if matches!(**expected_ret, Type::TypeVar(..)) {
                            Type::Inferred
                        } else {
                            *expected_ret.clone()
                        }
                    } else {
                        ret_type.clone()
                    };

                    let resolved_throws = if throws.is_none() {
                        expected_throws.as_ref().map(|t| *t.clone())
                    } else {
                        throws.clone()
                    };

                    return self.check_lambda(
                        &resolved_params,
                        &resolved_ret,
                        body,
                        generic_params,
                        &resolved_throws,
                        type_constraints,
                    );
                } else {
                    return Err(Diagnostic::error(
                        "Cannot infer lambda parameter types without a function type context. Add type annotations or pass to a function with known parameter types"
                            .to_string(),
                    )
                    .with_code("E001")
                    .with_label(expr.span(), "cannot infer types"));
                }
            }

            self.check_lambda(
                params,
                ret_type,
                body,
                generic_params,
                throws,
                type_constraints,
            )
        } else {
            self.check_expr(expr)
        }
    }

    fn check_lambda(
        &mut self,
        params: &[(String, Type)],
        ret_type: &Type,
        body: &[ast::Stmt],
        generic_params: &Option<Vec<String>>,
        throws: &Option<Type>,
        type_constraints: &[(String, Vec<ast::TypeConstraint>)],
    ) -> Result<Type, Diagnostic> {
        // Validate constraint targets exist before proceeding
        for (_, constraints) in type_constraints {
            for c in constraints {
                match c {
                    ast::TypeConstraint::Extends(class_name) => {
                        if self.env.get_class(class_name).is_none() {
                            return Err(Diagnostic::error(format!(
                                "Unknown class '{}' in extends constraint",
                                class_name
                            ))
                            .with_code("E024"));
                        }
                    }
                    ast::TypeConstraint::Includes(trait_name, _) => {
                        if self.env.get_trait(trait_name).is_none() {
                            return Err(Diagnostic::error(format!(
                                "Unknown trait '{}' in includes constraint",
                                trait_name
                            ))
                            .with_code("E024"));
                        }
                    }
                }
            }
        }

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

        // Check for unresolved Inferred types — means no context was available
        for (n, t) in params {
            if *t == Type::Inferred {
                return Err(Diagnostic::error(format!(
                    "Cannot infer type for parameter '{}'. Add type annotations or pass to a function with known parameter types",
                    n
                ))
                .with_code("E001"));
            }
        }

        let mut sub = self.child_checker();
        sub.throws_type = throws.clone();
        if *ret_type != Type::Void && *ret_type != Type::Inferred {
            sub.expected_return_type = Some(ret_type.clone());
        }

        // Register virtual ClassInfo for constrained type parameters so that
        // operations like `==`, `>`, `.to_string()` work on them inside the body.
        for (type_param_name, constraints) in type_constraints {
            let mut includes = Vec::new();
            let mut extends = None;
            let mut methods = std::collections::HashMap::new();
            for c in constraints {
                match c {
                    ast::TypeConstraint::Extends(class_name) => {
                        extends = Some(class_name.clone());
                        // Inherit methods and includes from parent class
                        if let Some(parent_info) = self.env.get_class(class_name) {
                            for (mname, mty) in &parent_info.methods {
                                methods.insert(mname.clone(), mty.clone());
                            }
                            for inc in &parent_info.includes {
                                if !includes.contains(inc) {
                                    includes.push(inc.clone());
                                }
                            }
                        }
                    }
                    ast::TypeConstraint::Includes(trait_name, _) => {
                        if !includes.contains(trait_name) {
                            includes.push(trait_name.clone());
                        }
                        // Add trait methods to the virtual class
                        if let Some(trait_info) = self.env.get_trait(trait_name) {
                            for (mname, mty) in &trait_info.methods {
                                methods.insert(mname.clone(), mty.clone());
                            }
                        }
                    }
                }
            }
            // Ord includes Eq
            if includes.contains(&"Ord".to_string()) && !includes.contains(&"Eq".to_string()) {
                includes.push("Eq".to_string());
            }
            sub.env.set_class(
                type_param_name.clone(),
                ast::ClassInfo {
                    ty: Type::Custom(type_param_name.clone(), vec![]),
                    fields: indexmap::IndexMap::new(),
                    methods,
                    generic_params: None,
                    extends,
                    includes,
                    overloaded_methods: std::collections::HashMap::new(),
                    parametric_includes: Vec::new(),
                },
            );
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
                    && *ret_type != Type::Inferred
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
            && *ret_type != Type::Inferred
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

        // If ret_type is Inferred, use the actual body result type
        let effective_ret = if *ret_type == Type::Inferred {
            last.clone()
        } else {
            ret_type.clone()
        };

        // Build the Function type. Convert inferred type params from Custom to TypeVar.
        let (final_params, final_ret) = if inferred_type_params.is_empty() {
            (param_types, effective_ret)
        } else {
            let fp = param_types
                .iter()
                .map(|t| {
                    Self::replace_custom_with_typevar(t, &inferred_type_params, type_constraints)
                })
                .collect();
            let fr = Self::replace_custom_with_typevar(
                &effective_ret,
                &inferred_type_params,
                type_constraints,
            );
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
        let mut collected = Vec::new();
        ty.collect_types(
            &|t| matches!(t, Type::Custom(name, args) if args.is_empty() && !self.is_known_type_name(name)),
            &mut collected,
        );
        for t in collected {
            if let Type::Custom(name, _) = t
                && !out.contains(&name)
            {
                out.push(name);
            }
        }
    }

    /// Check if a type name is a known class, trait, enum, or "Self".
    fn is_known_type_name(&self, name: &str) -> bool {
        self.env.get_class(name).is_some()
            || self.env.get_trait(name).is_some()
            || self.env.get_enum(name).is_some()
            || name == "Self"
    }

    /// Replace Custom(name, []) with TypeVar(name, constraints) for names in the given type param list.
    fn replace_custom_with_typevar(
        ty: &Type,
        type_params: &[String],
        type_constraints: &[(String, Vec<ast::TypeConstraint>)],
    ) -> Type {
        ty.map_type(&|t| {
            if let Type::Custom(name, args) = t
                && args.is_empty()
                && type_params.contains(name)
            {
                let constraints = type_constraints
                    .iter()
                    .find(|(n, _)| n == name)
                    .map(|(_, c)| c.clone())
                    .unwrap_or_default();
                return Some(Type::TypeVar(name.clone(), constraints));
            }
            None
        })
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

        // Check for namespace member access: ns.ExportedName
        if let Expr::Ident(name, _) = object
            && let Some(ns) = self.env.get_namespace(name)
        {
            // Try each export category
            if let Some(info) = ns.classes.get(field) {
                // Inject class into env so constructor calls and field access work
                self.env.set_class(field.to_string(), info.clone());
                // Return the constructor function type
                if let Some(ty) = ns.variables.get(field) {
                    self.env.set_var(field.to_string(), ty.clone());
                    return Ok(ty.clone());
                }
                return Ok(info.ty.clone());
            }
            if let Some(ty) = ns.variables.get(field) {
                return Ok(ty.clone());
            }
            if let Some(info) = ns.enums.get(field) {
                // Inject enum into env so EnumName.Variant access works downstream
                self.env.set_enum(field.to_string(), info.clone());
                return Ok(Type::Custom(field.to_string(), Vec::new()));
            }
            if let Some(info) = ns.traits.get(field) {
                // Inject trait into env for potential use
                self.env.set_trait(field.to_string(), info.clone());
                return Ok(Type::Void);
            }
            return Err(Diagnostic::error(format!(
                "'{}' is not found in namespace '{}'",
                field, name
            ))
            .with_code("M004")
            .with_label(object.span(), format!("'{}' not exported", field)));
        }

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
        // Handle List built-in methods (List implicitly includes Iterable)
        if let Type::List(ref inner) = obj_ty {
            return self.check_list_member(field, inner, object);
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
                        // Check Ord constraint for conditional Iterable methods
                        if (field == "min" || field == "max" || field == "sort")
                            && info.includes.contains(&"Iterable".to_string())
                            && let Some(elem_ty) = Self::get_iterable_element_type_from_class(&info)
                            && !self.type_includes_ord(&elem_ty)
                        {
                            return Err(Diagnostic::error(format!(
                                        "Cannot call '{}()' on '{}': element type {:?} does not include Ord. \
                                         Add 'includes Ord' to the element type to enable {}",
                                        field, class_name, elem_ty, field
                                    ))
                                    .with_code("E025")
                                    .with_label(
                                        object.span(),
                                        format!("{} requires element type to include Ord", field),
                                    ));
                        }
                        let resolved = Self::substitute_typevars(t, &bindings);
                        return Ok(resolved);
                    }
                    // Check overloaded methods (from multiple parametric trait inclusions)
                    if let Some(overloads) = info.overloaded_methods.get(field) {
                        return self.resolve_overloaded_method(
                            &class_name,
                            field,
                            overloads,
                            object,
                        );
                    }
                    // Check From/Into auto-reverse: .into() on source when target has From[Source]
                    if field == "into"
                        && !info.includes.contains(&"Into".to_string())
                        && let Some(resolved) = self.resolve_into_via_from_reverse(&Type::Custom(
                            class_name.clone(),
                            type_args.clone(),
                        ))
                    {
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

    /// Resolve an overloaded method (e.g., multiple into() from Into[A], Into[B])
    /// using expected_type context to disambiguate.
    fn resolve_overloaded_method(
        &self,
        class_name: &str,
        method_name: &str,
        overloads: &[Type],
        object: &Expr,
    ) -> Result<Type, Diagnostic> {
        // If only one overload, use it directly
        if overloads.len() == 1 {
            return Ok(overloads[0].clone());
        }
        // Try to disambiguate using expected type
        if let Some(ref expected) = self.expected_type {
            // For .into(), the expected type is the return type
            let matching: Vec<&Type> = overloads
                .iter()
                .filter(|oty| {
                    if let Type::Function { ret, .. } = oty {
                        let mut bindings = std::collections::HashMap::new();
                        Self::unify_type(expected, ret, &mut bindings).is_ok()
                    } else {
                        false
                    }
                })
                .collect();
            if matching.len() == 1 {
                return Ok(matching[0].clone());
            }
        }
        // Ambiguous — error with guidance
        let return_types: Vec<String> = overloads
            .iter()
            .filter_map(|oty| {
                if let Type::Function { ret, .. } = oty {
                    Some(format!("{:?}", ret))
                } else {
                    None
                }
            })
            .collect();
        Err(Diagnostic::error(format!(
            "Ambiguous call to '{}()' on '{}': multiple overloads available ({}). Add a type annotation to disambiguate",
            method_name, class_name, return_types.join(", ")
        ))
        .with_code("E014")
        .with_label(object.span(), "ambiguous method call"))
    }

    /// Resolve .into() via From/Into auto-reverse: if expected type's class includes From[SourceType],
    /// synthesize the appropriate into() return type.
    fn resolve_into_via_from_reverse(&self, source_type: &Type) -> Option<Type> {
        let expected = self.expected_type.as_ref()?;
        let target_name = match expected {
            Type::Custom(name, _) => name,
            _ => return None,
        };
        let target_info = self.env.get_class(target_name)?;
        // Check if target includes From[SourceType]
        for (trait_name, trait_args) in &target_info.parametric_includes {
            if trait_name == "From" && trait_args.len() == 1 && &trait_args[0] == source_type {
                // Target includes From[SourceType] — synthesize into() -> TargetType
                return Some(Type::Function {
                    param_names: vec![],
                    params: vec![],
                    ret: Box::new(expected.clone()),
                    throws: None,
                });
            }
        }
        None
    }

    /// Handle member access on List[T] -- built-in methods plus Iterable vocabulary.
    fn check_list_member(
        &self,
        field: &str,
        inner: &Type,
        object: &Expr,
    ) -> Result<Type, Diagnostic> {
        // List-specific methods (not part of Iterable vocabulary)
        match field {
            "len" => {
                return Ok(Type::Function {
                    param_names: vec![],
                    params: vec![],
                    ret: Box::new(Type::Int),
                    throws: None,
                });
            }
            "each" => {
                return Ok(Type::Function {
                    param_names: vec!["f".into()],
                    params: vec![Type::Function {
                        param_names: vec!["_0".into()],
                        params: vec![inner.clone()],
                        ret: Box::new(Type::Void),
                        throws: None,
                    }],
                    ret: Box::new(Type::Void),
                    throws: None,
                });
            }
            "push" => {
                return Ok(Type::Function {
                    param_names: vec!["item".into()],
                    params: vec![inner.clone()],
                    ret: Box::new(Type::Void),
                    throws: None,
                });
            }
            _ => {}
        }

        // Ord-gated methods: check constraint before returning type
        if matches!(field, "min" | "max" | "sort") && !self.type_includes_ord(inner) {
            return Err(Diagnostic::error(format!(
                "Cannot call '{}()': element type {:?} does not include Ord. \
                 Add 'includes Ord' to the element type to enable {}",
                field, inner, field
            ))
            .with_code("E025")
            .with_label(object.span(), format!("{} requires Ord", field)));
        }

        // Look up from shared Iterable vocabulary definitions
        let vocab = crate::check_class::iterable_vocabulary_methods(inner);
        for (name, ty) in &vocab {
            if name == field {
                return Ok(ty.clone());
            }
        }

        Err(Diagnostic::error(format!("List has no method '{}'", field))
            .with_code("E010")
            .with_label(object.span(), format!("no member '{}' on List", field)))
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
        let mut has_nil_arm = false;
        for (pattern, value) in arms {
            self.check_match_pattern(pattern, &scrutinee_ty)?;
            // Track nil literal arms so we know if nullable unwrapping is safe
            if let MatchPattern::Literal(expr, _) = pattern
                && matches!(**expr, Expr::Nil(_))
            {
                has_nil_arm = true;
            }
            let arm_ty = if let MatchPattern::Ident(name, _) = pattern {
                let mut sub = self.child_checker();
                // Only unwrap T? to T if a nil arm exists (meaning this arm only matches non-nil).
                // Without a nil arm, the catch-all also matches nil, so bind as T? to prevent unsoundness.
                let bind_ty = if let Type::Nullable(inner) = &scrutinee_ty {
                    if has_nil_arm {
                        *inner.clone()
                    } else {
                        scrutinee_ty.clone()
                    }
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
                Type::Custom(name, _) => {
                    // Check if this is an enum with all variants covered
                    if let Some(enum_info) = self.env.get_enum(name) {
                        let covered: std::collections::HashSet<&str> = arms
                            .iter()
                            .filter_map(|(p, _)| {
                                if let MatchPattern::EnumVariant { variant, .. } = p {
                                    Some(variant.as_str())
                                } else {
                                    None
                                }
                            })
                            .collect();
                        let missing: Vec<&str> = enum_info
                            .variants
                            .iter()
                            .filter(|v| !covered.contains(v.as_str()))
                            .map(|v| v.as_str())
                            .collect();
                        if !missing.is_empty() {
                            return Err(Diagnostic::error(format!(
                                "Non-exhaustive match: missing variants: {}",
                                missing.join(", ")
                            ))
                            .with_code("E011")
                            .with_label(scrutinee.span(), "non-exhaustive patterns"));
                        }
                    } else {
                        return Err(Diagnostic::error(
                            "Non-exhaustive match: must include a wildcard '_' or variable pattern as catch-all".to_string()
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
