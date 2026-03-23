use super::*;

impl Lowerer {
    /// Lower a match expression to nested if/else chains.
    /// Returns a FirExpr::LocalVar referencing the result.
    pub(crate) fn lower_match(
        &mut self,
        scrutinee: &Expr,
        arms: &[(MatchPattern, Expr)],
    ) -> Result<FirExpr, LowerError> {
        // Lower scrutinee and store in temp local
        let fir_scrutinee = self.lower_expr(scrutinee)?;
        let scrutinee_ty = self.infer_fir_type(&fir_scrutinee);
        // Use unique names to avoid collisions in nested match expressions
        let uid = self.next_local;
        let scrutinee_id = self.alloc_local();
        self.locals
            .insert(format!("__match_scrut_{}", uid), scrutinee_id);
        self.local_types.insert(scrutinee_id, scrutinee_ty.clone());

        // Allocate result temp local — infer type from the scrutinee's AST type
        // or default to I64 (the first arm body will be lowered inside build_match_chain)
        let result_id = self.alloc_local();
        let result_ty = scrutinee_ty.clone(); // Will be overridden by actual arm values
        // Try to infer result type from the first non-binding arm
        let inferred_ty = self.infer_match_result_type(arms);
        let result_ty = inferred_ty.unwrap_or(result_ty);
        self.locals
            .insert(format!("__match_result_{}", uid), result_id);
        self.local_types.insert(result_id, result_ty.clone());

        // Emit: let __match_scrut = <scrutinee>
        self.pending_stmts.push(FirStmt::Let {
            name: scrutinee_id,
            ty: scrutinee_ty.clone(),
            value: fir_scrutinee,
        });

        // Emit: let __match_result = 0 (placeholder)
        let default_val = match &result_ty {
            FirType::F64 => FirExpr::FloatLit(0.0),
            FirType::Bool => FirExpr::BoolLit(false),
            FirType::Ptr => FirExpr::NilLit,
            _ => FirExpr::IntLit(0),
        };
        self.pending_stmts.push(FirStmt::Let {
            name: result_id,
            ty: result_ty.clone(),
            value: default_val,
        });

        // Build nested if/else chain from arms
        let if_chain = self.build_match_chain(scrutinee_id, &scrutinee_ty, arms, 0, result_id)?;

        self.pending_stmts.push(if_chain);

        Ok(FirExpr::LocalVar(result_id, result_ty))
    }

    /// Try to infer the result type of a match from its arm bodies.
    /// Look at literal arms first (they don't need variable bindings).
    /// Try to infer the result type of a match from its arm bodies.
    /// Look at literal arms first (they don't need variable bindings).
    pub(crate) fn infer_match_result_type(&self, arms: &[(MatchPattern, Expr)]) -> Option<FirType> {
        for (_, body) in arms {
            match body {
                Expr::Int(_, _) => return Some(FirType::I64),
                Expr::Float(_, _) => return Some(FirType::F64),
                Expr::Bool(_, _) => return Some(FirType::Bool),
                Expr::Str(_, _) => return Some(FirType::Ptr),
                Expr::Nil(_) => return Some(FirType::Ptr),
                _ => continue,
            }
        }
        None
    }

    pub(crate) fn build_match_chain(
        &mut self,
        scrutinee_id: LocalId,
        scrutinee_ty: &FirType,
        arms: &[(MatchPattern, Expr)],
        index: usize,
        result_id: LocalId,
    ) -> Result<FirStmt, LowerError> {
        if index >= arms.len() {
            // No more arms — this shouldn't happen if patterns are exhaustive
            return Ok(FirStmt::Expr(FirExpr::IntLit(0)));
        }

        let (pattern, _body_expr) = &arms[index];

        match pattern {
            MatchPattern::Wildcard(_) | MatchPattern::Ident(_, _) => {
                // Wildcard/ident always matches — bind if ident, then lower body
                let mut then_body = Vec::new();
                if let MatchPattern::Ident(name, _) = pattern {
                    // Bind scrutinee to the name
                    let bind_id = self.alloc_local();
                    self.locals.insert(name.clone(), bind_id);
                    self.local_types.insert(bind_id, scrutinee_ty.clone());
                    then_body.push(FirStmt::Let {
                        name: bind_id,
                        ty: scrutinee_ty.clone(),
                        value: FirExpr::LocalVar(scrutinee_id, scrutinee_ty.clone()),
                    });
                }
                // Now lower the body (after binding)
                let fir_body = self.lower_expr(&arms[index].1)?;
                then_body.push(FirStmt::Assign {
                    target: FirPlace::Local(result_id),
                    value: fir_body,
                });
                // Wrap in if(true) to keep the single-stmt return contract
                Ok(FirStmt::If {
                    cond: FirExpr::BoolLit(true),
                    then_body,
                    else_body: vec![],
                })
            }
            MatchPattern::Literal(lit_expr, _) => {
                let fir_lit = self.lower_expr(lit_expr)?;
                let fir_body = self.lower_expr(&arms[index].1)?;
                let body_assign = FirStmt::Assign {
                    target: FirPlace::Local(result_id),
                    value: fir_body,
                };
                let cond = FirExpr::BinaryOp {
                    left: Box::new(FirExpr::LocalVar(scrutinee_id, scrutinee_ty.clone())),
                    op: BinOp::Eq,
                    right: Box::new(fir_lit),
                    result_ty: FirType::Bool,
                };
                let else_body = if index + 1 < arms.len() {
                    vec![self.build_match_chain(
                        scrutinee_id,
                        scrutinee_ty,
                        arms,
                        index + 1,
                        result_id,
                    )?]
                } else {
                    vec![]
                };
                Ok(FirStmt::If {
                    cond,
                    then_body: vec![body_assign],
                    else_body,
                })
            }
            MatchPattern::EnumVariant {
                enum_name, variant, ..
            } => {
                // Compare tag of scrutinee to variant tag
                let tag = self
                    .enum_variants
                    .get(enum_name.as_str())
                    .and_then(|vs| vs.iter().find(|(n, _, _)| n == variant))
                    .map(|(_, tag, _)| *tag)
                    .unwrap_or(0);

                let fir_body = self.lower_expr(&arms[index].1)?;
                let body_assign = FirStmt::Assign {
                    target: FirPlace::Local(result_id),
                    value: fir_body,
                };

                // Tag is at offset 0 of the enum ptr
                let tag_load = FirExpr::FieldGet {
                    object: Box::new(FirExpr::LocalVar(scrutinee_id, scrutinee_ty.clone())),
                    offset: 0,
                    ty: FirType::I64,
                };
                let cond = FirExpr::BinaryOp {
                    left: Box::new(tag_load),
                    op: BinOp::Eq,
                    right: Box::new(FirExpr::IntLit(tag)),
                    result_ty: FirType::Bool,
                };
                let else_body = if index + 1 < arms.len() {
                    vec![self.build_match_chain(
                        scrutinee_id,
                        scrutinee_ty,
                        arms,
                        index + 1,
                        result_id,
                    )?]
                } else {
                    vec![]
                };
                Ok(FirStmt::If {
                    cond,
                    then_body: vec![body_assign],
                    else_body,
                })
            }
        }
    }

    /// Lower an enum definition.
    /// Enum layout: [tag: i64][field0: i64][field1: i64]...
    /// Each variant gets a constructor function.
    /// Lower an enum definition.
    /// Enum layout: [tag: i64][field0: i64][field1: i64]...
    /// Each variant gets a constructor function.
    pub(crate) fn lower_enum(
        &mut self,
        name: &str,
        variants: &[EnumVariant],
        methods: &[Stmt],
    ) -> Result<(), LowerError> {
        // Compute max variant size for uniform allocation
        let max_fields = variants.iter().map(|v| v.fields.len()).max().unwrap_or(0);
        let alloc_size = 8 + max_fields * 8; // tag + fields

        // Generate a constructor function for each variant
        for (tag, variant) in variants.iter().enumerate() {
            let ctor_name = format!("{}.{}", name, variant.name);
            let id = if let Some(&existing_id) = self.functions.get(&ctor_name) {
                existing_id
            } else {
                let id = FunctionId(self.next_function);
                self.next_function += 1;
                self.functions.insert(ctor_name.clone(), id);
                id
            };

            // Parameters = variant fields
            let params: Vec<(String, FirType)> = variant
                .fields
                .iter()
                .map(|(fname, fty)| (fname.clone(), self.lower_type(fty)))
                .collect();

            // Body: alloc, store tag, store fields, return ptr
            let mut body = Vec::new();

            // let __ptr = aster_class_alloc(alloc_size)
            // Params occupy LocalId(0..N-1), ptr goes after them
            let ptr_id = LocalId(variant.fields.len() as u32);
            body.push(FirStmt::Let {
                name: ptr_id,
                ty: FirType::Ptr,
                value: FirExpr::RuntimeCall {
                    name: "aster_class_alloc".to_string(),
                    args: vec![FirExpr::IntLit(alloc_size as i64)],
                    ret_ty: FirType::Ptr,
                },
            });

            // Store tag at offset 0
            body.push(FirStmt::Assign {
                target: FirPlace::Field {
                    object: Box::new(FirExpr::LocalVar(ptr_id, FirType::Ptr)),
                    offset: 0,
                },
                value: FirExpr::IntLit(tag as i64),
            });

            // Store each field at offset 8 + i*8
            for (i, (_, _)) in variant.fields.iter().enumerate() {
                // Params are at LocalId(i) since they're declared before the ptr local
                body.push(FirStmt::Assign {
                    target: FirPlace::Field {
                        object: Box::new(FirExpr::LocalVar(ptr_id, FirType::Ptr)),
                        offset: 8 + i * 8,
                    },
                    value: FirExpr::LocalVar(LocalId(i as u32), params[i].1.clone()),
                });
            }

            // Return ptr
            body.push(FirStmt::Return(FirExpr::LocalVar(ptr_id, FirType::Ptr)));

            let func = FirFunction {
                id,
                name: ctor_name,
                params: params.clone(),
                ret_type: FirType::Ptr,
                body,
                is_entry: false,
                suspendable: false,
            };
            self.module.add_function(func);
        }

        // Lower methods
        for method_stmt in methods {
            if let Stmt::Let {
                name: method_name,
                value:
                    Expr::Lambda {
                        params,
                        ret_type,
                        body,
                        ..
                    },
                ..
            } = method_stmt
            {
                let mut full_params =
                    vec![("self".to_string(), Type::Custom(name.to_string(), vec![]))];
                full_params.extend(params.iter().cloned());
                // method_name is already qualified by the parser (e.g. "MyEnum.method")
                self.lower_function(method_name, &full_params, ret_type, body)?;
            }
        }

        Ok(())
    }
}
