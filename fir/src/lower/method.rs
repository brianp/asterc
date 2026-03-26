use super::*;

impl Lowerer {
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
            && name == builtin_class::FILE
        {
            match method {
                builtin_method::READ => {
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
                builtin_method::WRITE | builtin_method::APPEND => {
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
                    let runtime_name = if method == builtin_method::WRITE {
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
                Expr::Str(..) => Some(Type::String),
                _ => None,
            });

        if matches!(object_ast_ty, Some(Type::Task(_)))
            && let Some((runtime_name, ret_ty)) = match method {
                builtin_method::IS_READY => Some(("aster_task_is_ready", FirType::Bool)),
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
                builtin_method::CANCEL => {
                    return Ok(FirExpr::CancelTask {
                        task: Box::new(fir_object),
                    });
                }
                builtin_method::WAIT_CANCEL => {
                    return Ok(FirExpr::WaitCancel {
                        task: Box::new(fir_object),
                    });
                }
                _ => {}
            }
        }

        // Mutex[T] methods → runtime calls
        if matches!(&object_ast_ty, Some(Type::Custom(name, _)) if name == builtin_class::MUTEX) {
            match method {
                builtin_method::ACQUIRE => {
                    return Ok(FirExpr::RuntimeCall {
                        name: "aster_mutex_lock".to_string(),
                        args: vec![fir_object],
                        ret_ty: FirType::I64,
                    });
                }
                builtin_method::RELEASE => {
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
                builtin_method::LOCK => {
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
        if matches!(&object_ast_ty, Some(Type::Custom(name, _)) if name == builtin_class::CHANNEL || name == builtin_class::MULTI_SEND || name == builtin_class::MULTI_RECEIVE)
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
                builtin_method::SEND => {
                    return Ok(FirExpr::RuntimeCall {
                        name: "aster_channel_send".to_string(),
                        args: vec![fir_object, fir_value_arg()?],
                        ret_ty: FirType::Void,
                    });
                }
                builtin_method::WAIT_SEND => {
                    return Ok(FirExpr::RuntimeCall {
                        name: "aster_channel_wait_send".to_string(),
                        args: vec![fir_object, fir_value_arg()?],
                        ret_ty: FirType::Void,
                    });
                }
                builtin_method::TRY_SEND => {
                    return Ok(FirExpr::RuntimeCall {
                        name: "aster_channel_try_send".to_string(),
                        args: vec![fir_object, fir_value_arg()?],
                        ret_ty: FirType::Void,
                    });
                }
                builtin_method::RECEIVE => {
                    return Ok(FirExpr::RuntimeCall {
                        name: "aster_channel_receive".to_string(),
                        args: vec![fir_object],
                        ret_ty: FirType::I64,
                    });
                }
                builtin_method::WAIT_RECEIVE => {
                    return Ok(FirExpr::RuntimeCall {
                        name: "aster_channel_wait_receive".to_string(),
                        args: vec![fir_object],
                        ret_ty: FirType::I64,
                    });
                }
                builtin_method::TRY_RECEIVE => {
                    return Ok(FirExpr::RuntimeCall {
                        name: "aster_channel_try_receive".to_string(),
                        args: vec![fir_object],
                        ret_ty: FirType::I64,
                    });
                }
                builtin_method::CLOSE => {
                    return Ok(FirExpr::RuntimeCall {
                        name: "aster_channel_close".to_string(),
                        args: vec![fir_object],
                        ret_ty: FirType::Void,
                    });
                }
                builtin_method::CLONE_SENDER | builtin_method::CLONE_RECEIVER => {
                    // Clone returns the same handle (refcount bump is a future enhancement)
                    return Ok(fir_object);
                }
                _ => {}
            }
        }

        if method == builtin_method::OR_THROW {
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
        if method == builtin_method::OR
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

        // Nullable `.or_else(f: expr)` — lazy variant of `.or()`.
        // The fallback expression is only evaluated when the value is nil.
        if method == builtin_method::OR_ELSE
            && let FirType::TaggedUnion { ref variants, .. } = fir_object_ty
            && !variants.is_empty()
        {
            let inner_ty = variants[0].clone();
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

            // Lower fallback in an isolated pending_stmts scope so any
            // emitted statements land inside the else branch (lazy eval).
            let saved = std::mem::take(&mut self.pending_stmts);
            let fallback_expr = if let Some((_, _, fallback_arg)) = args.first() {
                self.lower_expr(fallback_arg)?
            } else {
                self.default_value_for_type(&inner_ty)
            };
            let mut else_body = std::mem::take(&mut self.pending_stmts);
            self.pending_stmts = saved;

            else_body.push(FirStmt::Assign {
                target: FirPlace::Local(result_id),
                value: fallback_expr,
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
                else_body,
            });

            return Ok(FirExpr::LocalVar(result_id, inner_ty));
        }

        // Check for Range methods
        if method == builtin_method::RANDOM && self.is_range_expr(object) {
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

        // Check for String built-in methods
        if matches!(object_ast_ty, Some(Type::String)) {
            match method {
                builtin_method::LEN => {
                    return Ok(FirExpr::RuntimeCall {
                        name: "aster_string_char_len".to_string(),
                        args: vec![fir_object],
                        ret_ty: FirType::I64,
                    });
                }
                builtin_method::CONTAINS => {
                    let substr = self.lower_first_arg(args)?;
                    return Ok(FirExpr::RuntimeCall {
                        name: "aster_string_contains".to_string(),
                        args: vec![fir_object, substr],
                        ret_ty: FirType::Bool,
                    });
                }
                builtin_method::STARTS_WITH => {
                    let prefix = self.lower_first_arg(args)?;
                    return Ok(FirExpr::RuntimeCall {
                        name: "aster_string_starts_with".to_string(),
                        args: vec![fir_object, prefix],
                        ret_ty: FirType::Bool,
                    });
                }
                builtin_method::ENDS_WITH => {
                    let suffix = self.lower_first_arg(args)?;
                    return Ok(FirExpr::RuntimeCall {
                        name: "aster_string_ends_with".to_string(),
                        args: vec![fir_object, suffix],
                        ret_ty: FirType::Bool,
                    });
                }
                builtin_method::TRIM => {
                    return Ok(FirExpr::RuntimeCall {
                        name: "aster_string_trim".to_string(),
                        args: vec![fir_object],
                        ret_ty: FirType::Ptr,
                    });
                }
                builtin_method::TO_UPPER => {
                    return Ok(FirExpr::RuntimeCall {
                        name: "aster_string_to_upper".to_string(),
                        args: vec![fir_object],
                        ret_ty: FirType::Ptr,
                    });
                }
                builtin_method::TO_LOWER => {
                    return Ok(FirExpr::RuntimeCall {
                        name: "aster_string_to_lower".to_string(),
                        args: vec![fir_object],
                        ret_ty: FirType::Ptr,
                    });
                }
                builtin_method::SLICE => {
                    let from_arg = self.lower_named_or_positional_arg(args, "from", 0)?;
                    let to_arg = self.lower_named_or_positional_arg(args, "to", 1)?;
                    return Ok(FirExpr::RuntimeCall {
                        name: "aster_string_slice".to_string(),
                        args: vec![fir_object, from_arg, to_arg],
                        ret_ty: FirType::Ptr,
                    });
                }
                builtin_method::REPLACE => {
                    let old_arg = self.lower_named_or_positional_arg(args, "old", 0)?;
                    let new_arg = self.lower_named_or_positional_arg(args, "new", 1)?;
                    return Ok(FirExpr::RuntimeCall {
                        name: "aster_string_replace".to_string(),
                        args: vec![fir_object, old_arg, new_arg],
                        ret_ty: FirType::Ptr,
                    });
                }
                builtin_method::SPLIT => {
                    let sep = self.lower_first_arg(args)?;
                    return Ok(FirExpr::RuntimeCall {
                        name: "aster_string_split".to_string(),
                        args: vec![fir_object, sep],
                        ret_ty: FirType::Ptr,
                    });
                }
                _ => {}
            }
        }

        // Check for list built-in methods
        match method {
            builtin_method::LEN => {
                return Ok(FirExpr::RuntimeCall {
                    name: "aster_list_len".to_string(),
                    args: vec![fir_object],
                    ret_ty: FirType::I64,
                });
            }
            builtin_method::PUSH => {
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
            builtin_method::INSERT => {
                let at_arg = args
                    .iter()
                    .find(|(n, _, _)| n == "at")
                    .or_else(|| args.first())
                    .map(|(_, _, e)| e);
                let item_arg = args
                    .iter()
                    .find(|(n, _, _)| n == "item")
                    .or_else(|| args.get(1))
                    .map(|(_, _, e)| e);
                let fir_at = self.lower_expr(at_arg.unwrap())?;
                let fir_item = self.lower_expr(item_arg.unwrap())?;
                return Ok(FirExpr::RuntimeCall {
                    name: "aster_list_insert".to_string(),
                    args: vec![fir_object, fir_at, fir_item],
                    ret_ty: FirType::Void,
                });
            }
            builtin_method::REMOVE => {
                let at_arg = args
                    .iter()
                    .find(|(n, _, _)| n == "at")
                    .or_else(|| args.first())
                    .map(|(_, _, e)| e);
                let fir_at = self.lower_expr(at_arg.unwrap())?;
                return Ok(FirExpr::RuntimeCall {
                    name: "aster_list_remove".to_string(),
                    args: vec![fir_object, fir_at],
                    ret_ty: FirType::I64,
                });
            }
            builtin_method::POP => {
                return Ok(FirExpr::RuntimeCall {
                    name: "aster_list_pop".to_string(),
                    args: vec![fir_object],
                    ret_ty: FirType::I64,
                });
            }
            builtin_method::CONTAINS => {
                // Check if this is the predicate form: contains(f: ...)
                let is_predicate = args.first().is_some_and(|(n, _, _)| n == "f");
                if is_predicate {
                    // Determine element type for the loop
                    let elem_ty = if let Some(Type::List(inner)) = &object_ast_ty {
                        self.lower_type(inner)
                    } else {
                        self.resolve_list_elem_type(object).unwrap_or(FirType::I64)
                    };
                    // Reuse `any` lowering
                    return self.lower_iterable_with_callback(
                        builtin_method::ANY,
                        fir_object,
                        args,
                        &elem_ty,
                        object,
                    );
                }
                // contains(item:) — runtime call with string flag
                let item_arg = args
                    .iter()
                    .find(|(n, _, _)| n == "item")
                    .or_else(|| args.first())
                    .map(|(_, _, e)| e);
                let fir_item = self.lower_expr(item_arg.unwrap())?;
                let is_string = if let Some(Type::List(inner)) = &object_ast_ty {
                    if **inner == Type::String {
                        FirExpr::IntLit(1)
                    } else {
                        FirExpr::IntLit(0)
                    }
                } else {
                    FirExpr::IntLit(0)
                };
                return Ok(FirExpr::RuntimeCall {
                    name: "aster_list_contains".to_string(),
                    args: vec![fir_object, fir_item, is_string],
                    ret_ty: FirType::Bool,
                });
            }
            builtin_method::REMOVE_FIRST => {
                let elem_ty = if let Some(Type::List(inner)) = &object_ast_ty {
                    self.lower_type(inner)
                } else {
                    self.resolve_list_elem_type(object).unwrap_or(FirType::I64)
                };
                return self.lower_list_remove_first(fir_object, args, &elem_ty, object);
            }
            builtin_method::RANDOM => {
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
                builtin_method::MAP
                | builtin_method::FILTER
                | builtin_method::FIND
                | builtin_method::ANY
                | builtin_method::ALL => {
                    return self
                        .lower_iterable_with_callback(method, fir_object, args, &elem_ty, object);
                }
                builtin_method::EACH => {
                    return self.lower_iterable_each(fir_object, args, &elem_ty, object);
                }
                builtin_method::REDUCE => {
                    return self.lower_iterable_reduce(fir_object, args, &elem_ty, object);
                }
                builtin_method::COUNT => {
                    return Ok(FirExpr::RuntimeCall {
                        name: "aster_list_len".to_string(),
                        args: vec![fir_object],
                        ret_ty: FirType::I64,
                    });
                }
                builtin_method::FIRST => {
                    return self.lower_iterable_first(fir_object, &elem_ty);
                }
                builtin_method::LAST => {
                    return self.lower_iterable_last(fir_object, &elem_ty);
                }
                builtin_method::TO_LIST => {
                    return self.lower_iterable_to_list(fir_object, &elem_ty);
                }
                builtin_method::MIN | builtin_method::MAX => {
                    return self.lower_iterable_min_max(method, fir_object, &elem_ty);
                }
                builtin_method::SORT => {
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

    /// Lower the first argument of a method call (by name or position).
    fn lower_first_arg(
        &mut self,
        args: &[(String, ast::Span, Expr)],
    ) -> Result<FirExpr, LowerError> {
        if let Some((_, _, arg)) = args.first() {
            self.lower_expr(arg)
        } else {
            Ok(FirExpr::IntLit(0))
        }
    }

    /// Lower a named argument, falling back to positional index.
    fn lower_named_or_positional_arg(
        &mut self,
        args: &[(String, ast::Span, Expr)],
        name: &str,
        position: usize,
    ) -> Result<FirExpr, LowerError> {
        let arg_expr = args
            .iter()
            .find(|(n, _, _)| n == name)
            .map(|(_, _, e)| e)
            .or_else(|| args.get(position).map(|(_, _, e)| e));
        if let Some(expr) = arg_expr {
            self.lower_expr(expr)
        } else {
            Ok(FirExpr::IntLit(0))
        }
    }
}
