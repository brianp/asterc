use std::collections::HashMap;

use ast::{Diagnostic, Expr, Type, TypeEnv};

use crate::typechecker::TypeChecker;

impl TypeChecker {
    pub(crate) fn is_nullable_compatible(expected: &Type, actual: &Type) -> bool {
        if let Type::Nullable(inner) = expected {
            *actual == **inner || *actual == Type::Nil
        } else {
            false
        }
    }

    /// Check if `actual` is a subtype of `expected` via the extends chain.
    pub(crate) fn is_subtype_compatible(actual: &Type, expected: &Type, env: &TypeEnv) -> bool {
        if let (Type::Custom(an, _), Type::Custom(en, _)) = (actual, expected)
            && an != en
        {
            return Self::is_subtype_of(an, en, env);
        }
        false
    }

    pub(crate) fn check_call(
        &mut self,
        func: &Expr,
        args: &[(String, Expr)],
    ) -> Result<Type, Diagnostic> {
        // Check for nullable method calls: x.or(), x.or_else(), x.or_throw()
        // Skip this interception for namespace member access (ns.func())
        if let Expr::Member { object, field, .. } = func {
            let is_namespace = matches!(object.as_ref(), Expr::Ident(name, _) if self.env.get_namespace(name).is_some());
            if is_namespace {
                return self.check_call_inner(func, args, false);
            }
            let obj_ty = self.check_expr(object)?;
            if obj_ty.is_error() {
                return Ok(Type::Error);
            }
            if let Type::Nullable(inner) = &obj_ty {
                match field.as_str() {
                    "or" | "or_else" => {
                        if args.len() != 1 {
                            return Err(Diagnostic::error(format!(
                                "T?.{}() takes exactly 1 argument",
                                field
                            ))
                            .with_code("E006")
                            .with_label(func.span(), "expected 1 argument"));
                        }
                        let arg_ty = self.check_expr(&args[0].1)?;
                        if arg_ty.is_error() {
                            return Ok(Type::Error);
                        }
                        if arg_ty != **inner {
                            return Err(Diagnostic::error(format!(
                                ".{}() type mismatch: expected {:?}, got {:?}",
                                field, inner, arg_ty
                            ))
                            .with_code("E018")
                            .with_label(args[0].1.span(), format!("expected {:?}", inner)));
                        }
                        return Ok(*inner.clone());
                    }
                    "or_throw" => {
                        if args.len() != 1 {
                            return Err(Diagnostic::error(
                                "T?.or_throw() takes exactly 1 argument".to_string(),
                            )
                            .with_code("E006")
                            .with_label(func.span(), "expected 1 argument"));
                        }
                        let arg_ty = self.check_expr(&args[0].1)?;
                        let throws_ty = self.throws_type.as_ref().ok_or_else(|| {
                            Diagnostic::error(
                                ".or_throw() can only be used in a function that declares 'throws'"
                                    .to_string(),
                            )
                            .with_code("E013")
                            .with_label(func.span(), "requires 'throws' declaration")
                        })?;
                        if !self.is_error_subtype(&arg_ty, throws_ty) {
                            return Err(Diagnostic::error(format!(
                                ".or_throw() error type {:?} not compatible with throws {:?}",
                                arg_ty, throws_ty
                            ))
                            .with_code("E013")
                            .with_label(args[0].1.span(), "incompatible error type"));
                        }
                        return Ok(*inner.clone());
                    }
                    _ => {
                        return Err(Diagnostic::error(format!(
                            "Cannot access '{}' on nullable type {:?}. Resolve with .or(), .or_else(), .or_throw(), or match first",
                            field, obj_ty
                        ))
                        .with_code("E018")
                        .with_label(object.span(), "nullable type"));
                    }
                }
            }
        }
        self.check_call_inner(func, args, false)
    }

    pub(crate) fn check_call_inner(
        &mut self,
        func: &Expr,
        args: &[(String, Expr)],
        bypass_throws_check: bool,
    ) -> Result<Type, Diagnostic> {
        // Handle polymorphic builtins that can't be expressed in the type system yet
        if let Expr::Ident(name, _) = func {
            match name.as_str() {
                "len" => {
                    if args.len() != 1 {
                        return Err(Diagnostic::error(format!(
                            "len() takes 1 argument, got {}",
                            args.len()
                        ))
                        .with_code("E006")
                        .with_label(func.span(), "expected 1 argument"));
                    }
                    let aty = self.check_expr(&args[0].1)?;
                    if aty.is_error() {
                        return Ok(Type::Error);
                    }
                    match aty {
                        Type::String | Type::List(_) => return Ok(Type::Int),
                        _ => {
                            return Err(Diagnostic::error(format!(
                                "len() expects String or List, got {:?}",
                                aty
                            ))
                            .with_code("E005")
                            .with_label(args[0].1.span(), "expected String or List"));
                        }
                    }
                }
                "to_string" => {
                    if args.len() != 1 {
                        return Err(Diagnostic::error(format!(
                            "to_string() takes 1 argument, got {}",
                            args.len()
                        ))
                        .with_code("E006")
                        .with_label(func.span(), "expected 1 argument"));
                    }
                    let aty = self.check_expr(&args[0].1)?;
                    if aty.is_error() {
                        return Ok(Type::Error);
                    }
                    match aty {
                        Type::Int | Type::Float | Type::Bool | Type::String => {
                            return Ok(Type::String);
                        }
                        _ => {
                            return Err(Diagnostic::error(format!(
                                "to_string() expects Int, Float, Bool, or String, got {:?}",
                                aty
                            ))
                            .with_code("E005")
                            .with_label(args[0].1.span(), "unsupported type"));
                        }
                    }
                }
                "log" | "print" => {
                    if args.len() != 1 {
                        return Err(Diagnostic::error(format!(
                            "{}() takes 1 argument, got {}",
                            name,
                            args.len()
                        ))
                        .with_code("E006")
                        .with_label(func.span(), "expected 1 argument"));
                    }
                    let aty = self.check_expr(&args[0].1)?;
                    if aty.is_error() {
                        return Ok(Type::Void);
                    }
                    match aty {
                        Type::Int | Type::Float | Type::Bool | Type::String => {
                            return Ok(Type::Void);
                        }
                        _ => {
                            return Err(Diagnostic::error(format!(
                                "{}() expects Int, Float, Bool, or String, got {:?}",
                                name, aty
                            ))
                            .with_code("E005")
                            .with_label(args[0].1.span(), "unsupported type"));
                        }
                    }
                }
                _ => {}
            }
        }

        // Handle Type.from() intrinsic for From[T] protocol
        if let Expr::Member { object, field, .. } = func
            && field == "from"
            && let Expr::Ident(type_name, _) = object.as_ref()
            && let Some(class_info) = self.env.get_class(type_name)
            && class_info.includes.contains(&"From".to_string())
        {
            // Get all from() overloads: single method or overloaded
            let from_methods: Vec<Type> =
                if let Some(overloads) = class_info.overloaded_methods.get("from") {
                    overloads.clone()
                } else if let Some(m) = class_info.methods.get("from") {
                    vec![m.clone()]
                } else {
                    vec![]
                };

            if from_methods.is_empty() {
                return Err(Diagnostic::error(format!(
                    "Class '{}' includes From but has no from() method",
                    type_name
                ))
                .with_code("E014")
                .with_label(func.span(), "missing from method"));
            }

            // Check arg types first to determine which overload matches
            let mut arg_types = Vec::new();
            for (_, arg_expr) in args {
                let aty = self.check_expr(arg_expr)?;
                if aty.is_error() {
                    return Ok(Type::Error);
                }
                arg_types.push(aty);
            }

            // Find matching overload by argument types
            let matching: Vec<&Type> = from_methods
                .iter()
                .filter(|m| {
                    if let Type::Function {
                        params,
                        param_names: pn,
                        ..
                    } = m
                    {
                        if params.len() != args.len() {
                            return false;
                        }
                        for (arg_name, _) in args {
                            if let Some(idx) = pn.iter().position(|n| n == arg_name) {
                                let mut bindings = HashMap::new();
                                if Self::unify_type(
                                    &params[idx],
                                    &arg_types[args
                                        .iter()
                                        .position(|(n, _)| n == arg_name)
                                        .expect("invariant: arg_name comes from this args list")],
                                    &mut bindings,
                                )
                                .is_err()
                                {
                                    return false;
                                }
                            } else {
                                return false;
                            }
                        }
                        true
                    } else {
                        false
                    }
                })
                .collect();

            let method = if matching.len() == 1 {
                matching[0]
            } else if from_methods.len() == 1 {
                &from_methods[0]
            } else {
                return Err(Diagnostic::error(format!(
                    "Ambiguous {}.from() call: multiple From inclusions match",
                    type_name
                ))
                .with_code("E014")
                .with_label(func.span(), "ambiguous from call"));
            };

            if let Type::Function {
                param_names,
                params,
                throws: fn_throws,
                ..
            } = method
            {
                if fn_throws.is_some() && !bypass_throws_check {
                    return Err(Diagnostic::error(
                        "Cannot call throwing function without error handling. Use !, !.or(), !.or_else(), or !.catch".to_string()
                    )
                    .with_code("E013")
                    .with_label(func.span(), "throwing function requires error handling"));
                }
                if params.len() != args.len() {
                    return Err(Diagnostic::error(format!(
                        "{}.from() expects {} argument(s), got {}",
                        type_name,
                        params.len(),
                        args.len()
                    ))
                    .with_code("E006")
                    .with_label(func.span(), "wrong number of arguments"));
                }
                for (arg_name, arg_expr) in args {
                    let param_idx = param_names.iter().position(|n| n == arg_name);
                    if let Some(idx) = param_idx {
                        let pty = &params[idx];
                        let aty = self.check_expr(arg_expr)?;
                        if aty.is_error() {
                            return Ok(Type::Error);
                        }
                        let mut bindings = HashMap::new();
                        Self::unify_type_with_env(pty, &aty, &mut bindings, Some(&self.env))?;
                    } else {
                        return Err(Diagnostic::error(format!(
                            "Unknown argument '{}' in {}.from()",
                            arg_name, type_name
                        ))
                        .with_code("E006")
                        .with_label(arg_expr.span(), "unknown argument name"));
                    }
                }
            }
            return Ok(Type::Custom(type_name.clone(), Vec::new()));
        }

        let fty = self.check_expr(func)?;
        if fty.is_error() {
            return Ok(Type::Error);
        }
        if let Type::Function {
            param_names,
            params,
            ret,
            throws: fn_throws,
        } = fty
        {
            if fn_throws.is_some() && !bypass_throws_check {
                return Err(Diagnostic::error(
                    "Cannot call throwing function without error handling. Use !, !.or(), !.or_else(), or !.catch".to_string()
                )
                .with_code("E013")
                .with_label(func.span(), "throwing function requires error handling"));
            }
            if params.len() != args.len() {
                // Build a helpful error listing missing/extra args
                let provided: std::collections::HashSet<&str> =
                    args.iter().map(|(n, _)| n.as_str()).collect();
                let expected: std::collections::HashSet<&str> =
                    param_names.iter().map(|n| n.as_str()).collect();
                let missing: Vec<&&str> = expected.difference(&provided).collect();
                let extra: Vec<&&str> = provided.difference(&expected).collect();
                let mut msg = format!(
                    "Function arity mismatch: expected {}, got {}",
                    params.len(),
                    args.len()
                );
                if !missing.is_empty() {
                    msg.push_str(&format!(
                        ", missing: {}",
                        missing
                            .iter()
                            .map(|s| format!("'{}'", s))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
                if !extra.is_empty() {
                    msg.push_str(&format!(
                        ", unknown: {}",
                        extra
                            .iter()
                            .map(|s| format!("'{}'", s))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
                return Err(Diagnostic::error(msg)
                    .with_code("E006")
                    .with_label(func.span(), format!("expected {} arguments", params.len())));
            }
            // Match args by name, order-independent
            let mut bindings: HashMap<String, Type> = HashMap::new();
            for (arg_name, arg_expr) in args {
                let param_idx = param_names.iter().position(|n| n == arg_name);
                let Some(idx) = param_idx else {
                    return Err(Diagnostic::error(format!(
                        "Unknown argument '{}'. Expected one of: {}",
                        arg_name,
                        param_names
                            .iter()
                            .map(|n| format!("'{}'", n))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ))
                    .with_code("E006")
                    .with_label(arg_expr.span(), "unknown argument name"));
                };
                let pty = &params[idx];
                // Substitute already-known bindings so lambda inference sees concrete types
                let resolved_pty = Self::substitute_typevars(pty, &bindings);
                // Set expected type for parametric trait resolution in args
                let prev_expected = self.expected_type.take();
                self.expected_type = Some(resolved_pty.clone());
                // If arg is a lambda with Inferred types, resolve them from expected type
                let aty = if let Expr::Lambda { .. } = arg_expr {
                    self.check_lambda_with_expected(arg_expr, Some(&resolved_pty))?
                } else {
                    self.check_expr(arg_expr)?
                };
                self.expected_type = prev_expected;
                if aty.is_error() {
                    return Ok(Type::Error);
                }
                Self::unify_type_with_env(pty, &aty, &mut bindings, Some(&self.env))?;
            }
            // Validate generic constraints after all bindings are established
            self.check_typevar_constraints(&params, &bindings)?;
            let resolved_ret = Self::substitute_typevars(&ret, &bindings);
            Ok(resolved_ret)
        } else {
            Err(
                Diagnostic::error(format!("Tried to call non-function type: {:?}", fty))
                    .with_code("E005")
                    .with_label(func.span(), "not a function"),
            )
        }
    }

    pub(crate) fn unify_type(
        expected: &Type,
        actual: &Type,
        bindings: &mut HashMap<String, Type>,
    ) -> Result<(), Diagnostic> {
        Self::unify_type_with_env(expected, actual, bindings, None)
    }

    pub(crate) fn unify_type_with_env(
        expected: &Type,
        actual: &Type,
        bindings: &mut HashMap<String, Type>,
        env: Option<&TypeEnv>,
    ) -> Result<(), Diagnostic> {
        Self::unify_inner(expected, actual, bindings, env, false)
    }

    fn unify_inner(
        expected: &Type,
        actual: &Type,
        bindings: &mut HashMap<String, Type>,
        env: Option<&TypeEnv>,
        invariant: bool,
    ) -> Result<(), Diagnostic> {
        match (expected, actual) {
            // If both sides are the same TypeVar, they trivially unify
            (Type::TypeVar(tv1, _), Type::TypeVar(tv2, _)) if tv1 == tv2 => Ok(()),
            (Type::TypeVar(tv, _constraints), _) => {
                if let Some(bound) = bindings.get(tv) {
                    if *bound != *actual {
                        return Err(Diagnostic::error(format!(
                            "Type parameter '{}' bound to {:?} but got {:?}",
                            tv, bound, actual
                        ))
                        .with_code("E001"));
                    }
                } else {
                    // Occurs check: prevent infinite types like T = List[T]
                    if Self::type_contains_var(actual, tv) {
                        return Err(Diagnostic::error(format!(
                            "Type parameter '{}' occurs in {:?}, creating an infinite type",
                            tv, actual
                        ))
                        .with_code("E001"));
                    }
                    bindings.insert(tv.clone(), actual.clone());
                }
                Ok(())
            }
            // H1: Symmetric TypeVar unification — TypeVar on the right (actual) side
            (_, Type::TypeVar(tv, _constraints)) => {
                if let Some(bound) = bindings.get(tv) {
                    if *bound != *expected {
                        return Err(Diagnostic::error(format!(
                            "Type parameter '{}' bound to {:?} but got {:?}",
                            tv, bound, expected
                        ))
                        .with_code("E001"));
                    }
                } else {
                    // Occurs check: prevent infinite types
                    if Self::type_contains_var(expected, tv) {
                        return Err(Diagnostic::error(format!(
                            "Type parameter '{}' occurs in {:?}, creating an infinite type",
                            tv, expected
                        ))
                        .with_code("E001"));
                    }
                    bindings.insert(tv.clone(), expected.clone());
                }
                Ok(())
            }
            (Type::List(e_inner), Type::List(a_inner)) => {
                // Empty list (List[Nil]) is compatible with any List[T]
                if **a_inner == Type::Nil {
                    return Ok(());
                }
                // Lists are invariant: List[Dog] ≠ List[Animal]
                Self::unify_inner(e_inner, a_inner, bindings, env, true)
            }
            (Type::Map(ek, ev), Type::Map(ak, av)) => {
                // Maps are invariant in both key and value types
                Self::unify_inner(ek, ak, bindings, env, true)?;
                Self::unify_inner(ev, av, bindings, env, true)
            }
            (Type::Task(e_inner), Type::Task(a_inner)) => {
                // Tasks are invariant
                Self::unify_inner(e_inner, a_inner, bindings, env, true)
            }
            (Type::Nullable(e_inner), Type::Nullable(a_inner)) => {
                Self::unify_inner(e_inner, a_inner, bindings, env, invariant)
            }
            (Type::Custom(en, eargs), Type::Custom(an, aargs)) => {
                // S3: If names differ, check subtype relationship via extends chain
                // Skip subtype coercion in invariant positions (container type params)
                if en != an {
                    if !invariant
                        && let Some(type_env) = env
                        && Self::is_subtype_of(an, en, type_env)
                    {
                        return Ok(());
                    }
                    return Err(Diagnostic::error(format!(
                        "Argument type mismatch: expected {:?}, got {:?}",
                        expected, actual
                    ))
                    .with_code("E001"));
                }
                if eargs.len() != aargs.len() {
                    return Err(Diagnostic::error(format!(
                        "Argument type mismatch: expected {:?}, got {:?}",
                        expected, actual
                    ))
                    .with_code("E001"));
                }
                for (e, a) in eargs.iter().zip(aargs.iter()) {
                    Self::unify_inner(e, a, bindings, env, invariant)?;
                }
                Ok(())
            }
            (
                Type::Function {
                    params: ep,
                    ret: er,
                    ..
                },
                Type::Function {
                    params: ap,
                    ret: ar,
                    ..
                },
            ) => {
                if ep.len() != ap.len() {
                    return Err(Diagnostic::error(format!(
                        "Function arity mismatch: expected {} params, got {}",
                        ep.len(),
                        ap.len()
                    ))
                    .with_code("E006"));
                }
                for (e, a) in ep.iter().zip(ap.iter()) {
                    Self::unify_inner(e, a, bindings, env, invariant)?;
                }
                Self::unify_inner(er, ar, bindings, env, invariant)
            }
            _ => {
                if expected != actual {
                    Err(Diagnostic::error(format!(
                        "Argument type mismatch: expected {:?}, got {:?}",
                        expected, actual
                    ))
                    .with_code("E001"))
                } else {
                    Ok(())
                }
            }
        }
    }

    /// Check if `child` is a subtype of `ancestor` by walking the extends chain.
    pub(crate) fn is_subtype_of(child: &str, ancestor: &str, env: &TypeEnv) -> bool {
        let mut current = child.to_string();
        // Limit chain depth to prevent infinite loops
        for _ in 0..100 {
            if let Some(class_info) = env.get_class(&current) {
                if let Some(ref parent) = class_info.extends {
                    if parent == ancestor {
                        return true;
                    }
                    current = parent.clone();
                } else {
                    return false;
                }
            } else {
                return false;
            }
        }
        false
    }

    pub(crate) fn substitute_typevars(ty: &Type, bindings: &HashMap<String, Type>) -> Type {
        ty.map_type(&|t| {
            if let Type::TypeVar(tv, _) = t {
                return bindings.get(tv).cloned();
            }
            None
        })
    }

    /// Check if a type contains a reference to the given type variable (occurs check).
    fn type_contains_var(ty: &Type, var: &str) -> bool {
        ty.any_type(&|t| matches!(t, Type::TypeVar(tv, _) if tv == var))
    }

    /// Validate that all TypeVar bindings satisfy their constraints.
    fn check_typevar_constraints(
        &self,
        params: &[Type],
        bindings: &HashMap<String, Type>,
    ) -> Result<(), Diagnostic> {
        // Collect constraints from TypeVars in the function params
        let mut checked = std::collections::HashSet::new();
        for p in params {
            self.collect_typevar_constraints(p, bindings, &mut checked)?;
        }
        Ok(())
    }

    fn collect_typevar_constraints(
        &self,
        ty: &Type,
        bindings: &HashMap<String, Type>,
        checked: &mut std::collections::HashSet<String>,
    ) -> Result<(), Diagnostic> {
        match ty {
            Type::TypeVar(name, constraints) if !constraints.is_empty() => {
                if !checked.insert(name.clone()) {
                    return Ok(()); // Already checked this TypeVar
                }
                if let Some(actual) = bindings.get(name) {
                    for c in constraints {
                        self.validate_constraint(name, actual, c)?;
                    }
                }
                Ok(())
            }
            Type::List(inner) | Type::Task(inner) | Type::Nullable(inner) => {
                self.collect_typevar_constraints(inner, bindings, checked)
            }
            Type::Map(k, v) => {
                self.collect_typevar_constraints(k, bindings, checked)?;
                self.collect_typevar_constraints(v, bindings, checked)
            }
            Type::Function { params, ret, .. } => {
                for p in params {
                    self.collect_typevar_constraints(p, bindings, checked)?;
                }
                self.collect_typevar_constraints(ret, bindings, checked)
            }
            _ => Ok(()),
        }
    }

    fn validate_constraint(
        &self,
        type_param: &str,
        actual: &Type,
        constraint: &ast::TypeConstraint,
    ) -> Result<(), Diagnostic> {
        match constraint {
            ast::TypeConstraint::Extends(class_name) => {
                let expected_ty = Type::Custom(class_name.clone(), vec![]);
                // actual must be the class itself or a subclass
                let is_same = match actual {
                    Type::Custom(name, _) => name == class_name,
                    _ => false,
                };
                if !is_same && !self.is_error_subtype(actual, &expected_ty) {
                    return Err(Diagnostic::error(format!(
                        "Type {:?} does not satisfy constraint '{} extends {}': \
                         {:?} is not a subclass of {}",
                        actual, type_param, class_name, actual, class_name
                    ))
                    .with_code("E024"));
                }
            }
            ast::TypeConstraint::Includes(trait_name, trait_args) => {
                if !self.type_includes_trait(actual, trait_name) {
                    return Err(Diagnostic::error(format!(
                        "Type {:?} does not satisfy constraint '{} includes {}': \
                         {:?} does not include {}",
                        actual, type_param, trait_name, actual, trait_name
                    ))
                    .with_code("E024"));
                }
                // Validate parametric trait type args if present
                if !trait_args.is_empty()
                    && !self.type_includes_parametric_trait(actual, trait_name, trait_args)
                {
                    let args_str: Vec<String> =
                        trait_args.iter().map(|t| format!("{:?}", t)).collect();
                    return Err(Diagnostic::error(format!(
                        "Type {:?} does not satisfy constraint '{} includes {}[{}]': \
                         {:?} does not include {}[{}]",
                        actual,
                        type_param,
                        trait_name,
                        args_str.join(", "),
                        actual,
                        trait_name,
                        args_str.join(", "),
                    ))
                    .with_code("E024"));
                }
            }
        }
        Ok(())
    }

    /// Check if a type includes a parametric trait with the specified type arguments.
    fn type_includes_parametric_trait(
        &self,
        ty: &Type,
        trait_name: &str,
        expected_args: &[Type],
    ) -> bool {
        match ty {
            Type::Custom(name, _) => {
                if let Some(info) = self.env.get_class(name) {
                    return info
                        .parametric_includes
                        .iter()
                        .any(|(tn, args)| tn == trait_name && args == expected_args);
                }
                // EnumInfo doesn't support parametric includes yet
                false
            }
            _ => false,
        }
    }

    /// Check if a type includes a given trait.
    fn type_includes_trait(&self, ty: &Type, trait_name: &str) -> bool {
        self.type_includes_protocol(ty, trait_name)
    }

    pub(crate) fn resolve_func_type(&mut self, func: &Expr) -> Result<Type, Diagnostic> {
        match func {
            Expr::Ident(name, span) => {
                match name.as_str() {
                    // Builtins never throw. Return Void as a sentinel — callers only
                    // inspect the `throws` field of Type::Function, so non-Function
                    // types correctly signal "no throws".
                    "len" | "to_string" | "log" | "print" => Ok(Type::Void),
                    _ => self.env.get_var(name).ok_or_else(|| {
                        let mut diag = Diagnostic::error(format!("Unknown identifier '{}'", name))
                            .with_code("E002")
                            .with_label(*span, "not found in this scope");
                        if let Some(suggestion) = self.suggest_similar_name(name) {
                            diag = diag.with_note(format!("did you mean '{}'?", suggestion));
                        }
                        diag
                    }),
                }
            }
            Expr::Member { object, field, .. } => self.check_member(object, field),
            _ => self.check_expr(func),
        }
    }
}
