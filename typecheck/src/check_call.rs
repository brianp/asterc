use std::collections::HashMap;

use ast::{Diagnostic, Expr, Span, Type, TypeEnv};

use crate::typechecker::TypeChecker;

/// Find the closest parameter name to a given argument name (edit distance <= 2).
fn closest_param_name<'a>(arg_name: &str, param_names: &'a [String]) -> Option<&'a str> {
    let mut best: Option<(&str, usize)> = None;
    for pn in param_names {
        let dist = TypeChecker::levenshtein(arg_name, pn);
        if dist <= 2 && dist > 0 && (best.is_none() || dist < best.unwrap().1) {
            best = Some((pn.as_str(), dist));
        }
    }
    best.map(|(name, _)| name)
}

impl TypeChecker {
    fn suspendable_call_fix(func: &Expr) -> String {
        match func {
            Expr::Ident(name, _) => {
                format!("Call with `blocking {name}()` or `async {name}()`.")
            }
            _ => "Call with `blocking f(...)` or `async f(...)`.".to_string(),
        }
    }

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
        args: &[(String, Span, Expr)],
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
                        let arg_ty = self.check_expr(&args[0].2)?;
                        if arg_ty.is_error() {
                            return Ok(Type::Error);
                        }
                        if arg_ty != **inner {
                            return Err(Diagnostic::error(format!(
                                ".{}() type mismatch: expected {}, got {}",
                                field, inner, arg_ty
                            ))
                            .with_code("E018")
                            .with_label(args[0].2.span(), format!("expected {}", inner)));
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
                        let arg_ty = self.check_expr(&args[0].2)?;
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
                                ".or_throw() error type {} not compatible with throws {}",
                                arg_ty, throws_ty
                            ))
                            .with_code("E013")
                            .with_label(args[0].2.span(), "incompatible error type"));
                        }
                        return Ok(*inner.clone());
                    }
                    _ => {
                        return Err(Diagnostic::error(format!(
                            "Cannot access '{}' on nullable type {}. Resolve with .or(), .or_else(), .or_throw(), or match first",
                            field, obj_ty
                        ))
                        .with_code("E018")
                        .with_label(object.span(), "nullable type"));
                    }
                }
            }

            // List[Nil] promotion: pushing into an empty list infers the element type
            if field == "push"
                && let Type::List(inner) = &obj_ty
                && **inner == Type::Nil
            {
                if args.len() != 1 {
                    return Err(Diagnostic::error(format!(
                        "push() takes 1 argument, got {}",
                        args.len()
                    ))
                    .with_code("E006")
                    .with_label(func.span(), "expected 1 argument"));
                }
                let arg_ty = self.check_expr(&args[0].2)?;
                if arg_ty.is_error() {
                    return Ok(Type::Void);
                }
                // Promote the variable from List[Nil] to List[T]
                if let Expr::Ident(var_name, _) = object.as_ref() {
                    let promoted = Type::List(Box::new(arg_ty));
                    self.env.set_var(var_name.clone(), promoted);
                }
                return Ok(Type::Void);
            }
        }
        self.check_call_inner(func, args, false)
    }

    pub(crate) fn check_call_inner(
        &mut self,
        func: &Expr,
        args: &[(String, Span, Expr)],
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
                    let aty = self.check_expr(&args[0].2)?;
                    if aty.is_error() {
                        return Ok(Type::Error);
                    }
                    match aty {
                        Type::String | Type::List(_) => return Ok(Type::Int),
                        _ => {
                            return Err(Diagnostic::error(format!(
                                "len() expects String or List, got {}",
                                aty
                            ))
                            .with_code("E005")
                            .with_label(args[0].2.span(), "expected String or List"));
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
                    let aty = self.check_expr(&args[0].2)?;
                    if aty.is_error() {
                        return Ok(Type::Error);
                    }
                    match aty {
                        Type::Int | Type::Float | Type::Bool | Type::String => {
                            return Ok(Type::String);
                        }
                        _ => {
                            return Err(Diagnostic::error(format!(
                                "to_string() expects Int, Float, Bool, or String, got {}",
                                aty
                            ))
                            .with_code("E005")
                            .with_label(args[0].2.span(), "unsupported type"));
                        }
                    }
                }
                "log" | "say" => {
                    if args.len() != 1 {
                        return Err(Diagnostic::error(format!(
                            "{}() takes 1 argument, got {}",
                            name,
                            args.len()
                        ))
                        .with_code("E006")
                        .with_label(func.span(), "expected 1 argument"));
                    }
                    let aty = self.check_expr(&args[0].2)?;
                    if aty.is_error() {
                        return Ok(Type::Void);
                    }
                    match aty {
                        Type::Int | Type::Float | Type::Bool | Type::String => {
                            return Ok(Type::Void);
                        }
                        _ => {
                            return Err(Diagnostic::error(format!(
                                "{}() expects Int, Float, Bool, or String, got {}",
                                name, aty
                            ))
                            .with_code("E005")
                            .with_label(args[0].2.span(), "unsupported type"));
                        }
                    }
                }
                "random" => {
                    return self.check_random_call(func, args);
                }
                "resolve_all" => {
                    if args.len() != 1 {
                        return Err(Diagnostic::error(format!(
                            "resolve_all() takes 1 argument, got {}",
                            args.len()
                        ))
                        .with_code("E006")
                        .with_label(func.span(), "expected 1 argument"));
                    }
                    let aty = self.check_expr(&args[0].2)?;
                    if aty.is_error() {
                        return Ok(Type::Error);
                    }
                    match aty {
                        Type::List(inner) => match *inner {
                            Type::Task(result) => {
                                return Ok(Type::List(result));
                            }
                            other => {
                                return Err(Diagnostic::error(format!(
                                    "resolve_all() expects List[Task[T]], got List[{other:?}]"
                                ))
                                .with_code("E005")
                                .with_label(args[0].2.span(), "expected List[Task[T]]"));
                            }
                        },
                        other => {
                            return Err(Diagnostic::error(format!(
                                "resolve_all() expects List[Task[T]], got {other:?}"
                            ))
                            .with_code("E005")
                            .with_label(args[0].2.span(), "expected List[Task[T]]"));
                        }
                    }
                }
                "resolve_first" => {
                    if args.len() != 1 {
                        return Err(Diagnostic::error(format!(
                            "resolve_first() takes 1 argument, got {}",
                            args.len()
                        ))
                        .with_code("E006")
                        .with_label(func.span(), "expected 1 argument"));
                    }
                    let aty = self.check_expr(&args[0].2)?;
                    if aty.is_error() {
                        return Ok(Type::Error);
                    }
                    match aty {
                        Type::List(inner) => match *inner {
                            Type::Task(result) => return Ok(*result),
                            other => {
                                return Err(Diagnostic::error(format!(
                                    "resolve_first() expects List[Task[T]], got List[{other:?}]"
                                ))
                                .with_code("E005")
                                .with_label(args[0].2.span(), "expected List[Task[T]]"));
                            }
                        },
                        other => {
                            return Err(Diagnostic::error(format!(
                                "resolve_first() expects List[Task[T]], got {other:?}"
                            ))
                            .with_code("E005")
                            .with_label(args[0].2.span(), "expected List[Task[T]]"));
                        }
                    }
                }
                // Mutex(value) → Mutex[T]
                "Mutex" => {
                    if args.len() != 1 {
                        return Err(Diagnostic::error(format!(
                            "Mutex() takes 1 argument (the initial value), got {}",
                            args.len()
                        ))
                        .with_code("E006")
                        .with_label(func.span(), "expected 1 argument"));
                    }
                    let val_ty = self.check_expr(&args[0].2)?;
                    if val_ty.is_error() {
                        return Ok(Type::Error);
                    }
                    return Ok(Type::Custom("Mutex".into(), vec![val_ty]));
                }
                // Channel(capacity: N) → Channel[T] (T inferred later from send/receive)
                "Channel" => {
                    if args.len() > 1 {
                        return Err(Diagnostic::error(format!(
                            "Channel() takes 0-1 arguments (optional capacity), got {}",
                            args.len()
                        ))
                        .with_code("E006")
                        .with_label(func.span(), "expected 0-1 arguments"));
                    }
                    if args.len() == 1 {
                        let cap_ty = self.check_expr(&args[0].2)?;
                        if cap_ty != Type::Int && !cap_ty.is_error() {
                            return Err(Diagnostic::error(format!(
                                "Channel capacity must be Int, got {}",
                                cap_ty
                            ))
                            .with_code("E005")
                            .with_label(args[0].2.span(), "expected Int"));
                        }
                    }
                    // Type parameter inferred from expected type
                    let elem_ty = if let Some(Type::Custom(_, ref type_args)) = self.expected_type
                        && !type_args.is_empty()
                    {
                        type_args[0].clone()
                    } else {
                        return Err(Diagnostic::error(
                            "cannot infer Channel element type; add a type annotation like `let ch: Channel[Int] = Channel()`"
                        .to_string())
                        .with_code("E005")
                        .with_label(func.span(), "element type unknown"));
                    };
                    return Ok(Type::Custom("Channel".into(), vec![elem_ty]));
                }
                "MultiSend" | "MultiReceive" => {
                    if args.len() > 1 {
                        return Err(Diagnostic::error(format!(
                            "{}() takes 0-1 arguments (optional capacity), got {}",
                            name,
                            args.len()
                        ))
                        .with_code("E006")
                        .with_label(func.span(), "expected 0-1 arguments"));
                    }
                    if args.len() == 1 {
                        let cap_ty = self.check_expr(&args[0].2)?;
                        if cap_ty != Type::Int && !cap_ty.is_error() {
                            return Err(Diagnostic::error(format!(
                                "{} capacity must be Int, got {}",
                                name, cap_ty
                            ))
                            .with_code("E005")
                            .with_label(args[0].2.span(), "expected Int"));
                        }
                    }
                    let elem_ty = if let Some(Type::Custom(_, ref type_args)) = self.expected_type
                        && !type_args.is_empty()
                    {
                        type_args[0].clone()
                    } else {
                        return Err(Diagnostic::error(format!(
                            "cannot infer {} element type; add a type annotation",
                            name
                        ))
                        .with_code("E005")
                        .with_label(func.span(), "element type unknown"));
                    };
                    return Ok(Type::Custom(name.clone(), vec![elem_ty]));
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
            for (_, _, arg_expr) in args {
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
                        for (arg_name, _, _) in args {
                            if let Some(idx) = pn.iter().position(|n| n == arg_name) {
                                let mut bindings = HashMap::new();
                                if Self::unify_type(
                                    &params[idx],
                                    &arg_types[args
                                        .iter()
                                        .position(|(n, _, _)| n == arg_name)
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
                for (arg_name, arg_name_span, arg_expr) in args {
                    let param_idx = param_names.iter().position(|n| n == arg_name);
                    if let Some(idx) = param_idx {
                        let pty = &params[idx];
                        let aty = self.check_expr(arg_expr)?;
                        if aty.is_error() {
                            return Ok(Type::Error);
                        }
                        let mut bindings = HashMap::new();
                        Self::unify_type_with_env(pty, &aty, &mut bindings, Some(&self.env))
                            .map_err(|_| {
                                Diagnostic::error(format!(
                                    "Argument '{arg_name}' expects {pty}, got {aty}",
                                ))
                                .with_code("E001")
                                .with_label(arg_expr.span(), format!("expected {pty}, got {aty}"))
                            })?;
                    } else {
                        let suggestion = closest_param_name(arg_name, param_names);
                        let label_msg = if let Some(s) = suggestion {
                            format!("did you mean '{s}'?")
                        } else {
                            "unknown argument name".to_string()
                        };
                        let diag = Diagnostic::error(format!(
                            "Unknown argument '{}' in {}.from()",
                            arg_name, type_name
                        ))
                        .with_code("E006")
                        .with_label(*arg_name_span, label_msg);
                        return Err(diag);
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
            suspendable,
        } = fty
        {
            self.last_call_suspendable = suspendable;
            if suspendable && !bypass_throws_check {
                return Err(Diagnostic::error(format!(
                    "Plain call crosses a suspension boundary. {}",
                    Self::suspendable_call_fix(func)
                ))
                .with_code("E012")
                .with_label(
                    func.span(),
                    "suspendable callee requires an explicit call site",
                ));
            }
            if fn_throws.is_some() && !bypass_throws_check {
                return Err(Diagnostic::error(
                    "Cannot call throwing function without error handling. Use !, !.or(), !.or_else(), or !.catch".to_string()
                )
                .with_code("E013")
                .with_label(func.span(), "throwing function requires error handling"));
            }
            // Detect whether this is a constructor-style call (uppercase first letter)
            let is_constructor = matches!(
                func,
                Expr::Ident(name, _) if name.starts_with(|c: char| c.is_uppercase())
            );

            // If any positional args on a non-constructor call, reject early with hint
            if !is_constructor {
                for (arg_name, _, arg_expr) in args {
                    if arg_name.starts_with('_')
                        && !param_names.contains(arg_name)
                        && let Ok(pos) = arg_name[1..].parse::<usize>()
                    {
                        let hint = if pos < param_names.len() {
                            format!(
                                "All arguments must be named (e.g. `{}: value`)",
                                param_names[pos]
                            )
                        } else {
                            "All arguments must be named (e.g. `name: value`)".to_string()
                        };
                        let label = if pos < param_names.len() {
                            format!("add `{}: ` before this", param_names[pos])
                        } else {
                            "expected argument name".to_string()
                        };
                        return Err(Diagnostic::error(hint)
                            .with_code("P001")
                            .with_label(arg_expr.span(), label));
                    }
                }
            }

            if params.len() != args.len() {
                // Check if the difference can be explained by default params
                let func_name: Option<String> = match func {
                    Expr::Ident(name, _) => Some(name.clone()),
                    Expr::Member { object, field, .. } => {
                        // For method calls, try qualified name like "ClassName.method"
                        let obj_type = self.check_expr(object).ok();
                        if let Some(ast::Type::Custom(class_name, _)) = obj_type {
                            Some(format!("{}.{}", class_name, field))
                        } else {
                            None
                        }
                    }
                    _ => None,
                };
                let default_param_names = func_name
                    .as_deref()
                    .and_then(|n| self.default_params.get(n));

                let provided: std::collections::HashSet<&str> =
                    args.iter().map(|(n, _, _)| n.as_str()).collect();
                let expected: std::collections::HashSet<&str> =
                    param_names.iter().map(|n| n.as_str()).collect();
                let missing: Vec<&&str> = expected.difference(&provided).collect();
                let extra: Vec<&&str> = provided.difference(&expected).collect();

                // If all missing params have defaults and there are no extra args, allow it
                let all_missing_have_defaults = if let Some(defaults) = default_param_names {
                    extra.is_empty() && missing.iter().all(|m| defaults.contains(**m))
                } else {
                    false
                };

                if !all_missing_have_defaults {
                    let mut msg = format!(
                        "Function arity mismatch: expected {}, got {}",
                        params.len(),
                        args.len()
                    );
                    if !missing.is_empty() {
                        // Filter out params that have defaults from the "missing" list
                        let truly_missing: Vec<&&str> = if let Some(defaults) = default_param_names
                        {
                            missing
                                .iter()
                                .filter(|m| !defaults.contains(***m))
                                .copied()
                                .collect()
                        } else {
                            missing.clone()
                        };
                        if !truly_missing.is_empty() {
                            msg.push_str(&format!(
                                ", missing: {}",
                                truly_missing
                                    .iter()
                                    .map(|s| format!("'{}'", s))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ));
                        }
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
            }
            // Match args by name, order-independent.
            // Positional args (synthesized names like `_0`, `_1`) map to params by index.
            let mut bindings: HashMap<String, Type> = HashMap::new();
            for (arg_name, arg_name_span, arg_expr) in args {
                let param_idx = if arg_name.starts_with('_') && !param_names.contains(arg_name) {
                    if let Ok(pos) = arg_name[1..].parse::<usize>() {
                        if pos < param_names.len() {
                            Some(pos)
                        } else {
                            None
                        }
                    } else {
                        param_names.iter().position(|n| n == arg_name)
                    }
                } else {
                    param_names.iter().position(|n| n == arg_name)
                };
                let Some(idx) = param_idx else {
                    // Pick a suggestion: edit-distance match first, then single-param fallback
                    let suggestion = closest_param_name(arg_name, &param_names).or_else(|| {
                        if param_names.len() == 1 {
                            Some(param_names[0].as_str())
                        } else {
                            None
                        }
                    });
                    let label_msg = if let Some(s) = suggestion {
                        format!("did you mean '{s}'?")
                    } else {
                        "unknown argument name".to_string()
                    };
                    let expected_list = param_names
                        .iter()
                        .map(|n| format!("'{}'", n))
                        .collect::<Vec<_>>()
                        .join(", ");
                    let diag = Diagnostic::error(format!(
                        "Unknown argument '{}'. Expected one of: {}",
                        arg_name, expected_list
                    ))
                    .with_code("E006")
                    .with_label(*arg_name_span, label_msg);
                    return Err(diag);
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
                Self::unify_type_with_env(pty, &aty, &mut bindings, Some(&self.env)).map_err(
                    |_| {
                        Diagnostic::error(
                            format!("Argument '{arg_name}' expects {pty}, got {aty}",),
                        )
                        .with_code("E001")
                        .with_label(arg_expr.span(), format!("expected {pty}, got {aty}"))
                    },
                )?;
                // Mark task idents as consumed when passed as arguments
                self.mark_task_ident_consumed(arg_expr);
            }
            // Validate generic constraints after all bindings are established
            self.check_typevar_constraints(&params, &bindings)?;
            let resolved_ret = Self::substitute_typevars(&ret, &bindings);
            Ok(resolved_ret)
        } else {
            Err(
                Diagnostic::error(format!("Tried to call non-function type: {}", fty))
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
                            "Type parameter '{}' bound to {} but got {}",
                            tv, bound, actual
                        ))
                        .with_code("E001"));
                    }
                } else {
                    // Occurs check: prevent infinite types like T = List[T]
                    if Self::type_contains_var(actual, tv) {
                        return Err(Diagnostic::error(format!(
                            "Type parameter '{}' occurs in {}, creating an infinite type",
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
                            "Type parameter '{}' bound to {} but got {}",
                            tv, bound, expected
                        ))
                        .with_code("E001"));
                    }
                } else {
                    // Occurs check: prevent infinite types
                    if Self::type_contains_var(expected, tv) {
                        return Err(Diagnostic::error(format!(
                            "Type parameter '{}' occurs in {}, creating an infinite type",
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
                        "Argument type mismatch: expected {}, got {}",
                        expected, actual
                    ))
                    .with_code("E001"));
                }
                if eargs.len() != aargs.len() {
                    return Err(Diagnostic::error(format!(
                        "Argument type mismatch: expected {}, got {}",
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
                        "Argument type mismatch: expected {}, got {}",
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
                        "Type {} does not satisfy constraint '{} extends {}': \
                         {} is not a subclass of {}",
                        actual, type_param, class_name, actual, class_name
                    ))
                    .with_code("E024"));
                }
            }
            ast::TypeConstraint::Includes(trait_name, trait_args) => {
                if !self.type_includes_trait(actual, trait_name) {
                    return Err(Diagnostic::error(format!(
                        "Type {} does not satisfy constraint '{} includes {}': \
                         {} does not include {}",
                        actual, type_param, trait_name, actual, trait_name
                    ))
                    .with_code("E024"));
                }
                // Validate parametric trait type args if present
                if !trait_args.is_empty()
                    && !self.type_includes_parametric_trait(actual, trait_name, trait_args)
                {
                    let args_str: Vec<String> =
                        trait_args.iter().map(|t| format!("{}", t)).collect();
                    return Err(Diagnostic::error(format!(
                        "Type {} does not satisfy constraint '{} includes {}[{}]': \
                         {} does not include {}[{}]",
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
                    "len" | "to_string" | "log" | "say" | "random" => Ok(Type::Void),
                    "resolve_all" => Ok(Type::Function {
                        param_names: vec!["tasks".into()],
                        params: vec![Type::List(Box::new(Type::Task(Box::new(Type::Int))))],
                        ret: Box::new(Type::Void),
                        throws: Some(Box::new(Type::Custom("CancelledError".into(), Vec::new()))),
                        suspendable: true,
                    }),
                    "resolve_first" => Ok(Type::Function {
                        param_names: vec!["tasks".into()],
                        params: vec![Type::List(Box::new(Type::Task(Box::new(Type::Int))))],
                        ret: Box::new(Type::Void),
                        throws: Some(Box::new(Type::Custom("CancelledError".into(), Vec::new()))),
                        suspendable: true,
                    }),
                    _ => self.env.get_var(name).cloned().ok_or_else(|| {
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

    fn check_random_call(
        &mut self,
        func: &Expr,
        args: &[(String, Span, Expr)],
    ) -> Result<Type, Diagnostic> {
        let target = self.expected_type.clone();

        // If we have an explicit type context, use it
        if let Some(ref ty) = target {
            match ty {
                Type::Int => {
                    return self.check_random_int(func, args);
                }
                Type::Float => {
                    return self.check_random_float(func, args);
                }
                Type::Bool => {
                    if !args.is_empty() {
                        return Err(Diagnostic::error("random() for Bool takes no arguments")
                            .with_code("E006")
                            .with_label(func.span(), "remove arguments"));
                    }
                    return Ok(Type::Bool);
                }
                _ => {}
            }
        }

        // No type context — infer from arguments
        if args.is_empty() {
            // No args, no context → Bool (the only zero-arg form)
            return Ok(Type::Bool);
        }

        // Check argument types to infer the return type
        let sample_arg = args.iter().find(|(n, _, _)| n == "max" || n == "min");
        if let Some((_, _, expr)) = sample_arg {
            let aty = self.check_expr(expr)?;
            match aty {
                Type::Int => return self.check_random_int(func, args),
                Type::Float => return self.check_random_float(func, args),
                _ => {
                    return Err(Diagnostic::error(format!(
                        "random() argument must be Int or Float, got {aty:?}"
                    ))
                    .with_code("E005")
                    .with_label(expr.span(), "expected Int or Float"));
                }
            }
        }

        Err(Diagnostic::error(
            "Cannot infer type for random(). Use Int or Float arguments, \
             or add a type annotation, e.g. `let n: Int = random(max: 100)`",
        )
        .with_code("E005")
        .with_label(func.span(), "needs type context"))
    }

    fn check_random_int(
        &mut self,
        func: &Expr,
        args: &[(String, Span, Expr)],
    ) -> Result<Type, Diagnostic> {
        for (name, _, expr) in args {
            if name == "max" || name == "min" {
                let aty = self.check_expr(expr)?;
                if aty != Type::Int {
                    return Err(Diagnostic::error(format!(
                        "random() {name}: argument must be Int, got {aty:?}"
                    ))
                    .with_code("E005")
                    .with_label(expr.span(), "expected Int"));
                }
            }
        }
        if !args.iter().any(|(n, _, _)| n == "max") {
            return Err(
                Diagnostic::error("random() for Int requires a max: argument")
                    .with_code("E006")
                    .with_label(func.span(), "add max: argument"),
            );
        }
        Ok(Type::Int)
    }

    fn check_random_float(
        &mut self,
        func: &Expr,
        args: &[(String, Span, Expr)],
    ) -> Result<Type, Diagnostic> {
        for (name, _, expr) in args {
            if name == "max" || name == "min" {
                let aty = self.check_expr(expr)?;
                if aty != Type::Float {
                    return Err(Diagnostic::error(format!(
                        "random() {name}: argument must be Float, got {aty:?}"
                    ))
                    .with_code("E005")
                    .with_label(expr.span(), "expected Float"));
                }
            }
        }
        if !args.iter().any(|(n, _, _)| n == "max") {
            return Err(
                Diagnostic::error("random() for Float requires a max: argument")
                    .with_code("E006")
                    .with_label(func.span(), "add max: argument"),
            );
        }
        Ok(Type::Float)
    }
}
