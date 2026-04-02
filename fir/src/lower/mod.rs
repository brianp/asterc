use std::collections::HashMap;

use ast::Span;
use ast::type_env::{Binding, TypeEnv};
use ast::type_table::TypeTable;
use ast::{EnumVariant, Expr, MatchPattern, Module, Stmt, Type};

use crate::builtins::{class as builtin_class, method as builtin_method};
use crate::exprs::{BinOp, FirExpr, UnaryOp};
use crate::module::{FirClass, FirFunction, FirModule};
use crate::stmts::{FirPlace, FirStmt};
use crate::types::{ClassId, FirType, FunctionId, LocalId};

mod closure;
mod expr;
mod for_loop;
mod introspection;
mod iterable;
mod match_lower;
mod method;
mod stmt;
mod synthesize;

/// Cached FIR data from a compiled imported module.
/// Stored alongside `ModuleExports` in the module loader cache.
#[derive(Debug, Clone)]
pub struct FirCache {
    /// All FIR functions compiled from this module (indexed by FunctionId).
    pub functions: Vec<FirFunction>,
    /// All FIR class layouts from this module.
    pub classes: Vec<FirClass>,
    /// Name-to-FunctionId mapping (e.g. "double", "Greeter.greet").
    pub function_names: HashMap<String, FunctionId>,
    /// Name-to-ClassId mapping (e.g. "Greeter").
    pub class_names: HashMap<String, ClassId>,
    /// Class field layouts.
    pub class_fields: HashMap<ClassId, Vec<(String, FirType, usize)>>,
    /// Enum variant metadata.
    #[allow(clippy::type_complexity)]
    pub enum_variants: HashMap<String, Vec<(String, i64, Vec<(String, FirType)>)>>,
    /// Default parameter values for functions/methods.
    #[allow(clippy::type_complexity)]
    pub function_defaults: HashMap<String, Vec<(String, Option<Expr>)>>,
}

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

/// Per-function-scope state in the Lowerer. Saved and restored when entering
/// nested function/lambda scopes.
#[derive(Default)]
pub(super) struct ScopeState {
    pub(super) locals: HashMap<String, LocalId>,
    pub(super) local_types: HashMap<LocalId, FirType>,
    pub(super) local_ast_types: HashMap<String, Type>,
    pub(super) closure_info: HashMap<String, (FunctionId, Option<LocalId>, Vec<String>)>,
    pub(super) next_local: u32,
    pub(super) current_return_type: Option<Type>,
    pub(super) function_scope_id: Option<LocalId>,
    pub(super) cleanup_locals: Vec<(LocalId, String, bool, bool)>,
    pub(super) cleanup_scope_stack: Vec<usize>,
    pub(super) async_scope_stack: Vec<LocalId>,
}

/// Module-level state accumulated while lowering.
pub(super) struct ModuleState {
    pub(super) module: FirModule,
    pub(super) functions: HashMap<String, FunctionId>,
    pub(super) classes: HashMap<String, ClassId>,
    pub(super) class_fields: HashMap<ClassId, Vec<(String, FirType, usize)>>,
    #[allow(clippy::type_complexity)]
    pub(super) enum_variants: HashMap<String, Vec<(String, i64, Vec<(String, FirType)>)>>,
    #[allow(clippy::type_complexity)]
    pub(super) function_defaults: HashMap<String, Vec<(String, Option<Expr>)>>,
    pub(super) next_function: u32,
    pub(super) next_class: u32,
}

/// Top-level (module-scope) statement and binding state.
pub(super) struct TopLevelState {
    pub(super) top_level_lets: Vec<(String, FirType, FirExpr)>,
    pub(super) top_level_stmts: Vec<Stmt>,
    pub(super) top_level_exprs: Vec<Stmt>,
    pub(super) globals: HashMap<String, LocalId>,
}

/// Per-variable entry in the eval env layout: name, FIR type, AST type.
pub struct EvalEnvEntry {
    pub name: String,
    pub fir_ty: FirType,
    pub ast_ty: Type,
}

pub struct Lowerer {
    pub(super) type_env: TypeEnv,
    pub(super) type_table: TypeTable,
    pub(super) scope: ScopeState,
    pub(super) ms: ModuleState,
    pub(super) tl: TopLevelState,
    pub(super) pending_stmts: Vec<FirStmt>,
    /// When set, the entry function receives `__eval_env: Ptr` as first param
    /// and locals are loaded from the env struct at known offsets.
    pub(super) eval_env: Option<Vec<EvalEnvEntry>>,
}

impl Lowerer {
    pub fn new(type_env: TypeEnv, type_table: TypeTable) -> Self {
        use crate::types::{ClassId, FirType};

        // Pre-register built-in introspection classes so field access works.
        // Use sentinel ClassIds (high values) that won't collide with user classes.
        // Layouts must match runtime construction (ptr fields first, then values).
        let mut classes = HashMap::new();
        let mut class_fields: HashMap<ClassId, Vec<(String, FirType, usize)>> = HashMap::new();

        // FieldInfo: name(ptr,0), type_name(ptr,8), is_public(val,16)
        let fi_id = ClassId(u32::MAX);
        classes.insert("FieldInfo".to_string(), fi_id);
        class_fields.insert(
            fi_id,
            vec![
                ("name".into(), FirType::Ptr, 0),
                ("type_name".into(), FirType::Ptr, 8),
                ("is_public".into(), FirType::Bool, 16),
            ],
        );

        // ParamInfo: name(ptr,0), param_type(ptr,8), has_default(val,16)
        let pi_id = ClassId(u32::MAX - 1);
        classes.insert("ParamInfo".to_string(), pi_id);
        class_fields.insert(
            pi_id,
            vec![
                ("name".into(), FirType::Ptr, 0),
                ("param_type".into(), FirType::Ptr, 8),
                ("has_default".into(), FirType::Bool, 16),
            ],
        );

        // MethodInfo: name(ptr,0), params(ptr,8), return_type(ptr,16), is_public(val,24)
        let mi_id = ClassId(u32::MAX - 2);
        classes.insert("MethodInfo".to_string(), mi_id);
        class_fields.insert(
            mi_id,
            vec![
                ("name".into(), FirType::Ptr, 0),
                ("params".into(), FirType::Ptr, 8),
                ("return_type".into(), FirType::Ptr, 16),
                ("is_public".into(), FirType::Bool, 24),
            ],
        );

        Self {
            type_env,
            type_table,
            scope: ScopeState::default(),
            ms: ModuleState {
                module: FirModule::new(),
                functions: HashMap::new(),
                classes,
                class_fields,
                enum_variants: HashMap::new(),
                function_defaults: HashMap::new(),
                next_function: 0,
                next_class: 0,
            },
            tl: TopLevelState {
                top_level_lets: Vec::new(),
                top_level_stmts: Vec::new(),
                top_level_exprs: Vec::new(),
                globals: HashMap::new(),
            },
            pending_stmts: Vec::new(),
            eval_env: None,
        }
    }

    /// Save per-function scope state and reset for a nested scope.
    fn save_scope(&mut self) -> ScopeState {
        std::mem::take(&mut self.scope)
    }

    /// Restore per-function scope state from a previous snapshot.
    fn restore_scope(&mut self, saved: ScopeState) {
        self.scope = saved;
    }

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
                    let id = FunctionId(self.ms.next_function);
                    self.ms.next_function += 1;
                    self.ms.functions.insert(name.clone(), id);
                    // Store defaults for filling in missing args at call sites
                    let param_defaults: Vec<(String, Option<Expr>)> = params
                        .iter()
                        .enumerate()
                        .map(|(i, (pname, _))| (pname.clone(), defaults.get(i).cloned().flatten()))
                        .collect();
                    if param_defaults.iter().any(|(_, d)| d.is_some()) {
                        self.ms
                            .function_defaults
                            .insert(name.clone(), param_defaults);
                    }
                }
                Stmt::Class { name, .. } => {
                    let id = ClassId(self.ms.next_class);
                    self.ms.next_class += 1;
                    self.ms.classes.insert(name.clone(), id);
                }
                Stmt::Trait { methods, .. } => {
                    // Register default method implementations (non-empty bodies)
                    for m in methods {
                        if let Stmt::Let {
                            name: mname,
                            value: Expr::Lambda { body, .. },
                            ..
                        } = m
                            && !body.is_empty()
                        {
                            let id = FunctionId(self.ms.next_function);
                            self.ms.next_function += 1;
                            self.ms.functions.insert(mname.clone(), id);
                        }
                    }
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
                    self.ms.enum_variants.insert(name.clone(), variant_info);
                    // Register variant constructors as functions (will be defined in lower_enum)
                    for v in variants {
                        let id = FunctionId(self.ms.next_function);
                        self.ms.next_function += 1;
                        let ctor_name = format!("{}.{}", name, v.name);
                        self.ms.functions.insert(ctor_name, id);
                    }
                }
                _ => {}
            }
        }

        // Inject ProcessResult builtin class if std/process is imported.
        let needs_process_result = module.body.iter().any(|stmt| {
            if let Stmt::Use { path, .. } = stmt {
                path.len() == 2 && path[0] == "std" && path[1] == "process"
            } else {
                false
            }
        });
        if needs_process_result && !self.ms.classes.contains_key("ProcessResult") {
            self.inject_process_result_builtin();
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
        if needs_ordering && !self.ms.functions.contains_key("Ordering.Less") {
            self.inject_ordering_builtin();
        }

        // Second pass: lower everything
        for stmt in &module.body {
            self.lower_top_level_stmt(stmt)?;
        }

        // If there are top-level exprs or stmts but no user-defined main function,
        // synthesize an entry function that runs globals + control flow + expressions.
        if (!self.tl.top_level_exprs.is_empty() || !self.tl.top_level_stmts.is_empty())
            && self.ms.module.entry.is_none()
        {
            self.synthesize_entry_function()?;
        }

        Ok(())
    }

    pub fn lower_stmt(&mut self, stmt: &Stmt) -> Result<(), LowerError> {
        self.lower_top_level_stmt(stmt)
    }

    /// Lower a bare expression (REPL path). Wraps in a temporary function,
    /// returns its FunctionId for immediate execution.
    pub fn lower_repl_expr(&mut self, expr: &Expr, ty: &Type) -> Result<FunctionId, LowerError> {
        let ret_type = self.lower_type(ty);
        let fir_expr = self.lower_expr(expr)?;
        let id = FunctionId(self.ms.next_function);
        self.ms.next_function += 1;
        let func = FirFunction {
            id,
            name: format!("__repl_expr_{}", id.0),
            params: vec![],
            ret_type,
            body: vec![FirStmt::Return(fir_expr)],
            is_entry: true,
            suspendable: false,
        };
        self.ms.module.add_function(func);
        Ok(id)
    }

    /// Export the FIR cache from this lowerer (after lowering a module).
    /// Used to cache imported module FIR data for merging into the main module.
    pub fn export_cache(&self) -> FirCache {
        FirCache {
            functions: self.ms.module.functions.clone(),
            classes: self.ms.module.classes.clone(),
            function_names: self.ms.functions.clone(),
            class_names: self.ms.classes.clone(),
            class_fields: self.ms.class_fields.clone(),
            enum_variants: self.ms.enum_variants.clone(),
            function_defaults: self.ms.function_defaults.clone(),
        }
    }

    /// Merge imported module FIR data into this lowerer's state.
    /// Builds per-ID remap tables that handle deduplication: if a function
    /// or class with the same name already exists, references are remapped
    /// to the existing ID instead of creating a duplicate.
    pub fn merge_imported(&mut self, cache: &FirCache) {
        // Build lookup tables for existing functions/classes by name
        let existing_funcs: HashMap<String, FunctionId> = self
            .ms
            .module
            .functions
            .iter()
            .filter(|f| !f.name.is_empty())
            .map(|f| (f.name.clone(), f.id))
            .collect();
        let existing_classes: HashMap<String, ClassId> = self
            .ms
            .module
            .classes
            .iter()
            .map(|c| (c.name.clone(), c.id))
            .collect();

        // Use module array length (not next_* counters) as offsets, since
        // next_* includes pre-registered IDs that haven't been added to the module array yet.
        let func_offset = self.ms.module.functions.len() as u32;
        let class_offset = self.ms.module.classes.len() as u32;

        // Build remap tables: for each old ID in the cache, determine the new ID.
        // Duplicates map to the already-existing ID; new items get sequential IDs
        // starting from the current class/function count (not old_id + offset, since
        // skipping duplicates would leave gaps in the sequential ID space).
        let mut func_remap: HashMap<u32, FunctionId> = HashMap::new();
        let mut class_remap: HashMap<u32, ClassId> = HashMap::new();

        let mut next_func_id = func_offset;
        for func in &cache.functions {
            if !func.name.is_empty()
                && let Some(&existing_id) = existing_funcs.get(&func.name)
            {
                // Duplicate: map to existing
                func_remap.insert(func.id.0, existing_id);
                continue;
            }
            // New function: assign next sequential ID
            func_remap.insert(func.id.0, FunctionId(next_func_id));
            next_func_id += 1;
        }

        let mut next_class_id = class_offset;
        for class in &cache.classes {
            if let Some(&existing_id) = existing_classes.get(&class.name) {
                class_remap.insert(class.id.0, existing_id);
                continue;
            }
            if class.id.0 >= u32::MAX - 10 {
                class_remap.insert(class.id.0, class.id);
            } else {
                // New class: assign next sequential ID
                class_remap.insert(class.id.0, ClassId(next_class_id));
                next_class_id += 1;
            }
        }

        let resolve_func = |old: FunctionId| -> FunctionId {
            func_remap
                .get(&old.0)
                .copied()
                .unwrap_or(FunctionId(old.0 + func_offset))
        };
        let resolve_class = |old: ClassId| -> ClassId {
            if old.0 >= u32::MAX - 10 {
                return old;
            }
            class_remap
                .get(&old.0)
                .copied()
                .unwrap_or(ClassId(old.0 + class_offset))
        };

        // Merge functions (skip duplicates)
        for func in &cache.functions {
            if !func.name.is_empty() && existing_funcs.contains_key(&func.name) {
                continue;
            }
            let new_id = resolve_func(func.id);
            let mut new_func = func.clone();
            new_func.id = new_id;
            Self::remap_stmts_with(
                &mut new_func.body,
                &func_remap,
                &class_remap,
                func_offset,
                class_offset,
            );
            self.ms.module.add_function(new_func);
        }

        // Merge classes (skip duplicates)
        for class in &cache.classes {
            if existing_classes.contains_key(&class.name) {
                continue;
            }
            let new_id = resolve_class(class.id);
            let mut new_class = class.clone();
            new_class.id = new_id;
            new_class.methods = class.methods.iter().map(|&f| resolve_func(f)).collect();
            new_class.vtable = class
                .vtable
                .iter()
                .map(|(name, fid)| (name.clone(), resolve_func(*fid)))
                .collect();
            new_class.parent = class.parent.map(&resolve_class);
            self.ms.module.add_class(new_class);
        }

        // Merge name mappings (skip if already present)
        for (name, &func_id) in &cache.function_names {
            let new_id = resolve_func(func_id);
            self.ms.functions.entry(name.clone()).or_insert(new_id);
        }

        for (name, &class_id) in &cache.class_names {
            let new_id = resolve_class(class_id);
            self.ms.classes.entry(name.clone()).or_insert(new_id);
        }

        // Merge class_fields with remapped ClassIds
        for (&class_id, fields) in &cache.class_fields {
            let new_id = resolve_class(class_id);
            self.ms
                .class_fields
                .entry(new_id)
                .or_insert_with(|| fields.clone());
        }

        // Merge enum variants
        for (name, variants) in &cache.enum_variants {
            self.ms
                .enum_variants
                .entry(name.clone())
                .or_insert_with(|| variants.clone());
        }

        // Merge function defaults
        for (name, defaults) in &cache.function_defaults {
            self.ms
                .function_defaults
                .entry(name.clone())
                .or_insert_with(|| defaults.clone());
        }

        // Update counters to reflect the actual module array lengths after merging.
        // This ensures the next lowering pass assigns correct IDs.
        self.ms.next_function = self
            .ms
            .next_function
            .max(self.ms.module.functions.len() as u32);
        self.ms.next_class = self.ms.next_class.max(self.ms.module.classes.len() as u32);
    }

    /// Recursively remap FunctionId and ClassId references in FIR statements
    /// using per-ID remap tables for dedup-aware remapping.
    fn remap_stmts_with(
        stmts: &mut [FirStmt],
        func_remap: &HashMap<u32, FunctionId>,
        class_remap: &HashMap<u32, ClassId>,
        func_offset: u32,
        class_offset: u32,
    ) {
        for stmt in stmts.iter_mut() {
            Self::remap_stmt_with(stmt, func_remap, class_remap, func_offset, class_offset);
        }
    }

    fn remap_stmt_with(
        stmt: &mut FirStmt,
        fr: &HashMap<u32, FunctionId>,
        cr: &HashMap<u32, ClassId>,
        fo: u32,
        co: u32,
    ) {
        match stmt {
            FirStmt::Let { value, .. } => {
                Self::remap_expr_with(value, fr, cr, fo, co);
            }
            FirStmt::Assign { target, value } => {
                Self::remap_place_with(target, fr, cr, fo, co);
                Self::remap_expr_with(value, fr, cr, fo, co);
            }
            FirStmt::Return(expr) | FirStmt::Expr(expr) => {
                Self::remap_expr_with(expr, fr, cr, fo, co);
            }
            FirStmt::If {
                cond,
                then_body,
                else_body,
            } => {
                Self::remap_expr_with(cond, fr, cr, fo, co);
                Self::remap_stmts_with(then_body, fr, cr, fo, co);
                Self::remap_stmts_with(else_body, fr, cr, fo, co);
            }
            FirStmt::While {
                cond,
                body,
                increment,
            } => {
                Self::remap_expr_with(cond, fr, cr, fo, co);
                Self::remap_stmts_with(body, fr, cr, fo, co);
                Self::remap_stmts_with(increment, fr, cr, fo, co);
            }
            FirStmt::Block(body) => {
                Self::remap_stmts_with(body, fr, cr, fo, co);
            }
            FirStmt::Break | FirStmt::Continue | FirStmt::NoOp => {}
        }
    }

    fn remap_place_with(
        place: &mut FirPlace,
        fr: &HashMap<u32, FunctionId>,
        cr: &HashMap<u32, ClassId>,
        fo: u32,
        co: u32,
    ) {
        match place {
            FirPlace::Local(_) => {}
            FirPlace::Field { object, .. } => {
                Self::remap_expr_with(object, fr, cr, fo, co);
            }
            FirPlace::Index { list, index } => {
                Self::remap_expr_with(list, fr, cr, fo, co);
                Self::remap_expr_with(index, fr, cr, fo, co);
            }
            FirPlace::MapIndex { map, key } => {
                Self::remap_expr_with(map, fr, cr, fo, co);
                Self::remap_expr_with(key, fr, cr, fo, co);
            }
        }
    }

    /// Resolve a FunctionId using the remap table, falling back to offset.
    fn resolve_fid(fid: FunctionId, fr: &HashMap<u32, FunctionId>, fo: u32) -> FunctionId {
        fr.get(&fid.0).copied().unwrap_or(FunctionId(fid.0 + fo))
    }

    /// Resolve a ClassId using the remap table, falling back to offset.
    fn resolve_cid(cid: ClassId, cr: &HashMap<u32, ClassId>, co: u32) -> ClassId {
        if cid.0 >= u32::MAX - 10 {
            return cid;
        }
        cr.get(&cid.0).copied().unwrap_or(ClassId(cid.0 + co))
    }

    fn remap_expr_with(
        expr: &mut FirExpr,
        fr: &HashMap<u32, FunctionId>,
        cr: &HashMap<u32, ClassId>,
        fo: u32,
        co: u32,
    ) {
        match expr {
            FirExpr::Call { func, args, .. } => {
                *func = Self::resolve_fid(*func, fr, fo);
                for arg in args.iter_mut() {
                    Self::remap_expr_with(arg, fr, cr, fo, co);
                }
            }
            FirExpr::Spawn { func, args, .. } => {
                *func = Self::resolve_fid(*func, fr, fo);
                for arg in args.iter_mut() {
                    Self::remap_expr_with(arg, fr, cr, fo, co);
                }
            }
            FirExpr::BlockOn { func, args, .. } => {
                *func = Self::resolve_fid(*func, fr, fo);
                for arg in args.iter_mut() {
                    Self::remap_expr_with(arg, fr, cr, fo, co);
                }
            }
            FirExpr::Construct { class, fields, .. } => {
                *class = Self::resolve_cid(*class, cr, co);
                for field_val in fields.iter_mut() {
                    Self::remap_expr_with(field_val, fr, cr, fo, co);
                }
            }
            FirExpr::FieldGet { object, .. } => {
                Self::remap_expr_with(object, fr, cr, fo, co);
            }
            FirExpr::FieldSet { object, value, .. } => {
                Self::remap_expr_with(object, fr, cr, fo, co);
                Self::remap_expr_with(value, fr, cr, fo, co);
            }
            FirExpr::BinaryOp { left, right, .. } => {
                Self::remap_expr_with(left, fr, cr, fo, co);
                Self::remap_expr_with(right, fr, cr, fo, co);
            }
            FirExpr::UnaryOp { operand, .. } => {
                Self::remap_expr_with(operand, fr, cr, fo, co);
            }
            FirExpr::RuntimeCall { args, .. } => {
                for arg in args.iter_mut() {
                    Self::remap_expr_with(arg, fr, cr, fo, co);
                }
            }
            FirExpr::ListNew { elements, .. } => {
                for elem in elements.iter_mut() {
                    Self::remap_expr_with(elem, fr, cr, fo, co);
                }
            }
            FirExpr::ListGet { list, index, .. } => {
                Self::remap_expr_with(list, fr, cr, fo, co);
                Self::remap_expr_with(index, fr, cr, fo, co);
            }
            FirExpr::ListSet {
                list, index, value, ..
            } => {
                Self::remap_expr_with(list, fr, cr, fo, co);
                Self::remap_expr_with(index, fr, cr, fo, co);
                Self::remap_expr_with(value, fr, cr, fo, co);
            }
            FirExpr::TagWrap { value, .. } => {
                Self::remap_expr_with(value, fr, cr, fo, co);
            }
            FirExpr::TagUnwrap { value, .. } => {
                Self::remap_expr_with(value, fr, cr, fo, co);
            }
            FirExpr::TagCheck { value, .. } => {
                Self::remap_expr_with(value, fr, cr, fo, co);
            }
            FirExpr::ClosureCreate { func, env, .. } => {
                *func = Self::resolve_fid(*func, fr, fo);
                Self::remap_expr_with(env, fr, cr, fo, co);
            }
            FirExpr::ClosureCall { closure, args, .. } => {
                Self::remap_expr_with(closure, fr, cr, fo, co);
                for arg in args.iter_mut() {
                    Self::remap_expr_with(arg, fr, cr, fo, co);
                }
            }
            FirExpr::EnvLoad { env, .. } => {
                Self::remap_expr_with(env, fr, cr, fo, co);
            }
            FirExpr::GlobalFunc(fid) => {
                *fid = Self::resolve_fid(*fid, fr, fo);
            }
            FirExpr::ResolveTask { task, .. } => {
                Self::remap_expr_with(task, fr, cr, fo, co);
            }
            FirExpr::CancelTask { task } => {
                Self::remap_expr_with(task, fr, cr, fo, co);
            }
            FirExpr::WaitCancel { task } => {
                Self::remap_expr_with(task, fr, cr, fo, co);
            }
            FirExpr::IntToFloat(inner) => {
                Self::remap_expr_with(inner, fr, cr, fo, co);
            }
            FirExpr::Bitcast { value, .. } => {
                Self::remap_expr_with(value, fr, cr, fo, co);
            }
            FirExpr::EvalCall { code, env, .. } => {
                Self::remap_expr_with(code, fr, cr, fo, co);
                if let Some(e) = env {
                    Self::remap_expr_with(e, fr, cr, fo, co);
                }
            }
            FirExpr::IntLit(_)
            | FirExpr::FloatLit(_)
            | FirExpr::BoolLit(_)
            | FirExpr::StringLit(_)
            | FirExpr::NilLit
            | FirExpr::LocalVar(_, _)
            | FirExpr::Safepoint => {}
        }
    }

    /// Build a [`ContextSnapshot`] of the current scope for an `evaluate()` call site.
    ///
    /// Captures local variables, current class context (if inside a method),
    /// and available function signatures.
    pub(crate) fn capture_context_snapshot(&self) -> ast::ContextSnapshot {
        use ast::context_snapshot::{ContextSnapshot, SnapshotClassInfo, SnapshotDynamicReceiver};

        // Determine if we're inside a class method by checking for "self" parameter
        let (current_class, class_info) =
            if let Some(Type::Custom(class_name, _)) = self.scope.local_ast_types.get("self") {
                let ci = self
                    .type_env
                    .get_class(class_name)
                    .map(|info| SnapshotClassInfo {
                        fields: info
                            .fields
                            .iter()
                            .map(|(n, t)| (n.clone(), t.clone()))
                            .collect(),
                        methods: info.methods.clone(),
                        dynamic_receiver: info.dynamic_receiver.as_ref().map(|dr| {
                            SnapshotDynamicReceiver {
                                args_value_ty: dr.args_value_ty.clone(),
                                return_ty: dr.return_ty.clone(),
                                known_names: dr
                                    .known_names
                                    .as_ref()
                                    .map(|s| s.iter().cloned().collect()),
                            }
                        }),
                    });
                (Some(class_name.clone()), ci)
            } else {
                (None, None)
            };

        // Capture local variable types (excluding "self" which is in class_info).
        // Prefer local_ast_types (exact AST type) but fall back to reconstructing
        // from FirType for variables whose AST type wasn't tracked (e.g. simple
        // literals like `let x = 10`).
        let mut variables: std::collections::HashMap<String, Type> =
            std::collections::HashMap::new();
        for (name, &local_id) in &self.scope.locals {
            if name == "self" {
                continue;
            }
            if let Some(ast_ty) = self.scope.local_ast_types.get(name) {
                variables.insert(name.clone(), ast_ty.clone());
            } else if let Some(fir_ty) = self.scope.local_types.get(&local_id)
                && let Some(approx) = Self::fir_type_to_ast_approx(fir_ty)
            {
                variables.insert(name.clone(), approx);
            }
        }

        // Capture available function signatures from the type environment
        let functions: std::collections::HashMap<String, Type> = self
            .type_env
            .all_var_names()
            .into_iter()
            .filter_map(|name| {
                let ty = self.type_env.get_var_type(name)?;
                if matches!(ty, Type::Function { .. }) {
                    Some((name.to_string(), ty.clone()))
                } else {
                    None
                }
            })
            .collect();

        ContextSnapshot {
            current_class,
            class_info,
            variables,
            functions,
            env_layout: None,
        }
    }

    /// Best-effort conversion of a FirType back to an AST Type.
    /// Returns None for ambiguous types (e.g. Ptr could be String, List, or Class).
    fn fir_type_to_ast_approx(fir_ty: &FirType) -> Option<Type> {
        match fir_ty {
            FirType::I64 => Some(Type::Int),
            FirType::F64 => Some(Type::Float),
            FirType::Bool => Some(Type::Bool),
            FirType::Void => Some(Type::Void),
            _ => None,
        }
    }

    /// Take ownership of the built FirModule.
    /// Configure the eval env layout for context-aware JIT evaluation.
    /// When set, the entry function's body is augmented with `__eval_env: Ptr`
    /// as first parameter and EnvLoad statements for each captured variable.
    pub fn set_eval_env(
        &mut self,
        env_layout: &[(String, ast::Type)],
        snapshot: &ast::ContextSnapshot,
    ) {
        let entries: Vec<EvalEnvEntry> = env_layout
            .iter()
            .map(|(name, ast_ty)| EvalEnvEntry {
                name: name.clone(),
                fir_ty: self.lower_type(ast_ty),
                ast_ty: ast_ty.clone(),
            })
            .collect();
        self.eval_env = Some(entries);

        // Pre-register class layout if "self" is in the env (for field access)
        if let Some(class_name) = &snapshot.current_class
            && let Some(ci) = &snapshot.class_info
        {
            let class_id = ClassId(self.ms.next_class);
            self.ms.next_class += 1;
            self.ms.classes.insert(class_name.clone(), class_id);

            // Build field layout with pointer-first ordering (matching host layout)
            let mut ptr_fields = Vec::new();
            let mut val_fields = Vec::new();
            for (fname, fty) in &ci.fields {
                let fir_ty = self.lower_type(fty);
                if fir_ty == FirType::Ptr {
                    ptr_fields.push((fname.clone(), fir_ty));
                } else {
                    val_fields.push((fname.clone(), fir_ty));
                }
            }
            let mut fields = Vec::new();
            let mut offset = 0;
            for (fname, fir_ty) in ptr_fields.into_iter().chain(val_fields) {
                fields.push((fname, fir_ty, offset));
                offset += 8;
            }
            self.ms.class_fields.insert(class_id, fields);
        }
    }

    pub fn finish(self) -> FirModule {
        self.ms.module
    }

    /// Get a reference to the module being built.
    pub fn module(&self) -> &FirModule {
        &self.ms.module
    }

    fn value_has_pending_stmts(&self, value: &Expr) -> bool {
        match value {
            Expr::Call { func, .. } => {
                if let Expr::Member { object, field, .. } = func.as_ref() {
                    // Iterable vocabulary methods produce loops
                    if matches!(
                        field.as_str(),
                        builtin_method::MAP
                            | builtin_method::FILTER
                            | builtin_method::FIND
                            | builtin_method::ANY
                            | builtin_method::ALL
                            | builtin_method::REDUCE
                            | builtin_method::COUNT
                            | builtin_method::FIRST
                            | builtin_method::LAST
                            | builtin_method::TO_LIST
                            | builtin_method::MIN
                            | builtin_method::MAX
                            | builtin_method::SORT
                            | builtin_method::OR
                            | builtin_method::OR_THROW
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
            Expr::Ident(name, _) => self.scope.local_ast_types.get(name).cloned(),
            _ => self.type_table.get(&expr.span()).cloned(),
        };
        match ast_ty {
            Some(Type::List(inner)) | Some(Type::Set(inner)) => Some(self.lower_type(&inner)),
            _ => None,
        }
    }

    /// Resolve the AST-level element type of a list or set expression.
    /// Used by for-loop lowering to set local_ast_types for the loop variable.
    fn resolve_list_ast_elem_type(&self, expr: &Expr) -> Option<Type> {
        let ast_ty = match expr {
            Expr::Ident(name, _) => self.scope.local_ast_types.get(name).cloned(),
            // For member access (e.g. s.deps), resolve the field type from class info
            Expr::Member { object, field, .. } => {
                let class_name = self.resolve_class_name(object).ok()?;
                let ci = self.type_env.get_class(&class_name)?;
                ci.fields
                    .iter()
                    .find(|(fname, _)| fname.as_str() == field.as_str())
                    .map(|(_, ftype)| ftype.clone())
            }
            _ => self.type_table.get(&expr.span()).cloned(),
        };
        match ast_ty {
            Some(Type::List(inner)) | Some(Type::Set(inner)) => Some(*inner),
            _ => None,
        }
    }

    /// Resolve the AST type of an expression from local_ast_types or type_table.
    fn resolve_expr_ast_type(&self, expr: &Expr) -> Option<Type> {
        match expr {
            Expr::Ident(name, _) => self.scope.local_ast_types.get(name).cloned(),
            _ => self.type_table.get(&expr.span()).cloned(),
        }
    }

    // -----------------------------------------------------------------------
    // Iterable vocabulary method helpers (list-based)
    // -----------------------------------------------------------------------

    fn lower_place(&self, expr: &Expr) -> Result<FirPlace, LowerError> {
        match expr {
            Expr::Ident(name, _) => {
                if let Some(&local_id) = self.scope.locals.get(name.as_str()) {
                    Ok(FirPlace::Local(local_id))
                } else if let Some(&self_id) = self.scope.locals.get("self") {
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
                        self.scope.local_ast_types.get(name.as_str()),
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
                if let Some(&local_id) = self.scope.locals.get(name.as_str()) {
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
            Type::Int | Type::TypeVar(_, _) => FirType::I64,
            Type::Float => FirType::F64,
            Type::Bool => FirType::Bool,
            Type::Nil | Type::Void => FirType::Void,
            Type::Never => FirType::Never,
            Type::Nullable(inner) => FirType::TaggedUnion {
                tag_bits: 1,
                variants: vec![self.lower_type(inner), FirType::Void],
            },
            Type::String
            | Type::List(_)
            | Type::Set(_)
            | Type::Custom(_, _)
            | Type::Function { .. }
            | Type::Task(_)
            | Type::Map(_, _) => FirType::Ptr,
            Type::Error => {
                debug_assert!(false, "Type::Error should not survive past typechecking");
                FirType::Void
            }
            // Inferred should ideally be resolved by the typechecker before reaching
            // FIR lowering. The I64 fallback is correct for Int/String/Class (all
            // 64-bit values or pointers) but wrong for Float (F64) and Bool (I8).
            Type::Inferred => {
                #[cfg(debug_assertions)]
                eprintln!(
                    "warning: Type::Inferred reached FIR lowering unresolved, \
                     falling back to I64"
                );
                FirType::I64
            }
        }
    }

    fn lower_binop(&self, op: &ast::BinOp) -> BinOp {
        match op {
            ast::BinOp::Add => BinOp::Add,
            ast::BinOp::Sub => BinOp::Sub,
            ast::BinOp::Mul => BinOp::Mul,
            ast::BinOp::Div => BinOp::Div,
            ast::BinOp::Mod => BinOp::Mod,
            ast::BinOp::Pow => unreachable!("Pow should be handled before reaching lower_binop"),
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
        let id = LocalId(self.scope.next_local);
        self.scope.next_local += 1;
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
            FirExpr::CancelTask { .. }
            | FirExpr::WaitCancel { .. }
            | FirExpr::Safepoint
            | FirExpr::FieldSet { .. }
            | FirExpr::ListSet { .. } => FirType::Void,
            FirExpr::FieldGet { ty, .. } => ty.clone(),
            FirExpr::Construct { ty, .. } => ty.clone(),
            FirExpr::ListNew { .. } => FirType::Ptr,
            FirExpr::ListGet { elem_ty, .. } => elem_ty.clone(),
            FirExpr::TagWrap { ty, .. } => ty.clone(),
            FirExpr::TagUnwrap { ty, .. } => ty.clone(),
            FirExpr::TagCheck { .. } => FirType::Bool,
            FirExpr::RuntimeCall { ret_ty, .. } => ret_ty.clone(),
            FirExpr::ClosureCreate { .. } => FirType::Ptr,
            FirExpr::ClosureCall { ret_ty, .. } => ret_ty.clone(),
            FirExpr::EnvLoad { ty, .. } => ty.clone(),
            FirExpr::GlobalFunc(_) => FirType::Ptr,
            FirExpr::IntToFloat(_) => FirType::F64,
            FirExpr::Bitcast { to, .. } => to.clone(),
            FirExpr::EvalCall { ret_ty, .. } => ret_ty.clone(),
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
        if let Some(&local_id) = self.scope.locals.get(name)
            && let Some(ty) = self.scope.local_types.get(&local_id)
        {
            return ty.clone();
        }
        // Fall back to type env (top-level bindings)
        if let Some(Binding { ty, .. }) = self.type_env.get_var(name) {
            self.lower_type(ty)
        } else {
            FirType::Void
        }
    }

    /// Resolve the return type of a closure-typed local variable.
    fn resolve_closure_ret_type(&self, name: &str) -> FirType {
        if let Some(Binding {
            ty: Type::Function { ret, .. },
            ..
        }) = self.type_env.get_var(name)
        {
            return self.lower_type(ret);
        }
        // Check local AST types
        if let Some(Type::Function { ret, .. }) = self.scope.local_ast_types.get(name) {
            return self.lower_type(ret);
        }
        FirType::I64 // fallback
    }

    fn resolve_function_ret_type(&self, name: &str) -> FirType {
        if let Some(Binding {
            ty: Type::Function { ret, .. },
            ..
        }) = self.type_env.get_var(name)
        {
            self.lower_type(ret)
        } else if let Some(&func_id) = self.ms.functions.get(name) {
            // Fallback: look up the return type from the already-compiled FIR function.
            // This handles imported methods whose signatures aren't in the main TypeEnv.
            self.resolve_function_ret_type_by_id(func_id)
        } else {
            FirType::Void
        }
    }

    /// For calls to generic functions, wrap arguments and return values in
    /// bitcasts so that Float/Bool values pass through the I64-erased params
    /// with correct Cranelift types. Returns (possibly-wrapped args, possibly-wrapped ret_ty).
    fn apply_generic_erasure_casts(
        &self,
        func_name: &str,
        mut fir_args: Vec<FirExpr>,
        ret_ty: FirType,
        call_span: &Span,
    ) -> (Vec<FirExpr>, FirType) {
        let func_binding = self.type_env.get_var(func_name);
        let (param_types, ret_ast) = match &func_binding {
            Some(Binding {
                ty: Type::Function { params, ret, .. },
                ..
            }) => (params.clone(), ret.as_ref().clone()),
            _ => return (fir_args, ret_ty),
        };

        // Build a mini substitution map: TypeVar name → concrete FirType,
        // inferred from argument types at this call site.
        let mut typevar_map: HashMap<String, FirType> = HashMap::new();
        for (i, param_ty) in param_types.iter().enumerate() {
            if let Type::TypeVar(tv_name, _) = param_ty
                && i < fir_args.len()
            {
                let arg_ty = self.infer_fir_type(&fir_args[i]);
                if arg_ty != FirType::I64 {
                    typevar_map.insert(tv_name.clone(), arg_ty);
                }
            }
        }

        if typevar_map.is_empty() {
            return (fir_args, ret_ty);
        }

        // Wrap args whose declared type is TypeVar and whose concrete type
        // is F64 or Bool — bitcast to I64 to match the erased signature.
        for (i, param_ty) in param_types.iter().enumerate() {
            if let Type::TypeVar(tv_name, _) = param_ty
                && let Some(concrete) = typevar_map.get(tv_name)
                && i < fir_args.len()
                && (*concrete == FirType::F64 || *concrete == FirType::Bool)
            {
                let arg = std::mem::replace(&mut fir_args[i], FirExpr::IntLit(0));
                fir_args[i] = FirExpr::Bitcast {
                    value: Box::new(arg),
                    to: FirType::I64,
                };
            }
        }

        // Resolve return type: if it references a TypeVar we've resolved,
        // the call returns I64 but the caller needs the concrete type.
        let mut cast_ret = ret_ty.clone();
        if let Type::TypeVar(tv_name, _) = &ret_ast
            && let Some(concrete) = typevar_map.get(tv_name)
            && (*concrete == FirType::F64 || *concrete == FirType::Bool)
        {
            cast_ret = concrete.clone();
        }
        // Fall back to type_table if available
        if cast_ret == ret_ty
            && let Some(resolved) = self.type_table.get(call_span)
        {
            let resolved_fir = self.lower_type(resolved);
            if resolved_fir != ret_ty
                && (resolved_fir == FirType::F64 || resolved_fir == FirType::Bool)
            {
                cast_ret = resolved_fir;
            }
        }

        (fir_args, cast_ret)
    }

    fn resolve_called_function_id(&self, func: &Expr) -> Result<FunctionId, LowerError> {
        match func {
            Expr::Ident(name, _) => self
                .ms
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
            Some(Binding {
                ty: Type::Function {
                    suspendable: true,
                    ..
                },
                ..
            })
        )
    }

    fn resolve_task_result_type(&self, expr: &Expr, task: &FirExpr) -> FirType {
        if let Some(Type::Task(inner_ty)) = self.type_table.get(&expr.span()) {
            return self.lower_type(inner_ty);
        }
        if let Expr::Ident(name, _) = expr {
            if let Some(Type::Task(inner_ty)) = self.scope.local_ast_types.get(name) {
                return self.lower_type(inner_ty);
            }
            if let Some(Binding {
                ty: Type::Task(inner_ty),
                ..
            }) = self.type_env.get_var(name)
            {
                return self.lower_type(inner_ty);
            }
        }
        self.infer_fir_type(task)
    }

    fn resolve_async_call_ast_type(&self, func: &Expr) -> Option<Type> {
        match func {
            Expr::Ident(name, _) => match self.type_env.get_var(name) {
                Some(Binding {
                    ty: Type::Function { ret, .. },
                    ..
                }) => Some(Type::Task(ret.clone())),
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
            && let Some(Type::List(inner)) = self.scope.local_ast_types.get(name)
            && let Type::Task(result) = inner.as_ref()
        {
            return Some(result.as_ref());
        }
        None
    }

    fn resolve_function_ret_type_by_id(&self, id: FunctionId) -> FirType {
        if (id.0 as usize) < self.ms.module.functions.len() {
            self.ms.module.get_function(id).ret_type.clone()
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
        let class_id = self.ms.classes.get(&class_name).ok_or_else(|| {
            LowerError::UnsupportedFeature(
                UnsupportedFeatureKind::Other(format!("unknown class: {}", class_name)),
                object.span(),
            )
        })?;

        // Look up the field in the class layout
        let fields = self.ms.class_fields.get(class_id).ok_or_else(|| {
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
            if let Some(Type::Custom(class_name, _)) = self.scope.local_ast_types.get(name.as_str())
                && class_name == builtin_class::RANGE
            {
                return true;
            }
            if let Some(Binding {
                ty: Type::Custom(class_name, _),
                ..
            }) = self.type_env.get_var(name)
                && class_name == builtin_class::RANGE
            {
                return true;
            }
        }
        matches!(
            self.type_table.get(&expr.span()),
            Some(Type::Custom(name, _)) if name == builtin_class::RANGE
        )
    }

    /// Determine the class name of an expression by inspecting local AST types
    /// and the type environment.
    fn resolve_class_name(&self, expr: &Expr) -> Result<String, LowerError> {
        match expr {
            Expr::Ident(name, _) => {
                // Check local AST types first (function-scoped variables)
                if let Some(ty) = self.scope.local_ast_types.get(name.as_str())
                    && let Type::Custom(class_name, _) = ty
                {
                    return Ok(class_name.clone());
                }
                // Fall back to the type env (top-level bindings)
                if let Some(Binding {
                    ty: Type::Custom(class_name, _),
                    ..
                }) = self.type_env.get_var(name)
                {
                    return Ok(class_name.clone());
                }
                // Inside a method body, bare field names resolve via self
                // e.g. `addr.zip` where `addr` is a field of the current class
                if let Some(Type::Custom(self_class, _)) = self.scope.local_ast_types.get("self") {
                    let self_class = self_class.clone();
                    if let Some(class_info) = self.type_env.get_class(&self_class) {
                        for (fname, ftype) in &class_info.fields {
                            if fname == name
                                && let Type::Custom(field_class, _) = ftype
                                && self.ms.classes.contains_key(field_class.as_str())
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
                    if self.ms.classes.contains_key(name.as_str()) {
                        return Ok(name.clone());
                    }
                    // Function call that returns a class instance: look up return type
                    if let Some(Binding {
                        ty: Type::Function { ret, .. },
                        ..
                    }) = self.type_env.get_var(name)
                        && let Type::Custom(class_name, _) = ret.as_ref()
                        && self.ms.classes.contains_key(class_name.as_str())
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
                            && self.ms.classes.contains_key(ret_class.as_str())
                        {
                            return Ok(ret_class.clone());
                        }
                        // Fall back: check FIR function registry for return type
                        let qualified = format!("{}.{}", class_name, field);
                        if let Some(ret_ty) = self
                            .ms
                            .functions
                            .get(&qualified)
                            .copied()
                            .map(|fid| self.resolve_function_ret_type_by_id(fid))
                            && let FirType::Struct(class_id) = ret_ty
                        {
                            for (cname, &cid) in &self.ms.classes {
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
                    for (cname, &cid) in &self.ms.classes {
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
                    // Check local AST types first, then fall back to type_env
                    let list_type = self
                        .scope
                        .local_ast_types
                        .get(name.as_str())
                        .or_else(|| self.type_env.get_var_type(name));
                    let elem_class = match list_type {
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
                        && self.ms.classes.contains_key(class_name.as_str())
                    {
                        return Ok(class_name);
                    }
                }
                // Chained index: expr.methods[0] or expr.fields[0]
                if let Expr::Member { field, .. } = object.as_ref() {
                    let elem_class = match field.as_str() {
                        "fields" => Some("FieldInfo".to_string()),
                        "methods" => Some("MethodInfo".to_string()),
                        "params" => Some("ParamInfo".to_string()),
                        _ => None,
                    };
                    if let Some(class_name) = elem_class
                        && self.ms.classes.contains_key(class_name.as_str())
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
}
