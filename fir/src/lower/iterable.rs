use super::*;

impl Lowerer {
    /// Root a Ptr-typed expression in a temporary Let binding so the GC shadow
    /// stack can track it. If the expression is already a LocalVar (already
    /// rooted) or is not a Ptr/Struct type, returns it unchanged.
    fn root_if_ptr(&mut self, expr: FirExpr, stmts: &mut Vec<FirStmt>) -> FirExpr {
        if matches!(expr, FirExpr::LocalVar(_, _)) {
            return expr;
        }
        let ty = self.infer_fir_type(&expr);
        if !ty.needs_gc_root() {
            return expr;
        }
        let tmp_id = self.alloc_local();
        self.local_types.insert(tmp_id, ty.clone());
        stmts.push(FirStmt::Let {
            name: tmp_id,
            ty: ty.clone(),
            value: expr,
        });
        FirExpr::LocalVar(tmp_id, ty)
    }

    /// Return a FirExpr flag indicating whether list elements are GC pointers.
    /// 1 for Ptr/Struct (need tracing), 0 for value types (Int/Float/Bool).
    pub(crate) fn list_ptr_elems_flag(elem_ty: &FirType) -> FirExpr {
        if elem_ty.needs_gc_root() {
            FirExpr::IntLit(1)
        } else {
            FirExpr::IntLit(0)
        }
    }

    /// Build a list iteration loop scaffold. Returns (list_id, len_id, idx_id, elem_id).
    /// Caller must fill in the loop body. Setup stmts go into pending_stmts.
    pub(crate) fn iter_loop_scaffold(
        &mut self,
        fir_list: FirExpr,
        elem_ty: &FirType,
    ) -> (LocalId, LocalId, LocalId, LocalId) {
        let uid = self.next_local;

        let list_id = self.alloc_local();
        self.locals.insert(format!("__iter_list_{}", uid), list_id);
        self.local_types.insert(list_id, FirType::Ptr);

        let len_id = self.alloc_local();
        self.locals.insert(format!("__iter_len_{}", uid), len_id);
        self.local_types.insert(len_id, FirType::I64);

        let idx_id = self.alloc_local();
        self.locals.insert(format!("__iter_idx_{}", uid), idx_id);
        self.local_types.insert(idx_id, FirType::I64);

        let elem_id = self.alloc_local();
        self.locals.insert(format!("__iter_elem_{}", uid), elem_id);
        self.local_types.insert(elem_id, elem_ty.clone());

        self.pending_stmts.push(FirStmt::Let {
            name: list_id,
            ty: FirType::Ptr,
            value: fir_list,
        });
        self.pending_stmts.push(FirStmt::Let {
            name: len_id,
            ty: FirType::I64,
            value: FirExpr::RuntimeCall {
                name: "aster_list_len".to_string(),
                args: vec![FirExpr::LocalVar(list_id, FirType::Ptr)],
                ret_ty: FirType::I64,
            },
        });
        self.pending_stmts.push(FirStmt::Let {
            name: idx_id,
            ty: FirType::I64,
            value: FirExpr::IntLit(0),
        });

        (list_id, len_id, idx_id, elem_id)
    }

    /// Standard increment stmt for iteration loops.
    /// Standard increment stmt for iteration loops.
    pub(crate) fn iter_increment(idx_id: LocalId) -> Vec<FirStmt> {
        vec![FirStmt::Assign {
            target: FirPlace::Local(idx_id),
            value: FirExpr::BinaryOp {
                left: Box::new(FirExpr::LocalVar(idx_id, FirType::I64)),
                op: BinOp::Add,
                right: Box::new(FirExpr::IntLit(1)),
                result_ty: FirType::I64,
            },
        }]
    }

    /// Standard loop condition: idx < len.
    /// Standard loop condition: idx < len.
    pub(crate) fn iter_cond(idx_id: LocalId, len_id: LocalId) -> FirExpr {
        FirExpr::BinaryOp {
            left: Box::new(FirExpr::LocalVar(idx_id, FirType::I64)),
            op: BinOp::Lt,
            right: Box::new(FirExpr::LocalVar(len_id, FirType::I64)),
            result_ty: FirType::Bool,
        }
    }

    /// Get element at current index.
    /// Get element at current index.
    pub(crate) fn iter_get_elem(
        list_id: LocalId,
        idx_id: LocalId,
        elem_id: LocalId,
        elem_ty: &FirType,
    ) -> FirStmt {
        FirStmt::Let {
            name: elem_id,
            ty: elem_ty.clone(),
            value: FirExpr::RuntimeCall {
                name: "aster_list_get".to_string(),
                args: vec![
                    FirExpr::LocalVar(list_id, FirType::Ptr),
                    FirExpr::LocalVar(idx_id, FirType::I64),
                ],
                ret_ty: elem_ty.clone(),
            },
        }
    }

    /// Resolve the callback argument `f` from a method call's args list.
    /// Looks for a named arg `f`, falling back to the first positional arg.
    pub(crate) fn resolve_callback_arg<'a>(
        args: &'a [(String, ast::Span, Expr)],
    ) -> Option<&'a Expr> {
        args.iter()
            .find(|(n, _, _)| n == "f")
            .map(|(_, _, e)| e)
            .or_else(|| args.first().map(|(_, _, e)| e))
    }

    /// Lower map, filter, find, any, all — methods that take a single callback `f`.
    pub(crate) fn lower_iterable_with_callback(
        &mut self,
        method: &str,
        fir_list: FirExpr,
        args: &[(String, ast::Span, Expr)],
        elem_ty: &FirType,
        object: &Expr,
    ) -> Result<FirExpr, LowerError> {
        let callback = Self::resolve_callback_arg(args);

        let (list_id, len_id, idx_id, elem_id) = self.iter_loop_scaffold(fir_list, elem_ty);

        match method {
            builtin_method::MAP => {
                // result = new list; for each elem: result.push(f(elem))
                let result_id = self.alloc_local();
                self.local_types.insert(result_id, FirType::Ptr);
                self.pending_stmts.push(FirStmt::Let {
                    name: result_id,
                    ty: FirType::Ptr,
                    value: FirExpr::RuntimeCall {
                        name: "aster_list_new".to_string(),
                        args: vec![
                            FirExpr::LocalVar(len_id, FirType::I64),
                            Self::list_ptr_elems_flag(elem_ty),
                        ],
                        ret_ty: FirType::Ptr,
                    },
                });

                let mut loop_body = vec![Self::iter_get_elem(list_id, idx_id, elem_id, elem_ty)];
                let saved_pending = std::mem::take(&mut self.pending_stmts);
                let mapped_val = self.apply_inline_lambda(callback, elem_id, elem_ty, object)?;
                loop_body.append(&mut self.pending_stmts);
                self.pending_stmts = saved_pending;

                // GH-1: Root the mapped value in a Let binding so the GC shadow
                // stack tracks it. Without this, a Ptr-typed expression temporary
                // (e.g. from string concat or object construction) can be collected
                // if aster_list_push triggers a GC cycle.
                let mapped_ref = self.root_if_ptr(mapped_val, &mut loop_body);

                loop_body.push(FirStmt::Expr(FirExpr::RuntimeCall {
                    name: "aster_list_push".to_string(),
                    args: vec![FirExpr::LocalVar(result_id, FirType::Ptr), mapped_ref],
                    ret_ty: FirType::Ptr,
                }));

                self.pending_stmts.push(FirStmt::While {
                    cond: Self::iter_cond(idx_id, len_id),
                    body: loop_body,
                    increment: Self::iter_increment(idx_id),
                });
                Ok(FirExpr::LocalVar(result_id, FirType::Ptr))
            }
            builtin_method::FILTER => {
                let result_id = self.alloc_local();
                self.local_types.insert(result_id, FirType::Ptr);
                self.pending_stmts.push(FirStmt::Let {
                    name: result_id,
                    ty: FirType::Ptr,
                    value: FirExpr::RuntimeCall {
                        name: "aster_list_new".to_string(),
                        args: vec![FirExpr::IntLit(4), Self::list_ptr_elems_flag(elem_ty)],
                        ret_ty: FirType::Ptr,
                    },
                });

                let mut loop_body = vec![Self::iter_get_elem(list_id, idx_id, elem_id, elem_ty)];
                let saved_pending = std::mem::take(&mut self.pending_stmts);
                let cond_val = self.apply_inline_lambda(callback, elem_id, elem_ty, object)?;
                loop_body.append(&mut self.pending_stmts);
                self.pending_stmts = saved_pending;
                loop_body.push(FirStmt::If {
                    cond: cond_val,
                    then_body: vec![FirStmt::Expr(FirExpr::RuntimeCall {
                        name: "aster_list_push".to_string(),
                        args: vec![
                            FirExpr::LocalVar(result_id, FirType::Ptr),
                            FirExpr::LocalVar(elem_id, elem_ty.clone()),
                        ],
                        ret_ty: FirType::Ptr,
                    })],
                    else_body: vec![],
                });

                self.pending_stmts.push(FirStmt::While {
                    cond: Self::iter_cond(idx_id, len_id),
                    body: loop_body,
                    increment: Self::iter_increment(idx_id),
                });
                Ok(FirExpr::LocalVar(result_id, FirType::Ptr))
            }
            builtin_method::FIND => {
                // result = nil; for each elem: if f(elem) then result = Some(elem), break
                let nullable_ty = FirType::TaggedUnion {
                    tag_bits: 1,
                    variants: vec![elem_ty.clone(), FirType::Void],
                };
                let result_id = self.alloc_local();
                self.local_types.insert(result_id, nullable_ty.clone());
                self.pending_stmts.push(FirStmt::Let {
                    name: result_id,
                    ty: nullable_ty.clone(),
                    value: FirExpr::TagWrap {
                        tag: 1,
                        value: Box::new(FirExpr::NilLit),
                        ty: FirType::Ptr,
                    },
                });

                let mut loop_body = vec![Self::iter_get_elem(list_id, idx_id, elem_id, elem_ty)];
                let saved_pending = std::mem::take(&mut self.pending_stmts);
                let cond_val = self.apply_inline_lambda(callback, elem_id, elem_ty, object)?;
                loop_body.append(&mut self.pending_stmts);
                self.pending_stmts = saved_pending;
                loop_body.push(FirStmt::If {
                    cond: cond_val,
                    then_body: vec![
                        FirStmt::Assign {
                            target: FirPlace::Local(result_id),
                            value: FirExpr::TagWrap {
                                tag: 0,
                                value: Box::new(FirExpr::LocalVar(elem_id, elem_ty.clone())),
                                ty: elem_ty.clone(),
                            },
                        },
                        FirStmt::Break,
                    ],
                    else_body: vec![],
                });

                self.pending_stmts.push(FirStmt::While {
                    cond: Self::iter_cond(idx_id, len_id),
                    body: loop_body,
                    increment: Self::iter_increment(idx_id),
                });
                Ok(FirExpr::LocalVar(result_id, nullable_ty))
            }
            builtin_method::ANY => {
                let result_id = self.alloc_local();
                self.local_types.insert(result_id, FirType::Bool);
                self.pending_stmts.push(FirStmt::Let {
                    name: result_id,
                    ty: FirType::Bool,
                    value: FirExpr::BoolLit(false),
                });

                let mut loop_body = vec![Self::iter_get_elem(list_id, idx_id, elem_id, elem_ty)];
                let saved_pending = std::mem::take(&mut self.pending_stmts);
                let cond_val = self.apply_inline_lambda(callback, elem_id, elem_ty, object)?;
                loop_body.append(&mut self.pending_stmts);
                self.pending_stmts = saved_pending;
                loop_body.push(FirStmt::If {
                    cond: cond_val,
                    then_body: vec![
                        FirStmt::Assign {
                            target: FirPlace::Local(result_id),
                            value: FirExpr::BoolLit(true),
                        },
                        FirStmt::Break,
                    ],
                    else_body: vec![],
                });

                self.pending_stmts.push(FirStmt::While {
                    cond: Self::iter_cond(idx_id, len_id),
                    body: loop_body,
                    increment: Self::iter_increment(idx_id),
                });
                Ok(FirExpr::LocalVar(result_id, FirType::Bool))
            }
            builtin_method::ALL => {
                let result_id = self.alloc_local();
                self.local_types.insert(result_id, FirType::Bool);
                self.pending_stmts.push(FirStmt::Let {
                    name: result_id,
                    ty: FirType::Bool,
                    value: FirExpr::BoolLit(true),
                });

                let mut loop_body = vec![Self::iter_get_elem(list_id, idx_id, elem_id, elem_ty)];
                let saved_pending = std::mem::take(&mut self.pending_stmts);
                let cond_val = self.apply_inline_lambda(callback, elem_id, elem_ty, object)?;
                loop_body.append(&mut self.pending_stmts);
                self.pending_stmts = saved_pending;
                // if NOT cond: result = false, break
                loop_body.push(FirStmt::If {
                    cond: FirExpr::UnaryOp {
                        op: UnaryOp::Not,
                        operand: Box::new(cond_val),
                        result_ty: FirType::Bool,
                    },
                    then_body: vec![
                        FirStmt::Assign {
                            target: FirPlace::Local(result_id),
                            value: FirExpr::BoolLit(false),
                        },
                        FirStmt::Break,
                    ],
                    else_body: vec![],
                });

                self.pending_stmts.push(FirStmt::While {
                    cond: Self::iter_cond(idx_id, len_id),
                    body: loop_body,
                    increment: Self::iter_increment(idx_id),
                });
                Ok(FirExpr::LocalVar(result_id, FirType::Bool))
            }
            _ => unreachable!(),
        }
    }

    /// Lower .each(f:) -- call the callback for every element, return Void.
    pub(crate) fn lower_iterable_each(
        &mut self,
        fir_list: FirExpr,
        args: &[(String, ast::Span, Expr)],
        elem_ty: &FirType,
        object: &Expr,
    ) -> Result<FirExpr, LowerError> {
        let callback = Self::resolve_callback_arg(args);

        let (list_id, len_id, idx_id, elem_id) = self.iter_loop_scaffold(fir_list, elem_ty);

        let mut loop_body = vec![Self::iter_get_elem(list_id, idx_id, elem_id, elem_ty)];
        let saved_pending = std::mem::take(&mut self.pending_stmts);
        let result_val = self.apply_inline_lambda(callback, elem_id, elem_ty, object)?;
        loop_body.append(&mut self.pending_stmts);
        self.pending_stmts = saved_pending;
        // Emit the callback result as a statement (for side effects only)
        loop_body.push(FirStmt::Expr(result_val));

        self.pending_stmts.push(FirStmt::While {
            cond: Self::iter_cond(idx_id, len_id),
            body: loop_body,
            increment: Self::iter_increment(idx_id),
        });
        Ok(FirExpr::NilLit)
    }

    /// Apply an inline lambda to the element variable. Binds the lambda param
    /// to `elem_id` and inlines the lambda body, returning the result expression.
    pub(crate) fn apply_inline_lambda(
        &mut self,
        callback: Option<&Expr>,
        elem_id: LocalId,
        elem_ty: &FirType,
        _object: &Expr,
    ) -> Result<FirExpr, LowerError> {
        if let Some(Expr::Lambda { params, body, .. }) = callback {
            // Bind the lambda parameter to the element local
            let param_name = params
                .first()
                .map(|(n, _)| n.clone())
                .unwrap_or_else(|| "__it".into());
            self.locals.insert(param_name, elem_id);
            self.local_types.insert(elem_id, elem_ty.clone());
            self.lower_inline_body(body)
        } else {
            // Fallback: identity (shouldn't happen for well-typed programs)
            Ok(FirExpr::LocalVar(elem_id, elem_ty.clone()))
        }
    }

    /// Apply a two-arg inline lambda (for reduce: (acc, elem) -> result).
    /// Apply a two-arg inline lambda (for reduce: (acc, elem) -> result).
    pub(crate) fn apply_inline_lambda2(
        &mut self,
        callback: Option<&Expr>,
        acc_id: LocalId,
        acc_ty: &FirType,
        elem_id: LocalId,
        elem_ty: &FirType,
    ) -> Result<FirExpr, LowerError> {
        if let Some(Expr::Lambda { params, body, .. }) = callback {
            let acc_name = params
                .first()
                .map(|(n, _)| n.clone())
                .unwrap_or_else(|| "__acc".into());
            let elem_name = params
                .get(1)
                .map(|(n, _)| n.clone())
                .unwrap_or_else(|| "__it".into());
            self.locals.insert(acc_name, acc_id);
            self.local_types.insert(acc_id, acc_ty.clone());
            self.locals.insert(elem_name, elem_id);
            self.local_types.insert(elem_id, elem_ty.clone());
            self.lower_inline_body(body)
        } else {
            Ok(FirExpr::LocalVar(acc_id, acc_ty.clone()))
        }
    }

    /// Lower reduce: (init: U, f: (U, T) -> U) -> U
    /// Lower reduce: (init: U, f: (U, T) -> U) -> U
    pub(crate) fn lower_iterable_reduce(
        &mut self,
        fir_list: FirExpr,
        args: &[(String, ast::Span, Expr)],
        elem_ty: &FirType,
        _object: &Expr,
    ) -> Result<FirExpr, LowerError> {
        let init_expr = args
            .iter()
            .find(|(n, _, _)| n == "init")
            .map(|(_, _, e)| e)
            .or_else(|| args.first().map(|(_, _, e)| e));
        let callback = args
            .iter()
            .find(|(n, _, _)| n == "f")
            .map(|(_, _, e)| e)
            .or_else(|| args.get(1).map(|(_, _, e)| e));

        let fir_init = if let Some(e) = init_expr {
            self.lower_expr(e)?
        } else {
            FirExpr::IntLit(0)
        };
        let acc_ty = self.infer_fir_type(&fir_init);

        let (list_id, len_id, idx_id, elem_id) = self.iter_loop_scaffold(fir_list, elem_ty);

        let acc_id = self.alloc_local();
        self.local_types.insert(acc_id, acc_ty.clone());
        self.pending_stmts.push(FirStmt::Let {
            name: acc_id,
            ty: acc_ty.clone(),
            value: fir_init,
        });

        let mut loop_body = vec![Self::iter_get_elem(list_id, idx_id, elem_id, elem_ty)];

        let saved_pending = std::mem::take(&mut self.pending_stmts);
        let result_val = self.apply_inline_lambda2(callback, acc_id, &acc_ty, elem_id, elem_ty)?;
        loop_body.append(&mut self.pending_stmts);
        self.pending_stmts = saved_pending;
        loop_body.push(FirStmt::Assign {
            target: FirPlace::Local(acc_id),
            value: result_val,
        });

        self.pending_stmts.push(FirStmt::While {
            cond: Self::iter_cond(idx_id, len_id),
            body: loop_body,
            increment: Self::iter_increment(idx_id),
        });
        Ok(FirExpr::LocalVar(acc_id, acc_ty))
    }

    /// Lower first() -> T?
    /// Lower first() -> T?
    pub(crate) fn lower_iterable_first(
        &mut self,
        fir_list: FirExpr,
        elem_ty: &FirType,
    ) -> Result<FirExpr, LowerError> {
        let nullable_ty = FirType::TaggedUnion {
            tag_bits: 1,
            variants: vec![elem_ty.clone(), FirType::Void],
        };

        // Store list in a local
        let list_id = self.alloc_local();
        self.local_types.insert(list_id, FirType::Ptr);
        self.pending_stmts.push(FirStmt::Let {
            name: list_id,
            ty: FirType::Ptr,
            value: fir_list,
        });

        let len_expr = FirExpr::RuntimeCall {
            name: "aster_list_len".to_string(),
            args: vec![FirExpr::LocalVar(list_id, FirType::Ptr)],
            ret_ty: FirType::I64,
        };

        let result_id = self.alloc_local();
        self.local_types.insert(result_id, nullable_ty.clone());
        self.pending_stmts.push(FirStmt::Let {
            name: result_id,
            ty: nullable_ty.clone(),
            value: FirExpr::TagWrap {
                tag: 1,
                value: Box::new(FirExpr::NilLit),
                ty: FirType::Ptr,
            },
        });

        // if len > 0: result = Some(list[0])
        self.pending_stmts.push(FirStmt::If {
            cond: FirExpr::BinaryOp {
                left: Box::new(len_expr),
                op: BinOp::Gt,
                right: Box::new(FirExpr::IntLit(0)),
                result_ty: FirType::Bool,
            },
            then_body: vec![FirStmt::Assign {
                target: FirPlace::Local(result_id),
                value: FirExpr::TagWrap {
                    tag: 0,
                    value: Box::new(FirExpr::RuntimeCall {
                        name: "aster_list_get".to_string(),
                        args: vec![FirExpr::LocalVar(list_id, FirType::Ptr), FirExpr::IntLit(0)],
                        ret_ty: elem_ty.clone(),
                    }),
                    ty: elem_ty.clone(),
                },
            }],
            else_body: vec![],
        });

        Ok(FirExpr::LocalVar(result_id, nullable_ty))
    }

    /// Lower last() -> T?
    /// Lower last() -> T?
    pub(crate) fn lower_iterable_last(
        &mut self,
        fir_list: FirExpr,
        elem_ty: &FirType,
    ) -> Result<FirExpr, LowerError> {
        let nullable_ty = FirType::TaggedUnion {
            tag_bits: 1,
            variants: vec![elem_ty.clone(), FirType::Void],
        };

        let list_id = self.alloc_local();
        self.local_types.insert(list_id, FirType::Ptr);
        self.pending_stmts.push(FirStmt::Let {
            name: list_id,
            ty: FirType::Ptr,
            value: fir_list,
        });

        let len_id = self.alloc_local();
        self.local_types.insert(len_id, FirType::I64);
        self.pending_stmts.push(FirStmt::Let {
            name: len_id,
            ty: FirType::I64,
            value: FirExpr::RuntimeCall {
                name: "aster_list_len".to_string(),
                args: vec![FirExpr::LocalVar(list_id, FirType::Ptr)],
                ret_ty: FirType::I64,
            },
        });

        let result_id = self.alloc_local();
        self.local_types.insert(result_id, nullable_ty.clone());
        self.pending_stmts.push(FirStmt::Let {
            name: result_id,
            ty: nullable_ty.clone(),
            value: FirExpr::TagWrap {
                tag: 1,
                value: Box::new(FirExpr::NilLit),
                ty: FirType::Ptr,
            },
        });

        // if len > 0: result = Some(list[len - 1])
        self.pending_stmts.push(FirStmt::If {
            cond: FirExpr::BinaryOp {
                left: Box::new(FirExpr::LocalVar(len_id, FirType::I64)),
                op: BinOp::Gt,
                right: Box::new(FirExpr::IntLit(0)),
                result_ty: FirType::Bool,
            },
            then_body: vec![FirStmt::Assign {
                target: FirPlace::Local(result_id),
                value: FirExpr::TagWrap {
                    tag: 0,
                    value: Box::new(FirExpr::RuntimeCall {
                        name: "aster_list_get".to_string(),
                        args: vec![
                            FirExpr::LocalVar(list_id, FirType::Ptr),
                            FirExpr::BinaryOp {
                                left: Box::new(FirExpr::LocalVar(len_id, FirType::I64)),
                                op: BinOp::Sub,
                                right: Box::new(FirExpr::IntLit(1)),
                                result_ty: FirType::I64,
                            },
                        ],
                        ret_ty: elem_ty.clone(),
                    }),
                    ty: elem_ty.clone(),
                },
            }],
            else_body: vec![],
        });

        Ok(FirExpr::LocalVar(result_id, nullable_ty))
    }

    /// Lower to_list() -> List[T] (copy the list)
    /// Lower to_list() -> List[T] (copy the list)
    pub(crate) fn lower_iterable_to_list(
        &mut self,
        fir_list: FirExpr,
        elem_ty: &FirType,
    ) -> Result<FirExpr, LowerError> {
        let (list_id, len_id, idx_id, elem_id) = self.iter_loop_scaffold(fir_list, elem_ty);

        let result_id = self.alloc_local();
        self.local_types.insert(result_id, FirType::Ptr);
        self.pending_stmts.push(FirStmt::Let {
            name: result_id,
            ty: FirType::Ptr,
            value: FirExpr::RuntimeCall {
                name: "aster_list_new".to_string(),
                args: vec![
                    FirExpr::LocalVar(len_id, FirType::I64),
                    Self::list_ptr_elems_flag(elem_ty),
                ],
                ret_ty: FirType::Ptr,
            },
        });

        let loop_body = vec![
            Self::iter_get_elem(list_id, idx_id, elem_id, elem_ty),
            FirStmt::Expr(FirExpr::RuntimeCall {
                name: "aster_list_push".to_string(),
                args: vec![
                    FirExpr::LocalVar(result_id, FirType::Ptr),
                    FirExpr::LocalVar(elem_id, elem_ty.clone()),
                ],
                ret_ty: FirType::Ptr,
            }),
        ];

        self.pending_stmts.push(FirStmt::While {
            cond: Self::iter_cond(idx_id, len_id),
            body: loop_body,
            increment: Self::iter_increment(idx_id),
        });
        Ok(FirExpr::LocalVar(result_id, FirType::Ptr))
    }

    /// Lower min() / max() -> T?  (integer comparison for now)
    /// Lower min() / max() -> T?  (integer comparison for now)
    pub(crate) fn lower_iterable_min_max(
        &mut self,
        method: &str,
        fir_list: FirExpr,
        elem_ty: &FirType,
    ) -> Result<FirExpr, LowerError> {
        let nullable_ty = FirType::TaggedUnion {
            tag_bits: 1,
            variants: vec![elem_ty.clone(), FirType::Void],
        };

        let (list_id, len_id, idx_id, elem_id) = self.iter_loop_scaffold(fir_list, elem_ty);

        let result_id = self.alloc_local();
        self.local_types.insert(result_id, nullable_ty.clone());
        self.pending_stmts.push(FirStmt::Let {
            name: result_id,
            ty: nullable_ty.clone(),
            value: FirExpr::TagWrap {
                tag: 1,
                value: Box::new(FirExpr::NilLit),
                ty: FirType::Ptr,
            },
        });

        let best_id = self.alloc_local();
        self.local_types.insert(best_id, elem_ty.clone());
        self.pending_stmts.push(FirStmt::Let {
            name: best_id,
            ty: elem_ty.clone(),
            value: self.default_value_for_type(elem_ty),
        });

        let has_value_id = self.alloc_local();
        self.local_types.insert(has_value_id, FirType::Bool);
        self.pending_stmts.push(FirStmt::Let {
            name: has_value_id,
            ty: FirType::Bool,
            value: FirExpr::BoolLit(false),
        });

        let cmp_op = if method == builtin_method::MIN {
            BinOp::Lt
        } else {
            BinOp::Gt
        };

        let loop_body = vec![
            Self::iter_get_elem(list_id, idx_id, elem_id, elem_ty),
            // if !has_value || elem <|> best: best = elem, has_value = true
            FirStmt::If {
                cond: FirExpr::BinaryOp {
                    left: Box::new(FirExpr::UnaryOp {
                        op: UnaryOp::Not,
                        operand: Box::new(FirExpr::LocalVar(has_value_id, FirType::Bool)),
                        result_ty: FirType::Bool,
                    }),
                    op: BinOp::Or,
                    right: Box::new(FirExpr::BinaryOp {
                        left: Box::new(FirExpr::LocalVar(elem_id, elem_ty.clone())),
                        op: cmp_op,
                        right: Box::new(FirExpr::LocalVar(best_id, elem_ty.clone())),
                        result_ty: FirType::Bool,
                    }),
                    result_ty: FirType::Bool,
                },
                then_body: vec![
                    FirStmt::Assign {
                        target: FirPlace::Local(best_id),
                        value: FirExpr::LocalVar(elem_id, elem_ty.clone()),
                    },
                    FirStmt::Assign {
                        target: FirPlace::Local(has_value_id),
                        value: FirExpr::BoolLit(true),
                    },
                ],
                else_body: vec![],
            },
        ];

        self.pending_stmts.push(FirStmt::While {
            cond: Self::iter_cond(idx_id, len_id),
            body: loop_body,
            increment: Self::iter_increment(idx_id),
        });

        // Wrap result: if has_value: Some(best) else nil
        self.pending_stmts.push(FirStmt::If {
            cond: FirExpr::LocalVar(has_value_id, FirType::Bool),
            then_body: vec![FirStmt::Assign {
                target: FirPlace::Local(result_id),
                value: FirExpr::TagWrap {
                    tag: 0,
                    value: Box::new(FirExpr::LocalVar(best_id, elem_ty.clone())),
                    ty: elem_ty.clone(),
                },
            }],
            else_body: vec![],
        });

        Ok(FirExpr::LocalVar(result_id, nullable_ty))
    }

    /// Lower sort() -> List[T] (insertion sort for now, integer comparison)
    /// Lower sort() -> List[T] (insertion sort for now, integer comparison)
    pub(crate) fn lower_iterable_sort(
        &mut self,
        fir_list: FirExpr,
        elem_ty: &FirType,
    ) -> Result<FirExpr, LowerError> {
        // Copy to new list, then insertion sort in place
        let (list_id, len_id, idx_id, elem_id) = self.iter_loop_scaffold(fir_list, elem_ty);

        // Build result list as a copy
        let result_id = self.alloc_local();
        self.local_types.insert(result_id, FirType::Ptr);
        self.pending_stmts.push(FirStmt::Let {
            name: result_id,
            ty: FirType::Ptr,
            value: FirExpr::RuntimeCall {
                name: "aster_list_new".to_string(),
                args: vec![
                    FirExpr::LocalVar(len_id, FirType::I64),
                    Self::list_ptr_elems_flag(elem_ty),
                ],
                ret_ty: FirType::Ptr,
            },
        });

        // Copy loop
        let copy_body = vec![
            Self::iter_get_elem(list_id, idx_id, elem_id, elem_ty),
            FirStmt::Expr(FirExpr::RuntimeCall {
                name: "aster_list_push".to_string(),
                args: vec![
                    FirExpr::LocalVar(result_id, FirType::Ptr),
                    FirExpr::LocalVar(elem_id, elem_ty.clone()),
                ],
                ret_ty: FirType::Ptr,
            }),
        ];
        self.pending_stmts.push(FirStmt::While {
            cond: Self::iter_cond(idx_id, len_id),
            body: copy_body,
            increment: Self::iter_increment(idx_id),
        });

        // Insertion sort: for i in 1..len: key=result[i]; j=i-1; while j>=0 && result[j]>key: result[j+1]=result[j]; j--; result[j+1]=key
        let uid2 = self.next_local;
        let i_id = self.alloc_local();
        self.locals.insert(format!("__sort_i_{}", uid2), i_id);
        self.local_types.insert(i_id, FirType::I64);
        self.pending_stmts.push(FirStmt::Let {
            name: i_id,
            ty: FirType::I64,
            value: FirExpr::IntLit(1),
        });

        let key_id = self.alloc_local();
        self.locals.insert(format!("__sort_key_{}", uid2), key_id);
        self.local_types.insert(key_id, elem_ty.clone());

        let j_id = self.alloc_local();
        self.locals.insert(format!("__sort_j_{}", uid2), j_id);
        self.local_types.insert(j_id, FirType::I64);

        // Inner while: j >= 0 && result[j] > key
        let inner_body = vec![
            // result[j+1] = result[j]
            FirStmt::Expr(FirExpr::RuntimeCall {
                name: "aster_list_set".to_string(),
                args: vec![
                    FirExpr::LocalVar(result_id, FirType::Ptr),
                    FirExpr::BinaryOp {
                        left: Box::new(FirExpr::LocalVar(j_id, FirType::I64)),
                        op: BinOp::Add,
                        right: Box::new(FirExpr::IntLit(1)),
                        result_ty: FirType::I64,
                    },
                    FirExpr::RuntimeCall {
                        name: "aster_list_get".to_string(),
                        args: vec![
                            FirExpr::LocalVar(result_id, FirType::Ptr),
                            FirExpr::LocalVar(j_id, FirType::I64),
                        ],
                        ret_ty: elem_ty.clone(),
                    },
                ],
                ret_ty: FirType::Void,
            }),
        ];
        let inner_increment = vec![
            // j = j - 1
            FirStmt::Assign {
                target: FirPlace::Local(j_id),
                value: FirExpr::BinaryOp {
                    left: Box::new(FirExpr::LocalVar(j_id, FirType::I64)),
                    op: BinOp::Sub,
                    right: Box::new(FirExpr::IntLit(1)),
                    result_ty: FirType::I64,
                },
            },
        ];

        let outer_body = vec![
            // key = result[i]
            FirStmt::Let {
                name: key_id,
                ty: elem_ty.clone(),
                value: FirExpr::RuntimeCall {
                    name: "aster_list_get".to_string(),
                    args: vec![
                        FirExpr::LocalVar(result_id, FirType::Ptr),
                        FirExpr::LocalVar(i_id, FirType::I64),
                    ],
                    ret_ty: elem_ty.clone(),
                },
            },
            // j = i - 1
            FirStmt::Let {
                name: j_id,
                ty: FirType::I64,
                value: FirExpr::BinaryOp {
                    left: Box::new(FirExpr::LocalVar(i_id, FirType::I64)),
                    op: BinOp::Sub,
                    right: Box::new(FirExpr::IntLit(1)),
                    result_ty: FirType::I64,
                },
            },
            // while j >= 0 && result[j] > key
            FirStmt::While {
                cond: FirExpr::BinaryOp {
                    left: Box::new(FirExpr::BinaryOp {
                        left: Box::new(FirExpr::LocalVar(j_id, FirType::I64)),
                        op: BinOp::Gte,
                        right: Box::new(FirExpr::IntLit(0)),
                        result_ty: FirType::Bool,
                    }),
                    op: BinOp::And,
                    right: Box::new(FirExpr::BinaryOp {
                        left: Box::new(FirExpr::RuntimeCall {
                            name: "aster_list_get".to_string(),
                            args: vec![
                                FirExpr::LocalVar(result_id, FirType::Ptr),
                                FirExpr::LocalVar(j_id, FirType::I64),
                            ],
                            ret_ty: elem_ty.clone(),
                        }),
                        op: BinOp::Gt,
                        right: Box::new(FirExpr::LocalVar(key_id, elem_ty.clone())),
                        result_ty: FirType::Bool,
                    }),
                    result_ty: FirType::Bool,
                },
                body: inner_body,
                increment: inner_increment,
            },
            // result[j+1] = key
            FirStmt::Expr(FirExpr::RuntimeCall {
                name: "aster_list_set".to_string(),
                args: vec![
                    FirExpr::LocalVar(result_id, FirType::Ptr),
                    FirExpr::BinaryOp {
                        left: Box::new(FirExpr::LocalVar(j_id, FirType::I64)),
                        op: BinOp::Add,
                        right: Box::new(FirExpr::IntLit(1)),
                        result_ty: FirType::I64,
                    },
                    FirExpr::LocalVar(key_id, elem_ty.clone()),
                ],
                ret_ty: FirType::Void,
            }),
        ];

        self.pending_stmts.push(FirStmt::While {
            cond: FirExpr::BinaryOp {
                left: Box::new(FirExpr::LocalVar(i_id, FirType::I64)),
                op: BinOp::Lt,
                right: Box::new(FirExpr::LocalVar(len_id, FirType::I64)),
                result_ty: FirType::Bool,
            },
            body: outer_body,
            increment: Self::iter_increment(i_id),
        });

        Ok(FirExpr::LocalVar(result_id, FirType::Ptr))
    }

    /// Check if the iterable expression refers to a class that includes Iterator.
    /// Returns the class name if so.
    /// Check if the iterable expression refers to a class that includes Iterator.
    /// Returns the class name if so.
    pub(crate) fn resolve_iter_type(&self, iter: &Expr) -> Option<Type> {
        if let Expr::Ident(name, _) = iter {
            self.local_ast_types.get(name.as_str()).cloned()
        } else {
            self.type_table.get(&iter.span()).cloned()
        }
    }
}
