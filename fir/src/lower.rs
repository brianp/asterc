use std::collections::HashMap;

use ast::type_env::TypeEnv;
use ast::{EnumVariant, Expr, MatchPattern, Module, Stmt, Type};

use crate::exprs::{BinOp, FirExpr, UnaryOp};
use crate::module::{FirClass, FirFunction, FirModule};
use crate::stmts::{FirPlace, FirStmt};
use crate::types::{ClassId, FirType, FunctionId, LocalId};

#[derive(Debug)]
pub enum LowerError {
    UnsupportedFeature(String),
    UnboundVariable(String),
}

impl std::fmt::Display for LowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LowerError::UnsupportedFeature(msg) => write!(f, "unsupported: {}", msg),
            LowerError::UnboundVariable(name) => write!(f, "unbound variable: {}", name),
        }
    }
}

impl std::error::Error for LowerError {}

pub struct Lowerer {
    type_env: TypeEnv,
    module: FirModule,
    /// Maps variable names to their LocalIds within the current function scope.
    locals: HashMap<String, LocalId>,
    /// Maps LocalIds to their FirTypes within the current function scope.
    local_types: HashMap<LocalId, FirType>,
    /// Maps variable names to their AST types within the current function scope.
    /// Used to resolve class names for field access.
    local_ast_types: HashMap<String, Type>,
    /// Maps function names to their FunctionIds.
    functions: HashMap<String, FunctionId>,
    /// Maps class names to their ClassIds.
    classes: HashMap<String, ClassId>,
    /// Maps ClassId to field layout: (field_name, fir_type, byte_offset).
    class_fields: HashMap<ClassId, Vec<(String, FirType, usize)>>,
    /// Maps enum names to their variant info: (variant_name, tag, fields).
    #[allow(clippy::type_complexity)]
    enum_variants: HashMap<String, Vec<(String, i64, Vec<(String, FirType)>)>>,
    /// Maps variable names to their closure info: (lifted_func_id, env_local_id, capture_names).
    /// Used to resolve closure calls statically.
    closure_info: HashMap<String, (FunctionId, Option<LocalId>, Vec<String>)>,
    /// Maps top-level non-function let binding names to their FirExprs.
    /// These are collected during lowering and injected into __init.
    top_level_lets: Vec<(String, FirType, FirExpr)>,
    /// Tracks which variables are top-level globals (accessible from any function).
    globals: HashMap<String, LocalId>,
    next_local: u32,
    next_function: u32,
    next_class: u32,
    /// Statement-lifting buffer: match/closure lowering emits setup statements
    /// that must be injected into the enclosing body.
    pending_stmts: Vec<FirStmt>,
    /// Monomorphization cache: (name, concrete_types) → FunctionId.
    #[allow(dead_code)]
    mono_cache: HashMap<(String, Vec<FirType>), FunctionId>,
}

impl Lowerer {
    pub fn new(type_env: TypeEnv) -> Self {
        Self {
            type_env,
            module: FirModule::new(),
            locals: HashMap::new(),
            local_types: HashMap::new(),
            local_ast_types: HashMap::new(),
            functions: HashMap::new(),
            classes: HashMap::new(),
            class_fields: HashMap::new(),
            enum_variants: HashMap::new(),
            closure_info: HashMap::new(),
            top_level_lets: Vec::new(),
            globals: HashMap::new(),
            next_local: 0,
            next_function: 0,
            next_class: 0,
            pending_stmts: Vec::new(),
            mono_cache: HashMap::new(),
        }
    }

    /// Lower an entire module (compiler path).
    pub fn lower_module(&mut self, module: &Module) -> Result<(), LowerError> {
        // First pass: register all top-level function names, class names, and enum names
        for stmt in &module.body {
            match stmt {
                Stmt::Let {
                    name,
                    value: Expr::Lambda { .. },
                    ..
                } => {
                    let id = FunctionId(self.next_function);
                    self.next_function += 1;
                    self.functions.insert(name.clone(), id);
                }
                Stmt::Class { name, .. } => {
                    let id = ClassId(self.next_class);
                    self.next_class += 1;
                    self.classes.insert(name.clone(), id);
                }
                Stmt::Enum { name, variants, .. } => {
                    // Register enum variant metadata for match lowering
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
                    // Register variant constructors as functions (will be defined in lower_enum)
                    for v in variants {
                        let id = FunctionId(self.next_function);
                        self.next_function += 1;
                        let ctor_name = format!("{}.{}", name, v.name);
                        self.functions.insert(ctor_name, id);
                    }
                }
                _ => {}
            }
        }

        // Second pass: lower everything
        for stmt in &module.body {
            self.lower_top_level_stmt(stmt)?;
        }

        // Top-level let values are inlined into each function via global_prelude
        // in lower_function, so no __init thunk is needed.

        Ok(())
    }

    /// Lower a single statement (REPL path). Appends to existing FirModule.
    pub fn lower_stmt(&mut self, stmt: &Stmt) -> Result<(), LowerError> {
        self.lower_top_level_stmt(stmt)
    }

    /// Lower a bare expression (REPL path). Wraps in a temporary function,
    /// returns its FunctionId for immediate execution.
    pub fn lower_repl_expr(&mut self, expr: &Expr, ty: &Type) -> Result<FunctionId, LowerError> {
        let ret_type = self.lower_type(ty);
        let fir_expr = self.lower_expr(expr)?;
        let id = FunctionId(self.next_function);
        self.next_function += 1;
        let func = FirFunction {
            id,
            name: format!("__repl_expr_{}", id.0),
            params: vec![],
            ret_type,
            body: vec![FirStmt::Return(fir_expr)],
            is_entry: true,
        };
        self.module.add_function(func);
        Ok(id)
    }

    /// Take ownership of the built FirModule.
    pub fn finish(self) -> FirModule {
        self.module
    }

    /// Get a reference to the module being built.
    pub fn module(&self) -> &FirModule {
        &self.module
    }

    fn lower_top_level_stmt(&mut self, stmt: &Stmt) -> Result<(), LowerError> {
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
                // Collect these; they will be injected into an __init thunk
                // that runs at the start of main.
                let fir_value = self.lower_expr(value)?;
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
                ..
            } => self.lower_class(name, fields, methods),
            Stmt::Enum {
                name,
                variants,
                methods,
                ..
            } => self.lower_enum(name, variants, methods),
            Stmt::Expr(expr, _) => {
                // Top-level expression — wrap in a thunk
                let fir_expr = self.lower_expr(expr)?;
                let id = FunctionId(self.next_function);
                self.next_function += 1;
                let func = FirFunction {
                    id,
                    name: format!("__top_expr_{}", id.0),
                    params: vec![],
                    ret_type: FirType::Void,
                    body: vec![FirStmt::Expr(fir_expr)],
                    is_entry: false,
                };
                self.module.add_function(func);
                self.module.entry = Some(id);
                Ok(())
            }
            _ => Err(LowerError::UnsupportedFeature(format!(
                "top-level statement: {:?}",
                std::mem::discriminant(stmt)
            ))),
        }
    }

    fn lower_function(
        &mut self,
        name: &str,
        params: &[(String, Type)],
        ret_type: &Type,
        body: &[Stmt],
    ) -> Result<FunctionId, LowerError> {
        // Save and reset local scope
        let saved_locals = std::mem::take(&mut self.locals);
        let saved_local_types = std::mem::take(&mut self.local_types);
        let saved_local_ast_types = std::mem::take(&mut self.local_ast_types);
        let saved_closure_info = std::mem::take(&mut self.closure_info);
        let saved_next_local = self.next_local;
        self.next_local = 0;

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

        // Lower body, converting last expression to implicit return
        let mut fir_body = self.lower_body(body)?;
        if let Some(last) = fir_body.last() {
            // If the last statement is an Expr (not Return), make it a Return
            if matches!(last, FirStmt::Expr(_))
                && *ret_type != Type::Void
                && let Some(FirStmt::Expr(expr)) = fir_body.pop()
            {
                fir_body.push(FirStmt::Return(expr));
            }
        }

        // Prepend global value definitions if any
        if !global_prelude.is_empty() {
            global_prelude.append(&mut fir_body);
            fir_body = global_prelude;
        }

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
        };
        self.module.add_function(func);

        if name == "main" {
            self.module.entry = Some(id);
        }

        // Restore outer scope
        self.locals = saved_locals;
        self.local_types = saved_local_types;
        self.local_ast_types = saved_local_ast_types;
        self.closure_info = saved_closure_info;
        self.next_local = saved_next_local;

        Ok(id)
    }

    fn lower_class(
        &mut self,
        name: &str,
        fields: &[(String, Type)],
        methods: &[Stmt],
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

        // Build field layout: 8 bytes per field, 8-byte aligned
        let mut fir_fields = Vec::new();
        let mut offset = 0usize;
        for (field_name, field_type) in fields {
            let fir_type = self.lower_type(field_type);
            fir_fields.push((field_name.clone(), fir_type, offset));
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
                        ..
                    },
                ..
            } = method_stmt
            {
                // Prepend `self: ClassName` as the first parameter
                let mut full_params =
                    vec![("self".to_string(), Type::Custom(name.to_string(), vec![]))];
                full_params.extend(params.iter().cloned());

                let qualified_name = format!("{}.{}", name, method_name);
                self.lower_function(&qualified_name, &full_params, ret_type, body)?;
            }
        }

        Ok(())
    }

    fn lower_body(&mut self, stmts: &[Stmt]) -> Result<Vec<FirStmt>, LowerError> {
        let mut result = Vec::new();
        for stmt in stmts {
            let fir_stmt = self.lower_stmt_inner(stmt)?;
            // Drain any pending statements emitted by expression lowering (e.g. match setup)
            result.append(&mut self.pending_stmts);
            result.push(fir_stmt);
        }
        Ok(result)
    }

    fn lower_stmt_inner(&mut self, stmt: &Stmt) -> Result<FirStmt, LowerError> {
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

                let fir_value = self.lower_expr(value)?;
                let fir_type = if let Some(ann) = type_ann {
                    self.lower_type(ann)
                } else {
                    self.infer_fir_type(&fir_value)
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
                        // It's the local that was returned as LocalVar
                        if let FirExpr::LocalVar(env_id, _) = &fir_value {
                            Some(*env_id)
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
                } else if let Expr::Call { func, .. } = value {
                    // Infer class type from constructor call
                    if let Expr::Ident(class_name, _) = func.as_ref()
                        && self.classes.contains_key(class_name.as_str())
                    {
                        self.local_ast_types
                            .insert(name.clone(), Type::Custom(class_name.clone(), vec![]));
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
                Ok(FirStmt::Return(fir_expr))
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
                let fir_body = self.lower_body(body)?;
                Ok(FirStmt::While {
                    cond: fir_cond,
                    body: fir_body,
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
            Stmt::Break(_) => Ok(FirStmt::Break),
            Stmt::Continue(_) => Ok(FirStmt::Continue),
            Stmt::Expr(expr, _) => {
                let fir_expr = self.lower_expr(expr)?;
                Ok(FirStmt::Expr(fir_expr))
            }
            Stmt::For {
                var, iter, body, ..
            } => self.lower_for_loop(var, iter, body),
            _ => Err(LowerError::UnsupportedFeature(format!(
                "statement: {:?}",
                std::mem::discriminant(stmt)
            ))),
        }
    }

    fn lower_if(
        &mut self,
        cond: &Expr,
        then_body: &[Stmt],
        elif_branches: &[(Expr, Vec<Stmt>)],
        else_body: &[Stmt],
    ) -> Result<FirStmt, LowerError> {
        let fir_cond = self.lower_expr(cond)?;
        let fir_then = self.lower_body(then_body)?;

        // Flatten elif chains into nested if/else
        if !elif_branches.is_empty() {
            let (elif_cond, elif_body) = &elif_branches[0];
            let nested_else =
                self.lower_if(elif_cond, elif_body, &elif_branches[1..], else_body)?;
            Ok(FirStmt::If {
                cond: fir_cond,
                then_body: fir_then,
                else_body: vec![nested_else],
            })
        } else {
            let fir_else = self.lower_body(else_body)?;
            Ok(FirStmt::If {
                cond: fir_cond,
                then_body: fir_then,
                else_body: fir_else,
            })
        }
    }

    fn lower_expr(&mut self, expr: &Expr) -> Result<FirExpr, LowerError> {
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
                } else {
                    Err(LowerError::UnboundVariable(name.clone()))
                }
            }

            Expr::BinaryOp {
                left, op, right, ..
            } => {
                let fir_left = self.lower_expr(left)?;
                let fir_right = self.lower_expr(right)?;
                let fir_op = self.lower_binop(op);
                let result_ty = self.infer_binop_type(&fir_op, &fir_left, &fir_right);
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
                // Method call: obj.method(args)
                if let Expr::Member { object, field, .. } = func.as_ref() {
                    return self.lower_method_call(object, field, args);
                }

                // Resolve function name
                if let Expr::Ident(name, _) = func.as_ref() {
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
                        for (_, arg) in args {
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
                            if let Some((_, expr)) =
                                args.iter().find(|(arg_name, _)| arg_name == field_name)
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
                            for (_, arg) in args {
                                fir_fields.push(self.lower_expr(arg)?);
                            }
                        }
                        Ok(FirExpr::Construct {
                            class: class_id,
                            fields: fir_fields,
                            ty: FirType::Ptr,
                        })
                    } else if let Some(&func_id) = self.functions.get(name.as_str()) {
                        let fir_args: Result<Vec<_>, _> =
                            args.iter().map(|(_, arg)| self.lower_expr(arg)).collect();
                        let ret_ty = self.resolve_function_ret_type(name);
                        Ok(FirExpr::Call {
                            func: func_id,
                            args: fir_args?,
                            ret_ty,
                        })
                    } else {
                        // Could be a runtime call (print, etc.)
                        let fir_args: Result<Vec<_>, _> =
                            args.iter().map(|(_, arg)| self.lower_expr(arg)).collect();
                        Ok(FirExpr::RuntimeCall {
                            name: name.clone(),
                            args: fir_args?,
                            ret_ty: FirType::Void,
                        })
                    }
                } else {
                    Err(LowerError::UnsupportedFeature(
                        "non-ident function call target".into(),
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
                let fir_obj = self.lower_expr(object)?;
                let fir_idx = self.lower_expr(index)?;
                Ok(FirExpr::ListGet {
                    list: Box::new(fir_obj),
                    index: Box::new(fir_idx),
                    elem_ty: FirType::I64, // TODO: proper element type
                })
            }

            Expr::Lambda {
                params,
                ret_type,
                body,
                ..
            } => self.lower_lambda(params, ret_type, body),

            Expr::Match {
                scrutinee, arms, ..
            } => self.lower_match(scrutinee, arms),

            // Async: `async f(args)` → eager call (no true concurrency yet)
            Expr::AsyncCall { func, args, .. } => {
                // Lower as a regular call — the result IS the task (eager execution)
                self.lower_expr(&Expr::Call {
                    func: func.clone(),
                    args: args.clone(),
                    span: expr.span(),
                })
            }

            // Resolve: `resolve expr!` → identity (already computed eagerly)
            Expr::Resolve { expr: inner, .. } => self.lower_expr(inner),

            // Propagate: `expr!` → identity for now (error propagation is step 8)
            Expr::Propagate(inner, _) => self.lower_expr(inner),

            // Throw: lower to a runtime trap for now
            Expr::Throw(inner, _) => {
                let _fir_inner = self.lower_expr(inner)?;
                Ok(FirExpr::RuntimeCall {
                    name: "aster_panic".to_string(),
                    args: vec![],
                    ret_ty: FirType::Never,
                })
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

            _ => Err(LowerError::UnsupportedFeature(format!(
                "expression: {:?}",
                std::mem::discriminant(expr)
            ))),
        }
    }

    /// Lower a method call: `obj.method(args)`.
    /// Handles list built-in methods and class method dispatch.
    fn lower_method_call(
        &mut self,
        object: &Expr,
        method: &str,
        args: &[(String, Expr)],
    ) -> Result<FirExpr, LowerError> {
        // Check for enum variant constructor with fields: EnumName.Variant(fields)
        if let Expr::Ident(name, _) = object
            && self.enum_variants.contains_key(name.as_str())
        {
            let ctor_name = format!("{}.{}", name, method);
            if let Some(&func_id) = self.functions.get(&ctor_name) {
                let fir_args: Result<Vec<_>, _> =
                    args.iter().map(|(_, arg)| self.lower_expr(arg)).collect();
                return Ok(FirExpr::Call {
                    func: func_id,
                    args: fir_args?,
                    ret_ty: FirType::Ptr,
                });
            }
        }

        let fir_object = self.lower_expr(object)?;

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
                for (_, arg) in args {
                    call_args.push(self.lower_expr(arg)?);
                }
                return Ok(FirExpr::RuntimeCall {
                    name: "aster_list_push".to_string(),
                    args: call_args,
                    ret_ty: FirType::Ptr,
                });
            }
            _ => {}
        }

        // Check for class method calls
        if let Ok(class_name) = self.resolve_class_name(object) {
            let qualified_name = format!("{}.{}", class_name, method);
            if let Some(&func_id) = self.functions.get(&qualified_name) {
                // Build args: self + explicit args
                let mut call_args = vec![fir_object];
                for (_, arg) in args {
                    call_args.push(self.lower_expr(arg)?);
                }
                let ret_ty = self.resolve_function_ret_type(&qualified_name);
                return Ok(FirExpr::Call {
                    func: func_id,
                    args: call_args,
                    ret_ty,
                });
            }
        }

        Err(LowerError::UnsupportedFeature(format!(
            "method call: .{}()",
            method
        )))
    }

    /// Lower `for var in iter: body` → index-based while loop.
    /// Desugars to: let __iter = iter; let __len = len(__iter); let __idx = 0;
    /// while __idx < __len: let var = get(__iter, __idx); body; __idx += 1
    fn lower_for_loop(
        &mut self,
        var: &str,
        iter: &Expr,
        body: &[Stmt],
    ) -> Result<FirStmt, LowerError> {
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

        // let var = aster_list_get(__iter, __idx)
        let var_id = self.alloc_local();
        self.locals.insert(var.to_string(), var_id);
        self.local_types.insert(var_id, FirType::I64);

        // Build the while loop body
        let mut while_body = Vec::new();

        // let var = aster_list_get(__iter, __idx)
        while_body.push(FirStmt::Let {
            name: var_id,
            ty: FirType::I64,
            value: FirExpr::RuntimeCall {
                name: "aster_list_get".to_string(),
                args: vec![
                    FirExpr::LocalVar(iter_id, FirType::Ptr),
                    FirExpr::LocalVar(idx_id, FirType::I64),
                ],
                ret_ty: FirType::I64,
            },
        });

        // Lower the user's loop body
        for stmt in body {
            while_body.push(self.lower_stmt_inner(stmt)?);
        }

        // __idx = __idx + 1
        while_body.push(FirStmt::Assign {
            target: FirPlace::Local(idx_id),
            value: FirExpr::BinaryOp {
                left: Box::new(FirExpr::LocalVar(idx_id, FirType::I64)),
                op: BinOp::Add,
                right: Box::new(FirExpr::IntLit(1)),
                result_ty: FirType::I64,
            },
        });

        // Build: let __iter = iter; let __len = len(iter); let __idx = 0; while __idx < __len { ... }
        // We need to emit multiple statements but lower_stmt_inner returns one.
        // Solution: wrap everything in a sequence by returning the first Let and injecting
        // the rest via a nested structure. Actually, let's return the while and prepend the
        // setup as Let statements before it.

        // Actually, we can only return one FirStmt from lower_stmt_inner.
        // Workaround: embed the setup into the while via init-lets and return an If(true, block, [])
        // Better workaround: use a synthetic block. But FIR doesn't have blocks.
        // Best approach: return the while loop, and set up the locals as part of the enclosing scope.
        // The let bindings for __iter, __len, __idx are already allocated in locals.
        // We need to emit them BEFORE the while. But we can only return ONE stmt.

        // Solution: use an If(true) wrapper with all statements inside:
        let setup_and_loop = vec![
            // let __iter = <iterable>
            FirStmt::Let {
                name: iter_id,
                ty: FirType::Ptr,
                value: fir_iter,
            },
            // let __len = aster_list_len(__iter)
            FirStmt::Let {
                name: len_id,
                ty: FirType::I64,
                value: FirExpr::RuntimeCall {
                    name: "aster_list_len".to_string(),
                    args: vec![FirExpr::LocalVar(iter_id, FirType::Ptr)],
                    ret_ty: FirType::I64,
                },
            },
            // let __idx = 0
            FirStmt::Let {
                name: idx_id,
                ty: FirType::I64,
                value: FirExpr::IntLit(0),
            },
            // while __idx < __len { ... }
            FirStmt::While {
                cond: FirExpr::BinaryOp {
                    left: Box::new(FirExpr::LocalVar(idx_id, FirType::I64)),
                    op: BinOp::Lt,
                    right: Box::new(FirExpr::LocalVar(len_id, FirType::I64)),
                    result_ty: FirType::Bool,
                },
                body: while_body,
            },
        ];

        // Wrap in If(true, setup_and_loop, []) to return a single statement
        Ok(FirStmt::If {
            cond: FirExpr::BoolLit(true),
            then_body: setup_and_loop,
            else_body: vec![],
        })
    }

    fn lower_place(&self, expr: &Expr) -> Result<FirPlace, LowerError> {
        match expr {
            Expr::Ident(name, _) => {
                if let Some(&local_id) = self.locals.get(name.as_str()) {
                    Ok(FirPlace::Local(local_id))
                } else {
                    Err(LowerError::UnboundVariable(name.clone()))
                }
            }
            Expr::Index { object, index, .. } => {
                let fir_obj = self.lower_expr_ref(object)?;
                let fir_idx = self.lower_expr_ref(index)?;
                Ok(FirPlace::Index {
                    list: Box::new(fir_obj),
                    index: Box::new(fir_idx),
                })
            }
            _ => Err(LowerError::UnsupportedFeature(
                "complex assignment target".into(),
            )),
        }
    }

    /// Lower an expression (non-mutable version for place contexts).
    fn lower_expr_ref(&self, expr: &Expr) -> Result<FirExpr, LowerError> {
        // For now, delegate to a simple version that doesn't allocate locals
        match expr {
            Expr::Int(n, _) => Ok(FirExpr::IntLit(*n)),
            Expr::Ident(name, _) => {
                if let Some(&local_id) = self.locals.get(name.as_str()) {
                    let ty = self.resolve_var_type(name);
                    Ok(FirExpr::LocalVar(local_id, ty))
                } else {
                    Err(LowerError::UnboundVariable(name.clone()))
                }
            }
            _ => Err(LowerError::UnsupportedFeature(
                "complex expression in place context".into(),
            )),
        }
    }

    fn lower_type(&self, ty: &Type) -> FirType {
        match ty {
            Type::Int => FirType::I64,
            Type::Float => FirType::F64,
            Type::Bool => FirType::Bool,
            Type::String => FirType::Ptr,
            Type::Nil | Type::Void => FirType::Void,
            Type::Never => FirType::Never,
            Type::List(_) => FirType::Ptr,
            Type::Nullable(inner) => FirType::TaggedUnion {
                tag_bits: 1,
                variants: vec![self.lower_type(inner), FirType::Void],
            },
            Type::Custom(_, _) => FirType::Ptr, // class instances are heap-allocated
            Type::Function { .. } => FirType::Ptr, // function pointers
            Type::Task(_) => FirType::Ptr,
            Type::Map(_, _) => FirType::Ptr,
            Type::Error | Type::Inferred => FirType::Void, // shouldn't appear after typechecking
            Type::TypeVar(_, _) => FirType::Void, // shouldn't appear after monomorphization
        }
    }

    fn lower_binop(&self, op: &ast::BinOp) -> BinOp {
        match op {
            ast::BinOp::Add => BinOp::Add,
            ast::BinOp::Sub => BinOp::Sub,
            ast::BinOp::Mul => BinOp::Mul,
            ast::BinOp::Div => BinOp::Div,
            ast::BinOp::Mod => BinOp::Mod,
            ast::BinOp::Pow => {
                panic!("** (power) operator not yet supported in codegen — needs runtime call")
            }
            ast::BinOp::Eq => BinOp::Eq,
            ast::BinOp::Neq => BinOp::Neq,
            ast::BinOp::Lt => BinOp::Lt,
            ast::BinOp::Gt => BinOp::Gt,
            ast::BinOp::Lte => BinOp::Lte,
            ast::BinOp::Gte => BinOp::Gte,
            ast::BinOp::And => BinOp::And,
            ast::BinOp::Or => BinOp::Or,
        }
    }

    fn lower_unaryop(&self, op: &ast::UnaryOp) -> UnaryOp {
        match op {
            ast::UnaryOp::Neg => UnaryOp::Neg,
            ast::UnaryOp::Not => UnaryOp::Not,
        }
    }

    fn alloc_local(&mut self) -> LocalId {
        let id = LocalId(self.next_local);
        self.next_local += 1;
        id
    }

    /// Infer FirType from a FirExpr.
    fn infer_fir_type(&self, expr: &FirExpr) -> FirType {
        match expr {
            FirExpr::IntLit(_) => FirType::I64,
            FirExpr::FloatLit(_) => FirType::F64,
            FirExpr::BoolLit(_) => FirType::Bool,
            FirExpr::StringLit(_) => FirType::Ptr,
            FirExpr::NilLit => FirType::Void,
            FirExpr::LocalVar(_, ty) => ty.clone(),
            FirExpr::BinaryOp { result_ty, .. } => result_ty.clone(),
            FirExpr::UnaryOp { result_ty, .. } => result_ty.clone(),
            FirExpr::Call { ret_ty, .. } => ret_ty.clone(),
            FirExpr::FieldGet { ty, .. } => ty.clone(),
            FirExpr::FieldSet { .. } => FirType::Void,
            FirExpr::Construct { ty, .. } => ty.clone(),
            FirExpr::ListNew { .. } => FirType::Ptr,
            FirExpr::ListGet { elem_ty, .. } => elem_ty.clone(),
            FirExpr::ListSet { .. } => FirType::Void,
            FirExpr::TagWrap { ty, .. } => ty.clone(),
            FirExpr::TagUnwrap { ty, .. } => ty.clone(),
            FirExpr::TagCheck { .. } => FirType::Bool,
            FirExpr::RuntimeCall { ret_ty, .. } => ret_ty.clone(),
            FirExpr::ClosureCreate { .. } => FirType::Ptr,
            FirExpr::ClosureCall { ret_ty, .. } => ret_ty.clone(),
            FirExpr::EnvLoad { ty, .. } => ty.clone(),
            FirExpr::GlobalFunc(_) => FirType::Ptr,
        }
    }

    fn infer_binop_type(&self, op: &BinOp, left: &FirExpr, _right: &FirExpr) -> FirType {
        match op {
            BinOp::Eq
            | BinOp::Neq
            | BinOp::Lt
            | BinOp::Gt
            | BinOp::Lte
            | BinOp::Gte
            | BinOp::And
            | BinOp::Or => FirType::Bool,
            _ => self.infer_fir_type(left),
        }
    }

    fn infer_unaryop_type(&self, op: &UnaryOp, operand: &FirExpr) -> FirType {
        match op {
            UnaryOp::Not => FirType::Bool,
            UnaryOp::Neg => self.infer_fir_type(operand),
        }
    }

    fn resolve_var_type(&self, name: &str) -> FirType {
        // Check local scope first (function params, let bindings)
        if let Some(&local_id) = self.locals.get(name)
            && let Some(ty) = self.local_types.get(&local_id)
        {
            return ty.clone();
        }
        // Fall back to type env (top-level bindings)
        if let Some(ty) = self.type_env.get_var(name) {
            self.lower_type(&ty)
        } else {
            FirType::Void
        }
    }

    fn resolve_function_ret_type(&self, name: &str) -> FirType {
        if let Some(Type::Function { ret, .. }) = self.type_env.get_var(name) {
            self.lower_type(&ret)
        } else {
            FirType::Void
        }
    }

    fn resolve_function_ret_type_by_id(&self, id: FunctionId) -> FirType {
        if let Some(func) = self.module.functions.iter().find(|f| f.id == id) {
            func.ret_type.clone()
        } else {
            FirType::Void
        }
    }

    /// Resolve a field access on an object expression, returning (byte_offset, field_fir_type).
    fn resolve_field_access(
        &self,
        object: &Expr,
        field: &str,
    ) -> Result<(usize, FirType), LowerError> {
        // Determine the class name from the object's type
        let class_name = self.resolve_class_name(object)?;

        // Look up the class ID
        let class_id = self.classes.get(&class_name).ok_or_else(|| {
            LowerError::UnsupportedFeature(format!("unknown class: {}", class_name))
        })?;

        // Look up the field in the class layout
        let fields = self.class_fields.get(class_id).ok_or_else(|| {
            LowerError::UnsupportedFeature(format!("no field layout for class: {}", class_name))
        })?;

        for (fname, fty, foffset) in fields {
            if fname == field {
                return Ok((*foffset, fty.clone()));
            }
        }

        Err(LowerError::UnsupportedFeature(format!(
            "unknown field '{}' on class '{}'",
            field, class_name
        )))
    }

    /// Determine the class name of an expression by inspecting local AST types
    /// and the type environment.
    fn resolve_class_name(&self, expr: &Expr) -> Result<String, LowerError> {
        match expr {
            Expr::Ident(name, _) => {
                // Check local AST types first (function-scoped variables)
                if let Some(ty) = self.local_ast_types.get(name.as_str())
                    && let Type::Custom(class_name, _) = ty
                {
                    return Ok(class_name.clone());
                }
                // Fall back to the type env (top-level bindings)
                if let Some(ty) = self.type_env.get_var(name)
                    && let Type::Custom(class_name, _) = ty
                {
                    return Ok(class_name);
                }
                Err(LowerError::UnsupportedFeature(format!(
                    "cannot determine class type of variable '{}'",
                    name
                )))
            }
            Expr::Call { func, .. } => {
                // Constructor call: the function name IS the class name
                if let Expr::Ident(name, _) = func.as_ref()
                    && self.classes.contains_key(name.as_str())
                {
                    return Ok(name.clone());
                }
                Err(LowerError::UnsupportedFeature(
                    "cannot determine class type of call expression".into(),
                ))
            }
            _ => Err(LowerError::UnsupportedFeature(
                "cannot determine class type of expression".into(),
            )),
        }
    }

    /// Create an __init thunk for top-level let bindings.
    /// The __init function stores values into a globals array, and a call to __init
    /// is injected at the start of main.
    /// Currently unused — globals are inlined into each function — but kept for
    /// future use with side-effectful initialization.
    #[allow(dead_code)]
    fn emit_init_thunk(&mut self) -> Result<(), LowerError> {
        let init_id = FunctionId(self.next_function);
        self.next_function += 1;
        self.functions.insert("__init".to_string(), init_id);

        // Save scope
        let saved_locals = std::mem::take(&mut self.locals);
        let saved_local_types = std::mem::take(&mut self.local_types);
        let saved_next_local = self.next_local;
        self.next_local = 0;

        let mut init_body = Vec::new();

        // For each top-level let, allocate a local and store the value.
        // We also store into a global heap array so functions can access them.
        // But since we need cross-function access, we use RuntimeCall to store
        // into a globals array.

        // Allocate globals array: one i64 per global
        let num_globals = self.top_level_lets.len();
        let globals_ptr_id = self.alloc_local();
        self.locals.insert("__globals".to_string(), globals_ptr_id);
        self.local_types.insert(globals_ptr_id, FirType::Ptr);

        init_body.push(FirStmt::Let {
            name: globals_ptr_id,
            ty: FirType::Ptr,
            value: FirExpr::RuntimeCall {
                name: "aster_class_alloc".to_string(),
                args: vec![FirExpr::IntLit((num_globals * 8) as i64)],
                ret_ty: FirType::Ptr,
            },
        });

        // Store each global value
        let top_level_lets = std::mem::take(&mut self.top_level_lets);
        for (i, (_name, ty, value)) in top_level_lets.iter().enumerate() {
            // We need to re-lower the value in the init scope.
            // But the value was already lowered. Clone it.
            let local_id = self.alloc_local();
            self.locals.insert(format!("__global_{}", i), local_id);
            self.local_types.insert(local_id, ty.clone());

            init_body.push(FirStmt::Let {
                name: local_id,
                ty: ty.clone(),
                value: value.clone(),
            });

            // Store into globals array at offset i*8
            init_body.push(FirStmt::Assign {
                target: FirPlace::Field {
                    object: Box::new(FirExpr::LocalVar(globals_ptr_id, FirType::Ptr)),
                    offset: i * 8,
                },
                value: FirExpr::LocalVar(local_id, ty.clone()),
            });
        }

        // Return the globals pointer
        init_body.push(FirStmt::Return(FirExpr::LocalVar(
            globals_ptr_id,
            FirType::Ptr,
        )));

        let init_func = FirFunction {
            id: init_id,
            name: "__init".to_string(),
            params: vec![],
            ret_type: FirType::Ptr,
            body: init_body,
            is_entry: false,
        };
        self.module.add_function(init_func);

        // Restore scope
        self.locals = saved_locals;
        self.local_types = saved_local_types;
        self.next_local = saved_next_local;

        // Find main function and inject __init call + global loads at the start
        if let Some(entry_id) = self.module.entry {
            let main_func = self.module.functions.iter_mut().find(|f| f.id == entry_id);

            if let Some(main_func) = main_func {
                let mut new_body = Vec::new();

                // Call __init, get globals ptr
                let globals_local = LocalId(main_func.params.len() as u32 + 1000); // high ID to avoid conflicts
                new_body.push(FirStmt::Let {
                    name: globals_local,
                    ty: FirType::Ptr,
                    value: FirExpr::Call {
                        func: init_id,
                        args: vec![],
                        ret_ty: FirType::Ptr,
                    },
                });

                // Load each global from the globals array
                for (i, (name, ty, _)) in top_level_lets.iter().enumerate() {
                    if let Some(&global_local_id) = self.globals.get(name) {
                        new_body.push(FirStmt::Let {
                            name: global_local_id,
                            ty: ty.clone(),
                            value: FirExpr::FieldGet {
                                object: Box::new(FirExpr::LocalVar(globals_local, FirType::Ptr)),
                                offset: i * 8,
                                ty: ty.clone(),
                            },
                        });
                    }
                }

                // Append original main body
                new_body.append(&mut main_func.body);
                main_func.body = new_body;
            }
        }

        // Put back the top_level_lets
        self.top_level_lets = top_level_lets;

        Ok(())
    }

    /// Lower a match expression to nested if/else chains.
    /// Returns a FirExpr::LocalVar referencing the result.
    fn lower_match(
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
    fn infer_match_result_type(&self, arms: &[(MatchPattern, Expr)]) -> Option<FirType> {
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

    fn build_match_chain(
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
    fn lower_enum(
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
                let qualified_name = format!("{}.{}", name, method_name);
                self.lower_function(&qualified_name, &full_params, ret_type, body)?;
            }
        }

        Ok(())
    }

    /// Lower a lambda/closure expression.
    /// All lambdas are lifted to top-level functions with `__env: Ptr` as first param.
    /// Captures are stored in a heap-allocated env struct.
    /// Returns a dummy value; the important side effect is registering closure_info
    /// so that call sites can resolve the closure statically.
    fn lower_lambda(
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

        let lambda_name = format!("__lambda_{}", self.next_function);

        // Build the lifted function's params: __env: Ptr, then original params
        let mut lifted_params =
            vec![("__env".to_string(), Type::Custom("Ptr".to_string(), vec![]))];
        lifted_params.extend(params.iter().cloned());

        // Before lowering the lambda body, set up the capture mapping.
        // Save outer scope, then set up inner scope with env loads.
        let saved_locals = std::mem::take(&mut self.locals);
        let saved_local_types = std::mem::take(&mut self.local_types);
        let saved_local_ast_types = std::mem::take(&mut self.local_ast_types);
        let saved_closure_info = std::mem::take(&mut self.closure_info);
        let saved_next_local = self.next_local;
        self.next_local = 0;

        // Allocate __env as local 0
        let env_local = self.alloc_local(); // LocalId(0)
        self.locals.insert("__env".to_string(), env_local);
        self.local_types.insert(env_local, FirType::Ptr);

        // Allocate params as locals 1..N
        let mut fir_params = vec![("__env".to_string(), FirType::Ptr)];
        for (pname, pty) in params {
            let local_id = self.alloc_local();
            let fir_type = self.lower_type(pty);
            self.locals.insert(pname.clone(), local_id);
            self.local_types.insert(local_id, fir_type.clone());
            self.local_ast_types.insert(pname.clone(), pty.clone());
            fir_params.push((pname.clone(), fir_type));
        }

        // Map captured variables to env loads
        for cap_name in &captures {
            let local_id = self.alloc_local();
            let cap_ty = saved_local_types
                .get(saved_locals.get(cap_name).unwrap_or(&LocalId(0)))
                .cloned()
                .unwrap_or(FirType::I64);
            self.locals.insert(cap_name.clone(), local_id);
            self.local_types.insert(local_id, cap_ty.clone());
        }

        // Lower the body
        let mut fir_body = Vec::new();

        // Emit env loads for captures at the start of the body
        for (i, cap_name) in captures.iter().enumerate() {
            let local_id = *self.locals.get(cap_name).unwrap();
            let cap_ty = self
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

        // Convert last expression to return (like lower_function does)
        if let Some(last) = fir_body.last()
            && matches!(last, FirStmt::Expr(_))
            && *ret_type != Type::Void
            && let Some(FirStmt::Expr(expr)) = fir_body.pop()
        {
            fir_body.push(FirStmt::Return(expr));
        }

        // Get or create function ID
        let func_id = if let Some(&existing_id) = self.functions.get(&lambda_name) {
            existing_id
        } else {
            let id = FunctionId(self.next_function);
            self.next_function += 1;
            self.functions.insert(lambda_name.clone(), id);
            id
        };

        let func = FirFunction {
            id: func_id,
            name: lambda_name.clone(),
            params: fir_params,
            ret_type: self.lower_type(ret_type),
            body: fir_body,
            is_entry: false,
        };
        self.module.add_function(func);

        // Restore outer scope
        self.locals = saved_locals;
        self.local_types = saved_local_types;
        self.local_ast_types = saved_local_ast_types;
        self.closure_info = saved_closure_info;
        self.next_local = saved_next_local;

        // Re-register the function name
        self.functions.insert(lambda_name, func_id);

        if captures.is_empty() {
            // No captures: env is null. Return a NilLit as placeholder.
            // The important thing is closure_info registration (done at let-binding site).
            Ok(FirExpr::NilLit)
        } else {
            // Allocate env struct and store captures
            let env_size = captures.len() * 8;
            let env_id = self.alloc_local();
            let env_name = format!("__env_{}", func_id.0);
            self.locals.insert(env_name.clone(), env_id);
            self.local_types.insert(env_id, FirType::Ptr);

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
                if let Some(&local_id) = self.locals.get(cap_name.as_str()) {
                    let ty = self
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

            Ok(FirExpr::LocalVar(env_id, FirType::Ptr))
        }
    }

    /// Find variables referenced in a body that are not in the given param set
    /// but exist in the current local scope.
    fn find_captures(
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
                Stmt::Let { value, .. } => {
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
                Stmt::Assignment { value, .. } => {
                    self.find_captures_expr(value, param_names, captures);
                }
                _ => {}
            }
        }
    }

    fn find_captures_expr(
        &self,
        expr: &Expr,
        param_names: &std::collections::HashSet<&str>,
        captures: &mut Vec<String>,
    ) {
        match expr {
            Expr::Ident(name, _) => {
                if !param_names.contains(name.as_str()) && self.locals.contains_key(name.as_str()) {
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
                for (_, arg) in args {
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
            _ => {}
        }
    }
}
