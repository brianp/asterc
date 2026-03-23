use super::*;

impl Lowerer {
    pub(crate) fn lower_expr(&mut self, expr: &Expr) -> Result<FirExpr, LowerError> {
        match expr {
            Expr::Int(n, _) => Ok(FirExpr::IntLit(*n)),
            Expr::Float(f, _) => Ok(FirExpr::FloatLit(*f)),
            Expr::Bool(b, _) => Ok(FirExpr::BoolLit(*b)),
            Expr::Str(s, _) => Ok(FirExpr::StringLit(s.clone())),
            Expr::Nil(_) => Ok(FirExpr::NilLit),

            Expr::Ident(name, _) => {
                if let Some(&local_id) = self.locals.get(name.as_str()) {
                    // Resolve the type from the type env
                    let ty = self.resolve_var_type(name);
                    Ok(FirExpr::LocalVar(local_id, ty))
                } else if let Some(&self_id) = self.locals.get("self") {
                    // Inside a method body — resolve bare field names as self.field
                    let self_expr = Expr::Ident("self".to_string(), expr.span());
                    match self.resolve_field_access(&self_expr, name) {
                        Ok((offset, ty)) => Ok(FirExpr::FieldGet {
                            object: Box::new(FirExpr::LocalVar(self_id, FirType::Ptr)),
                            offset,
                            ty,
                        }),
                        Err(_) => Err(LowerError::UnboundVariable(name.clone(), expr.span())),
                    }
                } else {
                    Err(LowerError::UnboundVariable(name.clone(), expr.span()))
                }
            }

            Expr::BinaryOp {
                left, op, right, ..
            } => {
                if matches!(op, ast::BinOp::Pow) {
                    let fir_left = self.lower_expr(left)?;
                    let fir_right = self.lower_expr(right)?;
                    let lt = self.infer_fir_type(&fir_left);
                    let rt = self.infer_fir_type(&fir_right);
                    let any_float = lt == FirType::F64 || rt == FirType::F64;
                    if any_float {
                        // Promote Int operands to Float, call aster_pow_float
                        let fl = if lt == FirType::I64 {
                            FirExpr::IntToFloat(Box::new(fir_left))
                        } else {
                            fir_left
                        };
                        let fr = if rt == FirType::I64 {
                            FirExpr::IntToFloat(Box::new(fir_right))
                        } else {
                            fir_right
                        };
                        return Ok(FirExpr::RuntimeCall {
                            name: "aster_pow_float".to_string(),
                            args: vec![fl, fr],
                            ret_ty: FirType::F64,
                        });
                    }
                    return Ok(FirExpr::RuntimeCall {
                        name: "aster_pow_int".to_string(),
                        args: vec![fir_left, fir_right],
                        ret_ty: FirType::I64,
                    });
                }
                // Dispatch == / != on custom types to ClassName.eq(self, other)
                if matches!(op, ast::BinOp::Eq | ast::BinOp::Neq)
                    && let Ok(class_name) = self.resolve_class_name(left)
                {
                    let eq_name = format!("{}.eq", class_name);
                    if let Some(&func_id) = self.functions.get(&eq_name) {
                        let fir_left = self.lower_expr(left)?;
                        let fir_right = self.lower_expr(right)?;
                        let eq_result = FirExpr::Call {
                            func: func_id,
                            args: vec![fir_left, fir_right],
                            ret_ty: FirType::Bool,
                        };
                        return if matches!(op, ast::BinOp::Neq) {
                            Ok(FirExpr::UnaryOp {
                                op: UnaryOp::Not,
                                operand: Box::new(eq_result),
                                result_ty: FirType::Bool,
                            })
                        } else {
                            Ok(eq_result)
                        };
                    }
                }
                // Dispatch <, >, <=, >= on custom types to ClassName.cmp(self, other)
                if matches!(
                    op,
                    ast::BinOp::Lt | ast::BinOp::Gt | ast::BinOp::Lte | ast::BinOp::Gte
                ) && let Ok(class_name) = self.resolve_class_name(left)
                {
                    let cmp_name = format!("{}.cmp", class_name);
                    if let Some(&func_id) = self.functions.get(&cmp_name) {
                        let fir_left = self.lower_expr(left)?;
                        let fir_right = self.lower_expr(right)?;
                        // cmp returns Ordering (tag: 0=Less, 1=Equal, 2=Greater)
                        // Extract the tag from the struct at offset 0
                        let cmp_result = FirExpr::Call {
                            func: func_id,
                            args: vec![fir_left, fir_right],
                            ret_ty: FirType::Ptr,
                        };
                        let tag = FirExpr::FieldGet {
                            object: Box::new(cmp_result),
                            offset: 0,
                            ty: FirType::I64,
                        };
                        // Compare the tag against expected value
                        let (cmp_op, cmp_val) = match op {
                            ast::BinOp::Lt => (BinOp::Eq, 0i64),   // Less = 0
                            ast::BinOp::Gt => (BinOp::Eq, 2i64),   // Greater = 2
                            ast::BinOp::Lte => (BinOp::Neq, 2i64), // not Greater
                            ast::BinOp::Gte => (BinOp::Neq, 0i64), // not Less
                            _ => unreachable!(),
                        };
                        return Ok(FirExpr::BinaryOp {
                            left: Box::new(tag),
                            op: cmp_op,
                            right: Box::new(FirExpr::IntLit(cmp_val)),
                            result_ty: FirType::Bool,
                        });
                    }
                }
                let fir_left = self.lower_expr(left)?;
                let fir_right = self.lower_expr(right)?;
                let fir_op = self.lower_binop(op);
                // Int/Float coercion: promote Int operand to Float
                let lt = self.infer_fir_type(&fir_left);
                let rt = self.infer_fir_type(&fir_right);
                let (fir_left, fir_right) = match (&lt, &rt) {
                    (FirType::I64, FirType::F64) => {
                        (FirExpr::IntToFloat(Box::new(fir_left)), fir_right)
                    }
                    (FirType::F64, FirType::I64) => {
                        (fir_left, FirExpr::IntToFloat(Box::new(fir_right)))
                    }
                    _ => (fir_left, fir_right),
                };
                let result_ty = self.infer_binop_type(&fir_op, &fir_left, &fir_right);
                // String + String → aster_string_concat runtime call
                if matches!(fir_op, BinOp::Add) && matches!(result_ty, FirType::Ptr) {
                    return Ok(FirExpr::RuntimeCall {
                        name: "aster_string_concat".to_string(),
                        args: vec![fir_left, fir_right],
                        ret_ty: FirType::Ptr,
                    });
                }
                Ok(FirExpr::BinaryOp {
                    left: Box::new(fir_left),
                    op: fir_op,
                    right: Box::new(fir_right),
                    result_ty,
                })
            }

            Expr::UnaryOp { op, operand, .. } => {
                let fir_operand = self.lower_expr(operand)?;
                let fir_op = self.lower_unaryop(op);
                let result_ty = self.infer_unaryop_type(&fir_op, &fir_operand);
                Ok(FirExpr::UnaryOp {
                    op: fir_op,
                    operand: Box::new(fir_operand),
                    result_ty,
                })
            }

            Expr::Call { func, args, .. } => {
                self.pending_stmts.push(FirStmt::Expr(FirExpr::Safepoint));
                // Method call: obj.method(args)
                if let Expr::Member { object, field, .. } = func.as_ref() {
                    return self.lower_method_call(object, field, args);
                }

                // Resolve function name
                if let Expr::Ident(name, _) = func.as_ref() {
                    if name == "to_string"
                        && let Some((_, _, arg)) = args.first()
                    {
                        let fir_arg = self.lower_expr(arg)?;
                        return Ok(self.to_string_expr(arg, fir_arg));
                    }

                    // random(max: n) or random(min: a, max: b) → aster_random_int/float/bool
                    if name == "random" {
                        let ret_ty = self
                            .type_table
                            .get(&expr.span())
                            .map(|ty| self.lower_type(ty))
                            .unwrap_or(FirType::I64);
                        let max_arg = args.iter().find(|(n, _, _)| n == "max");
                        let min_arg = args.iter().find(|(n, _, _)| n == "min");
                        return match ret_ty {
                            FirType::I64 => {
                                let fir_max = if let Some((_, _, e)) = max_arg {
                                    self.lower_expr(e)?
                                } else {
                                    FirExpr::IntLit(100)
                                };
                                let raw_random = FirExpr::RuntimeCall {
                                    name: "aster_random_int".to_string(),
                                    args: vec![if let Some(min_val) = &min_arg {
                                        // random_int(max - min) + min
                                        let fir_min = self.lower_expr(&min_val.2)?;
                                        FirExpr::BinaryOp {
                                            left: Box::new(fir_max),
                                            op: BinOp::Sub,
                                            right: Box::new(fir_min),
                                            result_ty: FirType::I64,
                                        }
                                    } else {
                                        fir_max
                                    }],
                                    ret_ty: FirType::I64,
                                };
                                if let Some((_, _, min_expr)) = min_arg {
                                    let fir_min = self.lower_expr(min_expr)?;
                                    Ok(FirExpr::BinaryOp {
                                        left: Box::new(raw_random),
                                        op: BinOp::Add,
                                        right: Box::new(fir_min),
                                        result_ty: FirType::I64,
                                    })
                                } else {
                                    Ok(raw_random)
                                }
                            }
                            FirType::F64 => {
                                let fir_max = if let Some((_, _, e)) = max_arg {
                                    self.lower_expr(e)?
                                } else {
                                    FirExpr::FloatLit(1.0)
                                };
                                Ok(FirExpr::RuntimeCall {
                                    name: "aster_random_float".to_string(),
                                    args: vec![fir_max],
                                    ret_ty: FirType::F64,
                                })
                            }
                            _ => Ok(FirExpr::RuntimeCall {
                                name: "aster_random_bool".to_string(),
                                args: vec![],
                                ret_ty: FirType::Bool,
                            }),
                        };
                    }

                    // Mutex(value: x) → aster_mutex_new(x)
                    if name == "Mutex" {
                        let value_arg = args
                            .iter()
                            .find(|(n, _, _)| n == "value")
                            .map(|(_, _, e)| e)
                            .or_else(|| args.first().map(|(_, _, e)| e));
                        if let Some(val) = value_arg {
                            let fir_val = self.lower_expr(val)?;
                            return Ok(FirExpr::RuntimeCall {
                                name: "aster_mutex_new".to_string(),
                                args: vec![fir_val],
                                ret_ty: FirType::Ptr,
                            });
                        }
                    }

                    // Channel(capacity?: x) → aster_channel_new(x)
                    if name == "Channel" || name == "MultiSend" || name == "MultiReceive" {
                        let cap_arg = args
                            .iter()
                            .find(|(n, _, _)| n == "capacity")
                            .map(|(_, _, e)| e);
                        let fir_cap = if let Some(cap) = cap_arg {
                            self.lower_expr(cap)?
                        } else {
                            FirExpr::IntLit(0) // 0 = unbuffered
                        };
                        return Ok(FirExpr::RuntimeCall {
                            name: "aster_channel_new".to_string(),
                            args: vec![fir_cap],
                            ret_ty: FirType::Ptr,
                        });
                    }

                    if name == "resolve_all"
                        && let Some((_, _, arg)) = args.first()
                    {
                        let fir_arg = self.lower_expr(arg)?;
                        let ret_ty = self
                            .type_table
                            .get(&expr.span())
                            .map(|ty| self.lower_type(ty))
                            .unwrap_or(FirType::Ptr);
                        return Ok(FirExpr::RuntimeCall {
                            name: self.resolve_all_runtime_name(arg).to_string(),
                            args: vec![fir_arg],
                            ret_ty,
                        });
                    }

                    if name == "resolve_first"
                        && let Some((_, _, arg)) = args.first()
                    {
                        let fir_arg = self.lower_expr(arg)?;
                        let ret_ty = self
                            .type_table
                            .get(&expr.span())
                            .map(|ty| self.lower_type(ty))
                            .unwrap_or(FirType::I64);
                        return Ok(FirExpr::RuntimeCall {
                            name: self.resolve_first_runtime_name(arg).to_string(),
                            args: vec![fir_arg],
                            ret_ty,
                        });
                    }

                    // Check if this is a closure call (statically resolved)
                    if let Some((func_id, env_local, _captures)) =
                        self.closure_info.get(name).cloned()
                    {
                        let mut fir_args = Vec::new();
                        // First arg: env pointer (or nil if no captures)
                        if let Some(env_id) = env_local {
                            fir_args.push(FirExpr::LocalVar(env_id, FirType::Ptr));
                        } else {
                            fir_args.push(FirExpr::NilLit);
                        }
                        // Then the explicit args
                        for (_, _, arg) in args {
                            fir_args.push(self.lower_expr(arg)?);
                        }
                        let ret_ty = self.resolve_function_ret_type_by_id(func_id);
                        return Ok(FirExpr::Call {
                            func: func_id,
                            args: fir_args,
                            ret_ty,
                        });
                    }

                    // Check if this is a class constructor call
                    if let Some(&class_id) = self.classes.get(name.as_str()) {
                        // Constructor: lower args in field order from class layout
                        let field_layout = self
                            .class_fields
                            .get(&class_id)
                            .cloned()
                            .unwrap_or_default();
                        let mut fir_fields = Vec::new();
                        // Match named args to field order from the class layout
                        for (field_name, _, _) in &field_layout {
                            // Find the arg matching this field name
                            if let Some((_, _, expr)) =
                                args.iter().find(|(arg_name, _, _)| arg_name == field_name)
                            {
                                fir_fields.push(self.lower_expr(expr)?);
                            } else {
                                // Positional fallback: use args in order
                                break;
                            }
                        }
                        // If named matching didn't get all fields, try positional
                        if fir_fields.len() != field_layout.len() {
                            fir_fields.clear();
                            for (_, _, arg) in args {
                                fir_fields.push(self.lower_expr(arg)?);
                            }
                        }
                        Ok(FirExpr::Construct {
                            class: class_id,
                            fields: fir_fields,
                            ty: FirType::Ptr,
                        })
                    } else if let Some(&func_id) = self.functions.get(name.as_str()) {
                        let fir_args = self.lower_call_args_with_defaults(name, args)?;
                        let ret_ty = self.resolve_function_ret_type(name);
                        // Generic type erasure: if params are TypeVar, the FIR
                        // signature uses I64. Bitcast Float/Bool args to I64 so
                        // Cranelift types match, and bitcast the result back.
                        let (fir_args, cast_ret) = self.apply_generic_erasure_casts(
                            name,
                            fir_args,
                            ret_ty.clone(),
                            &expr.span(),
                        );
                        let needs_ret_cast = cast_ret != ret_ty;
                        let call = FirExpr::Call {
                            func: func_id,
                            args: fir_args,
                            ret_ty,
                        };
                        if needs_ret_cast {
                            Ok(FirExpr::Bitcast {
                                value: Box::new(call),
                                to: cast_ret,
                            })
                        } else {
                            Ok(call)
                        }
                    } else if self.locals.contains_key(name.as_str()) {
                        // Local variable with function type — closure call (dynamic dispatch)
                        let closure_var = self.lower_expr(func)?;
                        let fir_args: Result<Vec<_>, _> = args
                            .iter()
                            .map(|(_, _, arg)| self.lower_expr(arg))
                            .collect();
                        let ret_ty = self.resolve_closure_ret_type(name);
                        Ok(FirExpr::ClosureCall {
                            closure: Box::new(closure_var),
                            args: fir_args?,
                            ret_ty,
                        })
                    } else {
                        // Could be a runtime call (say, etc.)
                        let fir_args: Result<Vec<_>, _> = args
                            .iter()
                            .map(|(_, _, arg)| self.lower_expr(arg))
                            .collect();
                        Ok(FirExpr::RuntimeCall {
                            name: name.clone(),
                            args: fir_args?,
                            ret_ty: FirType::Void,
                        })
                    }
                } else {
                    Err(LowerError::UnsupportedFeature(
                        UnsupportedFeatureKind::Other("non-ident function call target".into()),
                        expr.span(),
                    ))
                }
            }

            Expr::ListLiteral(elems, _) => {
                if elems.is_empty() {
                    Ok(FirExpr::ListNew {
                        elements: vec![],
                        elem_ty: FirType::Void,
                    })
                } else {
                    let first = self.lower_expr(&elems[0])?;
                    let elem_ty = self.infer_fir_type(&first);
                    let mut fir_elems = vec![first];
                    for elem in &elems[1..] {
                        fir_elems.push(self.lower_expr(elem)?);
                    }
                    Ok(FirExpr::ListNew {
                        elements: fir_elems,
                        elem_ty,
                    })
                }
            }

            Expr::Index { object, index, .. } => {
                // Check if the object is a map (use local_ast_types to determine)
                let is_map = if let Expr::Ident(name, _) = object.as_ref() {
                    matches!(
                        self.local_ast_types.get(name.as_str()),
                        Some(Type::Map(_, _))
                    )
                } else {
                    false
                };

                let fir_obj = self.lower_expr(object)?;
                let fir_idx = self.lower_expr(index)?;
                if is_map {
                    Ok(FirExpr::RuntimeCall {
                        name: "aster_map_get".to_string(),
                        args: vec![fir_obj, fir_idx],
                        ret_ty: FirType::I64,
                    })
                } else {
                    // Resolve element type from AST type info when available
                    let elem_ty = if let Expr::Ident(name, _) = object.as_ref() {
                        if let Some(Type::List(inner)) = self.local_ast_types.get(name.as_str()) {
                            self.lower_type(inner)
                        } else {
                            FirType::I64
                        }
                    } else {
                        FirType::I64
                    };
                    Ok(FirExpr::ListGet {
                        list: Box::new(fir_obj),
                        index: Box::new(fir_idx),
                        elem_ty,
                    })
                }
            }

            Expr::Lambda {
                params,
                ret_type,
                body,
                span,
                ..
            } => {
                // If the typechecker resolved this lambda's types (e.g. inferred
                // params from call context), use the resolved function type.
                if let Some(Type::Function {
                    params: resolved_param_types,
                    ret: resolved_ret,
                    ..
                }) = self.type_table.get(span)
                {
                    let resolved_params: Vec<(String, Type)> = params
                        .iter()
                        .enumerate()
                        .map(|(i, (name, ty))| {
                            if *ty == Type::Inferred {
                                let resolved =
                                    resolved_param_types.get(i).cloned().unwrap_or(ty.clone());
                                (name.clone(), resolved)
                            } else {
                                (name.clone(), ty.clone())
                            }
                        })
                        .collect();
                    let resolved_ret = if *ret_type == Type::Inferred {
                        resolved_ret.as_ref().clone()
                    } else {
                        ret_type.clone()
                    };
                    self.lower_lambda(&resolved_params, &resolved_ret, body)
                } else {
                    self.lower_lambda(params, ret_type, body)
                }
            }

            Expr::Match {
                scrutinee, arms, ..
            } => self.lower_match(scrutinee, arms),

            Expr::StringInterpolation { parts, .. } => self.lower_string_interpolation(parts),

            Expr::AsyncCall { func, args, .. } => {
                let args = self.lower_explicit_call_args(func, args)?;
                let func_id = self.resolve_called_function_id(func)?;
                let result_ty = self.resolve_called_function_ret_type(func, func_id);
                Ok(FirExpr::Spawn {
                    func: func_id,
                    args,
                    ret_ty: FirType::Ptr,
                    result_ty,
                    scope: self.async_scope_stack.last().copied(),
                })
            }

            Expr::BlockingCall { func, args, .. } => {
                // Method calls (e.g. blocking m.lock(...)): dispatch to method lowering
                if let Expr::Member { object, field, .. } = func.as_ref() {
                    self.pending_stmts.push(FirStmt::Expr(FirExpr::Safepoint));
                    return self.lower_method_call(object, field, args);
                }
                let args = self.lower_explicit_call_args(func, args)?;
                let func_id = self.resolve_called_function_id(func)?;
                let ret_ty = self.resolve_called_function_ret_type(func, func_id);
                Ok(FirExpr::BlockOn {
                    func: func_id,
                    args,
                    ret_ty,
                })
            }

            Expr::DetachedCall { func, args, .. } => {
                let args = self.lower_explicit_call_args(func, args)?;
                let func_id = self.resolve_called_function_id(func)?;
                let result_ty = self.resolve_called_function_ret_type(func, func_id);
                Ok(FirExpr::Spawn {
                    func: func_id,
                    args,
                    ret_ty: FirType::Ptr,
                    result_ty,
                    scope: self.async_scope_stack.last().copied(),
                })
            }

            Expr::Resolve { expr: inner, .. } => {
                let task = self.lower_expr(inner)?;
                let ret_ty = self.resolve_task_result_type(inner, &task);
                Ok(FirExpr::ResolveTask {
                    task: Box::new(task),
                    ret_ty,
                })
            }

            // Propagate: `expr!` → evaluate, check error flag, trap if error
            Expr::Propagate(inner, _) => {
                let fir_inner = self.lower_expr(inner)?;
                let result_ty = self.infer_fir_type(&fir_inner);
                let result_id = self.alloc_local();
                self.local_types.insert(result_id, result_ty.clone());

                // let __result = inner_expr
                self.pending_stmts.push(FirStmt::Let {
                    name: result_id,
                    ty: result_ty.clone(),
                    value: fir_inner,
                });

                // if aster_error_check(): cleanup then aster_panic()
                let check = FirExpr::RuntimeCall {
                    name: "aster_error_check".to_string(),
                    args: vec![],
                    ret_ty: FirType::Bool,
                };
                // Build cleanup + panic as the error branch body.
                // Save pending_stmts so cleanup emission doesn't steal earlier stmts.
                let saved = std::mem::take(&mut self.pending_stmts);
                self.emit_cleanup_calls();
                if let Some(scope_id) = self.function_scope_id {
                    self.emit_scope_exit(scope_id);
                }
                let mut error_body = std::mem::take(&mut self.pending_stmts);
                self.pending_stmts = saved;
                error_body.push(FirStmt::Expr(FirExpr::RuntimeCall {
                    name: "aster_panic".to_string(),
                    args: vec![],
                    ret_ty: FirType::Void,
                }));
                self.pending_stmts.push(FirStmt::If {
                    cond: check,
                    then_body: error_body,
                    else_body: vec![],
                });

                Ok(FirExpr::LocalVar(result_id, result_ty))
            }

            // Error handling: `expr!.or(default)` → evaluate, check error, fallback
            Expr::ErrorOr { expr, default, .. } => {
                // Extract inner expression (skip Propagate wrapper)
                let inner = if let Expr::Propagate(inner, _) = expr.as_ref() {
                    inner
                } else {
                    expr
                };
                let fir_inner = self.lower_expr(inner)?;
                let fir_default = self.lower_expr(default)?;
                let result_ty = self.infer_fir_type(&fir_inner);
                let result_id = self.alloc_local();
                self.local_types.insert(result_id, result_ty.clone());

                // let __result = inner_expr
                self.pending_stmts.push(FirStmt::Let {
                    name: result_id,
                    ty: result_ty.clone(),
                    value: fir_inner,
                });

                // if aster_error_check(): __result = default
                let check = FirExpr::RuntimeCall {
                    name: "aster_error_check".to_string(),
                    args: vec![],
                    ret_ty: FirType::Bool,
                };
                self.pending_stmts.push(FirStmt::If {
                    cond: check,
                    then_body: vec![FirStmt::Assign {
                        target: FirPlace::Local(result_id),
                        value: fir_default,
                    }],
                    else_body: vec![],
                });

                Ok(FirExpr::LocalVar(result_id, result_ty))
            }

            // Error handling: `expr!.or_else(-> handler)` → evaluate, check error, call handler
            Expr::ErrorOrElse { expr, handler, .. } => {
                let inner = if let Expr::Propagate(inner, _) = expr.as_ref() {
                    inner
                } else {
                    expr
                };
                let fir_inner = self.lower_expr(inner)?;
                // For zero-param lambdas (-> expr), inline the body directly
                let fir_handler = if let Expr::Lambda { params, body, .. } = handler.as_ref()
                    && params.is_empty()
                {
                    self.lower_inline_body(body)?
                } else {
                    self.lower_expr(handler)?
                };
                let result_ty = self.infer_fir_type(&fir_inner);
                let result_id = self.alloc_local();
                self.local_types.insert(result_id, result_ty.clone());

                self.pending_stmts.push(FirStmt::Let {
                    name: result_id,
                    ty: result_ty.clone(),
                    value: fir_inner,
                });

                let check = FirExpr::RuntimeCall {
                    name: "aster_error_check".to_string(),
                    args: vec![],
                    ret_ty: FirType::Bool,
                };
                self.pending_stmts.push(FirStmt::If {
                    cond: check,
                    then_body: vec![FirStmt::Assign {
                        target: FirPlace::Local(result_id),
                        value: fir_handler,
                    }],
                    else_body: vec![],
                });

                Ok(FirExpr::LocalVar(result_id, result_ty))
            }

            // Error handling: `expr!.catch { arms }` → evaluate expr, check error
            // For now, same as ErrorOr with the first arm's body as fallback
            Expr::ErrorCatch { expr, arms, .. } => {
                let inner = if let Expr::Propagate(inner, _) = expr.as_ref() {
                    inner
                } else {
                    expr
                };
                let fir_inner = self.lower_expr(inner)?;
                let result_ty = self.infer_fir_type(&fir_inner);
                let result_id = self.alloc_local();
                self.local_types.insert(result_id, result_ty.clone());

                self.pending_stmts.push(FirStmt::Let {
                    name: result_id,
                    ty: result_ty.clone(),
                    value: fir_inner,
                });

                // Use first arm as catch-all fallback
                let fallback = if let Some((_, body)) = arms.first() {
                    self.lower_expr(body)?
                } else {
                    FirExpr::IntLit(0) // no arms → default to 0
                };

                let check = FirExpr::RuntimeCall {
                    name: "aster_error_check".to_string(),
                    args: vec![],
                    ret_ty: FirType::Bool,
                };
                self.pending_stmts.push(FirStmt::If {
                    cond: check,
                    then_body: vec![FirStmt::Assign {
                        target: FirPlace::Local(result_id),
                        value: fallback,
                    }],
                    else_body: vec![],
                });

                Ok(FirExpr::LocalVar(result_id, result_ty))
            }

            // Throw: set error flag, return dummy value matching the function's return type
            Expr::Throw(inner, _) => {
                let _fir_inner = self.lower_expr(inner)?;
                // Set the error flag
                self.pending_stmts.push(FirStmt::Expr(FirExpr::RuntimeCall {
                    name: "aster_error_set".to_string(),
                    args: vec![],
                    ret_ty: FirType::Void,
                }));
                // Return a type-correct dummy value — the caller checks the error flag
                let dummy = match self
                    .current_return_type
                    .as_ref()
                    .map(|t| self.lower_type(t))
                {
                    Some(FirType::F64) => FirExpr::FloatLit(0.0),
                    Some(FirType::Bool) => FirExpr::BoolLit(false),
                    _ => FirExpr::IntLit(0),
                };
                Ok(dummy)
            }

            Expr::Member { object, field, .. } => {
                // Check if this is an enum variant construction: EnumName.Variant
                if let Expr::Ident(name, _) = object.as_ref()
                    && self.enum_variants.contains_key(name.as_str())
                {
                    // Fieldless enum variant: call the constructor with no args
                    let ctor_name = format!("{}.{}", name, field);
                    if let Some(&func_id) = self.functions.get(&ctor_name) {
                        return Ok(FirExpr::Call {
                            func: func_id,
                            args: vec![],
                            ret_ty: FirType::Ptr,
                        });
                    }
                }
                let fir_object = self.lower_expr(object)?;
                // Determine the class of the object to find field offset and type
                let (offset, field_ty) = self.resolve_field_access(object, field)?;
                Ok(FirExpr::FieldGet {
                    object: Box::new(fir_object),
                    offset,
                    ty: field_ty,
                })
            }

            Expr::Map { entries, .. } => {
                // Lower map literal: create map, then set each entry
                // Desugars to: let m = aster_map_new(cap); m = aster_map_set(m, k1, v1); ...
                let cap = entries.len().max(4) as i64;
                let mut result = FirExpr::RuntimeCall {
                    name: "aster_map_new".to_string(),
                    args: vec![FirExpr::IntLit(cap)],
                    ret_ty: FirType::Ptr,
                };
                for (key, value) in entries {
                    let fir_key = self.lower_expr(key)?;
                    let fir_value = self.lower_expr(value)?;
                    result = FirExpr::RuntimeCall {
                        name: "aster_map_set".to_string(),
                        args: vec![result, fir_key, fir_value],
                        ret_ty: FirType::Ptr,
                    };
                }
                Ok(result)
            }

            Expr::Range {
                start,
                end,
                inclusive,
                ..
            } => {
                let fir_start = self.lower_expr(start)?;
                let fir_end = self.lower_expr(end)?;
                let fir_inclusive = FirExpr::BoolLit(*inclusive);
                // Pack range as a 3-field struct: [start: i64, end: i64, inclusive: bool]
                Ok(FirExpr::RuntimeCall {
                    name: "aster_range_new".to_string(),
                    args: vec![fir_start, fir_end, fir_inclusive],
                    ret_ty: FirType::Ptr,
                })
            }
        }
    }

    /// Lower a method call: `obj.method(args)`.
    /// Handles list built-in methods and class method dispatch.
    /// Lower a method call: `obj.method(args)`.
    /// Handles list built-in methods and class method dispatch.
    pub(crate) fn lower_method_call(
        &mut self,
        object: &Expr,
        method: &str,
        args: &[(String, ast::Span, Expr)],
    ) -> Result<FirExpr, LowerError> {
        // Check for enum variant constructor with fields: EnumName.Variant(fields)
        if let Expr::Ident(name, _) = object
            && self.enum_variants.contains_key(name.as_str())
        {
            let ctor_name = format!("{}.{}", name, method);
            if let Some(&func_id) = self.functions.get(&ctor_name) {
                let fir_args: Result<Vec<_>, _> = args
                    .iter()
                    .map(|(_, _, arg)| self.lower_expr(arg))
                    .collect();
                return Ok(FirExpr::Call {
                    func: func_id,
                    args: fir_args?,
                    ret_ty: FirType::Ptr,
                });
            }
        }

        // File static methods → runtime calls
        if let Expr::Ident(name, _) = object
            && name == "File"
        {
            match method {
                "read" => {
                    let path = args
                        .iter()
                        .find(|(n, _, _)| n == "path")
                        .or_else(|| args.first())
                        .map(|(_, _, e)| e);
                    let fir_path = if let Some(p) = path {
                        self.lower_expr(p)?
                    } else {
                        FirExpr::IntLit(0)
                    };
                    return Ok(FirExpr::RuntimeCall {
                        name: "aster_file_read".to_string(),
                        args: vec![fir_path],
                        ret_ty: FirType::Ptr,
                    });
                }
                "write" | "append" => {
                    let path = args
                        .iter()
                        .find(|(n, _, _)| n == "path")
                        .or_else(|| args.first())
                        .map(|(_, _, e)| e);
                    let content = args
                        .iter()
                        .find(|(n, _, _)| n == "content")
                        .or_else(|| args.get(1))
                        .map(|(_, _, e)| e);
                    let fir_path = if let Some(p) = path {
                        self.lower_expr(p)?
                    } else {
                        FirExpr::IntLit(0)
                    };
                    let fir_content = if let Some(c) = content {
                        self.lower_expr(c)?
                    } else {
                        FirExpr::IntLit(0)
                    };
                    let runtime_name = if method == "write" {
                        "aster_file_write"
                    } else {
                        "aster_file_append"
                    };
                    return Ok(FirExpr::RuntimeCall {
                        name: runtime_name.to_string(),
                        args: vec![fir_path, fir_content],
                        ret_ty: FirType::Void,
                    });
                }
                _ => {}
            }
        }

        // Check for static method calls on class names: ClassName.method(args)
        // e.g. Celsius.from(value: x) — the method has a self param in FIR (all methods do),
        // so pass nil (0) as the self pointer since no receiver instance exists.
        if let Expr::Ident(name, _) = object
            && self.classes.contains_key(name.as_str())
            && !self.locals.contains_key(name.as_str())
        {
            let qualified_name = format!("{}.{}", name, method);
            if let Some(&func_id) = self.functions.get(&qualified_name) {
                let mut fir_args = vec![FirExpr::NilLit]; // nil self pointer
                fir_args.extend(self.lower_call_args_with_defaults(&qualified_name, args)?);
                let ret_ty = self.resolve_function_ret_type(&qualified_name);
                return Ok(FirExpr::Call {
                    func: func_id,
                    args: fir_args,
                    ret_ty,
                });
            }
        }

        let fir_object = self.lower_expr(object)?;
        let fir_object_ty = self.infer_fir_type(&fir_object);
        let object_ast_ty = self
            .type_table
            .get(&object.span())
            .cloned()
            .or_else(|| match object {
                Expr::Ident(name, _) => self.local_ast_types.get(name).cloned(),
                _ => None,
            });

        if matches!(object_ast_ty, Some(Type::Task(_)))
            && let Some((runtime_name, ret_ty)) = match method {
                "is_ready" => Some(("aster_task_is_ready", FirType::Bool)),
                "cancel" => None,
                "wait_cancel" => None,
                _ => None,
            }
        {
            return Ok(FirExpr::RuntimeCall {
                name: runtime_name.to_string(),
                args: vec![fir_object],
                ret_ty,
            });
        }
        if matches!(object_ast_ty, Some(Type::Task(_))) {
            match method {
                "cancel" => {
                    return Ok(FirExpr::CancelTask {
                        task: Box::new(fir_object),
                    });
                }
                "wait_cancel" => {
                    return Ok(FirExpr::WaitCancel {
                        task: Box::new(fir_object),
                    });
                }
                _ => {}
            }
        }

        // Mutex[T] methods → runtime calls
        if matches!(&object_ast_ty, Some(Type::Custom(name, _)) if name == "Mutex") {
            match method {
                "acquire" => {
                    return Ok(FirExpr::RuntimeCall {
                        name: "aster_mutex_lock".to_string(),
                        args: vec![fir_object],
                        ret_ty: FirType::I64,
                    });
                }
                "release" => {
                    let value_arg = args
                        .iter()
                        .find(|(n, _, _)| n == "value")
                        .map(|(_, _, e)| e)
                        .or_else(|| args.first().map(|(_, _, e)| e));
                    let fir_value = if let Some(val) = value_arg {
                        self.lower_expr(val)?
                    } else {
                        FirExpr::IntLit(0)
                    };
                    return Ok(FirExpr::RuntimeCall {
                        name: "aster_mutex_unlock".to_string(),
                        args: vec![fir_object, fir_value],
                        ret_ty: FirType::Void,
                    });
                }
                "lock" => {
                    // Scoped lock: m.lock(block: -> v : body)
                    // Lowers to: let __mx = m; let v = acquire(__mx); body; unlock(__mx, v)
                    let block_arg = args
                        .iter()
                        .find(|(n, _, _)| n == "block")
                        .map(|(_, _, e)| e)
                        .or_else(|| args.first().map(|(_, _, e)| e));
                    if let Some(Expr::Lambda {
                        params,
                        body: lbody,
                        ..
                    }) = block_arg
                    {
                        // Store mutex ptr in a local for reuse in unlock
                        let mx_id = self.alloc_local();
                        self.local_types.insert(mx_id, FirType::Ptr);
                        self.pending_stmts.push(FirStmt::Let {
                            name: mx_id,
                            ty: FirType::Ptr,
                            value: fir_object,
                        });

                        // Acquire: let v = aster_mutex_lock(__mx)
                        let param_name = params
                            .first()
                            .map(|(n, _)| n.clone())
                            .unwrap_or_else(|| "__lock_val".into());
                        let val_id = self.alloc_local();
                        self.locals.insert(param_name, val_id);
                        self.local_types.insert(val_id, FirType::I64);
                        self.pending_stmts.push(FirStmt::Let {
                            name: val_id,
                            ty: FirType::I64,
                            value: FirExpr::RuntimeCall {
                                name: "aster_mutex_lock".to_string(),
                                args: vec![FirExpr::LocalVar(mx_id, FirType::Ptr)],
                                ret_ty: FirType::I64,
                            },
                        });

                        // Inline the lambda body
                        let body_result = self.lower_inline_body(lbody)?;
                        self.pending_stmts.push(FirStmt::Expr(body_result));

                        // Unlock: aster_mutex_unlock(__mx, v)
                        // Use the original value — inline lambda can't reassign it
                        return Ok(FirExpr::RuntimeCall {
                            name: "aster_mutex_unlock".to_string(),
                            args: vec![
                                FirExpr::LocalVar(mx_id, FirType::Ptr),
                                FirExpr::LocalVar(val_id, FirType::I64),
                            ],
                            ret_ty: FirType::Void,
                        });
                    }
                    return Ok(FirExpr::IntLit(0));
                }
                _ => {}
            }
        }

        // Channel[T] / MultiSend[T] / MultiReceive[T] methods → runtime calls
        if matches!(&object_ast_ty, Some(Type::Custom(name, _)) if name == "Channel" || name == "MultiSend" || name == "MultiReceive")
        {
            let mut fir_value_arg = || -> Result<FirExpr, LowerError> {
                let value_expr = args
                    .iter()
                    .find(|(n, _, _)| n == "value")
                    .map(|(_, _, e)| e)
                    .or_else(|| args.first().map(|(_, _, e)| e));
                if let Some(val) = value_expr {
                    self.lower_expr(val)
                } else {
                    Ok(FirExpr::IntLit(0))
                }
            };
            match method {
                "send" => {
                    return Ok(FirExpr::RuntimeCall {
                        name: "aster_channel_send".to_string(),
                        args: vec![fir_object, fir_value_arg()?],
                        ret_ty: FirType::Void,
                    });
                }
                "wait_send" => {
                    return Ok(FirExpr::RuntimeCall {
                        name: "aster_channel_wait_send".to_string(),
                        args: vec![fir_object, fir_value_arg()?],
                        ret_ty: FirType::Void,
                    });
                }
                "try_send" => {
                    return Ok(FirExpr::RuntimeCall {
                        name: "aster_channel_try_send".to_string(),
                        args: vec![fir_object, fir_value_arg()?],
                        ret_ty: FirType::Void,
                    });
                }
                "receive" => {
                    return Ok(FirExpr::RuntimeCall {
                        name: "aster_channel_receive".to_string(),
                        args: vec![fir_object],
                        ret_ty: FirType::I64,
                    });
                }
                "wait_receive" => {
                    return Ok(FirExpr::RuntimeCall {
                        name: "aster_channel_wait_receive".to_string(),
                        args: vec![fir_object],
                        ret_ty: FirType::I64,
                    });
                }
                "try_receive" => {
                    return Ok(FirExpr::RuntimeCall {
                        name: "aster_channel_try_receive".to_string(),
                        args: vec![fir_object],
                        ret_ty: FirType::I64,
                    });
                }
                "close" => {
                    return Ok(FirExpr::RuntimeCall {
                        name: "aster_channel_close".to_string(),
                        args: vec![fir_object],
                        ret_ty: FirType::Void,
                    });
                }
                "clone_sender" | "clone_receiver" => {
                    // Clone returns the same handle (refcount bump is a future enhancement)
                    return Ok(fir_object);
                }
                _ => {}
            }
        }

        if method == "or_throw" {
            let inner_ty = match &fir_object_ty {
                FirType::TaggedUnion { variants, .. } if !variants.is_empty() => {
                    variants[0].clone()
                }
                other => {
                    return Err(LowerError::UnsupportedFeature(
                        UnsupportedFeatureKind::Other(format!(
                            ".or_throw() on non-nullable FIR type: {:?}",
                            other
                        )),
                        object.span(),
                    ));
                }
            };

            let nullable_id = self.alloc_local();
            self.local_types.insert(nullable_id, fir_object_ty.clone());
            self.pending_stmts.push(FirStmt::Let {
                name: nullable_id,
                ty: fir_object_ty.clone(),
                value: fir_object,
            });

            let result_id = self.alloc_local();
            self.local_types.insert(result_id, inner_ty.clone());
            self.pending_stmts.push(FirStmt::Let {
                name: result_id,
                ty: inner_ty.clone(),
                value: self.default_value_for_type(&inner_ty),
            });

            self.pending_stmts.push(FirStmt::If {
                cond: FirExpr::TagCheck {
                    value: Box::new(FirExpr::LocalVar(nullable_id, fir_object_ty.clone())),
                    tag: 1,
                },
                then_body: vec![FirStmt::Expr(FirExpr::RuntimeCall {
                    name: "aster_error_set".to_string(),
                    args: vec![],
                    ret_ty: FirType::Void,
                })],
                else_body: vec![FirStmt::Assign {
                    target: FirPlace::Local(result_id),
                    value: FirExpr::TagUnwrap {
                        value: Box::new(FirExpr::LocalVar(nullable_id, fir_object_ty)),
                        expected_tag: 0,
                        ty: inner_ty.clone(),
                    },
                }],
            });

            return Ok(FirExpr::LocalVar(result_id, inner_ty));
        }

        // Nullable `.or(default: value)` — returns the inner value or the default if nil.
        // TaggedUnion tag 0 = Some(value), tag 1 = nil.
        if method == "or"
            && let FirType::TaggedUnion { ref variants, .. } = fir_object_ty
            && !variants.is_empty()
        {
            let inner_ty = variants[0].clone();
            let default_expr = if let Some((_, _, default_arg)) = args.first() {
                self.lower_expr(default_arg)?
            } else {
                self.default_value_for_type(&inner_ty)
            };

            let nullable_id = self.alloc_local();
            self.local_types.insert(nullable_id, fir_object_ty.clone());
            self.pending_stmts.push(FirStmt::Let {
                name: nullable_id,
                ty: fir_object_ty.clone(),
                value: fir_object,
            });

            let result_id = self.alloc_local();
            self.local_types.insert(result_id, inner_ty.clone());
            self.pending_stmts.push(FirStmt::Let {
                name: result_id,
                ty: inner_ty.clone(),
                value: default_expr,
            });

            self.pending_stmts.push(FirStmt::If {
                cond: FirExpr::TagCheck {
                    value: Box::new(FirExpr::LocalVar(nullable_id, fir_object_ty.clone())),
                    tag: 0,
                },
                then_body: vec![FirStmt::Assign {
                    target: FirPlace::Local(result_id),
                    value: FirExpr::TagUnwrap {
                        value: Box::new(FirExpr::LocalVar(nullable_id, fir_object_ty)),
                        expected_tag: 0,
                        ty: inner_ty.clone(),
                    },
                }],
                else_body: vec![],
            });

            return Ok(FirExpr::LocalVar(result_id, inner_ty));
        }

        // Check for Range methods
        if method == "random" && self.is_range_expr(object) {
            // range.random() → aster_random_int(end - start) + start
            // Range layout: [start: i64 @ 0][end: i64 @ 8][inclusive: i64 @ 16]
            let start = FirExpr::FieldGet {
                object: Box::new(fir_object.clone()),
                offset: 0,
                ty: FirType::I64,
            };
            let end = FirExpr::FieldGet {
                object: Box::new(fir_object),
                offset: 8,
                ty: FirType::I64,
            };
            let range_size = FirExpr::BinaryOp {
                left: Box::new(end),
                op: BinOp::Sub,
                right: Box::new(start.clone()),
                result_ty: FirType::I64,
            };
            let random_offset = FirExpr::RuntimeCall {
                name: "aster_random_int".to_string(),
                args: vec![range_size],
                ret_ty: FirType::I64,
            };
            return Ok(FirExpr::BinaryOp {
                left: Box::new(random_offset),
                op: BinOp::Add,
                right: Box::new(start),
                result_ty: FirType::I64,
            });
        }

        // Check for list built-in methods
        match method {
            "len" => {
                return Ok(FirExpr::RuntimeCall {
                    name: "aster_list_len".to_string(),
                    args: vec![fir_object],
                    ret_ty: FirType::I64,
                });
            }
            "push" => {
                let mut call_args = vec![fir_object];
                for (_, _, arg) in args {
                    call_args.push(self.lower_expr(arg)?);
                }
                return Ok(FirExpr::RuntimeCall {
                    name: "aster_list_push".to_string(),
                    args: call_args,
                    ret_ty: FirType::Ptr,
                });
            }
            "random" => {
                // list.random() → aster_list_random(list)
                // Single runtime call avoids double-evaluation and GC issues
                // with unrooted temporaries.
                return Ok(FirExpr::RuntimeCall {
                    name: "aster_list_random".to_string(),
                    args: vec![fir_object],
                    ret_ty: FirType::I64,
                });
            }
            _ => {}
        }

        // Check for class method calls — walk the ancestor chain to find inherited methods
        if let Ok(class_name) = self.resolve_class_name(object) {
            // Try the class itself first, then walk parent chain
            let mut current = Some(class_name.clone());
            while let Some(ref cname) = current.clone() {
                let qualified_name = format!("{}.{}", cname, method);
                if let Some(&func_id) = self.functions.get(&qualified_name) {
                    // Build args: self + explicit args (with defaults filled in)
                    let mut call_args = vec![fir_object];
                    let method_args = self.lower_call_args_with_defaults(&qualified_name, args)?;
                    call_args.extend(method_args);
                    let ret_ty = self.resolve_function_ret_type(&qualified_name);
                    return Ok(FirExpr::Call {
                        func: func_id,
                        args: call_args,
                        ret_ty,
                    });
                }
                // Walk up to parent class
                current = self
                    .type_env
                    .get_class(cname)
                    .and_then(|ci| ci.extends.clone());
            }
        }

        // Iterable vocabulary methods — catch-all for list methods not found as class methods
        {
            let elem_ty = if let Some(Type::List(inner)) = &object_ast_ty {
                self.lower_type(inner)
            } else {
                self.resolve_list_elem_type(object).unwrap_or(FirType::I64)
            };
            match method {
                "map" | "filter" | "find" | "any" | "all" => {
                    return self
                        .lower_iterable_with_callback(method, fir_object, args, &elem_ty, object);
                }
                "reduce" => {
                    return self.lower_iterable_reduce(fir_object, args, &elem_ty, object);
                }
                "count" => {
                    return Ok(FirExpr::RuntimeCall {
                        name: "aster_list_len".to_string(),
                        args: vec![fir_object],
                        ret_ty: FirType::I64,
                    });
                }
                "first" => {
                    return self.lower_iterable_first(fir_object, &elem_ty);
                }
                "last" => {
                    return self.lower_iterable_last(fir_object, &elem_ty);
                }
                "to_list" => {
                    return self.lower_iterable_to_list(fir_object, &elem_ty);
                }
                "min" | "max" => {
                    return self.lower_iterable_min_max(method, fir_object, &elem_ty);
                }
                "sort" => {
                    return self.lower_iterable_sort(fir_object, &elem_ty);
                }
                _ => {}
            }
        }

        Err(LowerError::UnsupportedFeature(
            UnsupportedFeatureKind::Other(format!("method call: .{}()", method)),
            object.span(),
        ))
    }

    pub(crate) fn to_string_expr(&self, ast_expr: &Expr, fir_expr: FirExpr) -> FirExpr {
        match self.infer_fir_type(&fir_expr) {
            FirType::Ptr => {
                if let Ok(class_name) = self.resolve_class_name(ast_expr) {
                    let qualified = format!("{}.to_string", class_name);
                    if let Some(&func_id) = self.functions.get(&qualified) {
                        return FirExpr::Call {
                            func: func_id,
                            args: vec![fir_expr],
                            ret_ty: FirType::Ptr,
                        };
                    }
                }
                fir_expr
            }
            FirType::I64 => FirExpr::RuntimeCall {
                name: "aster_int_to_string".to_string(),
                args: vec![fir_expr],
                ret_ty: FirType::Ptr,
            },
            FirType::F64 => FirExpr::RuntimeCall {
                name: "aster_float_to_string".to_string(),
                args: vec![fir_expr],
                ret_ty: FirType::Ptr,
            },
            FirType::Bool => FirExpr::RuntimeCall {
                name: "aster_bool_to_string".to_string(),
                args: vec![fir_expr],
                ret_ty: FirType::Ptr,
            },
            _ => fir_expr,
        }
    }

    pub(crate) fn default_value_for_type(&self, ty: &FirType) -> FirExpr {
        match ty {
            FirType::F64 => FirExpr::FloatLit(0.0),
            FirType::Bool => FirExpr::BoolLit(false),
            FirType::Ptr | FirType::TaggedUnion { .. } | FirType::Struct(_) | FirType::FnPtr(_) => {
                FirExpr::NilLit
            }
            _ => FirExpr::IntLit(0),
        }
    }

    /// Wrap a return value in TagWrap if the current function returns a nullable type.
    /// `return nil` → TagWrap(tag=1, NilLit)  [nil]
    /// `return expr` → TagWrap(tag=0, expr)    [Some(value)]
    /// Wrap a return value in TagWrap if the current function returns a nullable type.
    /// `return nil` → TagWrap(tag=1, NilLit)  [nil]
    /// `return expr` → TagWrap(tag=0, expr)    [Some(value)]
    pub(crate) fn maybe_wrap_nullable_return(&self, fir_expr: FirExpr, ast_expr: &Expr) -> FirExpr {
        if let Some(Type::Nullable(inner)) = &self.current_return_type {
            let result_ty = self.lower_type(inner);
            if matches!(ast_expr, Expr::Nil(_)) {
                // return nil → TagWrap(1, nil)
                FirExpr::TagWrap {
                    tag: 1,
                    value: Box::new(FirExpr::NilLit),
                    ty: FirType::Ptr,
                }
            } else {
                // return value → TagWrap(0, value)
                FirExpr::TagWrap {
                    tag: 0,
                    value: Box::new(fir_expr),
                    ty: result_ty,
                }
            }
        } else {
            fir_expr
        }
    }

    pub(crate) fn wrap_nullable_binding(
        &self,
        type_ann: Option<&Type>,
        ast_expr: &Expr,
        fir_expr: FirExpr,
    ) -> FirExpr {
        if let Some(Type::Nullable(inner)) = type_ann {
            let result_ty = self.lower_type(inner);
            if matches!(ast_expr, Expr::Nil(_)) {
                FirExpr::TagWrap {
                    tag: 1,
                    value: Box::new(FirExpr::NilLit),
                    ty: FirType::Ptr,
                }
            } else {
                FirExpr::TagWrap {
                    tag: 0,
                    value: Box::new(fir_expr),
                    ty: result_ty,
                }
            }
        } else {
            fir_expr
        }
    }

    /// Lower call arguments, filling in default values for any missing named parameters.
    /// If the function has no defaults or all args are provided, this just lowers args in order.
    /// Lower call arguments, filling in default values for any missing named parameters.
    /// If the function has no defaults or all args are provided, this just lowers args in order.
    pub(crate) fn lower_call_args_with_defaults(
        &mut self,
        func_name: &str,
        args: &[(String, ast::Span, Expr)],
    ) -> Result<Vec<FirExpr>, LowerError> {
        if let Some(param_defaults) = self.function_defaults.get(func_name).cloned() {
            // Build args in parameter order, using defaults for missing args
            let mut fir_args = Vec::new();
            for (param_name, default_expr) in &param_defaults {
                if let Some((_, _, arg_expr)) = args.iter().find(|(name, _, _)| name == param_name)
                {
                    fir_args.push(self.lower_expr(arg_expr)?);
                } else if let Some(default) = default_expr {
                    fir_args.push(self.lower_expr(default)?);
                } else {
                    // No arg provided and no default — shouldn't happen (typechecker catches this)
                    return Err(LowerError::UnsupportedFeature(
                        UnsupportedFeatureKind::Other(format!(
                            "missing argument '{}' with no default for {}",
                            param_name, func_name
                        )),
                        Span::dummy(),
                    ));
                }
            }
            Ok(fir_args)
        } else {
            // No defaults — lower args in the order provided
            args.iter()
                .map(|(_, _, arg)| self.lower_expr(arg))
                .collect()
        }
    }

    /// Lower `for var in iter: body`.
    /// For List types: index-based while loop (aster_list_len/aster_list_get).
    /// For Iterator classes: next()-based loop with nullable check.
    /// Lower a zero-param lambda body inline, returning the last expression as a FirExpr.
    /// Emits any preceding statements into pending_stmts.
    /// Lower `for var in iter: body`.
    /// For List types: index-based while loop (aster_list_len/aster_list_get).
    /// For Iterator classes: next()-based loop with nullable check.
    /// Lower a zero-param lambda body inline, returning the last expression as a FirExpr.
    /// Emits any preceding statements into pending_stmts.
    pub(crate) fn lower_inline_body(&mut self, body: &[Stmt]) -> Result<FirExpr, LowerError> {
        if body.is_empty() {
            return Ok(FirExpr::IntLit(0));
        }
        // Lower all but the last statement into pending_stmts
        for stmt in &body[..body.len() - 1] {
            let fir_stmt = self.lower_stmt_inner(stmt)?;
            self.pending_stmts.push(fir_stmt);
        }
        // Last statement: extract its expression value
        let last = &body[body.len() - 1];
        match last {
            Stmt::Expr(expr, _) | Stmt::Return(expr, _) => self.lower_expr(expr),
            other => {
                let fir_stmt = self.lower_stmt_inner(other)?;
                self.pending_stmts.push(fir_stmt);
                Ok(FirExpr::IntLit(0))
            }
        }
    }

    pub(crate) fn lower_explicit_call_args(
        &mut self,
        func: &Expr,
        args: &[(String, ast::Span, Expr)],
    ) -> Result<Vec<FirExpr>, LowerError> {
        match func {
            Expr::Ident(name, _) => self.lower_call_args_with_defaults(name, args),
            _ => args
                .iter()
                .map(|(_, _, arg)| self.lower_expr(arg))
                .collect(),
        }
    }

}
