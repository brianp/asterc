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
                if let Some(&local_id) = self.scope.locals.get(name.as_str()) {
                    // Resolve the type from the type env
                    let ty = self.resolve_var_type(name);
                    Ok(FirExpr::LocalVar(local_id, ty))
                } else if let Some(&self_id) = self.scope.locals.get("self") {
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
            } => self.lower_binop_expr(left, op, right),

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

            Expr::Call { func, args, .. } => self.lower_call_expr(func, args, expr),

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
                        self.scope.local_ast_types.get(name.as_str()),
                        Some(Type::Map(_, _))
                    )
                } else {
                    false
                };

                let fir_obj = self.lower_expr(object)?;
                let fir_idx = self.lower_expr(index)?;
                if is_map {
                    // Determine value FIR type from AST type info
                    let val_fir_ty = if let Expr::Ident(name, _) = object.as_ref() {
                        if let Some(Type::Map(_, v)) = self.scope.local_ast_types.get(name.as_str())
                        {
                            self.lower_type(v)
                        } else {
                            FirType::I64
                        }
                    } else {
                        FirType::I64
                    };

                    if matches!(val_fir_ty, FirType::Ptr) {
                        // Pointer value types (String, List, Class, Map): aster_map_get
                        // returning 0 naturally represents nil (null pointer = nil,
                        // non-null = Some). The result type is TaggedUnion[Ptr, Void]
                        // so that .or()/.or_throw() and nullable match work correctly.
                        let nullable_ty = FirType::TaggedUnion {
                            tag_bits: 1,
                            variants: vec![FirType::Ptr, FirType::Void],
                        };
                        let uid = self.scope.next_local;
                        let result_id = self.alloc_local();
                        self.scope
                            .locals
                            .insert(format!("__map_get_{}", uid), result_id);
                        self.scope
                            .local_types
                            .insert(result_id, nullable_ty.clone());
                        self.pending_stmts.push(FirStmt::Let {
                            name: result_id,
                            ty: nullable_ty.clone(),
                            value: FirExpr::RuntimeCall {
                                name: "aster_map_get".to_string(),
                                args: vec![fir_obj, fir_idx],
                                ret_ty: FirType::Ptr,
                            },
                        });
                        Ok(FirExpr::LocalVar(result_id, nullable_ty))
                    } else {
                        // Value types (Int, Float, Bool): need has_key check + TagWrap boxing.
                        // Result type is TaggedUnion[val_fir_ty, Void] so that
                        // .or()/.or_throw() and nullable match work correctly.
                        let nullable_ty = FirType::TaggedUnion {
                            tag_bits: 1,
                            variants: vec![val_fir_ty.clone(), FirType::Void],
                        };
                        let uid = self.scope.next_local;
                        let result_id = self.alloc_local();
                        self.scope
                            .locals
                            .insert(format!("__map_get_{}", uid), result_id);
                        self.scope
                            .local_types
                            .insert(result_id, nullable_ty.clone());

                        // let __map_get = nil (default: key not found)
                        self.pending_stmts.push(FirStmt::Let {
                            name: result_id,
                            ty: nullable_ty.clone(),
                            value: FirExpr::TagWrap {
                                tag: 1,
                                value: Box::new(FirExpr::NilLit),
                                ty: FirType::Ptr,
                            },
                        });

                        // if aster_map_has_key(map, key) != 0
                        let has_key = FirExpr::RuntimeCall {
                            name: "aster_map_has_key".to_string(),
                            args: vec![fir_obj.clone(), fir_idx.clone()],
                            ret_ty: FirType::I64,
                        };
                        let cond = FirExpr::BinaryOp {
                            left: Box::new(has_key),
                            op: BinOp::Neq,
                            right: Box::new(FirExpr::IntLit(0)),
                            result_ty: FirType::Bool,
                        };

                        // then: __map_get = TagWrap(0, aster_map_get(map, key))
                        let get_val = FirExpr::RuntimeCall {
                            name: "aster_map_get".to_string(),
                            args: vec![fir_obj, fir_idx],
                            ret_ty: val_fir_ty.clone(),
                        };
                        let some_expr = FirExpr::TagWrap {
                            tag: 0,
                            value: Box::new(get_val),
                            ty: val_fir_ty,
                        };

                        self.pending_stmts.push(FirStmt::If {
                            cond,
                            then_body: vec![FirStmt::Assign {
                                target: FirPlace::Local(result_id),
                                value: some_expr,
                            }],
                            else_body: vec![],
                        });

                        Ok(FirExpr::LocalVar(result_id, nullable_ty))
                    }
                } else {
                    // Resolve element type from AST type info when available
                    let elem_ty = if let Expr::Ident(name, _) = object.as_ref() {
                        if let Some(Type::List(inner)) =
                            self.scope.local_ast_types.get(name.as_str())
                        {
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

            Expr::AsyncCall { func, args, .. } | Expr::DetachedCall { func, args, .. } => {
                let args = self.lower_explicit_call_args(func, args)?;
                let func_id = self.resolve_called_function_id(func)?;
                let result_ty = self.resolve_called_function_ret_type(func, func_id);
                Ok(FirExpr::Spawn {
                    func: func_id,
                    args,
                    ret_ty: FirType::Ptr,
                    result_ty,
                    scope: self.scope.async_scope_stack.last().copied(),
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

            Expr::Resolve { expr: inner, .. } => {
                let task = self.lower_expr(inner)?;
                let ret_ty = self.resolve_task_result_type(inner, &task);
                Ok(FirExpr::ResolveTask {
                    task: Box::new(task),
                    ret_ty,
                })
            }

            Expr::Propagate(inner, _) => self.lower_propagate_expr(inner),

            Expr::ErrorOr { expr, default, .. } => self.lower_error_or_expr(expr, default),

            Expr::ErrorOrElse { expr, handler, .. } => self.lower_error_or_else_expr(expr, handler),

            Expr::ErrorCatch { expr, arms, .. } => self.lower_error_catch_expr(expr, arms),

            Expr::Throw(inner, _) => self.lower_throw_expr(inner),

            Expr::Member { object, field, .. } => {
                // Check if this is an enum variant construction: EnumName.Variant
                if let Expr::Ident(name, _) = object.as_ref()
                    && self.ms.enum_variants.contains_key(name.as_str())
                {
                    // Fieldless enum variant: call the constructor with no args
                    let ctor_name = format!("{}.{}", name, field);
                    if let Some(&func_id) = self.ms.functions.get(&ctor_name) {
                        return Ok(FirExpr::Call {
                            func: func_id,
                            args: vec![],
                            ret_ty: FirType::Ptr,
                        });
                    }
                }
                // Introspection property access (class_name, fields, methods, ancestors, children)
                if let Some(result) = self.lower_introspection_member(object, field)? {
                    return Ok(result);
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

    fn lower_binop_expr(
        &mut self,
        left: &Expr,
        op: &ast::BinOp,
        right: &Expr,
    ) -> Result<FirExpr, LowerError> {
        if matches!(op, ast::BinOp::Pow) {
            let fir_left = self.lower_expr(left)?;
            let fir_right = self.lower_expr(right)?;
            let lt = self.infer_fir_type(&fir_left);
            let rt = self.infer_fir_type(&fir_right);
            let any_float = lt == FirType::F64 || rt == FirType::F64;
            if any_float {
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
            if let Some(&func_id) = self.ms.functions.get(&eq_name) {
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
            if let Some(&func_id) = self.ms.functions.get(&cmp_name) {
                let fir_left = self.lower_expr(left)?;
                let fir_right = self.lower_expr(right)?;
                // cmp returns Ordering (tag: 0=Less, 1=Equal, 2=Greater)
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
            (FirType::I64, FirType::F64) => (FirExpr::IntToFloat(Box::new(fir_left)), fir_right),
            (FirType::F64, FirType::I64) => (fir_left, FirExpr::IntToFloat(Box::new(fir_right))),
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

    fn lower_call_expr(
        &mut self,
        func: &Expr,
        args: &[(String, ast::Span, Expr)],
        call_expr: &Expr,
    ) -> Result<FirExpr, LowerError> {
        self.pending_stmts.push(FirStmt::Expr(FirExpr::Safepoint));
        // Method call: obj.method(args)
        if let Expr::Member { object, field, .. } = func {
            return self.lower_method_call(object, field, args);
        }

        // Set[T]() constructor: Set[T]() → aster_set_new(cap)
        if let Expr::Index { object, .. } = func
            && let Expr::Ident(type_name, _) = object.as_ref()
            && type_name == "Set"
        {
            return Ok(FirExpr::RuntimeCall {
                name: "aster_set_new".to_string(),
                args: vec![FirExpr::IntLit(4)],
                ret_ty: FirType::Ptr,
            });
        }

        let Expr::Ident(name, _) = func else {
            // Indirect call: arbitrary expression as call target (e.g. get_handler()(x))
            let callee = self.lower_expr(func)?;
            let fir_args: Result<Vec<_>, _> = args
                .iter()
                .map(|(_, _, arg)| self.lower_expr(arg))
                .collect();
            // Resolve return type from the type table (populated by type checker).
            // Fall back to the callee expression's function type if available.
            let ret_ty = self
                .type_table
                .get(&call_expr.span())
                .or_else(|| {
                    self.type_table
                        .get(&func.span())
                        .filter(|ty| matches!(ty, Type::Function { .. }))
                        .map(|ty| match ty {
                            Type::Function { ret, .. } => ret.as_ref(),
                            _ => ty,
                        })
                })
                .map(|ty| self.lower_type(ty))
                .unwrap_or(FirType::I64);
            return Ok(FirExpr::ClosureCall {
                closure: Box::new(callee),
                args: fir_args?,
                ret_ty,
            });
        };

        if name == "to_string"
            && let Some((_, _, arg)) = args.first()
        {
            let fir_arg = self.lower_expr(arg)?;
            return Ok(self.to_string_expr(arg, fir_arg));
        }

        // random(max: n) or random(min: a, max: b) → aster_random_int/float/bool
        if name == "random" {
            return self.lower_random_call(args, call_expr);
        }

        // Mutex(value: x) → aster_mutex_new(x)
        if name == builtin_class::MUTEX {
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
        if name == builtin_class::CHANNEL
            || name == builtin_class::MULTI_SEND
            || name == builtin_class::MULTI_RECEIVE
        {
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
                .get(&call_expr.span())
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
                .get(&call_expr.span())
                .map(|ty| self.lower_type(ty))
                .unwrap_or(FirType::I64);
            return Ok(FirExpr::RuntimeCall {
                name: self.resolve_first_runtime_name(arg).to_string(),
                args: vec![fir_arg],
                ret_ty,
            });
        }

        // Closure call (statically resolved)
        if let Some((func_id, env_local, _captures)) = self.scope.closure_info.get(name).cloned() {
            let mut fir_args = Vec::new();
            if let Some(env_id) = env_local {
                fir_args.push(FirExpr::LocalVar(env_id, FirType::Ptr));
            } else {
                fir_args.push(FirExpr::NilLit);
            }
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

        // Class constructor call
        if let Some(&class_id) = self.ms.classes.get(name.as_str()) {
            let field_layout = self
                .ms
                .class_fields
                .get(&class_id)
                .cloned()
                .unwrap_or_default();
            // Count how many pointer-typed fields are at the front of the layout.
            // The layout has already been sorted pointer-first in the class lowering.
            let ptr_field_count = field_layout
                .iter()
                .take_while(|(_, ty, _)| ty.needs_gc_root())
                .count() as u8;
            let mut fir_fields = Vec::new();
            for (field_name, _, _) in &field_layout {
                if let Some((_, _, expr)) =
                    args.iter().find(|(arg_name, _, _)| arg_name == field_name)
                {
                    fir_fields.push(self.lower_expr(expr)?);
                } else {
                    break;
                }
            }
            if fir_fields.len() != field_layout.len() {
                fir_fields.clear();
                for (_, _, arg) in args {
                    fir_fields.push(self.lower_expr(arg)?);
                }
            }
            return Ok(FirExpr::Construct {
                class: class_id,
                fields: fir_fields,
                ty: FirType::Ptr,
                ptr_field_count,
            });
        }

        if let Some(&func_id) = self.ms.functions.get(name.as_str()) {
            let fir_args = self.lower_call_args_with_defaults(name, args)?;
            let ret_ty = self.resolve_function_ret_type(name);
            let (fir_args, cast_ret) =
                self.apply_generic_erasure_casts(name, fir_args, ret_ty.clone(), &call_expr.span());
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
        } else if self.scope.locals.contains_key(name.as_str()) {
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
        } else if name == "evaluate" {
            // Runtime evaluation: capture context snapshot and emit EvalCall
            let code_arg = args
                .iter()
                .find(|(n, _, _)| n == "code")
                .map(|(_, _, e)| e)
                .or_else(|| args.first().map(|(_, _, e)| e));
            let code_expr = if let Some(expr) = code_arg {
                self.lower_expr(expr)?
            } else {
                FirExpr::StringLit(String::new())
            };
            let snapshot = self.capture_context_snapshot();
            let context_idx = self.ms.module.eval_contexts.len() as u32;
            self.ms
                .module
                .eval_contexts
                .push(crate::eval_context::EvalContext {
                    snapshot_json: serde_json::to_vec(&snapshot)
                        .expect("ContextSnapshot serialization"),
                });
            Ok(FirExpr::EvalCall {
                code: Box::new(code_expr),
                context_idx,
                ret_ty: FirType::Void,
            })
        } else if let Some((runtime_name, ret_ty)) = Self::stdlib_runtime_mapping(name) {
            // Stdlib function imported via `use std/...`
            let fir_args: Result<Vec<_>, _> = args
                .iter()
                .map(|(_, _, arg)| self.lower_expr(arg))
                .collect();
            Ok(FirExpr::RuntimeCall {
                name: runtime_name.to_string(),
                args: fir_args?,
                ret_ty,
            })
        } else {
            // Runtime call (say, etc.)
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
    }

    /// Map stdlib function names (from `use std/...`) to their runtime call names and return types.
    fn stdlib_runtime_mapping(name: &str) -> Option<(&'static str, FirType)> {
        match name {
            // std/sys
            "args" => Some(("aster_sys_args", FirType::Ptr)),
            "env" => Some(("aster_sys_env_get", FirType::Ptr)),
            "set_env" => Some(("aster_sys_env_set", FirType::Void)),
            "exit" => Some(("aster_sys_exit", FirType::Void)),
            // std/fs
            "read_file" => Some(("aster_fs_read_file", FirType::Ptr)),
            "write_file" => Some(("aster_fs_write_file", FirType::Void)),
            "append_file" => Some(("aster_fs_append_file", FirType::Void)),
            "exists" => Some(("aster_fs_exists", FirType::Bool)),
            "is_dir" => Some(("aster_fs_is_dir", FirType::Bool)),
            "mkdir" => Some(("aster_fs_mkdir", FirType::Void)),
            "remove" => Some(("aster_fs_remove", FirType::Void)),
            "list_dir" => Some(("aster_fs_list_dir", FirType::Ptr)),
            "copy" => Some(("aster_fs_copy", FirType::Void)),
            "rename" => Some(("aster_fs_rename", FirType::Void)),
            // std/process
            "run" => Some(("aster_process_run", FirType::Ptr)),
            // std/crypto
            "sha256" => Some(("aster_crypto_sha256", FirType::Ptr)),
            // std/runtime
            "jit_run" => Some(("aster_runtime_jit_eval", FirType::I64)),
            _ => None,
        }
    }

    fn lower_random_call(
        &mut self,
        args: &[(String, ast::Span, Expr)],
        call_expr: &Expr,
    ) -> Result<FirExpr, LowerError> {
        let ret_ty = self
            .type_table
            .get(&call_expr.span())
            .map(|ty| self.lower_type(ty))
            .unwrap_or(FirType::I64);
        let max_arg = args.iter().find(|(n, _, _)| n == "max");
        let min_arg = args.iter().find(|(n, _, _)| n == "min");
        match ret_ty {
            FirType::I64 => {
                let fir_max = if let Some((_, _, e)) = max_arg {
                    self.lower_expr(e)?
                } else {
                    FirExpr::IntLit(100)
                };
                let raw_random = FirExpr::RuntimeCall {
                    name: "aster_random_int".to_string(),
                    args: vec![if let Some(min_val) = &min_arg {
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
        }
    }

    /// Evaluate expr, check error flag, cleanup + panic if error.
    fn lower_propagate_expr(&mut self, inner: &Expr) -> Result<FirExpr, LowerError> {
        let fir_inner = self.lower_expr(inner)?;
        let result_ty = self.infer_fir_type(&fir_inner);
        let result_id = self.alloc_local();
        self.scope.local_types.insert(result_id, result_ty.clone());

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
        // Save pending_stmts so cleanup emission doesn't steal earlier stmts.
        let saved = std::mem::take(&mut self.pending_stmts);
        self.emit_cleanup_calls();
        if let Some(scope_id) = self.scope.function_scope_id {
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

    /// Evaluate expr, on error use default value instead of panicking.
    fn lower_error_or_expr(&mut self, expr: &Expr, default: &Expr) -> Result<FirExpr, LowerError> {
        let inner = if let Expr::Propagate(inner, _) = expr {
            inner
        } else {
            expr
        };
        let fir_inner = self.lower_expr(inner)?;
        let fir_default = self.lower_expr(default)?;
        let result_ty = self.infer_fir_type(&fir_inner);
        let result_id = self.alloc_local();
        self.scope.local_types.insert(result_id, result_ty.clone());

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
                value: fir_default,
            }],
            else_body: vec![],
        });

        Ok(FirExpr::LocalVar(result_id, result_ty))
    }

    /// Evaluate expr, on error call handler lambda.
    fn lower_error_or_else_expr(
        &mut self,
        expr: &Expr,
        handler: &Expr,
    ) -> Result<FirExpr, LowerError> {
        let inner = if let Expr::Propagate(inner, _) = expr {
            inner
        } else {
            expr
        };
        let fir_inner = self.lower_expr(inner)?;
        let fir_handler = if let Expr::Lambda { params, body, .. } = handler
            && params.is_empty()
        {
            self.lower_inline_body(body)?
        } else {
            self.lower_expr(handler)?
        };
        let result_ty = self.infer_fir_type(&fir_inner);
        let result_id = self.alloc_local();
        self.scope.local_types.insert(result_id, result_ty.clone());

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

    /// Evaluate expr, on error dispatch to the matching catch arm by error type.
    fn lower_error_catch_expr(
        &mut self,
        expr: &Expr,
        arms: &[(ast::ErrorCatchPattern, Expr)],
    ) -> Result<FirExpr, LowerError> {
        let inner = if let Expr::Propagate(inner, _) = expr {
            inner
        } else {
            expr
        };
        let fir_inner = self.lower_expr(inner)?;
        let result_ty = self.infer_fir_type(&fir_inner);
        let result_id = self.alloc_local();
        self.scope.local_types.insert(result_id, result_ty.clone());

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

        // Build the dispatch body for inside the error-check if-block
        let error_body = self.lower_catch_arms_dispatch(arms, result_id)?;

        self.pending_stmts.push(FirStmt::If {
            cond: check,
            then_body: error_body,
            else_body: vec![],
        });

        Ok(FirExpr::LocalVar(result_id, result_ty))
    }

    /// Generate a nested if/else chain dispatching on error type tag for catch arms.
    fn lower_catch_arms_dispatch(
        &mut self,
        arms: &[(ast::ErrorCatchPattern, Expr)],
        result_id: LocalId,
    ) -> Result<Vec<FirStmt>, LowerError> {
        // Collect the typed arms and the optional wildcard
        let mut typed_arms: Vec<(&str, &str, &Expr)> = Vec::new();
        let mut wildcard_body: Option<&Expr> = None;

        for (pat, body) in arms {
            match pat {
                ast::ErrorCatchPattern::Typed {
                    error_type, var, ..
                } => {
                    typed_arms.push((error_type.as_str(), var.as_str(), body));
                }
                ast::ErrorCatchPattern::Wildcard(_) => {
                    wildcard_body = Some(body);
                }
            }
        }

        // If there are no typed arms, just use the wildcard (or default 0)
        if typed_arms.is_empty() {
            let fallback = if let Some(body) = wildcard_body {
                self.lower_expr(body)?
            } else {
                FirExpr::IntLit(0)
            };
            return Ok(vec![FirStmt::Assign {
                target: FirPlace::Local(result_id),
                value: fallback,
            }]);
        }

        // Get the error tag into a local
        let tag_id = self.alloc_local();
        self.scope.local_types.insert(tag_id, FirType::I64);
        let tag_let = FirStmt::Let {
            name: tag_id,
            ty: FirType::I64,
            value: FirExpr::RuntimeCall {
                name: "aster_error_get_tag".to_string(),
                args: vec![],
                ret_ty: FirType::I64,
            },
        };

        // Get the error value into a local
        let err_val_id = self.alloc_local();
        self.scope.local_types.insert(err_val_id, FirType::Ptr);
        let err_val_let = FirStmt::Let {
            name: err_val_id,
            ty: FirType::Ptr,
            value: FirExpr::RuntimeCall {
                name: "aster_error_get_value".to_string(),
                args: vec![],
                ret_ty: FirType::Ptr,
            },
        };

        let mut stmts = vec![tag_let, err_val_let];

        // Build a nested if/else chain: if tag == T1 then arm1 else if tag == T2 then arm2 ...
        // We build from the inside out (last arm first) to nest properly.
        // Collect all class IDs (including parent classes for subtype matching).
        let mut arm_data: Vec<(Vec<i64>, &str, &str, &Expr)> = Vec::new();
        for (error_type, var, body) in &typed_arms {
            let mut matching_tags = Vec::new();
            // The arm matches the exact type AND all subtypes (classes that extend it).
            // First, get the exact class ID for this error type.
            if let Some(&class_id) = self.ms.classes.get(*error_type) {
                matching_tags.push(class_id.0 as i64);
                // Also find all classes that transitively extend this type
                self.collect_subclass_tags(error_type, &mut matching_tags);
            }
            arm_data.push((matching_tags, var, error_type, body));
        }

        // Build the innermost else (wildcard fallback or re-raise)
        let wildcard_fallback = if let Some(body) = wildcard_body {
            let val = self.lower_expr(body)?;
            vec![FirStmt::Assign {
                target: FirPlace::Local(result_id),
                value: val,
            }]
        } else {
            // No wildcard: no arm matched, re-set the error flag so it propagates
            vec![FirStmt::Expr(FirExpr::RuntimeCall {
                name: "aster_error_set_typed".to_string(),
                args: vec![
                    FirExpr::LocalVar(tag_id, FirType::I64),
                    FirExpr::LocalVar(err_val_id, FirType::Ptr),
                ],
                ret_ty: FirType::Void,
            })]
        };

        // Build from last typed arm to first, nesting each into the else of the next
        let mut else_body = wildcard_fallback;
        for (matching_tags, var, error_type, body) in arm_data.into_iter().rev() {
            // Bind the error variable in locals and register its AST type
            // so that field accesses like e.code can resolve the class.
            let var_id = self.alloc_local();
            self.scope.local_types.insert(var_id, FirType::Ptr);
            let old_binding = self.scope.locals.insert(var.to_string(), var_id);
            let old_ast_type = self.scope.local_ast_types.insert(
                var.to_string(),
                ast::Type::Custom(error_type.to_string(), vec![]),
            );

            let body_val = self.lower_expr(body)?;

            // Restore previous bindings
            if let Some(old) = old_binding {
                self.scope.locals.insert(var.to_string(), old);
            } else {
                self.scope.locals.remove(var);
            }
            if let Some(old) = old_ast_type {
                self.scope.local_ast_types.insert(var.to_string(), old);
            } else {
                self.scope.local_ast_types.remove(var);
            }

            // Build the condition: tag == t1 || tag == t2 || ...
            let cond = self.build_tag_match_cond(tag_id, &matching_tags);

            let then_body = vec![
                FirStmt::Let {
                    name: var_id,
                    ty: FirType::Ptr,
                    value: FirExpr::LocalVar(err_val_id, FirType::Ptr),
                },
                FirStmt::Assign {
                    target: FirPlace::Local(result_id),
                    value: body_val,
                },
            ];

            let if_stmt = FirStmt::If {
                cond,
                then_body,
                else_body,
            };
            else_body = vec![if_stmt];
        }

        stmts.extend(else_body);
        Ok(stmts)
    }

    /// Build a condition expression: tag == t1 || tag == t2 || ...
    fn build_tag_match_cond(&self, tag_id: LocalId, tags: &[i64]) -> FirExpr {
        if tags.is_empty() {
            return FirExpr::BoolLit(false);
        }
        let mut cond = FirExpr::BinaryOp {
            left: Box::new(FirExpr::LocalVar(tag_id, FirType::I64)),
            op: crate::exprs::BinOp::Eq,
            right: Box::new(FirExpr::IntLit(tags[0])),
            result_ty: FirType::Bool,
        };
        for &tag in &tags[1..] {
            let next = FirExpr::BinaryOp {
                left: Box::new(FirExpr::LocalVar(tag_id, FirType::I64)),
                op: crate::exprs::BinOp::Eq,
                right: Box::new(FirExpr::IntLit(tag)),
                result_ty: FirType::Bool,
            };
            cond = FirExpr::BinaryOp {
                left: Box::new(cond),
                op: crate::exprs::BinOp::Or,
                right: Box::new(next),
                result_ty: FirType::Bool,
            };
        }
        cond
    }

    /// Collect ClassId tags for all classes that (transitively) extend the given type.
    fn collect_subclass_tags(&self, parent_name: &str, tags: &mut Vec<i64>) {
        for (name, &class_id) in &self.ms.classes {
            if let Some(class_info) = self.type_env.get_class(name)
                && class_info.extends.as_deref() == Some(parent_name)
            {
                let tag = class_id.0 as i64;
                if !tags.contains(&tag) {
                    tags.push(tag);
                    self.collect_subclass_tags(name, tags);
                }
            }
        }
    }

    /// Set error flag with type tag and error value, return a type-correct dummy.
    fn lower_throw_expr(&mut self, inner: &Expr) -> Result<FirExpr, LowerError> {
        let fir_inner = self.lower_expr(inner)?;

        // Extract the class name from the constructor call to get its type tag
        let class_name = match inner {
            Expr::Call { func, .. } => match func.as_ref() {
                Expr::Ident(name, _) => Some(name.clone()),
                _ => None,
            },
            _ => None,
        };
        let type_tag = class_name
            .as_deref()
            .and_then(|name| self.ms.classes.get(name))
            .map(|cid| cid.0 as i64)
            .unwrap_or(0);

        self.pending_stmts.push(FirStmt::Expr(FirExpr::RuntimeCall {
            name: "aster_error_set_typed".to_string(),
            args: vec![FirExpr::IntLit(type_tag), fir_inner],
            ret_ty: FirType::Void,
        }));
        let dummy = match self
            .scope
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

    pub(crate) fn to_string_expr(&self, ast_expr: &Expr, fir_expr: FirExpr) -> FirExpr {
        match self.infer_fir_type(&fir_expr) {
            FirType::Ptr => {
                // Check if this is a List (needs runtime to_string)
                let ast_ty = self.resolve_expr_ast_type(ast_expr);
                if matches!(ast_ty.as_ref(), Some(Type::List(_))) {
                    return FirExpr::RuntimeCall {
                        name: "aster_list_to_string".to_string(),
                        args: vec![fir_expr],
                        ret_ty: FirType::Ptr,
                    };
                }
                if let Ok(class_name) = self.resolve_class_name(ast_expr) {
                    let qualified = format!("{}.to_string", class_name);
                    if let Some(&func_id) = self.ms.functions.get(&qualified) {
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
    pub(crate) fn maybe_wrap_nullable_return(&self, fir_expr: FirExpr, ast_expr: &Expr) -> FirExpr {
        if let Some(Type::Nullable(inner)) = &self.scope.current_return_type {
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
    pub(crate) fn lower_call_args_with_defaults(
        &mut self,
        func_name: &str,
        args: &[(String, ast::Span, Expr)],
    ) -> Result<Vec<FirExpr>, LowerError> {
        if let Some(param_defaults) = self.ms.function_defaults.get(func_name).cloned() {
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
