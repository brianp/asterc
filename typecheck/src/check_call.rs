use std::collections::HashMap;

use ast::{Expr, Type};

use crate::typechecker::TypeChecker;

impl TypeChecker {
    pub(crate) fn is_nullable_compatible(expected: &Type, actual: &Type) -> bool {
        if let Type::Nullable(inner) = expected {
            *actual == **inner || *actual == Type::Nil
        } else {
            false
        }
    }

    pub(crate) fn check_call(&mut self, func: &Expr, args: &[Expr]) -> Result<Type, String> {
        // Check for nullable method calls: x.or(), x.or_else(), x.or_throw()
        if let Expr::Member { object, field } = func {
            let obj_ty = self.check_expr(object)?;
            if let Type::Nullable(inner) = &obj_ty {
                match field.as_str() {
                    "or" | "or_else" => {
                        if args.len() != 1 {
                            return Err(format!("T?.{}() takes exactly 1 argument", field));
                        }
                        let arg_ty = self.check_expr(&args[0])?;
                        if arg_ty != **inner {
                            return Err(format!(
                                ".{}() type mismatch: expected {:?}, got {:?}",
                                field, inner, arg_ty
                            ));
                        }
                        return Ok(*inner.clone());
                    }
                    "or_throw" => {
                        if args.len() != 1 {
                            return Err("T?.or_throw() takes exactly 1 argument".to_string());
                        }
                        // Verify the argument is an error class instance
                        let arg_ty = self.check_expr(&args[0])?;
                        // Must be in a throws context
                        let throws_ty = self.throws_type.as_ref().ok_or_else(|| {
                            ".or_throw() can only be used in a function that declares 'throws'".to_string()
                        })?;
                        if !self.is_error_subtype(&arg_ty, throws_ty) {
                            return Err(format!(
                                ".or_throw() error type {:?} not compatible with throws {:?}",
                                arg_ty, throws_ty
                            ));
                        }
                        return Ok(*inner.clone());
                    }
                    _ => {
                        return Err(format!(
                            "Cannot access '{}' on nullable type {:?}. Resolve with .or(), .or_else(), .or_throw(), or match first",
                            field, obj_ty
                        ));
                    }
                }
            }
        }
        self.check_call_inner(func, args, false)
    }

    pub(crate) fn check_call_inner(&mut self, func: &Expr, args: &[Expr], bypass_async_check: bool) -> Result<Type, String> {
        self.check_call_inner_impl(func, args, bypass_async_check, false)
    }

    pub(crate) fn check_call_inner_throws_ok(&mut self, func: &Expr, args: &[Expr], bypass_async_check: bool) -> Result<Type, String> {
        self.check_call_inner_impl(func, args, bypass_async_check, true)
    }

    fn check_call_inner_impl(&mut self, func: &Expr, args: &[Expr], bypass_async_check: bool, bypass_throws_check: bool) -> Result<Type, String> {
        // Handle polymorphic builtins that can't be expressed in the type system yet
        if let Expr::Ident(name) = func {
            match name.as_str() {
                "len" => {
                    if args.len() != 1 {
                        return Err(format!("len() takes 1 argument, got {}", args.len()));
                    }
                    let aty = self.check_expr(&args[0])?;
                    match aty {
                        Type::String | Type::List(_) => return Ok(Type::Int),
                        _ => return Err(format!("len() expects String or List, got {:?}", aty)),
                    }
                }
                "to_string" => {
                    if args.len() != 1 {
                        return Err(format!("to_string() takes 1 argument, got {}", args.len()));
                    }
                    let aty = self.check_expr(&args[0])?;
                    match aty {
                        Type::Int | Type::Float | Type::Bool | Type::String => {
                            return Ok(Type::String)
                        }
                        _ => {
                            return Err(format!(
                                "to_string() expects Int, Float, Bool, or String, got {:?}",
                                aty
                            ))
                        }
                    }
                }
                _ => {}
            }
        }

        let fty = self.check_expr(func)?;
        if let Type::Function { params, ret, is_async: fn_is_async, throws: fn_throws } = fty {
            // Sync call to an async function is an error (unless bypassed by blocking/detached)
            if fn_is_async && !bypass_async_check && !self.is_in_async_context() {
                return Err(
                    "Cannot call async function synchronously from sync context. Use 'resolve' or 'async' modifier.".to_string()
                );
            }
            // Calling a throws function requires error handling (!, !.or(), !.or_else(), !.catch)
            if fn_throws.is_some() && !bypass_throws_check {
                return Err(
                    "Cannot call throwing function without error handling. Use !, !.or(), !.or_else(), or !.catch".to_string()
                );
            }
            if params.len() != args.len() {
                return Err(format!(
                    "Function arity mismatch: expected {}, got {}",
                    params.len(),
                    args.len()
                ));
            }
            // Build TypeVar bindings for generic instantiation
            let mut bindings: HashMap<String, Type> = HashMap::new();
            for (a, pty) in args.iter().zip(params.iter()) {
                let aty = self.check_expr(a)?;
                Self::unify_type(pty, &aty, &mut bindings)?;
            }
            // Substitute TypeVars in return type
            let resolved_ret = Self::substitute_typevars(&ret, &bindings);
            Ok(resolved_ret)
        } else {
            Err(format!("Tried to call non-function type: {:?}", fty))
        }
    }

    pub(crate) fn unify_type(
        expected: &Type,
        actual: &Type,
        bindings: &mut HashMap<String, Type>,
    ) -> Result<(), String> {
        match (expected, actual) {
            (Type::TypeVar(tv), _) => {
                if let Some(bound) = bindings.get(tv) {
                    if *bound != *actual {
                        return Err(format!(
                            "Type parameter '{}' bound to {:?} but got {:?}",
                            tv, bound, actual
                        ));
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
                    return Err(format!(
                        "Argument type mismatch: expected {:?}, got {:?}",
                        expected, actual
                    ));
                }
                for (e, a) in eargs.iter().zip(aargs.iter()) {
                    Self::unify_type(e, a, bindings)?;
                }
                Ok(())
            }
            (
                Type::Function { params: ep, ret: er, .. },
                Type::Function { params: ap, ret: ar, .. },
            ) => {
                if ep.len() != ap.len() {
                    return Err(format!(
                        "Function arity mismatch: expected {} params, got {}",
                        ep.len(), ap.len()
                    ));
                }
                for (e, a) in ep.iter().zip(ap.iter()) {
                    Self::unify_type(e, a, bindings)?;
                }
                Self::unify_type(er, ar, bindings)
            }
            _ => {
                if expected != actual {
                    Err(format!(
                        "Argument type mismatch: expected {:?}, got {:?}",
                        expected, actual
                    ))
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
                params,
                ret,
                is_async,
                throws,
            } => Type::Function {
                params: params
                    .iter()
                    .map(|p| Self::substitute_typevars(p, bindings))
                    .collect(),
                ret: Box::new(Self::substitute_typevars(ret, bindings)),
                is_async: *is_async,
                throws: throws.as_ref().map(|t| Box::new(Self::substitute_typevars(t, bindings))),
            },
            Type::Task(inner) => Type::Task(Box::new(Self::substitute_typevars(inner, bindings))),
            Type::Nullable(inner) => Type::Nullable(Box::new(Self::substitute_typevars(inner, bindings))),
            Type::Custom(name, args) => {
                let new_args = args.iter().map(|a| Self::substitute_typevars(a, bindings)).collect();
                Type::Custom(name.clone(), new_args)
            }
            _ => ty.clone(),
        }
    }

    pub(crate) fn resolve_func_type(&mut self, func: &Expr) -> Result<Type, String> {
        match func {
            Expr::Ident(name) => {
                // Check polymorphic builtins
                match name.as_str() {
                    "len" | "to_string" => Ok(Type::Void), // Not a throws function
                    _ => self.env.get_var(name).ok_or_else(|| format!("Unknown identifier '{}'", name)),
                }
            }
            Expr::Member { object, field } => self.check_member(object, field),
            _ => self.check_expr(func),
        }
    }
}
