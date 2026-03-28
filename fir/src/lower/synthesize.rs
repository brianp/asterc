use super::*;

impl Lowerer {
    /// Synthesize a `__top_main` entry function from accumulated top-level stmts.
    /// Injects the global prelude + top-level control flow in a proper function scope.
    pub(crate) fn synthesize_entry_function(&mut self) -> Result<(), LowerError> {
        let snapshot = self.save_scope();
        self.scope.current_return_type = Some(Type::Void);

        // Inject globals
        let mut body: Vec<FirStmt> = Vec::new();
        let top_level_snapshot: Vec<_> = self
            .tl
            .top_level_lets
            .iter()
            .map(|(n, t, v)| (n.clone(), t.clone(), v.clone()))
            .collect();
        for (tl_name, tl_ty, tl_value) in top_level_snapshot {
            let local_id = self.alloc_local();
            self.scope.locals.insert(tl_name, local_id);
            self.scope.local_types.insert(local_id, tl_ty.clone());
            body.push(FirStmt::Let {
                name: local_id,
                ty: tl_ty,
                value: tl_value,
            });
        }

        // Lower top-level control flow stmts (for, if, while, assignment)
        let tl_stmts: Vec<_> = self.tl.top_level_stmts.clone();
        for tl_stmt in &tl_stmts {
            let fir_stmt = self.lower_stmt_inner(tl_stmt)?;
            body.append(&mut self.pending_stmts);
            body.push(fir_stmt);
        }

        // Lower top-level bare expressions (say, etc.)
        let tl_exprs: Vec<_> = self.tl.top_level_exprs.clone();
        for tl_expr in &tl_exprs {
            let fir_stmt = self.lower_stmt_inner(tl_expr)?;
            body.append(&mut self.pending_stmts);
            body.push(fir_stmt);
        }

        let id = FunctionId(self.ms.next_function);
        self.ms.next_function += 1;
        let func = FirFunction {
            id,
            name: "__top_main".to_string(),
            params: vec![],
            ret_type: FirType::Void,
            body,
            is_entry: true,
            suspendable: false,
        };
        self.ms.module.add_function(func);
        self.ms.module.entry = Some(id);

        self.restore_scope(snapshot);
        Ok(())
    }

    /// Lower a single statement (REPL path). Appends to existing FirModule.
    /// Synthesize a FIR function body for auto-derived `to_string`.
    /// Produces: "ClassName(field1_str, field2_str, ...)"
    pub(crate) fn synthesize_to_string(
        &mut self,
        class_name: &str,
        class_id: ClassId,
    ) -> Result<(), LowerError> {
        let qualified = format!("{}.to_string", class_name);
        let func_id = FunctionId(self.ms.next_function);
        self.ms.next_function += 1;
        self.ms.functions.insert(qualified.clone(), func_id);

        let field_layout = self
            .ms
            .class_fields
            .get(&class_id)
            .cloned()
            .unwrap_or_default();

        // Build the body: concat "ClassName(" + field_to_strings + ")"
        // Start with "ClassName("
        let mut result_expr = FirExpr::StringLit(format!("{}(", class_name));

        let self_local = LocalId(0);
        for (i, (field_name, field_ty, offset)) in field_layout.iter().enumerate() {
            // Add separator ", " between fields
            if i > 0 {
                result_expr = FirExpr::RuntimeCall {
                    name: "aster_string_concat".to_string(),
                    args: vec![result_expr, FirExpr::StringLit(", ".to_string())],
                    ret_ty: FirType::Ptr,
                };
            }

            // Load field value from self
            let field_val = FirExpr::FieldGet {
                object: Box::new(FirExpr::LocalVar(self_local, FirType::Ptr)),
                offset: *offset,
                ty: field_ty.clone(),
            };

            // Convert field to string based on type
            let field_str = match field_ty {
                FirType::I64 => FirExpr::RuntimeCall {
                    name: "aster_int_to_string".to_string(),
                    args: vec![field_val],
                    ret_ty: FirType::Ptr,
                },
                FirType::F64 => FirExpr::RuntimeCall {
                    name: "aster_float_to_string".to_string(),
                    args: vec![field_val],
                    ret_ty: FirType::Ptr,
                },
                FirType::Bool => FirExpr::RuntimeCall {
                    name: "aster_bool_to_string".to_string(),
                    args: vec![field_val],
                    ret_ty: FirType::Ptr,
                },
                FirType::Ptr => {
                    // Could be a String or another class — check if field has its own to_string
                    let _ = field_name; // suppress unused warning
                    // For now, pass through as string (most common case)
                    field_val
                }
                _ => field_val,
            };

            // Concat field string to result
            result_expr = FirExpr::RuntimeCall {
                name: "aster_string_concat".to_string(),
                args: vec![result_expr, field_str],
                ret_ty: FirType::Ptr,
            };
        }

        // Append closing ")"
        result_expr = FirExpr::RuntimeCall {
            name: "aster_string_concat".to_string(),
            args: vec![result_expr, FirExpr::StringLit(")".to_string())],
            ret_ty: FirType::Ptr,
        };

        let func = FirFunction {
            id: func_id,
            name: qualified,
            params: vec![("self".to_string(), FirType::Ptr)],
            ret_type: FirType::Ptr,
            body: vec![FirStmt::Return(result_expr)],
            is_entry: false,
            suspendable: false,
        };
        self.ms.module.add_function(func);
        Ok(())
    }

    /// Synthesize an auto-derived `eq` method for a class.
    /// Compares all fields pairwise: self.f1 == other.f1 and self.f2 == other.f2 ...
    pub(crate) fn synthesize_eq(
        &mut self,
        class_name: &str,
        class_id: ClassId,
    ) -> Result<(), LowerError> {
        let qualified = format!("{}.eq", class_name);
        let func_id = FunctionId(self.ms.next_function);
        self.ms.next_function += 1;
        self.ms.functions.insert(qualified.clone(), func_id);

        let field_layout = self
            .ms
            .class_fields
            .get(&class_id)
            .cloned()
            .unwrap_or_default();

        let self_local = LocalId(0);
        let other_local = LocalId(1);

        // Build: self.f1 == other.f1 and self.f2 == other.f2 and ...
        let mut result_expr: Option<FirExpr> = None;
        for (_field_name, field_ty, offset) in &field_layout {
            let self_field = FirExpr::FieldGet {
                object: Box::new(FirExpr::LocalVar(self_local, FirType::Ptr)),
                offset: *offset,
                ty: field_ty.clone(),
            };
            let other_field = FirExpr::FieldGet {
                object: Box::new(FirExpr::LocalVar(other_local, FirType::Ptr)),
                offset: *offset,
                ty: field_ty.clone(),
            };
            let cmp = FirExpr::BinaryOp {
                left: Box::new(self_field),
                op: BinOp::Eq,
                right: Box::new(other_field),
                result_ty: FirType::Bool,
            };
            result_expr = Some(match result_expr {
                None => cmp,
                Some(prev) => FirExpr::BinaryOp {
                    left: Box::new(prev),
                    op: BinOp::And,
                    right: Box::new(cmp),
                    result_ty: FirType::Bool,
                },
            });
        }
        let body_expr = result_expr.unwrap_or(FirExpr::BoolLit(true));

        let func = FirFunction {
            id: func_id,
            name: qualified,
            params: vec![
                ("self".to_string(), FirType::Ptr),
                ("other".to_string(), FirType::Ptr),
            ],
            ret_type: FirType::Bool,
            body: vec![FirStmt::Return(body_expr)],
            is_entry: false,
            suspendable: false,
        };
        self.ms.module.add_function(func);
        Ok(())
    }

    /// Synthesize an auto-derived `cmp` method for a class.
    /// Compares fields in order, returning an Ordering enum variant.
    pub(crate) fn synthesize_cmp(
        &mut self,
        class_name: &str,
        class_id: ClassId,
    ) -> Result<(), LowerError> {
        let qualified = format!("{}.cmp", class_name);
        let func_id = FunctionId(self.ms.next_function);
        self.ms.next_function += 1;
        self.ms.functions.insert(qualified.clone(), func_id);

        let field_layout = self
            .ms
            .class_fields
            .get(&class_id)
            .cloned()
            .unwrap_or_default();

        let self_local = LocalId(0);
        let other_local = LocalId(1);

        // For each field, compare and return early if not equal.
        // Return Ordering.Equal at the end.
        // Ordering enum: tag 0 = Less, tag 1 = Equal, tag 2 = Greater
        let ordering_ctor = |tag: i64| -> FirExpr {
            // Ordering variant is a struct with a single tag field
            if let Some(&ctor_func_id) = self.ms.functions.get(match tag {
                0 => "Ordering.Less",
                1 => "Ordering.Equal",
                _ => "Ordering.Greater",
            }) {
                FirExpr::Call {
                    func: ctor_func_id,
                    args: vec![],
                    ret_ty: FirType::Ptr,
                }
            } else {
                // Fallback: construct inline with tag
                FirExpr::IntLit(tag)
            }
        };

        let mut body = Vec::new();
        for (_field_name, field_ty, offset) in &field_layout {
            let self_field = FirExpr::FieldGet {
                object: Box::new(FirExpr::LocalVar(self_local, FirType::Ptr)),
                offset: *offset,
                ty: field_ty.clone(),
            };
            let other_field = FirExpr::FieldGet {
                object: Box::new(FirExpr::LocalVar(other_local, FirType::Ptr)),
                offset: *offset,
                ty: field_ty.clone(),
            };
            // if self.f < other.f: return Less
            body.push(FirStmt::If {
                cond: FirExpr::BinaryOp {
                    left: Box::new(self_field.clone()),
                    op: BinOp::Lt,
                    right: Box::new(other_field.clone()),
                    result_ty: FirType::Bool,
                },
                then_body: vec![FirStmt::Return(ordering_ctor(0))],
                else_body: vec![],
            });
            // if self.f > other.f: return Greater
            body.push(FirStmt::If {
                cond: FirExpr::BinaryOp {
                    left: Box::new(self_field),
                    op: BinOp::Gt,
                    right: Box::new(other_field),
                    result_ty: FirType::Bool,
                },
                then_body: vec![FirStmt::Return(ordering_ctor(2))],
                else_body: vec![],
            });
        }
        body.push(FirStmt::Return(ordering_ctor(1)));

        let func = FirFunction {
            id: func_id,
            name: qualified,
            params: vec![
                ("self".to_string(), FirType::Ptr),
                ("other".to_string(), FirType::Ptr),
            ],
            ret_type: FirType::Ptr,
            body,
            is_entry: false,
            suspendable: false,
        };
        self.ms.module.add_function(func);
        Ok(())
    }

    /// Inject the built-in `Ordering` enum (Less/Equal/Greater) into the FIR.
    /// This must run before any user code is lowered so that synthesize_cmp can
    /// reference `Ordering.Less`, `Ordering.Equal`, and `Ordering.Greater`.
    /// Inject ProcessResult as a built-in class so field access works.
    /// Layout: [exit_code: i64, stdout: Ptr, stderr: Ptr]
    /// Fields are sorted pointer-first for GC: stdout(0), stderr(1), exit_code(2).
    /// But ProcessResult is returned from runtime as [exit_code(0), stdout(1), stderr(2)].
    /// We match the runtime layout: exit_code at offset 0, stdout at offset 1, stderr at offset 2.
    pub(crate) fn inject_process_result_builtin(&mut self) {
        let class_id = ClassId(self.ms.next_class);
        self.ms.next_class += 1;
        self.ms
            .classes
            .insert("ProcessResult".to_string(), class_id);
        // Field layout matches runtime: [exit_code: i64][stdout: Ptr][stderr: Ptr]
        // Offsets are in bytes (each field is 8 bytes)
        self.ms.class_fields.insert(
            class_id,
            vec![
                ("exit_code".to_string(), FirType::I64, 0),
                ("stdout".to_string(), FirType::Ptr, 8),
                ("stderr".to_string(), FirType::Ptr, 16),
            ],
        );
    }

    pub(crate) fn inject_ordering_builtin(&mut self) {
        // Ordering is a unit enum: each variant is a struct with a single tag field.
        // Layout: alloc 8 bytes, store tag at offset 0.
        let alloc_size = 8i64;
        let variants = [
            ("Ordering.Less", 0i64),
            ("Ordering.Equal", 1),
            ("Ordering.Greater", 2),
        ];

        for (ctor_name, tag) in variants {
            let func_id = FunctionId(self.ms.next_function);
            self.ms.next_function += 1;
            self.ms.functions.insert(ctor_name.to_string(), func_id);

            // Body: alloc, store tag, return ptr
            let ptr_id = LocalId(0);
            let body = vec![
                FirStmt::Let {
                    name: ptr_id,
                    ty: FirType::Ptr,
                    value: FirExpr::RuntimeCall {
                        name: "aster_class_alloc".to_string(),
                        args: vec![FirExpr::IntLit(alloc_size)],
                        ret_ty: FirType::Ptr,
                    },
                },
                FirStmt::Assign {
                    target: FirPlace::Field {
                        object: Box::new(FirExpr::LocalVar(ptr_id, FirType::Ptr)),
                        offset: 0,
                    },
                    value: FirExpr::IntLit(tag),
                },
                FirStmt::Return(FirExpr::LocalVar(ptr_id, FirType::Ptr)),
            ];

            self.ms.module.add_function(FirFunction {
                id: func_id,
                name: ctor_name.to_string(),
                params: vec![],
                ret_type: FirType::Ptr,
                body,
                is_entry: false,
                suspendable: false,
            });
        }

        // Register enum_variants for match lowering
        self.ms.enum_variants.insert(
            "Ordering".to_string(),
            vec![
                ("Less".to_string(), 0, vec![]),
                ("Equal".to_string(), 1, vec![]),
                ("Greater".to_string(), 2, vec![]),
            ],
        );
    }
}
