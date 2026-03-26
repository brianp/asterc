use super::*;

impl Lowerer {
    pub(crate) fn lower_top_level_stmt(&mut self, stmt: &Stmt) -> Result<(), LowerError> {
        match stmt {
            Stmt::Let {
                name,
                value:
                    Expr::Lambda {
                        params,
                        ret_type,
                        body,
                        ..
                    },
                ..
            } => {
                self.lower_function(name, params, ret_type, body)?;
                Ok(())
            }
            Stmt::Let {
                name,
                type_ann,
                value,
                ..
            } => {
                // Top-level let binding outside a function.
                // If the value involves method calls that produce pending stmts
                // (iterable methods, nullable ops), defer to top_level_stmts so
                // pending_stmts are drained correctly in the function body.
                if self.value_has_pending_stmts(value) {
                    self.top_level_stmts.push(stmt.clone());
                    return Ok(());
                }

                // Simple values: collect and inject into every function's global prelude.
                let raw_value = self.lower_expr(value)?;
                let fir_value = self.wrap_nullable_binding(type_ann.as_ref(), value, raw_value);
                let fir_type = if let Some(ann) = type_ann {
                    self.lower_type(ann)
                } else {
                    self.infer_fir_type(&fir_value)
                };
                // Allocate a global local ID for this binding
                let local_id = self.alloc_local();
                self.locals.insert(name.clone(), local_id);
                self.local_types.insert(local_id, fir_type.clone());
                self.globals.insert(name.clone(), local_id);
                self.top_level_lets
                    .push((name.clone(), fir_type, fir_value));
                Ok(())
            }
            Stmt::Class {
                name,
                fields,
                methods,
                extends,
                ..
            } => self.lower_class(name, fields, methods, extends.as_deref()),
            Stmt::Enum {
                name,
                variants,
                methods,
                ..
            } => self.lower_enum(name, variants, methods),
            // Const is treated like Let at the FIR level
            Stmt::Const {
                name,
                type_ann,
                value,
                ..
            } => {
                let raw_value = self.lower_expr(value)?;
                let fir_value = self.wrap_nullable_binding(type_ann.as_ref(), value, raw_value);
                let fir_type = if let Some(ann) = type_ann {
                    self.lower_type(ann)
                } else {
                    self.infer_fir_type(&fir_value)
                };
                let local_id = self.alloc_local();
                self.locals.insert(name.clone(), local_id);
                self.local_types.insert(local_id, fir_type.clone());
                self.globals.insert(name.clone(), local_id);
                self.top_level_lets
                    .push((name.clone(), fir_type, fir_value));
                Ok(())
            }
            Stmt::Expr(_, _) => {
                self.top_level_exprs.push(stmt.clone());
                Ok(())
            }
            Stmt::For { .. } | Stmt::If { .. } | Stmt::While { .. } | Stmt::Assignment { .. } => {
                self.top_level_stmts.push(stmt.clone());
                Ok(())
            }
            _ => Err(unsupported_top_level_stmt(stmt)),
        }
    }

    pub(crate) fn lower_function(
        &mut self,
        name: &str,
        params: &[(String, Type)],
        ret_type: &Type,
        body: &[Stmt],
    ) -> Result<FunctionId, LowerError> {
        let snapshot = self.save_scope();
        self.current_return_type = Some(ret_type.clone());

        // Allocate parameters as locals FIRST (codegen expects params at LocalId(0..N))
        let mut fir_params = Vec::new();
        for (param_name, param_type) in params {
            let local_id = self.alloc_local();
            let fir_type = self.lower_type(param_type);
            self.locals.insert(param_name.clone(), local_id);
            self.local_types.insert(local_id, fir_type.clone());
            self.local_ast_types
                .insert(param_name.clone(), param_type.clone());
            fir_params.push((param_name.clone(), fir_type));
        }

        // Every function body is an implicit task scope — spawned tasks are
        // automatically cancelled when the function exits (return, throw, or
        // fall-through).  Allocated after params so codegen param layout is intact.
        let scope_id = self.alloc_local();
        self.local_types.insert(scope_id, FirType::Ptr);
        self.async_scope_stack.push(scope_id);
        self.function_scope_id = Some(scope_id);

        // Inject globals into the function scope: allocate fresh local IDs
        // and record the values so we can prepend Let stmts to the body.
        let mut global_prelude: Vec<FirStmt> = Vec::new();
        let top_level_snapshot: Vec<_> = self
            .top_level_lets
            .iter()
            .map(|(n, t, v)| (n.clone(), t.clone(), v.clone()))
            .collect();
        for (tl_name, tl_ty, tl_value) in top_level_snapshot {
            let local_id = self.alloc_local();
            self.locals.insert(tl_name, local_id);
            self.local_types.insert(local_id, tl_ty.clone());
            global_prelude.push(FirStmt::Let {
                name: local_id,
                ty: tl_ty,
                value: tl_value,
            });
        }

        // Lower top-level control flow stmts in this function's scope
        let tl_stmts: Vec<_> = self.top_level_stmts.clone();
        for tl_stmt in &tl_stmts {
            let fir_stmt = self.lower_stmt_inner(tl_stmt)?;
            global_prelude.append(&mut self.pending_stmts);
            global_prelude.push(fir_stmt);
        }

        // Lower body, converting last expression to implicit return
        let mut fir_body = self.lower_body(body)?;
        let mut emitted_cleanup = false;
        if let Some(last) = fir_body.last() {
            // If the last statement is an Expr (not Return), make it a Return
            if matches!(last, FirStmt::Expr(_))
                && *ret_type != Type::Void
                && *ret_type != Type::Inferred
                && let Some(FirStmt::Expr(expr)) = fir_body.pop()
            {
                // Emit cleanup calls before implicit return
                self.emit_cleanup_calls();
                self.emit_scope_exit(scope_id);
                fir_body.append(&mut self.pending_stmts);
                fir_body.push(FirStmt::Return(expr));
                emitted_cleanup = true;
            }
        }
        // For void functions (or functions whose last stmt isn't an expr),
        // emit cleanup + scope exit at the end of the body
        if !emitted_cleanup {
            self.emit_cleanup_calls();
            self.emit_scope_exit(scope_id);
            fir_body.append(&mut self.pending_stmts);
        }

        self.async_scope_stack.pop();

        // Prepend scope_enter + global value definitions
        let mut prologue = vec![FirStmt::Let {
            name: scope_id,
            ty: FirType::Ptr,
            value: FirExpr::RuntimeCall {
                name: "aster_async_scope_enter".to_string(),
                args: vec![],
                ret_ty: FirType::Ptr,
            },
        }];
        if !global_prelude.is_empty() {
            prologue.append(&mut global_prelude);
        }
        prologue.append(&mut fir_body);
        fir_body = prologue;

        // Get or create function ID
        let id = if let Some(&existing_id) = self.functions.get(name) {
            existing_id
        } else {
            let id = FunctionId(self.next_function);
            self.next_function += 1;
            self.functions.insert(name.to_string(), id);
            id
        };

        let func = FirFunction {
            id,
            name: name.to_string(),
            params: fir_params,
            ret_type: self.lower_type(ret_type),
            body: fir_body,
            is_entry: name == "main",
            suspendable: self.function_is_suspendable(name),
        };
        self.module.add_function(func);

        if name == "main" {
            self.module.entry = Some(id);
        }

        self.restore_scope(snapshot);

        Ok(id)
    }

    pub(crate) fn lower_class(
        &mut self,
        name: &str,
        fields: &[(String, Type, bool)],
        methods: &[Stmt],
        extends: Option<&str>,
    ) -> Result<(), LowerError> {
        // Get or create ClassId (should already be registered from first pass)
        let class_id = if let Some(&id) = self.classes.get(name) {
            id
        } else {
            let id = ClassId(self.next_class);
            self.next_class += 1;
            self.classes.insert(name.to_string(), id);
            id
        };

        // Build field layout including inherited fields from parent chain.
        // Within each class segment (ancestor or own fields), pointer-typed
        // fields are placed before value-typed fields so the GC can trace
        // precisely using the ptr_field_count stored in the object header.

        // Collect inherited fields from ancestor chain (outermost parent first)
        let mut ancestor_chain: Vec<String> = Vec::new();
        let mut current = extends.map(|s| s.to_string());
        while let Some(parent_name) = current {
            ancestor_chain.push(parent_name.clone());
            current = self
                .type_env
                .get_class(&parent_name)
                .and_then(|ci| ci.extends.clone());
        }
        // Reverse so outermost ancestor is processed first
        ancestor_chain.reverse();

        // Collect all fields (ancestor + own) without offsets first, then
        // assign offsets with pointer fields sorted to the front.
        let mut unordered: Vec<(String, FirType)> = Vec::new();
        for ancestor_name in &ancestor_chain {
            if let Some(ancestor_info) = self.type_env.get_class(ancestor_name) {
                for (field_name, field_type) in &ancestor_info.fields {
                    let fir_type = self.lower_type(field_type);
                    unordered.push((field_name.clone(), fir_type));
                }
            }
        }
        for (field_name, field_type, _) in fields {
            let fir_type = self.lower_type(field_type);
            unordered.push((field_name.clone(), fir_type));
        }

        // Stable partition: pointer fields first, then value fields.
        // stable_partition preserves relative order within each group.
        let (mut ptr_fields, val_fields): (Vec<_>, Vec<_>) = unordered
            .into_iter()
            .partition(|(_, ty)| ty.needs_gc_root());
        ptr_fields.extend(val_fields);

        // Now assign byte offsets in the new order.
        let mut fir_fields = Vec::with_capacity(ptr_fields.len());
        let mut offset = 0usize;
        for (field_name, fir_type) in ptr_fields {
            fir_fields.push((field_name, fir_type, offset));
            offset += 8;
        }
        let total_size = offset;

        // Store field layout for later use in FieldGet/Construct
        self.class_fields.insert(class_id, fir_fields.clone());

        // Create FirClass and add to module
        let fir_class = FirClass {
            id: class_id,
            name: name.to_string(),
            fields: fir_fields,
            methods: vec![],
            vtable: vec![],
            size: total_size,
            alignment: 8,
            parent: None,
        };
        self.module.add_class(fir_class);

        // Lower methods as regular functions with the class instance as first hidden parameter
        for method_stmt in methods {
            if let Stmt::Let {
                name: method_name,
                value:
                    Expr::Lambda {
                        params,
                        ret_type,
                        body,
                        defaults,
                        ..
                    },
                ..
            } = method_stmt
            {
                // Prepend `self: ClassName` as the first parameter
                let mut full_params =
                    vec![("self".to_string(), Type::Custom(name.to_string(), vec![]))];
                full_params.extend(params.iter().cloned());

                // Store defaults for method calls (qualified name)
                let param_defaults: Vec<(String, Option<Expr>)> = params
                    .iter()
                    .enumerate()
                    .map(|(i, (pname, _))| (pname.clone(), defaults.get(i).cloned().flatten()))
                    .collect();
                if param_defaults.iter().any(|(_, d)| d.is_some()) {
                    self.function_defaults
                        .insert(method_name.clone(), param_defaults);
                }

                // method_name is already qualified by the parser (e.g. "Point.to_string")
                self.lower_function(method_name, &full_params, ret_type, body)?;
            }
        }

        // Synthesize auto-derived to_string if class includes Printable but has no explicit impl
        let qualified_to_string = format!("{}.to_string", name);
        if !self.functions.contains_key(&qualified_to_string) {
            let has_printable = self
                .type_env
                .get_class(name)
                .map(|ci| ci.includes.contains(&"Printable".to_string()))
                .unwrap_or(false);
            if has_printable {
                self.synthesize_to_string(name, class_id)?;
            }
        }

        // Synthesize auto-derived eq if class includes Eq but has no explicit impl
        let qualified_eq = format!("{}.eq", name);
        if !self.functions.contains_key(&qualified_eq) {
            let has_eq = self
                .type_env
                .get_class(name)
                .map(|ci| ci.includes.contains(&"Eq".to_string()))
                .unwrap_or(false);
            if has_eq {
                self.synthesize_eq(name, class_id)?;
            }
        }

        // Synthesize auto-derived cmp if class includes Ord but has no explicit impl
        let qualified_cmp = format!("{}.cmp", name);
        if !self.functions.contains_key(&qualified_cmp) {
            let has_ord = self
                .type_env
                .get_class(name)
                .map(|ci| ci.includes.contains(&"Ord".to_string()))
                .unwrap_or(false);
            if has_ord {
                self.synthesize_cmp(name, class_id)?;
            }
        }

        Ok(())
    }

    /// Emit a scope_exit call for the given scope local. This cancels any
    /// unresolved tasks owned by the scope. Pushed to `self.pending_stmts`.
    pub(crate) fn emit_scope_exit(&mut self, scope_id: LocalId) {
        self.pending_stmts.push(FirStmt::Expr(FirExpr::RuntimeCall {
            name: "aster_async_scope_exit".to_string(),
            args: vec![FirExpr::LocalVar(scope_id, FirType::Ptr)],
            ret_ty: FirType::Void,
        }));
    }

    /// Emit cleanup calls for all locals that implement Close or Drop,
    /// in reverse declaration order. Close is called before Drop.
    /// Cleanup calls are pushed to `self.pending_stmts`.
    pub(crate) fn emit_cleanup_calls(&mut self) {
        self.emit_cleanup_calls_since(0);
    }

    /// Emit cleanup calls for locals declared since `scope_start` index
    /// in cleanup_locals. Emits in reverse declaration order.
    pub(crate) fn emit_cleanup_calls_since(&mut self, scope_start: usize) {
        if self.cleanup_locals.len() <= scope_start {
            return;
        }
        // Reverse declaration order: last declared = first cleaned
        for &(local_id, ref class_name, has_drop, has_close) in
            self.cleanup_locals[scope_start..].iter().rev()
        {
            // Close first (async cleanup), then Drop (sync cleanup)
            if has_close
                && let Some(&func_id) = self.functions.get(&format!("{}.close", class_name))
            {
                let fir_type = self
                    .local_types
                    .get(&local_id)
                    .cloned()
                    .unwrap_or(FirType::Ptr);
                self.pending_stmts.push(FirStmt::Expr(FirExpr::Call {
                    func: func_id,
                    args: vec![FirExpr::LocalVar(local_id, fir_type)],
                    ret_ty: FirType::Void,
                }));
            }
            if has_drop && let Some(&func_id) = self.functions.get(&format!("{}.drop", class_name))
            {
                let fir_type = self
                    .local_types
                    .get(&local_id)
                    .cloned()
                    .unwrap_or(FirType::Ptr);
                self.pending_stmts.push(FirStmt::Expr(FirExpr::Call {
                    func: func_id,
                    args: vec![FirExpr::LocalVar(local_id, fir_type)],
                    ret_ty: FirType::Void,
                }));
            }
        }
    }

    pub(crate) fn lower_body(&mut self, stmts: &[Stmt]) -> Result<Vec<FirStmt>, LowerError> {
        let mut result = Vec::new();
        for stmt in stmts {
            let fir_stmt = self.lower_stmt_inner(stmt)?;
            // Drain any pending statements emitted by expression lowering (e.g. match setup)
            result.append(&mut self.pending_stmts);
            result.push(fir_stmt);
        }
        Ok(result)
    }

    pub(crate) fn lower_stmt_inner(&mut self, stmt: &Stmt) -> Result<FirStmt, LowerError> {
        match stmt {
            Stmt::Let {
                name,
                type_ann,
                value,
                ..
            } => {
                // Check if the value is a lambda — if so, register closure info
                // BEFORE lowering, so we can find the lambda name
                let is_lambda = matches!(value, Expr::Lambda { .. });

                // Peek at captures for closure_info registration
                let lambda_captures = if let Expr::Lambda {
                    params: lp,
                    body: lb,
                    ..
                } = value
                {
                    let pnames: std::collections::HashSet<&str> =
                        lp.iter().map(|(n, _)| n.as_str()).collect();
                    let mut caps = Vec::new();
                    self.find_captures(lb, &pnames, &mut caps);
                    caps.sort();
                    caps.dedup();
                    caps
                } else {
                    vec![]
                };

                let expected_func_id = if is_lambda {
                    Some(FunctionId(self.next_function))
                } else {
                    None
                };

                let raw_value = self.lower_expr(value)?;
                let fir_type = if let Some(ann) = type_ann {
                    self.lower_type(ann)
                } else {
                    self.infer_fir_type(&raw_value)
                };
                let local_id = self.alloc_local();
                self.locals.insert(name.clone(), local_id);
                self.local_types.insert(local_id, fir_type.clone());

                // Register closure info for lambda bindings
                if let Some(func_id) = expected_func_id {
                    let env_local = if lambda_captures.is_empty() {
                        None
                    } else {
                        // The env local was created by lower_lambda in pending_stmts
                        // Extract it from the ClosureCreate's env field
                        if let FirExpr::ClosureCreate { env, .. } = &raw_value {
                            if let FirExpr::LocalVar(env_id, _) = env.as_ref() {
                                Some(*env_id)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    };
                    self.closure_info
                        .insert(name.clone(), (func_id, env_local, lambda_captures));
                }

                // Track AST type for class resolution in field access
                if let Some(ann) = type_ann {
                    self.local_ast_types.insert(name.clone(), ann.clone());
                } else if matches!(value, Expr::Str(..)) {
                    self.local_ast_types.insert(name.clone(), Type::String);
                } else if matches!(value, Expr::Range { .. }) {
                    // Range expressions always produce Type::Custom("Range", [])
                    self.local_ast_types.insert(
                        name.clone(),
                        Type::Custom(builtin_class::RANGE.into(), vec![]),
                    );
                } else if let Expr::AsyncCall { func, .. } = value {
                    if let Some(async_ty) = self.resolve_async_call_ast_type(func) {
                        self.local_ast_types.insert(name.clone(), async_ty);
                    }
                } else if let Some(inferred_ty) = self.type_table.get(&value.span()) {
                    self.local_ast_types
                        .insert(name.clone(), inferred_ty.clone());
                } else if let Expr::Call { func, .. } = value {
                    // Infer class type from constructor call: ClassName(...)
                    if let Expr::Ident(class_name, _) = func.as_ref()
                        && (self.classes.contains_key(class_name.as_str())
                            || class_name == builtin_class::MUTEX
                            || class_name == builtin_class::CHANNEL
                            || class_name == builtin_class::MULTI_SEND
                            || class_name == builtin_class::MULTI_RECEIVE)
                    {
                        self.local_ast_types
                            .insert(name.clone(), Type::Custom(class_name.clone(), vec![]));
                    // Infer class type from static method call: ClassName.method(...)
                    } else if let Expr::Member {
                        object: method_obj, ..
                    } = func.as_ref()
                        && let Expr::Ident(class_name, _) = method_obj.as_ref()
                        && self.classes.contains_key(class_name.as_str())
                    {
                        self.local_ast_types
                            .insert(name.clone(), Type::Custom(class_name.clone(), vec![]));
                    // Infer class type from function call that returns a class instance
                    } else if let Expr::Ident(func_name, _) = func.as_ref()
                        && let Some(Type::Function { ret, .. }) = self.type_env.get_var(func_name)
                        && let Type::Custom(class_name, type_args) = ret.as_ref()
                        && self.classes.contains_key(class_name.as_str())
                    {
                        self.local_ast_types.insert(
                            name.clone(),
                            Type::Custom(class_name.clone(), type_args.clone()),
                        );
                    }
                }
                let fir_value = self.wrap_nullable_binding(type_ann.as_ref(), value, raw_value);

                // Track locals that implement Drop or Close for cleanup
                if let Some(class_name) = self.local_ast_types.get(name).and_then(|t| match t {
                    Type::Custom(n, _) => Some(n.clone()),
                    _ => None,
                }) && let Some(ci) = self.type_env.get_class(&class_name)
                {
                    let has_drop = ci.includes.contains(&"Drop".to_string());
                    let has_close = ci.includes.contains(&"Close".to_string());
                    if has_drop || has_close {
                        self.cleanup_locals
                            .push((local_id, class_name, has_drop, has_close));
                    }
                }

                Ok(FirStmt::Let {
                    name: local_id,
                    ty: fir_type,
                    value: fir_value,
                })
            }
            Stmt::Return(expr, _) => {
                let fir_expr = self.lower_expr(expr)?;
                // Wrap return value in TagWrap for nullable return types
                let wrapped = self.maybe_wrap_nullable_return(fir_expr, expr);
                // Emit cleanup + scope exit before return
                self.emit_cleanup_calls();
                if let Some(scope_id) = self.function_scope_id {
                    self.emit_scope_exit(scope_id);
                }
                Ok(FirStmt::Return(wrapped))
            }
            Stmt::If {
                cond,
                then_body,
                elif_branches,
                else_body,
                ..
            } => self.lower_if(cond, then_body, elif_branches, else_body),
            Stmt::While { cond, body, .. } => {
                let fir_cond = self.lower_expr(cond)?;
                // Push scope boundary for cleanup tracking
                let scope_start = self.cleanup_locals.len();
                self.cleanup_scope_stack.push(scope_start);
                let mut fir_body = self.lower_body(body)?;
                // Emit end-of-iteration cleanup for loop-body locals
                self.emit_cleanup_calls_since(scope_start);
                fir_body.append(&mut self.pending_stmts);
                fir_body.push(FirStmt::Expr(FirExpr::Safepoint));
                // Pop scope and remove loop-body locals from function-level cleanup
                self.cleanup_scope_stack.pop();
                self.cleanup_locals.truncate(scope_start);
                Ok(FirStmt::While {
                    cond: fir_cond,
                    body: fir_body,
                    increment: vec![],
                })
            }
            Stmt::Assignment { target, value, .. } => {
                let fir_value = self.lower_expr(value)?;
                let fir_place = self.lower_place(target)?;
                Ok(FirStmt::Assign {
                    target: fir_place,
                    value: fir_value,
                })
            }
            Stmt::Break(_) => {
                // Emit cleanup for locals declared inside the loop body
                if let Some(&scope_start) = self.cleanup_scope_stack.last() {
                    self.emit_cleanup_calls_since(scope_start);
                }
                Ok(FirStmt::Break)
            }
            Stmt::Continue(_) => {
                // Emit cleanup for locals declared inside the loop body
                if let Some(&scope_start) = self.cleanup_scope_stack.last() {
                    self.emit_cleanup_calls_since(scope_start);
                }
                Ok(FirStmt::Continue)
            }
            Stmt::Expr(expr, _) => {
                let fir_expr = self.lower_expr(expr)?;
                Ok(FirStmt::Expr(fir_expr))
            }
            Stmt::For {
                var, iter, body, ..
            } => self.lower_for_loop(var, iter, body),
            Stmt::Const {
                name,
                type_ann,
                value,
                ..
            } => {
                let raw_value = self.lower_expr(value)?;
                let fir_value = self.wrap_nullable_binding(type_ann.as_ref(), value, raw_value);
                let fir_type = if let Some(ann) = type_ann {
                    self.lower_type(ann)
                } else {
                    self.infer_fir_type(&fir_value)
                };
                let local_id = self.alloc_local();
                self.locals.insert(name.clone(), local_id);
                self.local_types.insert(local_id, fir_type.clone());
                Ok(FirStmt::Let {
                    name: local_id,
                    ty: fir_type,
                    value: fir_value,
                })
            }
            Stmt::Class {
                name,
                fields,
                methods,
                extends,
                ..
            } => {
                // Register class ID (mirrors the first-pass registration in lower_module)
                if !self.classes.contains_key(name.as_str()) {
                    let id = ClassId(self.next_class);
                    self.next_class += 1;
                    self.classes.insert(name.clone(), id);
                }
                self.lower_class(name, fields, methods, extends.as_deref())?;
                Ok(FirStmt::NoOp)
            }
            Stmt::Enum {
                name,
                variants,
                methods,
                ..
            } => {
                // Register enum variant metadata (mirrors the first-pass registration in lower_module)
                if !self.enum_variants.contains_key(name.as_str()) {
                    let mut variant_info = Vec::new();
                    for (tag, v) in variants.iter().enumerate() {
                        let fields: Vec<(String, FirType)> = v
                            .fields
                            .iter()
                            .map(|(fname, fty)| (fname.clone(), self.lower_type(fty)))
                            .collect();
                        variant_info.push((v.name.clone(), tag as i64, fields));
                    }
                    self.enum_variants.insert(name.clone(), variant_info);
                    // Register variant constructors as functions
                    for v in variants {
                        let id = FunctionId(self.next_function);
                        self.next_function += 1;
                        let ctor_name = format!("{}.{}", name, v.name);
                        self.functions.insert(ctor_name, id);
                    }
                }
                self.lower_enum(name, variants, methods)?;
                Ok(FirStmt::NoOp)
            }
            _ => Err(unsupported_stmt(stmt)),
        }
    }

    pub(crate) fn lower_if(
        &mut self,
        cond: &Expr,
        then_body: &[Stmt],
        elif_branches: &[(Expr, Vec<Stmt>)],
        else_body: &[Stmt],
    ) -> Result<FirStmt, LowerError> {
        let fir_cond = self.lower_expr(cond)?;
        // Save condition's pending_stmts so they don't leak into the body
        let cond_pending = std::mem::take(&mut self.pending_stmts);
        let fir_then = self.lower_body(then_body)?;

        // Flatten elif chains into nested if/else
        let result = if !elif_branches.is_empty() {
            let (elif_cond, elif_body) = &elif_branches[0];
            let nested_else =
                self.lower_if(elif_cond, elif_body, &elif_branches[1..], else_body)?;
            FirStmt::If {
                cond: fir_cond,
                then_body: fir_then,
                else_body: vec![nested_else],
            }
        } else {
            let fir_else = self.lower_body(else_body)?;
            FirStmt::If {
                cond: fir_cond,
                then_body: fir_then,
                else_body: fir_else,
            }
        };
        // Restore condition's pending_stmts so the caller drains them before the If
        let mut restored = cond_pending;
        restored.append(&mut self.pending_stmts);
        self.pending_stmts = restored;
        Ok(result)
    }
}

pub(crate) fn unsupported_top_level_stmt(stmt: &Stmt) -> LowerError {
    let name = match stmt {
        Stmt::Trait { .. } => "trait",
        Stmt::Return(..) => "return",
        Stmt::Break(..) => "break",
        Stmt::Continue(..) => "continue",
        Stmt::Use { .. } => "use",
        _ => "statement",
    };
    LowerError::UnsupportedFeature(UnsupportedFeatureKind::TopLevelStatement(name), stmt.span())
}

pub(crate) fn unsupported_stmt(stmt: &Stmt) -> LowerError {
    let name = match stmt {
        Stmt::Class { .. } => "class",
        Stmt::Trait { .. } => "trait",
        Stmt::Use { .. } => "use",
        Stmt::Enum { .. } => "enum",
        Stmt::Const { .. } => "const",
        _ => "statement",
    };
    LowerError::UnsupportedFeature(UnsupportedFeatureKind::Statement(name), stmt.span())
}
