use std::collections::HashMap;

use ast::Span;
use ast::type_env::TypeEnv;
use ast::type_table::TypeTable;
use ast::{EnumVariant, Expr, MatchPattern, Module, Stmt, Type};

use crate::exprs::{BinOp, FirExpr, UnaryOp};
use crate::module::{FirClass, FirFunction, FirModule};
use crate::stmts::{FirPlace, FirStmt};
use crate::types::{ClassId, FirType, FunctionId, LocalId};

#[derive(Debug)]
pub enum UnsupportedFeatureKind {
    TopLevelStatement(&'static str),
    Statement(&'static str),
    Other(String),
}

impl UnsupportedFeatureKind {
    pub fn detail(&self) -> String {
        match self {
            UnsupportedFeatureKind::TopLevelStatement(name) => {
                format!("top-level `{name}` statements")
            }
            UnsupportedFeatureKind::Statement(name) => format!("`{name}` statements"),
            UnsupportedFeatureKind::Other(msg) => msg.clone(),
        }
    }
}

#[derive(Debug)]
pub enum LowerError {
    UnsupportedFeature(UnsupportedFeatureKind, Span),
    UnboundVariable(String, Span),
}

impl LowerError {
    pub fn span(&self) -> Span {
        match self {
            LowerError::UnsupportedFeature(_, span) => *span,
            LowerError::UnboundVariable(_, span) => *span,
        }
    }
}

impl std::fmt::Display for LowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LowerError::UnsupportedFeature(kind, _) => {
                write!(f, "unsupported: {}", kind.detail())
            }
            LowerError::UnboundVariable(name, _) => write!(f, "unbound variable: {}", name),
        }
    }
}

impl std::error::Error for LowerError {}

fn unsupported_top_level_stmt(stmt: &Stmt) -> LowerError {
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

fn unsupported_stmt(stmt: &Stmt) -> LowerError {
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

pub struct Lowerer {
    type_env: TypeEnv,
    /// Resolved types from the typechecker, keyed by expression span.
    type_table: TypeTable,
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
    /// Top-level control flow statements (if, while, for, assignment)
    /// stored as AST and re-lowered inside each function's scope.
    top_level_stmts: Vec<Stmt>,
    /// Top-level bare expressions (say, function calls) — only run in entry function.
    top_level_exprs: Vec<Stmt>,
    /// Tracks which variables are top-level globals (accessible from any function).
    globals: HashMap<String, LocalId>,
    next_local: u32,
    next_function: u32,
    next_class: u32,
    /// Statement-lifting buffer: match/closure lowering emits setup statements
    /// that must be injected into the enclosing body.
    pending_stmts: Vec<FirStmt>,
    /// Default parameter values for functions: maps function name → [(param_name, default_expr?)].
    /// Used to fill in missing arguments at call sites.
    #[allow(clippy::type_complexity)]
    function_defaults: HashMap<String, Vec<(String, Option<Expr>)>>,
    /// The AST return type of the function currently being lowered.
    /// Used to wrap return values in TagWrap for nullable return types.
    current_return_type: Option<Type>,
    /// Tracks the current async-scope ownership context.
    async_scope_stack: Vec<LocalId>,
    /// The implicit task scope for the current function. Used to emit
    /// scope_exit before returns and throws for automatic task cleanup.
    function_scope_id: Option<LocalId>,
    /// Locals that implement Drop or Close, in declaration order.
    /// On scope exit, cleanup calls are emitted in reverse order.
    /// Each entry: (local_id, class_name, has_drop, has_close).
    cleanup_locals: Vec<(LocalId, String, bool, bool)>,
    /// Stack of scope boundaries for cleanup tracking.
    /// Each entry is the length of `cleanup_locals` when a loop scope was entered.
    /// Used to determine which locals to clean up on break/continue.
    cleanup_scope_stack: Vec<usize>,
}

impl Lowerer {
    pub fn new(type_env: TypeEnv, type_table: TypeTable) -> Self {
        Self {
            type_env,
            type_table,
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
            top_level_stmts: Vec::new(),
            top_level_exprs: Vec::new(),
            globals: HashMap::new(),
            next_local: 0,
            next_function: 0,
            next_class: 0,
            pending_stmts: Vec::new(),
            function_defaults: HashMap::new(),
            current_return_type: None,
            async_scope_stack: Vec::new(),
            function_scope_id: None,
            cleanup_locals: Vec::new(),
            cleanup_scope_stack: Vec::new(),
        }
    }

    /// Inject the built-in `Ordering` enum (Less/Equal/Greater) into the FIR.
    /// This must run before any user code is lowered so that synthesize_cmp can
    /// reference `Ordering.Less`, `Ordering.Equal`, and `Ordering.Greater`.
    fn inject_ordering_builtin(&mut self) {
        // Ordering is a unit enum: each variant is a struct with a single tag field.
        // Layout: alloc 8 bytes, store tag at offset 0.
        let alloc_size = 8i64;
        let variants = [
            ("Ordering.Less", 0i64),
            ("Ordering.Equal", 1),
            ("Ordering.Greater", 2),
        ];

        for (ctor_name, tag) in variants {
            let func_id = FunctionId(self.next_function);
            self.next_function += 1;
            self.functions.insert(ctor_name.to_string(), func_id);

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

            self.module.add_function(FirFunction {
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
        self.enum_variants.insert(
            "Ordering".to_string(),
            vec![
                ("Less".to_string(), 0, vec![]),
                ("Equal".to_string(), 1, vec![]),
                ("Greater".to_string(), 2, vec![]),
            ],
        );
    }

    /// Lower an entire module (compiler path).
    pub fn lower_module(&mut self, module: &Module) -> Result<(), LowerError> {
        // First pass: register all top-level function names, class names, and enum names
        for stmt in &module.body {
            match stmt {
                Stmt::Let {
                    name,
                    value:
                        Expr::Lambda {
                            params, defaults, ..
                        },
                    ..
                } => {
                    let id = FunctionId(self.next_function);
                    self.next_function += 1;
                    self.functions.insert(name.clone(), id);
                    // Store defaults for filling in missing args at call sites
                    let param_defaults: Vec<(String, Option<Expr>)> = params
                        .iter()
                        .enumerate()
                        .map(|(i, (pname, _))| (pname.clone(), defaults.get(i).cloned().flatten()))
                        .collect();
                    if param_defaults.iter().any(|(_, d)| d.is_some()) {
                        self.function_defaults.insert(name.clone(), param_defaults);
                    }
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

        // Inject built-in Ordering enum only if some class includes Ord.
        // This ensures synthesize_cmp can reference Ordering constructors
        // without polluting programs that don't use Ord.
        let needs_ordering = module.body.iter().any(|stmt| {
            if let Stmt::Class { includes, .. } = stmt {
                includes
                    .as_ref()
                    .is_some_and(|incls| incls.iter().any(|(name, _)| name == "Ord"))
            } else {
                false
            }
        });
        if needs_ordering && !self.functions.contains_key("Ordering.Less") {
            self.inject_ordering_builtin();
        }

        // Second pass: lower everything
        for stmt in &module.body {
            self.lower_top_level_stmt(stmt)?;
        }

        // If there are top-level exprs or stmts but no user-defined main function,
        // synthesize an entry function that runs globals + control flow + expressions.
        if (!self.top_level_exprs.is_empty() || !self.top_level_stmts.is_empty())
            && self.module.entry.is_none()
        {
            self.synthesize_entry_function()?;
        }

        Ok(())
    }

    /// Synthesize a `__top_main` entry function from accumulated top-level stmts.
    /// Injects the global prelude + top-level control flow in a proper function scope.
    fn synthesize_entry_function(&mut self) -> Result<(), LowerError> {
        let saved_locals = std::mem::take(&mut self.locals);
        let saved_local_types = std::mem::take(&mut self.local_types);
        let saved_local_ast_types = std::mem::take(&mut self.local_ast_types);
        let saved_closure_info = std::mem::take(&mut self.closure_info);
        let saved_next_local = self.next_local;
        let saved_return_type = self.current_return_type.take();
        let saved_cleanup_locals = std::mem::take(&mut self.cleanup_locals);
        let saved_cleanup_scope_stack = std::mem::take(&mut self.cleanup_scope_stack);
        self.next_local = 0;
        self.current_return_type = Some(Type::Void);

        // Inject globals
        let mut body: Vec<FirStmt> = Vec::new();
        let top_level_snapshot: Vec<_> = self
            .top_level_lets
            .iter()
            .map(|(n, t, v)| (n.clone(), t.clone(), v.clone()))
            .collect();
        for (tl_name, tl_ty, tl_value) in top_level_snapshot {
            let local_id = self.alloc_local();
            self.locals.insert(tl_name, local_id);
            self.local_types.insert(local_id, tl_ty.clone());
            body.push(FirStmt::Let {
                name: local_id,
                ty: tl_ty,
                value: tl_value,
            });
        }

        // Lower top-level control flow stmts (for, if, while, assignment)
        let tl_stmts: Vec<_> = self.top_level_stmts.clone();
        for tl_stmt in &tl_stmts {
            let fir_stmt = self.lower_stmt_inner(tl_stmt)?;
            body.append(&mut self.pending_stmts);
            body.push(fir_stmt);
        }

        // Lower top-level bare expressions (say, etc.)
        let tl_exprs: Vec<_> = self.top_level_exprs.clone();
        for tl_expr in &tl_exprs {
            let fir_stmt = self.lower_stmt_inner(tl_expr)?;
            body.append(&mut self.pending_stmts);
            body.push(fir_stmt);
        }

        let id = FunctionId(self.next_function);
        self.next_function += 1;
        let func = FirFunction {
            id,
            name: "__top_main".to_string(),
            params: vec![],
            ret_type: FirType::Void,
            body,
            is_entry: true,
            suspendable: false,
        };
        self.module.add_function(func);
        self.module.entry = Some(id);

        // Restore
        self.locals = saved_locals;
        self.local_types = saved_local_types;
        self.local_ast_types = saved_local_ast_types;
        self.closure_info = saved_closure_info;
        self.next_local = saved_next_local;
        self.current_return_type = saved_return_type;
        self.cleanup_locals = saved_cleanup_locals;
        self.cleanup_scope_stack = saved_cleanup_scope_stack;
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
            suspendable: false,
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
        let saved_return_type = self.current_return_type.take();
        let saved_function_scope_id = self.function_scope_id.take();
        let saved_cleanup_locals = std::mem::take(&mut self.cleanup_locals);
        let saved_cleanup_scope_stack = std::mem::take(&mut self.cleanup_scope_stack);
        let saved_async_scope_stack = std::mem::take(&mut self.async_scope_stack);
        self.next_local = 0;
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

        // Restore outer scope
        self.locals = saved_locals;
        self.local_types = saved_local_types;
        self.local_ast_types = saved_local_ast_types;
        self.closure_info = saved_closure_info;
        self.next_local = saved_next_local;
        self.current_return_type = saved_return_type;
        self.function_scope_id = saved_function_scope_id;
        self.cleanup_locals = saved_cleanup_locals;
        self.cleanup_scope_stack = saved_cleanup_scope_stack;
        self.async_scope_stack = saved_async_scope_stack;

        Ok(id)
    }

    fn lower_class(
        &mut self,
        name: &str,
        fields: &[(String, Type)],
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
        // Parent fields come first (so subclass instances are layout-compatible).
        let mut fir_fields = Vec::new();
        let mut offset = 0usize;

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
        for ancestor_name in &ancestor_chain {
            if let Some(ancestor_info) = self.type_env.get_class(ancestor_name) {
                for (field_name, field_type) in &ancestor_info.fields {
                    let fir_type = self.lower_type(field_type);
                    fir_fields.push((field_name.clone(), fir_type, offset));
                    offset += 8;
                }
            }
        }

        // Then the class's own fields
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
    fn emit_scope_exit(&mut self, scope_id: LocalId) {
        self.pending_stmts.push(FirStmt::Expr(FirExpr::RuntimeCall {
            name: "aster_async_scope_exit".to_string(),
            args: vec![FirExpr::LocalVar(scope_id, FirType::Ptr)],
            ret_ty: FirType::Void,
        }));
    }

    /// Emit cleanup calls for all locals that implement Close or Drop,
    /// in reverse declaration order. Close is called before Drop.
    /// Cleanup calls are pushed to `self.pending_stmts`.
    fn emit_cleanup_calls(&mut self) {
        self.emit_cleanup_calls_since(0);
    }

    /// Emit cleanup calls for locals declared since `scope_start` index
    /// in cleanup_locals. Emits in reverse declaration order.
    fn emit_cleanup_calls_since(&mut self, scope_start: usize) {
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
                } else if matches!(value, Expr::Range { .. }) {
                    // Range expressions always produce Type::Custom("Range", [])
                    self.local_ast_types
                        .insert(name.clone(), Type::Custom("Range".into(), vec![]));
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
                            || class_name == "Mutex"
                            || class_name == "Channel"
                            || class_name == "MultiSend"
                            || class_name == "MultiReceive")
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
            _ => Err(unsupported_stmt(stmt)),
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
                        Ok(FirExpr::Call {
                            func: func_id,
                            args: fir_args,
                            ret_ty,
                        })
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
    fn lower_method_call(
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

    fn to_string_expr(&self, ast_expr: &Expr, fir_expr: FirExpr) -> FirExpr {
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

    fn default_value_for_type(&self, ty: &FirType) -> FirExpr {
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
    fn maybe_wrap_nullable_return(&self, fir_expr: FirExpr, ast_expr: &Expr) -> FirExpr {
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

    fn wrap_nullable_binding(
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
    fn lower_call_args_with_defaults(
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
    fn lower_inline_body(&mut self, body: &[Stmt]) -> Result<FirExpr, LowerError> {
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

    fn lower_for_loop(
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

        // Wrap in If(true, setup_and_loop, []) to return a single statement
        Ok(FirStmt::If {
            cond: FirExpr::BoolLit(true),
            then_body: setup_and_loop,
            else_body: vec![],
        })
    }

    /// Check if lowering a value expression would produce pending stmts
    /// (e.g. iterable method calls, nullable ops, chained method calls).
    fn value_has_pending_stmts(&self, value: &Expr) -> bool {
        match value {
            Expr::Call { func, .. } => {
                if let Expr::Member { object, field, .. } = func.as_ref() {
                    // Iterable vocabulary methods produce loops
                    if matches!(
                        field.as_str(),
                        "map"
                            | "filter"
                            | "find"
                            | "any"
                            | "all"
                            | "reduce"
                            | "count"
                            | "first"
                            | "last"
                            | "to_list"
                            | "min"
                            | "max"
                            | "sort"
                            | "or"
                            | "or_throw"
                    ) {
                        return true;
                    }
                    // Check recursively if the receiver is complex
                    self.value_has_pending_stmts(object)
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    /// Resolve the element type of a list expression from local_ast_types or type_table.
    fn resolve_list_elem_type(&self, expr: &Expr) -> Option<FirType> {
        let ast_ty = match expr {
            Expr::Ident(name, _) => self.local_ast_types.get(name).cloned(),
            _ => self.type_table.get(&expr.span()).cloned(),
        };
        if let Some(Type::List(inner)) = ast_ty {
            Some(self.lower_type(&inner))
        } else {
            None
        }
    }

    /// Resolve the AST type of an expression from local_ast_types or type_table.
    fn resolve_expr_ast_type(&self, expr: &Expr) -> Option<Type> {
        match expr {
            Expr::Ident(name, _) => self.local_ast_types.get(name).cloned(),
            _ => self.type_table.get(&expr.span()).cloned(),
        }
    }

    // -----------------------------------------------------------------------
    // Iterable vocabulary method helpers (list-based)
    // -----------------------------------------------------------------------

    /// Return a FirExpr flag indicating whether list elements are GC pointers.
    /// 1 for Ptr/Struct (need tracing), 0 for value types (Int/Float/Bool).
    fn list_ptr_elems_flag(elem_ty: &FirType) -> FirExpr {
        if matches!(elem_ty, FirType::Ptr | FirType::Struct(_)) {
            FirExpr::IntLit(1)
        } else {
            FirExpr::IntLit(0)
        }
    }

    /// Build a list iteration loop scaffold. Returns (list_id, len_id, idx_id, elem_id).
    /// Caller must fill in the loop body. Setup stmts go into pending_stmts.
    fn iter_loop_scaffold(
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
    fn iter_increment(idx_id: LocalId) -> Vec<FirStmt> {
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
    fn iter_cond(idx_id: LocalId, len_id: LocalId) -> FirExpr {
        FirExpr::BinaryOp {
            left: Box::new(FirExpr::LocalVar(idx_id, FirType::I64)),
            op: BinOp::Lt,
            right: Box::new(FirExpr::LocalVar(len_id, FirType::I64)),
            result_ty: FirType::Bool,
        }
    }

    /// Get element at current index.
    fn iter_get_elem(
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

    /// Lower map, filter, find, any, all — methods that take a single callback `f`.
    fn lower_iterable_with_callback(
        &mut self,
        method: &str,
        fir_list: FirExpr,
        args: &[(String, ast::Span, Expr)],
        elem_ty: &FirType,
        object: &Expr,
    ) -> Result<FirExpr, LowerError> {
        let callback = args
            .iter()
            .find(|(n, _, _)| n == "f")
            .map(|(_, _, e)| e)
            .or_else(|| args.first().map(|(_, _, e)| e));

        let (list_id, len_id, idx_id, elem_id) = self.iter_loop_scaffold(fir_list, elem_ty);

        match method {
            "map" => {
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
                loop_body.push(FirStmt::Expr(FirExpr::RuntimeCall {
                    name: "aster_list_push".to_string(),
                    args: vec![FirExpr::LocalVar(result_id, FirType::Ptr), mapped_val],
                    ret_ty: FirType::Ptr,
                }));

                self.pending_stmts.push(FirStmt::While {
                    cond: Self::iter_cond(idx_id, len_id),
                    body: loop_body,
                    increment: Self::iter_increment(idx_id),
                });
                Ok(FirExpr::LocalVar(result_id, FirType::Ptr))
            }
            "filter" => {
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
            "find" => {
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
            "any" => {
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
            "all" => {
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

    /// Apply an inline lambda to the element variable. Binds the lambda param
    /// to `elem_id` and inlines the lambda body, returning the result expression.
    fn apply_inline_lambda(
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
    fn apply_inline_lambda2(
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
    fn lower_iterable_reduce(
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
    fn lower_iterable_first(
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
    fn lower_iterable_last(
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
    fn lower_iterable_to_list(
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
    fn lower_iterable_min_max(
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

        let cmp_op = if method == "min" {
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
    fn lower_iterable_sort(
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
    fn resolve_iter_type(&self, iter: &Expr) -> Option<Type> {
        if let Expr::Ident(name, _) = iter {
            self.local_ast_types.get(name.as_str()).cloned()
        } else {
            self.type_table.get(&iter.span()).cloned()
        }
    }

    /// Lower `for var in start..end` or `for var in start..=end` to a counting loop.
    fn lower_range_for_loop(
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

        Ok(FirStmt::If {
            cond: FirExpr::BoolLit(true),
            then_body: setup_and_loop,
            else_body: vec![],
        })
    }

    /// Lower `for var in range_var` where range_var is a Range variable.
    /// Extracts start/end/inclusive from the runtime range struct.
    fn lower_range_var_for_loop(
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

        Ok(FirStmt::If {
            cond: FirExpr::BoolLit(true),
            then_body: setup_and_loop,
            else_body: vec![],
        })
    }

    fn resolve_iterator_class(&self, iter: &Expr) -> Option<String> {
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
    fn lower_iterator_for_loop(
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
                } else if let Some(&self_id) = self.locals.get("self") {
                    // Inside a method body — resolve bare field names as self.field
                    let self_expr = Expr::Ident("self".to_string(), expr.span());
                    match self.resolve_field_access(&self_expr, name) {
                        Ok((offset, _ty)) => Ok(FirPlace::Field {
                            object: Box::new(FirExpr::LocalVar(self_id, FirType::Ptr)),
                            offset,
                        }),
                        Err(_) => Err(LowerError::UnboundVariable(name.clone(), expr.span())),
                    }
                } else {
                    Err(LowerError::UnboundVariable(name.clone(), expr.span()))
                }
            }
            Expr::Index { object, index, .. } => {
                let is_map = if let Expr::Ident(name, _) = object.as_ref() {
                    matches!(
                        self.local_ast_types.get(name.as_str()),
                        Some(Type::Map(_, _))
                    )
                } else {
                    false
                };
                let fir_obj = self.lower_expr_ref(object)?;
                let fir_idx = self.lower_expr_ref(index)?;
                if is_map {
                    Ok(FirPlace::MapIndex {
                        map: Box::new(fir_obj),
                        key: Box::new(fir_idx),
                    })
                } else {
                    Ok(FirPlace::Index {
                        list: Box::new(fir_obj),
                        index: Box::new(fir_idx),
                    })
                }
            }
            Expr::Member { object, field, .. } => {
                let fir_obj = self.lower_expr_ref(object)?;
                let (offset, _field_ty) = self.resolve_field_access(object, field)?;
                Ok(FirPlace::Field {
                    object: Box::new(fir_obj),
                    offset,
                })
            }
            _ => Err(LowerError::UnsupportedFeature(
                UnsupportedFeatureKind::Other("complex assignment target".into()),
                expr.span(),
            )),
        }
    }

    /// Lower an expression (non-mutable version for place contexts).
    fn lower_expr_ref(&self, expr: &Expr) -> Result<FirExpr, LowerError> {
        // For now, delegate to a simple version that doesn't allocate locals
        match expr {
            Expr::Int(n, _) => Ok(FirExpr::IntLit(*n)),
            Expr::Str(s, _) => Ok(FirExpr::StringLit(s.clone())),
            Expr::Ident(name, _) => {
                if let Some(&local_id) = self.locals.get(name.as_str()) {
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
            // Support chained member access as object in place context: o.inner.val = x
            Expr::Member { object, field, .. } => {
                let fir_obj = self.lower_expr_ref(object)?;
                let (offset, ty) = self.resolve_field_access(object, field)?;
                Ok(FirExpr::FieldGet {
                    object: Box::new(fir_obj),
                    offset,
                    ty,
                })
            }
            _ => Err(LowerError::UnsupportedFeature(
                UnsupportedFeatureKind::Other("complex expression in place context".into()),
                expr.span(),
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
            Type::Error => {
                debug_assert!(false, "Type::Error should not survive past typechecking");
                FirType::Void
            }
            // Inferred/TypeVar should be resolved via TypeTable before reaching here.
            // The I64 fallback is correct for Int/String/Class (all 64-bit pointers)
            // but wrong for Float (F64) and Bool (I8). Lambda params are resolved
            // in lower_expr's Lambda arm via the TypeTable.
            Type::Inferred => FirType::I64,
            Type::TypeVar(_, _) => FirType::I64,
        }
    }

    fn lower_binop(&self, op: &ast::BinOp) -> BinOp {
        match op {
            ast::BinOp::Add => BinOp::Add,
            ast::BinOp::Sub => BinOp::Sub,
            ast::BinOp::Mul => BinOp::Mul,
            ast::BinOp::Div => BinOp::Div,
            ast::BinOp::Mod => BinOp::Mod,
            ast::BinOp::Pow => BinOp::Add, // unreachable: Pow handled in lower_expr
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
            FirExpr::Spawn { ret_ty, .. } => ret_ty.clone(),
            FirExpr::BlockOn { ret_ty, .. } => ret_ty.clone(),
            FirExpr::ResolveTask { ret_ty, .. } => ret_ty.clone(),
            FirExpr::CancelTask { .. } | FirExpr::WaitCancel { .. } | FirExpr::Safepoint => {
                FirType::Void
            }
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
            FirExpr::IntToFloat(_) => FirType::F64,
        }
    }

    fn infer_binop_type(&self, op: &BinOp, left: &FirExpr, right: &FirExpr) -> FirType {
        match op {
            BinOp::Eq
            | BinOp::Neq
            | BinOp::Lt
            | BinOp::Gt
            | BinOp::Lte
            | BinOp::Gte
            | BinOp::And
            | BinOp::Or => FirType::Bool,
            _ => {
                let lt = self.infer_fir_type(left);
                let rt = self.infer_fir_type(right);
                if lt == FirType::F64 || rt == FirType::F64 {
                    FirType::F64
                } else {
                    lt
                }
            }
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

    /// Resolve the return type of a closure-typed local variable.
    fn resolve_closure_ret_type(&self, name: &str) -> FirType {
        if let Some(Type::Function { ret, .. }) = self.type_env.get_var(name) {
            return self.lower_type(&ret);
        }
        // Check local AST types
        if let Some(Type::Function { ret, .. }) = self.local_ast_types.get(name) {
            return self.lower_type(ret);
        }
        FirType::I64 // fallback
    }

    fn resolve_function_ret_type(&self, name: &str) -> FirType {
        if let Some(Type::Function { ret, .. }) = self.type_env.get_var(name) {
            self.lower_type(&ret)
        } else {
            FirType::Void
        }
    }

    fn lower_explicit_call_args(
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

    fn resolve_called_function_id(&self, func: &Expr) -> Result<FunctionId, LowerError> {
        match func {
            Expr::Ident(name, _) => self
                .functions
                .get(name)
                .copied()
                .ok_or_else(|| LowerError::UnboundVariable(name.clone(), func.span())),
            _ => Err(LowerError::UnsupportedFeature(
                UnsupportedFeatureKind::Other("indirect async/blocking call".into()),
                func.span(),
            )),
        }
    }

    fn resolve_called_function_ret_type(&self, func: &Expr, func_id: FunctionId) -> FirType {
        match func {
            Expr::Ident(name, _) => self.resolve_function_ret_type(name),
            _ => self.resolve_function_ret_type_by_id(func_id),
        }
    }

    fn function_is_suspendable(&self, name: &str) -> bool {
        matches!(
            self.type_env.get_var(name),
            Some(Type::Function {
                suspendable: true,
                ..
            })
        )
    }

    fn resolve_task_result_type(&self, expr: &Expr, task: &FirExpr) -> FirType {
        if let Some(Type::Task(inner_ty)) = self.type_table.get(&expr.span()) {
            return self.lower_type(inner_ty);
        }
        if let Expr::Ident(name, _) = expr {
            if let Some(Type::Task(inner_ty)) = self.local_ast_types.get(name) {
                return self.lower_type(inner_ty);
            }
            if let Some(Type::Task(inner_ty)) = self.type_env.get_var(name) {
                return self.lower_type(&inner_ty);
            }
        }
        self.infer_fir_type(task)
    }

    fn resolve_async_call_ast_type(&self, func: &Expr) -> Option<Type> {
        match func {
            Expr::Ident(name, _) => match self.type_env.get_var(name) {
                Some(Type::Function { ret, .. }) => Some(Type::Task(Box::new((*ret).clone()))),
                _ => None,
            },
            _ => None,
        }
    }

    fn resolve_all_runtime_name(&self, expr: &Expr) -> &'static str {
        match self.task_list_result_type(expr) {
            Some(Type::Float) => "aster_task_resolve_all_f64",
            Some(Type::Bool) => "aster_task_resolve_all_i8",
            _ => "aster_task_resolve_all_i64",
        }
    }

    fn resolve_first_runtime_name(&self, expr: &Expr) -> &'static str {
        match self.task_list_result_type(expr) {
            Some(Type::Float) => "aster_task_resolve_first_f64",
            Some(Type::Bool) => "aster_task_resolve_first_i8",
            _ => "aster_task_resolve_first_i64",
        }
    }

    fn task_list_result_type(&self, expr: &Expr) -> Option<&Type> {
        if let Some(Type::List(inner)) = self.type_table.get(&expr.span())
            && let Type::Task(result) = inner.as_ref()
        {
            return Some(result.as_ref());
        }
        if let Expr::Ident(name, _) = expr
            && let Some(Type::List(inner)) = self.local_ast_types.get(name)
            && let Type::Task(result) = inner.as_ref()
        {
            return Some(result.as_ref());
        }
        None
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
            LowerError::UnsupportedFeature(
                UnsupportedFeatureKind::Other(format!("unknown class: {}", class_name)),
                object.span(),
            )
        })?;

        // Look up the field in the class layout
        let fields = self.class_fields.get(class_id).ok_or_else(|| {
            LowerError::UnsupportedFeature(
                UnsupportedFeatureKind::Other(format!("no field layout for class: {}", class_name)),
                object.span(),
            )
        })?;

        for (fname, fty, foffset) in fields {
            if fname == field {
                return Ok((*foffset, fty.clone()));
            }
        }

        Err(LowerError::UnsupportedFeature(
            UnsupportedFeatureKind::Other(format!(
                "unknown field '{}' on class '{}'",
                field, class_name
            )),
            object.span(),
        ))
    }

    fn is_range_expr(&self, expr: &Expr) -> bool {
        if matches!(expr, Expr::Range { .. }) {
            return true;
        }
        if let Expr::Ident(name, _) = expr {
            if let Some(Type::Custom(class_name, _)) = self.local_ast_types.get(name.as_str())
                && class_name == "Range"
            {
                return true;
            }
            if let Some(Type::Custom(class_name, _)) = self.type_env.get_var(name)
                && class_name == "Range"
            {
                return true;
            }
        }
        matches!(
            self.type_table.get(&expr.span()),
            Some(Type::Custom(name, _)) if name == "Range"
        )
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
                // Inside a method body, bare field names resolve via self
                // e.g. `addr.zip` where `addr` is a field of the current class
                if let Some(Type::Custom(self_class, _)) = self.local_ast_types.get("self") {
                    let self_class = self_class.clone();
                    if let Some(class_info) = self.type_env.get_class(&self_class) {
                        for (fname, ftype) in &class_info.fields {
                            if fname == name
                                && let Type::Custom(field_class, _) = ftype
                                && self.classes.contains_key(field_class.as_str())
                            {
                                return Ok(field_class.clone());
                            }
                        }
                    }
                }
                Err(LowerError::UnsupportedFeature(
                    UnsupportedFeatureKind::Other(format!(
                        "cannot determine class type of variable '{}'",
                        name
                    )),
                    expr.span(),
                ))
            }
            Expr::Call { func, .. } => {
                if let Expr::Ident(name, _) = func.as_ref() {
                    // Constructor call: the function name IS the class name
                    if self.classes.contains_key(name.as_str()) {
                        return Ok(name.clone());
                    }
                    // Function call that returns a class instance: look up return type
                    if let Some(Type::Function { ret, .. }) = self.type_env.get_var(name)
                        && let Type::Custom(class_name, _) = ret.as_ref()
                        && self.classes.contains_key(class_name.as_str())
                    {
                        return Ok(class_name.clone());
                    }
                } else if let Expr::Member { object, field, .. } = func.as_ref() {
                    // Method call chaining: obj.method(args) — look up method's return type
                    if let Ok(class_name) = self.resolve_class_name(object) {
                        // Look up method return type via ClassInfo.methods
                        if let Some(class_info) = self.type_env.get_class(&class_name)
                            && let Some(Type::Function { ret, .. }) =
                                class_info.methods.get(field.as_str())
                            && let Type::Custom(ret_class, _) = ret.as_ref()
                            && self.classes.contains_key(ret_class.as_str())
                        {
                            return Ok(ret_class.clone());
                        }
                        // Fall back: check FIR function registry for return type
                        let qualified = format!("{}.{}", class_name, field);
                        if let Some(ret_ty) = self
                            .functions
                            .get(&qualified)
                            .copied()
                            .map(|fid| self.resolve_function_ret_type_by_id(fid))
                            && let FirType::Struct(class_id) = ret_ty
                        {
                            for (cname, &cid) in &self.classes {
                                if cid == class_id {
                                    return Ok(cname.clone());
                                }
                            }
                        }
                    }
                }
                Err(LowerError::UnsupportedFeature(
                    UnsupportedFeatureKind::Other(
                        "cannot determine class type of call expression".into(),
                    ),
                    expr.span(),
                ))
            }
            // Chained member access: o.inner.field — resolve the field type
            Expr::Member { object, field, .. } => {
                let (_, field_ty) = self.resolve_field_access(object, field)?;
                // field_ty must be a Struct (class instance) for this to be meaningful
                if let FirType::Struct(class_id) = &field_ty {
                    // Find the class name from the id
                    for (cname, &cid) in &self.classes {
                        if cid == *class_id {
                            return Ok(cname.clone());
                        }
                    }
                }
                // Fall back: check the FIR type's associated class info via type env
                // Walk the parent object chain to get the field's declared type
                let parent_class = self.resolve_class_name(object)?;
                if let Some(class_info) = self.type_env.get_class(&parent_class) {
                    for (fname, ftype) in &class_info.fields {
                        if fname == field
                            && let Type::Custom(class_name, _) = ftype
                        {
                            return Ok(class_name.clone());
                        }
                    }
                }
                Err(LowerError::UnsupportedFeature(
                    UnsupportedFeatureKind::Other(
                        "cannot determine class type of expression".into(),
                    ),
                    expr.span(),
                ))
            }
            // List index: points[i] — element type from list's AST type
            Expr::Index { object, .. } => {
                if let Expr::Ident(name, _) = object.as_ref() {
                    let elem_class = match self.local_ast_types.get(name.as_str()) {
                        Some(Type::List(inner)) => {
                            if let Type::Custom(class_name, _) = inner.as_ref() {
                                Some(class_name.clone())
                            } else {
                                None
                            }
                        }
                        Some(Type::Map(_, val_ty)) => {
                            if let Type::Custom(class_name, _) = val_ty.as_ref() {
                                Some(class_name.clone())
                            } else {
                                None
                            }
                        }
                        _ => None,
                    };
                    if let Some(class_name) = elem_class
                        && self.classes.contains_key(class_name.as_str())
                    {
                        return Ok(class_name);
                    }
                }
                Err(LowerError::UnsupportedFeature(
                    UnsupportedFeatureKind::Other(
                        "cannot determine class type of expression".into(),
                    ),
                    expr.span(),
                ))
            }
            _ => Err(LowerError::UnsupportedFeature(
                UnsupportedFeatureKind::Other("cannot determine class type of expression".into()),
                expr.span(),
            )),
        }
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

    /// Synthesize a FIR function body for auto-derived `to_string`.
    /// Produces: "ClassName(field1_str, field2_str, ...)"
    fn synthesize_to_string(
        &mut self,
        class_name: &str,
        class_id: ClassId,
    ) -> Result<(), LowerError> {
        let qualified = format!("{}.to_string", class_name);
        let func_id = FunctionId(self.next_function);
        self.next_function += 1;
        self.functions.insert(qualified.clone(), func_id);

        let field_layout = self
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
        self.module.add_function(func);
        Ok(())
    }

    /// Synthesize an auto-derived `eq` method for a class.
    /// Compares all fields pairwise: self.f1 == other.f1 and self.f2 == other.f2 ...
    fn synthesize_eq(&mut self, class_name: &str, class_id: ClassId) -> Result<(), LowerError> {
        let qualified = format!("{}.eq", class_name);
        let func_id = FunctionId(self.next_function);
        self.next_function += 1;
        self.functions.insert(qualified.clone(), func_id);

        let field_layout = self
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
        self.module.add_function(func);
        Ok(())
    }

    /// Synthesize an auto-derived `cmp` method for a class.
    /// Compares fields in order, returning an Ordering enum variant.
    fn synthesize_cmp(&mut self, class_name: &str, class_id: ClassId) -> Result<(), LowerError> {
        let qualified = format!("{}.cmp", class_name);
        let func_id = FunctionId(self.next_function);
        self.next_function += 1;
        self.functions.insert(qualified.clone(), func_id);

        let field_layout = self
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
            if let Some(&ctor_func_id) = self.functions.get(match tag {
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
        self.module.add_function(func);
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
        let saved_return_type = self.current_return_type.take();
        let saved_function_scope_id = self.function_scope_id.take();
        let saved_cleanup_locals = std::mem::take(&mut self.cleanup_locals);
        let saved_cleanup_scope_stack = std::mem::take(&mut self.cleanup_scope_stack);
        let saved_async_scope_stack = std::mem::take(&mut self.async_scope_stack);
        self.next_local = 0;
        self.current_return_type = Some(ret_type.clone());

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

        // Implicit task scope for lambda (same as functions)
        let scope_id = self.alloc_local();
        self.local_types.insert(scope_id, FirType::Ptr);
        self.async_scope_stack.push(scope_id);
        self.function_scope_id = Some(scope_id);

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
            let local_id = match self.locals.get(cap_name) {
                Some(&id) => id,
                None => {
                    return Err(LowerError::UnboundVariable(
                        format!("closure capture '{}'", cap_name),
                        Span::dummy(),
                    ));
                }
            };
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
            self.emit_scope_exit(scope_id);
            fir_body.append(&mut self.pending_stmts);
            fir_body.push(FirStmt::Return(expr));
        } else {
            // Void lambda or no trailing expr — emit scope exit at end
            self.emit_scope_exit(scope_id);
            fir_body.append(&mut self.pending_stmts);
        }

        self.async_scope_stack.pop();

        // Prepend scope_enter
        fir_body.insert(
            0,
            FirStmt::Let {
                name: scope_id,
                ty: FirType::Ptr,
                value: FirExpr::RuntimeCall {
                    name: "aster_async_scope_enter".to_string(),
                    args: vec![],
                    ret_ty: FirType::Ptr,
                },
            },
        );

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
            suspendable: false,
        };
        self.module.add_function(func);

        // Restore outer scope
        self.locals = saved_locals;
        self.local_types = saved_local_types;
        self.local_ast_types = saved_local_ast_types;
        self.closure_info = saved_closure_info;
        self.next_local = saved_next_local;
        self.current_return_type = saved_return_type;
        self.function_scope_id = saved_function_scope_id;
        self.cleanup_locals = saved_cleanup_locals;
        self.cleanup_scope_stack = saved_cleanup_scope_stack;
        self.async_scope_stack = saved_async_scope_stack;

        // Re-register the function name
        self.functions.insert(lambda_name, func_id);

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

            Ok(FirExpr::ClosureCreate {
                func: func_id,
                env: Box::new(FirExpr::LocalVar(env_id, FirType::Ptr)),
                ret_ty: self.lower_type(ret_type),
            })
        }
    }

    /// Lower a string interpolation to a chain of to_string + concat calls.
    fn lower_string_interpolation(
        &mut self,
        parts: &[ast::StringPart],
    ) -> Result<FirExpr, LowerError> {
        // Convert each part to a string FirExpr, then fold-concat them.
        let mut string_exprs = Vec::new();

        for part in parts {
            match part {
                ast::StringPart::Literal(s) => {
                    string_exprs.push(FirExpr::StringLit(s.clone()));
                }
                ast::StringPart::Expr(expr) => {
                    let fir_expr = self.lower_expr(expr)?;
                    let fir_ty = self.infer_fir_type(&fir_expr);
                    // Convert to string based on type
                    let str_expr = match fir_ty {
                        FirType::Ptr => {
                            // Check if this is a List (needs runtime to_string)
                            let ast_ty = self.resolve_expr_ast_type(expr);
                            if matches!(ast_ty.as_ref(), Some(Type::List(_))) {
                                FirExpr::RuntimeCall {
                                    name: "aster_list_to_string".to_string(),
                                    args: vec![fir_expr],
                                    ret_ty: FirType::Ptr,
                                }
                            // Check if this is a class instance (needs to_string call)
                            } else if let Ok(class_name) = self.resolve_class_name(expr) {
                                let qualified = format!("{}.to_string", class_name);
                                if let Some(&func_id) = self.functions.get(&qualified) {
                                    FirExpr::Call {
                                        func: func_id,
                                        args: vec![fir_expr],
                                        ret_ty: FirType::Ptr,
                                    }
                                } else {
                                    fir_expr // no to_string lowered, pass through
                                }
                            } else {
                                fir_expr // plain string or unknown — pass through
                            }
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
                        _ => fir_expr, // fallback: pass through
                    };
                    string_exprs.push(str_expr);
                }
            }
        }

        // If empty, return empty string
        if string_exprs.is_empty() {
            return Ok(FirExpr::StringLit(String::new()));
        }

        // If single part, return it directly
        if string_exprs.len() == 1 {
            return Ok(string_exprs.into_iter().next().unwrap());
        }

        // Fold left with aster_string_concat
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
            _ => {}
        }
    }
}
