use super::*;

impl Lowerer {
    /// Lower a lambda/closure expression.
    /// All lambdas are lifted to top-level functions with `__env: Ptr` as first param.
    /// Captures are stored in a heap-allocated env struct.
    /// Returns a dummy value; the important side effect is registering closure_info
    /// so that call sites can resolve the closure statically.
    pub(crate) fn lower_lambda(
        &mut self,
        params: &[(String, Type)],
        ret_type: &Type,
        body: &[Stmt],
    ) -> Result<FirExpr, LowerError> {
        // Capture analysis: find references to outer locals
        let param_names: std::collections::HashSet<&str> =
            params.iter().map(|(n, _)| n.as_str()).collect();
        let mut captures = Vec::new();
        self.find_captures(body, &param_names, &mut captures);
        captures.sort();
        captures.dedup();

        let lambda_name = format!("__lambda_{}", self.ms.next_function);

        // Build the lifted function's params: __env: Ptr, then original params
        let mut lifted_params =
            vec![("__env".to_string(), Type::Custom("Ptr".to_string(), vec![]))];
        lifted_params.extend(params.iter().cloned());

        // Before lowering the lambda body, set up the capture mapping.
        // Save outer scope, then set up inner scope with env loads.
        let snapshot = self.save_scope();
        self.scope.next_local = 0;
        self.scope.current_return_type = Some(ret_type.clone());

        // Allocate __env as local 0
        let env_local = self.alloc_local(); // LocalId(0)
        self.scope.locals.insert("__env".to_string(), env_local);
        self.scope.local_types.insert(env_local, FirType::Ptr);

        // Allocate params as locals 1..N
        let mut fir_params = vec![("__env".to_string(), FirType::Ptr)];
        for (pname, pty) in params {
            let local_id = self.alloc_local();
            let fir_type = self.lower_type(pty);
            self.scope.locals.insert(pname.clone(), local_id);
            self.scope.local_types.insert(local_id, fir_type.clone());
            self.scope
                .local_ast_types
                .insert(pname.clone(), pty.clone());
            fir_params.push((pname.clone(), fir_type));
        }

        // Implicit task scope for lambda (same as functions)
        let scope_id = self.alloc_local();
        self.scope.local_types.insert(scope_id, FirType::Ptr);
        self.scope.async_scope_stack.push(scope_id);
        self.scope.function_scope_id = Some(scope_id);

        // Map captured variables to env loads
        for cap_name in &captures {
            let local_id = self.alloc_local();
            let cap_ty = snapshot
                .local_types
                .get(snapshot.locals.get(cap_name).unwrap_or(&LocalId(0)))
                .cloned()
                .unwrap_or(FirType::I64);
            self.scope.locals.insert(cap_name.clone(), local_id);
            self.scope.local_types.insert(local_id, cap_ty.clone());
        }

        // Lower the body
        let mut fir_body = Vec::new();

        // Emit env loads for captures at the start of the body
        for (i, cap_name) in captures.iter().enumerate() {
            let local_id = match self.scope.locals.get(cap_name) {
                Some(&id) => id,
                None => {
                    return Err(LowerError::UnboundVariable(
                        format!("closure capture '{}'", cap_name),
                        Span::dummy(),
                    ));
                }
            };
            let cap_ty = self
                .scope
                .local_types
                .get(&local_id)
                .cloned()
                .unwrap_or(FirType::I64);
            fir_body.push(FirStmt::Let {
                name: local_id,
                ty: cap_ty.clone(),
                value: FirExpr::EnvLoad {
                    env: Box::new(FirExpr::LocalVar(env_local, FirType::Ptr)),
                    offset: i * 8,
                    ty: cap_ty,
                },
            });
        }

        // Lower the user's body statements
        let user_body = self.lower_body(body)?;
        fir_body.extend(user_body);

        self.finalize_function_body(&mut fir_body, ret_type, scope_id, false, false);

        let func_id = self.register_function(&lambda_name, fir_params, ret_type, fir_body);

        self.restore_scope(snapshot);

        // Re-register the function name
        self.ms.functions.insert(lambda_name, func_id);

        if captures.is_empty() {
            // No captures: env is null. Return a ClosureCreate so the value
            // can be passed as a first-class closure.
            Ok(FirExpr::ClosureCreate {
                func: func_id,
                env: Box::new(FirExpr::NilLit),
                ret_ty: self.lower_type(ret_type),
            })
        } else {
            // Allocate env struct and store captures
            let env_size = captures.len() * 8;
            let env_id = self.alloc_local();
            let env_name = format!("__env_{}", func_id.0);
            self.scope.locals.insert(env_name.clone(), env_id);
            self.scope.local_types.insert(env_id, FirType::Ptr);

            self.pending_stmts.push(FirStmt::Let {
                name: env_id,
                ty: FirType::Ptr,
                value: FirExpr::RuntimeCall {
                    name: "aster_class_alloc".to_string(),
                    args: vec![FirExpr::IntLit(env_size as i64)],
                    ret_ty: FirType::Ptr,
                },
            });

            // Store capture values into env
            for (i, cap_name) in captures.iter().enumerate() {
                if let Some(&local_id) = self.scope.locals.get(cap_name.as_str()) {
                    let ty = self
                        .scope
                        .local_types
                        .get(&local_id)
                        .cloned()
                        .unwrap_or(FirType::I64);
                    self.pending_stmts.push(FirStmt::Assign {
                        target: FirPlace::Field {
                            object: Box::new(FirExpr::LocalVar(env_id, FirType::Ptr)),
                            offset: i * 8,
                        },
                        value: FirExpr::LocalVar(local_id, ty),
                    });
                }
            }

            Ok(FirExpr::ClosureCreate {
                func: func_id,
                env: Box::new(FirExpr::LocalVar(env_id, FirType::Ptr)),
                ret_ty: self.lower_type(ret_type),
            })
        }
    }

    /// Lower a string interpolation to a chain of to_string + concat calls.
    pub(crate) fn lower_string_interpolation(
        &mut self,
        parts: &[ast::StringPart],
    ) -> Result<FirExpr, LowerError> {
        let mut string_exprs = Vec::new();

        for part in parts {
            match part {
                ast::StringPart::Literal(s) => {
                    string_exprs.push(FirExpr::StringLit(s.clone()));
                }
                ast::StringPart::Expr(expr) => {
                    let fir_expr = self.lower_expr(expr)?;
                    string_exprs.push(self.to_string_expr(expr, fir_expr));
                }
            }
        }

        if string_exprs.is_empty() {
            return Ok(FirExpr::StringLit(String::new()));
        }

        if string_exprs.len() == 1 {
            return Ok(string_exprs.into_iter().next().unwrap());
        }

        let mut result = string_exprs.remove(0);
        for part in string_exprs {
            result = FirExpr::RuntimeCall {
                name: "aster_string_concat".to_string(),
                args: vec![result, part],
                ret_ty: FirType::Ptr,
            };
        }

        Ok(result)
    }

    /// Find variables referenced in a body that are not in the given param set
    /// but exist in the current local scope.
    pub(crate) fn find_captures(
        &self,
        stmts: &[Stmt],
        param_names: &std::collections::HashSet<&str>,
        captures: &mut Vec<String>,
    ) {
        for stmt in stmts {
            match stmt {
                Stmt::Expr(expr, _) | Stmt::Return(expr, _) => {
                    self.find_captures_expr(expr, param_names, captures);
                }
                Stmt::Let { value, .. } | Stmt::Assignment { value, .. } => {
                    self.find_captures_expr(value, param_names, captures);
                }
                Stmt::If {
                    cond,
                    then_body,
                    elif_branches,
                    else_body,
                    ..
                } => {
                    self.find_captures_expr(cond, param_names, captures);
                    self.find_captures(then_body, param_names, captures);
                    for (c, b) in elif_branches {
                        self.find_captures_expr(c, param_names, captures);
                        self.find_captures(b, param_names, captures);
                    }
                    self.find_captures(else_body, param_names, captures);
                }
                Stmt::While { cond, body, .. } => {
                    self.find_captures_expr(cond, param_names, captures);
                    self.find_captures(body, param_names, captures);
                }
                Stmt::For { iter, body, .. } => {
                    self.find_captures_expr(iter, param_names, captures);
                    self.find_captures(body, param_names, captures);
                }
                _ => {}
            }
        }
    }

    pub(crate) fn find_captures_expr(
        &self,
        expr: &Expr,
        param_names: &std::collections::HashSet<&str>,
        captures: &mut Vec<String>,
    ) {
        match expr {
            Expr::Ident(name, _) => {
                if !param_names.contains(name.as_str())
                    && self.scope.locals.contains_key(name.as_str())
                {
                    captures.push(name.clone());
                }
            }
            Expr::BinaryOp { left, right, .. } => {
                self.find_captures_expr(left, param_names, captures);
                self.find_captures_expr(right, param_names, captures);
            }
            Expr::UnaryOp { operand, .. } => {
                self.find_captures_expr(operand, param_names, captures);
            }
            Expr::Call { func, args, .. } => {
                self.find_captures_expr(func, param_names, captures);
                for (_, _, arg) in args {
                    self.find_captures_expr(arg, param_names, captures);
                }
            }
            Expr::Member { object, .. } => {
                self.find_captures_expr(object, param_names, captures);
            }
            Expr::Index { object, index, .. } => {
                self.find_captures_expr(object, param_names, captures);
                self.find_captures_expr(index, param_names, captures);
            }
            Expr::ListLiteral(elems, _) => {
                for e in elems {
                    self.find_captures_expr(e, param_names, captures);
                }
            }
            Expr::Match {
                scrutinee, arms, ..
            } => {
                self.find_captures_expr(scrutinee, param_names, captures);
                for (pattern, body) in arms {
                    if let ast::MatchPattern::Literal(lit, _) = pattern {
                        self.find_captures_expr(lit, param_names, captures);
                    }
                    self.find_captures_expr(body, param_names, captures);
                }
            }
            Expr::StringInterpolation { parts, .. } => {
                for part in parts {
                    if let ast::StringPart::Expr(e) = part {
                        self.find_captures_expr(e, param_names, captures);
                    }
                }
            }
            Expr::Range { start, end, .. } => {
                self.find_captures_expr(start, param_names, captures);
                self.find_captures_expr(end, param_names, captures);
            }
            Expr::Map { entries, .. } => {
                for (k, v) in entries {
                    self.find_captures_expr(k, param_names, captures);
                    self.find_captures_expr(v, param_names, captures);
                }
            }
            Expr::Lambda { body, params, .. } => {
                // Walk the lambda body but exclude the lambda's own params from captures
                let mut inner_params = param_names.clone();
                for (name, _) in params {
                    inner_params.insert(name.as_str());
                }
                self.find_captures(body, &inner_params, captures);
            }
            Expr::AsyncCall { func, args, .. }
            | Expr::BlockingCall { func, args, .. }
            | Expr::DetachedCall { func, args, .. } => {
                self.find_captures_expr(func, param_names, captures);
                for (_, _, arg) in args {
                    self.find_captures_expr(arg, param_names, captures);
                }
            }
            Expr::Resolve { expr, .. } | Expr::Propagate(expr, _) | Expr::Throw(expr, _) => {
                self.find_captures_expr(expr, param_names, captures);
            }
            Expr::ErrorOr { expr, default, .. }
            | Expr::ErrorOrElse {
                expr,
                handler: default,
                ..
            } => {
                self.find_captures_expr(expr, param_names, captures);
                self.find_captures_expr(default, param_names, captures);
            }
            Expr::ErrorCatch { expr, arms, .. } => {
                self.find_captures_expr(expr, param_names, captures);
                for (_, body) in arms {
                    self.find_captures_expr(body, param_names, captures);
                }
            }
            // Terminal expressions with no sub-expressions
            Expr::Int(..) | Expr::Float(..) | Expr::Bool(..) | Expr::Nil(..) | Expr::Str(..) => {}
        }
    }
}
