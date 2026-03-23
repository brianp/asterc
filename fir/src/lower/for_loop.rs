use super::*;

impl Lowerer {
    pub(crate) fn lower_for_loop(
        &mut self,
        var: &str,
        iter: &Expr,
        body: &[Stmt],
    ) -> Result<FirStmt, LowerError> {
        // Check if iterating over a Range expression
        if let Expr::Range {
            start,
            end,
            inclusive,
            ..
        } = iter
        {
            return self.lower_range_for_loop(var, start, end, *inclusive, body);
        }

        // Check if iterating over a builtin Range variable (not a user-defined Range class).
        // Builtin Range has no user-defined methods — check that it's not an Iterator.
        if let Some(Type::Custom(ref name, _)) = self.resolve_iter_type(iter)
            && name == "Range"
            && self
                .type_env
                .get_class(name)
                .is_some_and(|ci| !ci.includes.contains(&"Iterator".to_string()))
        {
            return self.lower_range_var_for_loop(var, iter, body);
        }

        // Check if iterating over an Iterator class
        if let Some(class_name) = self.resolve_iterator_class(iter) {
            return self.lower_iterator_for_loop(var, iter, body, &class_name);
        }

        // Default: list-based iteration
        // Lower the iterable expression
        let fir_iter = self.lower_expr(iter)?;

        // Use unique names to avoid collisions in nested for-loops
        let uid = self.next_local;

        // let __iter = <iterable>
        let iter_id = self.alloc_local();
        self.locals.insert(format!("__for_iter_{}", uid), iter_id);
        self.local_types.insert(iter_id, FirType::Ptr);

        // let __len = aster_list_len(__iter)
        let len_id = self.alloc_local();
        self.locals.insert(format!("__for_len_{}", uid), len_id);
        self.local_types.insert(len_id, FirType::I64);

        // let __idx = 0
        let idx_id = self.alloc_local();
        self.locals.insert(format!("__for_idx_{}", uid), idx_id);
        self.local_types.insert(idx_id, FirType::I64);

        // Resolve element type from AST type info when available
        let elem_ty = if let Expr::Ident(name, _) = iter {
            if let Some(Type::List(inner)) = self.local_ast_types.get(name.as_str()) {
                self.lower_type(inner)
            } else {
                FirType::I64
            }
        } else {
            FirType::I64
        };

        // let var = aster_list_get(__iter, __idx)
        let var_id = self.alloc_local();
        self.locals.insert(var.to_string(), var_id);
        self.local_types.insert(var_id, elem_ty.clone());

        // Build the while loop body
        let mut while_body = Vec::new();

        // let var = aster_list_get(__iter, __idx)
        while_body.push(FirStmt::Let {
            name: var_id,
            ty: elem_ty.clone(),
            value: FirExpr::RuntimeCall {
                name: "aster_list_get".to_string(),
                args: vec![
                    FirExpr::LocalVar(iter_id, FirType::Ptr),
                    FirExpr::LocalVar(idx_id, FirType::I64),
                ],
                ret_ty: elem_ty,
            },
        });

        // Push scope boundary for cleanup tracking in for-loop body
        let scope_start = self.cleanup_locals.len();
        self.cleanup_scope_stack.push(scope_start);

        // Lower the user's loop body
        for stmt in body {
            let fir_stmt = self.lower_stmt_inner(stmt)?;
            while_body.append(&mut self.pending_stmts);
            while_body.push(fir_stmt);
        }

        // Emit end-of-iteration cleanup for loop-body locals
        self.emit_cleanup_calls_since(scope_start);
        while_body.append(&mut self.pending_stmts);

        // Pop scope and remove loop-body locals from function-level cleanup
        self.cleanup_scope_stack.pop();
        self.cleanup_locals.truncate(scope_start);

        // Increment: __idx = __idx + 1 (runs after body and on continue)
        let increment = vec![
            FirStmt::Assign {
                target: FirPlace::Local(idx_id),
                value: FirExpr::BinaryOp {
                    left: Box::new(FirExpr::LocalVar(idx_id, FirType::I64)),
                    op: BinOp::Add,
                    right: Box::new(FirExpr::IntLit(1)),
                    result_ty: FirType::I64,
                },
            },
            FirStmt::Expr(FirExpr::Safepoint),
        ];

        let setup_and_loop = vec![
            FirStmt::Let {
                name: iter_id,
                ty: FirType::Ptr,
                value: fir_iter,
            },
            FirStmt::Let {
                name: len_id,
                ty: FirType::I64,
                value: FirExpr::RuntimeCall {
                    name: "aster_list_len".to_string(),
                    args: vec![FirExpr::LocalVar(iter_id, FirType::Ptr)],
                    ret_ty: FirType::I64,
                },
            },
            FirStmt::Let {
                name: idx_id,
                ty: FirType::I64,
                value: FirExpr::IntLit(0),
            },
            FirStmt::While {
                cond: FirExpr::BinaryOp {
                    left: Box::new(FirExpr::LocalVar(idx_id, FirType::I64)),
                    op: BinOp::Lt,
                    right: Box::new(FirExpr::LocalVar(len_id, FirType::I64)),
                    result_ty: FirType::Bool,
                },
                body: while_body,
                increment,
            },
        ];

        Ok(FirStmt::Block(setup_and_loop))
    }

    /// Check if lowering a value expression would produce pending stmts
    /// (e.g. iterable method calls, nullable ops, chained method calls).
    /// Lower `for var in start..end` or `for var in start..=end` to a counting loop.
    pub(crate) fn lower_range_for_loop(
        &mut self,
        var: &str,
        start: &Expr,
        end: &Expr,
        inclusive: bool,
        body: &[Stmt],
    ) -> Result<FirStmt, LowerError> {
        let fir_start = self.lower_expr(start)?;
        let fir_end = self.lower_expr(end)?;
        let uid = self.next_local;

        // let __end = <end>
        let end_id = self.alloc_local();
        self.locals.insert(format!("__range_end_{}", uid), end_id);
        self.local_types.insert(end_id, FirType::I64);

        // let var = <start>
        let var_id = self.alloc_local();
        self.locals.insert(var.to_string(), var_id);
        self.local_types.insert(var_id, FirType::I64);

        // Build the while loop body
        let mut while_body = Vec::new();

        let scope_start = self.cleanup_locals.len();
        self.cleanup_scope_stack.push(scope_start);

        for stmt in body {
            let fir_stmt = self.lower_stmt_inner(stmt)?;
            while_body.append(&mut self.pending_stmts);
            while_body.push(fir_stmt);
        }

        self.emit_cleanup_calls_since(scope_start);
        while_body.append(&mut self.pending_stmts);
        self.cleanup_scope_stack.pop();
        self.cleanup_locals.truncate(scope_start);

        // Increment: var = var + 1 (runs after body and on continue)
        let increment = vec![
            FirStmt::Assign {
                target: FirPlace::Local(var_id),
                value: FirExpr::BinaryOp {
                    left: Box::new(FirExpr::LocalVar(var_id, FirType::I64)),
                    op: BinOp::Add,
                    right: Box::new(FirExpr::IntLit(1)),
                    result_ty: FirType::I64,
                },
            },
            FirStmt::Expr(FirExpr::Safepoint),
        ];

        // Condition: var < end (exclusive) or var <= end (inclusive)
        let cmp_op = if inclusive { BinOp::Lte } else { BinOp::Lt };
        let cond = FirExpr::BinaryOp {
            left: Box::new(FirExpr::LocalVar(var_id, FirType::I64)),
            op: cmp_op,
            right: Box::new(FirExpr::LocalVar(end_id, FirType::I64)),
            result_ty: FirType::Bool,
        };

        let setup_and_loop = vec![
            FirStmt::Let {
                name: end_id,
                ty: FirType::I64,
                value: fir_end,
            },
            FirStmt::Let {
                name: var_id,
                ty: FirType::I64,
                value: fir_start,
            },
            FirStmt::While {
                cond,
                body: while_body,
                increment,
            },
        ];

        Ok(FirStmt::Block(setup_and_loop))
    }

    /// Lower `for var in range_var` where range_var is a Range variable.
    /// Extracts start/end/inclusive from the runtime range struct.
    pub(crate) fn lower_range_var_for_loop(
        &mut self,
        var: &str,
        iter: &Expr,
        body: &[Stmt],
    ) -> Result<FirStmt, LowerError> {
        let fir_range = self.lower_expr(iter)?;
        let uid = self.next_local;

        // let __range = <iter>
        let range_id = self.alloc_local();
        self.locals.insert(format!("__range_ptr_{}", uid), range_id);
        self.local_types.insert(range_id, FirType::Ptr);

        // Extract start, end, inclusive from range struct fields
        // Range layout: [start: i64 @ 0][end: i64 @ 8][inclusive: i64 @ 16]
        let start_expr = FirExpr::FieldGet {
            object: Box::new(FirExpr::LocalVar(range_id, FirType::Ptr)),
            offset: 0,
            ty: FirType::I64,
        };
        let end_expr = FirExpr::FieldGet {
            object: Box::new(FirExpr::LocalVar(range_id, FirType::Ptr)),
            offset: 8,
            ty: FirType::I64,
        };
        let inclusive_expr = FirExpr::FieldGet {
            object: Box::new(FirExpr::LocalVar(range_id, FirType::Ptr)),
            offset: 16,
            ty: FirType::Bool,
        };

        let start_id = self.alloc_local();
        self.locals
            .insert(format!("__range_start_{}", uid), start_id);
        self.local_types.insert(start_id, FirType::I64);

        let end_id = self.alloc_local();
        self.locals.insert(format!("__range_end_{}", uid), end_id);
        self.local_types.insert(end_id, FirType::I64);

        let incl_id = self.alloc_local();
        self.locals.insert(format!("__range_incl_{}", uid), incl_id);
        self.local_types.insert(incl_id, FirType::Bool);

        let var_id = self.alloc_local();
        self.locals.insert(var.to_string(), var_id);
        self.local_types.insert(var_id, FirType::I64);

        // Build while loop body
        let mut while_body = Vec::new();

        let scope_start = self.cleanup_locals.len();
        self.cleanup_scope_stack.push(scope_start);

        for stmt in body {
            let fir_stmt = self.lower_stmt_inner(stmt)?;
            while_body.append(&mut self.pending_stmts);
            while_body.push(fir_stmt);
        }

        self.emit_cleanup_calls_since(scope_start);
        while_body.append(&mut self.pending_stmts);
        self.cleanup_scope_stack.pop();
        self.cleanup_locals.truncate(scope_start);

        // Increment: var = var + 1 (runs after body and on continue)
        let increment = vec![
            FirStmt::Assign {
                target: FirPlace::Local(var_id),
                value: FirExpr::BinaryOp {
                    left: Box::new(FirExpr::LocalVar(var_id, FirType::I64)),
                    op: BinOp::Add,
                    right: Box::new(FirExpr::IntLit(1)),
                    result_ty: FirType::I64,
                },
            },
            FirStmt::Expr(FirExpr::Safepoint),
        ];

        // Condition: if inclusive -> var <= end, else var < end
        // Use: (inclusive AND var <= end) OR (NOT inclusive AND var < end)
        // Simpler: use a runtime call that checks both
        let cond = FirExpr::RuntimeCall {
            name: "aster_range_check".to_string(),
            args: vec![
                FirExpr::LocalVar(var_id, FirType::I64),
                FirExpr::LocalVar(end_id, FirType::I64),
                FirExpr::LocalVar(incl_id, FirType::Bool),
            ],
            ret_ty: FirType::Bool,
        };

        let setup_and_loop = vec![
            FirStmt::Let {
                name: range_id,
                ty: FirType::Ptr,
                value: fir_range,
            },
            FirStmt::Let {
                name: start_id,
                ty: FirType::I64,
                value: start_expr,
            },
            FirStmt::Let {
                name: end_id,
                ty: FirType::I64,
                value: end_expr,
            },
            FirStmt::Let {
                name: incl_id,
                ty: FirType::Bool,
                value: inclusive_expr,
            },
            FirStmt::Let {
                name: var_id,
                ty: FirType::I64,
                value: FirExpr::LocalVar(start_id, FirType::I64),
            },
            FirStmt::While {
                cond,
                body: while_body,
                increment,
            },
        ];

        Ok(FirStmt::Block(setup_and_loop))
    }

    pub(crate) fn resolve_iterator_class(&self, iter: &Expr) -> Option<String> {
        if let Expr::Ident(name, _) = iter
            && let Some(Type::Custom(class_name, _)) = self.local_ast_types.get(name.as_str())
            && let Some(class_info) = self.type_env.get_class(class_name)
            && class_info.includes.contains(&"Iterator".to_string())
        {
            return Some(class_name.clone());
        }
        None
    }

    /// Lower `for var in iter: body` for Iterator classes.
    /// Desugars to:
    ///   let __iter = iter
    ///   while true:
    ///     let __next = __iter.next()   // returns nullable (Ptr: 0=nil, non-zero=boxed value)
    ///     if __next == 0: break        // nil → done
    ///     let var = *__next            // unwrap boxed value
    ///     body...
    /// Lower `for var in iter: body` for Iterator classes.
    /// Desugars to:
    ///   let __iter = iter
    ///   while true:
    ///     let __next = __iter.next()   // returns nullable (Ptr: 0=nil, non-zero=boxed value)
    ///     if __next == 0: break        // nil → done
    ///     let var = *__next            // unwrap boxed value
    ///     body...
    pub(crate) fn lower_iterator_for_loop(
        &mut self,
        var: &str,
        iter: &Expr,
        body: &[Stmt],
        class_name: &str,
    ) -> Result<FirStmt, LowerError> {
        let fir_iter = self.lower_expr(iter)?;
        let uid = self.next_local;

        // let __iter = <iterable>
        let iter_id = self.alloc_local();
        self.locals.insert(format!("__iter_{}", uid), iter_id);
        self.local_types.insert(iter_id, FirType::Ptr);

        // Resolve the next() method
        let next_name = format!("{}.next", class_name);
        let next_func_id = self.functions.get(&next_name).copied().ok_or_else(|| {
            LowerError::UnsupportedFeature(
                UnsupportedFeatureKind::Other(format!(
                    "Iterator class '{}' has no next() method in FIR",
                    class_name
                )),
                iter.span(),
            )
        })?;

        // let __next (will be reassigned each iteration)
        let next_id = self.alloc_local();
        self.locals.insert(format!("__next_{}", uid), next_id);
        self.local_types.insert(next_id, FirType::Ptr); // nullable = Ptr (0=nil, non-zero=boxed)

        // let var (the loop variable, unwrapped value)
        let var_id = self.alloc_local();
        self.locals.insert(var.to_string(), var_id);
        self.local_types.insert(var_id, FirType::I64);

        // Build while(true) loop body:
        let mut while_body = Vec::new();

        // let __next = __iter.next()
        while_body.push(FirStmt::Let {
            name: next_id,
            ty: FirType::Ptr,
            value: FirExpr::Call {
                func: next_func_id,
                args: vec![FirExpr::LocalVar(iter_id, FirType::Ptr)], // self arg
                ret_ty: FirType::Ptr,
            },
        });

        // if __next == nil (0): break
        while_body.push(FirStmt::If {
            cond: FirExpr::TagCheck {
                value: Box::new(FirExpr::LocalVar(next_id, FirType::Ptr)),
                tag: 1, // check for nil
            },
            then_body: vec![FirStmt::Break],
            else_body: vec![],
        });

        // let var = unwrap(__next) — load boxed value
        while_body.push(FirStmt::Let {
            name: var_id,
            ty: FirType::I64,
            value: FirExpr::TagUnwrap {
                value: Box::new(FirExpr::LocalVar(next_id, FirType::Ptr)),
                expected_tag: 0,
                ty: FirType::I64,
            },
        });

        // Push scope boundary for cleanup tracking in iterator for-loop
        let scope_start = self.cleanup_locals.len();
        self.cleanup_scope_stack.push(scope_start);

        // Lower user's loop body
        for stmt in body {
            let fir_stmt = self.lower_stmt_inner(stmt)?;
            while_body.append(&mut self.pending_stmts);
            while_body.push(fir_stmt);
        }

        // Emit end-of-iteration cleanup for loop-body locals
        self.emit_cleanup_calls_since(scope_start);
        while_body.append(&mut self.pending_stmts);

        // Pop scope and remove loop-body locals from function-level cleanup
        self.cleanup_scope_stack.pop();
        self.cleanup_locals.truncate(scope_start);

        while_body.push(FirStmt::Expr(FirExpr::Safepoint));

        // Setup: let __iter = iterable, then while(true) { ... }
        let setup_and_loop = vec![
            FirStmt::Let {
                name: iter_id,
                ty: FirType::Ptr,
                value: fir_iter,
            },
            FirStmt::While {
                cond: FirExpr::BoolLit(true),
                body: while_body,
                increment: vec![],
            },
        ];

        Ok(FirStmt::Block(setup_and_loop))
    }
}
