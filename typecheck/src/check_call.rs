use std::collections::HashMap;

use ast::{Diagnostic, Expr, Type};

use crate::typechecker::TypeChecker;

impl TypeChecker {
    pub(crate) fn is_nullable_compatible(expected: &Type, actual: &Type) -> bool {
        if let Type::Nullable(inner) = expected {
            *actual == **inner || *actual == Type::Nil
        } else {
            false
        }
    }

    pub(crate) fn check_call(
        &mut self,
        func: &Expr,
        args: &[(String, Expr)],
    ) -> Result<Type, Diagnostic> {
        // Check for nullable method calls: x.or(), x.or_else(), x.or_throw()
        if let Expr::Member { object, field, .. } = func {
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
                _ => {}
            }
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
                let aty = self.check_expr(arg_expr)?;
                if aty.is_error() {
                    return Ok(Type::Error);
                }
                Self::unify_type(pty, &aty, &mut bindings)?;
            }
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
        match (expected, actual) {
            (Type::TypeVar(tv), _) => {
                if let Some(bound) = bindings.get(tv) {
                    if *bound != *actual {
                        return Err(Diagnostic::error(format!(
                            "Type parameter '{}' bound to {:?} but got {:?}",
                            tv, bound, actual
                        ))
                        .with_code("E001"));
                    }
                } else {
                    bindings.insert(tv.clone(), actual.clone());
                }
                Ok(())
            }
            (Type::List(e_inner), Type::List(a_inner)) => {
                Self::unify_type(e_inner, a_inner, bindings)
            }
            (Type::Map(ek, ev), Type::Map(ak, av)) => {
                Self::unify_type(ek, ak, bindings)?;
                Self::unify_type(ev, av, bindings)
            }
            (Type::Task(e_inner), Type::Task(a_inner)) => {
                Self::unify_type(e_inner, a_inner, bindings)
            }
            (Type::Nullable(e_inner), Type::Nullable(a_inner)) => {
                Self::unify_type(e_inner, a_inner, bindings)
            }
            (Type::Custom(en, eargs), Type::Custom(an, aargs)) => {
                if en != an || eargs.len() != aargs.len() {
                    return Err(Diagnostic::error(format!(
                        "Argument type mismatch: expected {:?}, got {:?}",
                        expected, actual
                    ))
                    .with_code("E001"));
                }
                for (e, a) in eargs.iter().zip(aargs.iter()) {
                    Self::unify_type(e, a, bindings)?;
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
                    Self::unify_type(e, a, bindings)?;
                }
                Self::unify_type(er, ar, bindings)
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

    pub(crate) fn substitute_typevars(ty: &Type, bindings: &HashMap<String, Type>) -> Type {
        match ty {
            Type::TypeVar(tv) => bindings.get(tv).cloned().unwrap_or_else(|| ty.clone()),
            Type::List(inner) => Type::List(Box::new(Self::substitute_typevars(inner, bindings))),
            Type::Map(k, v) => Type::Map(
                Box::new(Self::substitute_typevars(k, bindings)),
                Box::new(Self::substitute_typevars(v, bindings)),
            ),
            Type::Function {
                param_names,
                params,
                ret,
                throws,
            } => Type::Function {
                param_names: param_names.clone(),
                params: params
                    .iter()
                    .map(|p| Self::substitute_typevars(p, bindings))
                    .collect(),
                ret: Box::new(Self::substitute_typevars(ret, bindings)),
                throws: throws
                    .as_ref()
                    .map(|t| Box::new(Self::substitute_typevars(t, bindings))),
            },
            Type::Task(inner) => Type::Task(Box::new(Self::substitute_typevars(inner, bindings))),
            Type::Nullable(inner) => {
                Type::Nullable(Box::new(Self::substitute_typevars(inner, bindings)))
            }
            Type::Custom(name, args) => {
                let new_args = args
                    .iter()
                    .map(|a| Self::substitute_typevars(a, bindings))
                    .collect();
                Type::Custom(name.clone(), new_args)
            }
            _ => ty.clone(),
        }
    }

    pub(crate) fn resolve_func_type(&mut self, func: &Expr) -> Result<Type, Diagnostic> {
        match func {
            Expr::Ident(name, span) => {
                match name.as_str() {
                    "len" | "to_string" => Ok(Type::Void), // Not a throws function
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
