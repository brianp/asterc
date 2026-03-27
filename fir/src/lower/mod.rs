use std::collections::HashMap;

use ast::Span;
use ast::type_env::TypeEnv;
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
mod iterable;
mod match_lower;
mod method;
mod stmt;
mod synthesize;

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

pub struct Lowerer {
    pub(super) type_env: TypeEnv,
    pub(super) type_table: TypeTable,
    pub(super) scope: ScopeState,
    pub(super) ms: ModuleState,
    pub(super) tl: TopLevelState,
    pub(super) pending_stmts: Vec<FirStmt>,
}

impl Lowerer {
    pub fn new(type_env: TypeEnv, type_table: TypeTable) -> Self {
        Self {
            type_env,
            type_table,
            scope: ScopeState::default(),
            ms: ModuleState {
                module: FirModule::new(),
                functions: HashMap::new(),
                classes: HashMap::new(),
                class_fields: HashMap::new(),
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

    /// Take ownership of the built FirModule.
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
        if let Some(Type::List(inner)) = ast_ty {
            Some(self.lower_type(&inner))
        } else {
            None
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
        if let Some(ty) = self.type_env.get_var(name) {
            self.lower_type(ty)
        } else {
            FirType::Void
        }
    }

    /// Resolve the return type of a closure-typed local variable.
    fn resolve_closure_ret_type(&self, name: &str) -> FirType {
        if let Some(Type::Function { ret, .. }) = self.type_env.get_var(name) {
            return self.lower_type(ret);
        }
        // Check local AST types
        if let Some(Type::Function { ret, .. }) = self.scope.local_ast_types.get(name) {
            return self.lower_type(ret);
        }
        FirType::I64 // fallback
    }

    fn resolve_function_ret_type(&self, name: &str) -> FirType {
        if let Some(Type::Function { ret, .. }) = self.type_env.get_var(name) {
            self.lower_type(ret)
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
        let func_ty = self.type_env.get_var(func_name);
        let (param_types, ret_ast) = match &func_ty {
            Some(Type::Function { params, ret, .. }) => (params.clone(), ret.as_ref().clone()),
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
            if let Some(Type::Task(inner_ty)) = self.scope.local_ast_types.get(name) {
                return self.lower_type(inner_ty);
            }
            if let Some(Type::Task(inner_ty)) = self.type_env.get_var(name) {
                return self.lower_type(inner_ty);
            }
        }
        self.infer_fir_type(task)
    }

    fn resolve_async_call_ast_type(&self, func: &Expr) -> Option<Type> {
        match func {
            Expr::Ident(name, _) => match self.type_env.get_var(name) {
                Some(Type::Function { ret, .. }) => Some(Type::Task(ret.clone())),
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
            if let Some(Type::Custom(class_name, _)) = self.type_env.get_var(name)
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
                if let Some(Type::Custom(class_name, _)) = self.type_env.get_var(name) {
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
                    if let Some(Type::Function { ret, .. }) = self.type_env.get_var(name)
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
                    let elem_class = match self.scope.local_ast_types.get(name.as_str()) {
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
