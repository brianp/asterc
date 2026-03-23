use ast::{
    ClassInfo, Diagnostic, EnumInfo, Expr, MatchPattern, Span, Stmt, TraitInfo, Type, TypeEnv,
    TypeTable,
};
use indexmap::IndexMap;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::module_loader::ModuleLoader;

pub struct TypeChecker {
    pub env: TypeEnv,
    pub loop_depth: usize,
    pub expected_return_type: Option<Type>,
    /// Current function name for better error messages.
    pub current_function: Option<String>,
    /// The error type this function declares via `throws`.
    pub throws_type: Option<Type>,
    /// Accumulated diagnostics from error recovery.
    pub diagnostics: Vec<Diagnostic>,
    /// Optional module loader for resolving `use` imports.
    /// When None, `use` statements are ignored (prelude mode).
    pub module_loader: Option<Rc<RefCell<ModuleLoader>>>,
    /// Built-in protocol traits (Eq, Ord, Printable, etc.) — source of truth for `use std`.
    /// In prelude mode (no loader), these are also copied to env.
    /// Wrapped in Rc since these are read-only after initialization; avoids cloning on every child scope.
    pub(crate) builtin_traits: Rc<HashMap<String, TraitInfo>>,
    /// Built-in enum types (Ordering) — source of truth for `use std`.
    /// Wrapped in Rc since these are read-only after initialization.
    pub(crate) builtin_enums: Rc<HashMap<String, EnumInfo>>,
    /// Expected type from context (e.g., let binding type annotation, function arg type).
    /// Used to resolve ambiguous parametric trait methods like `.into()`.
    pub(crate) expected_type: Option<Type>,
    /// Names of const bindings — cannot be reassigned.
    pub(crate) const_names: std::collections::HashSet<String>,
    /// For functions with default parameters: maps function name -> set of param names that have defaults.
    pub(crate) default_params: HashMap<String, std::collections::HashSet<String>>,
    /// Detectable single-consumer tracking for task bindings resolved in the current checker.
    pub(crate) consumed_tasks: std::collections::HashSet<String>,
    /// Task bindings created in the current scope via `let t = async f()`.
    /// Maps variable name to creation span for must-consume enforcement.
    pub(crate) task_bindings: HashMap<String, Span>,
    /// Variables that have crossed a thread boundary (passed to `async f()`).
    /// Maps variable name to the span where the crossing happened.
    /// Used for data sharing warnings (W002).
    pub(crate) boundary_crossed: HashMap<String, Span>,
    /// Maps expression spans to their resolved types. Consumed by FIR lowerer.
    pub type_table: TypeTable,
    /// Set by `check_call_inner` to indicate whether the resolved callee was suspendable.
    /// Read by `check_blocking_call` to avoid re-evaluating the func expression.
    pub(crate) last_call_suspendable: bool,
}

struct ScopeState {
    loop_depth: usize,
    expected_return_type: Option<Type>,
    current_function: Option<String>,
    throws_type: Option<Type>,
    diagnostics: Vec<Diagnostic>,
    expected_type: Option<Type>,
    const_names: std::collections::HashSet<String>,
    consumed_tasks: std::collections::HashSet<String>,
    task_bindings: HashMap<String, Span>,
}

impl Default for TypeChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeChecker {
    pub fn new() -> Self {
        let mut env = TypeEnv::new();
        let (builtin_traits, builtin_enums) = Self::register_builtins(&mut env);
        Self {
            env,
            loop_depth: 0,
            expected_return_type: None,
            current_function: None,
            throws_type: None,
            diagnostics: Vec::new(),
            module_loader: None,
            builtin_traits,
            builtin_enums,
            expected_type: None,
            const_names: std::collections::HashSet::new(),
            default_params: HashMap::new(),
            consumed_tasks: std::collections::HashSet::new(),
            task_bindings: HashMap::new(),
            boundary_crossed: HashMap::new(),
            type_table: TypeTable::new(),
            last_call_suspendable: false,
        }
    }

    /// Register all builtin types, traits, error classes, and enums on the given TypeEnv.
    /// Returns the builtin_traits and builtin_enums maps wrapped in Rc.
    #[allow(clippy::type_complexity)]
    fn register_builtins(
        env: &mut TypeEnv,
    ) -> (
        Rc<HashMap<String, TraitInfo>>,
        Rc<HashMap<String, EnumInfo>>,
    ) {
        // Register log/say so they appear in scope for diagnostics (e.g. typo suggestions).
        // Actual type checking is handled as polymorphic builtins in check_call_inner.
        env.set_var(
            "log".into(),
            Type::func(vec!["message".into()], vec![Type::String], Type::Void),
        );
        env.set_var(
            "say".into(),
            Type::func(vec!["message".into()], vec![Type::String], Type::Void),
        );
        // Note: `len`, `to_string`, and `random` are handled as polymorphic
        // builtins in check_call_inner rather than registered here, because
        // their type signatures depend on context.
        env.set_var("random".into(), Type::func(vec![], vec![], Type::Int));

        // Built-in error hierarchy: Exception (root) -> Error (app base)
        env.set_class(
            "Exception".into(),
            ClassInfo::new(
                Type::Custom("Exception".into(), Vec::new()),
                IndexMap::from([("message".into(), Type::String)]),
                HashMap::new(),
            ),
        );
        env.set_var(
            "Exception".into(),
            Type::func(
                vec!["message".into()],
                vec![Type::String],
                Type::Custom("Exception".into(), Vec::new()),
            ),
        );
        env.set_class("Error".into(), {
            let mut info = ClassInfo::new(
                Type::Custom("Error".into(), Vec::new()),
                IndexMap::new(),
                HashMap::new(),
            );
            info.extends = Some("Exception".into());
            info
        });
        env.set_var(
            "Error".into(),
            Type::func(
                vec!["message".into()], // inherited message field
                vec![Type::String],
                Type::Custom("Error".into(), Vec::new()),
            ),
        );
        // Built-in CancelledError for async task cancellation
        env.set_class("CancelledError".into(), {
            let mut info = ClassInfo::new(
                Type::Custom("CancelledError".into(), Vec::new()),
                IndexMap::new(),
                HashMap::new(),
            );
            info.extends = Some("Error".into());
            info
        });
        env.set_var(
            "CancelledError".into(),
            Type::func(
                vec!["message".into()],
                vec![Type::String],
                Type::Custom("CancelledError".into(), Vec::new()),
            ),
        );

        // Built-in error types for Mutex, Channel, and I/O
        for (name, parent) in [
            ("LockTimeoutError", "Error"),
            ("ChannelFullError", "Error"),
            ("ChannelEmptyError", "Error"),
            ("ChannelClosedError", "Error"),
            ("IOError", "Error"),
        ] {
            env.set_class(name.into(), {
                let mut info = ClassInfo::new(
                    Type::Custom(name.into(), Vec::new()),
                    IndexMap::new(),
                    HashMap::new(),
                );
                info.extends = Some(parent.into());
                info
            });
            env.set_var(
                name.into(),
                Type::func(
                    vec!["message".into()],
                    vec![Type::String],
                    Type::Custom(name.into(), Vec::new()),
                ),
            );
        }

        // I/O namespaces — static methods only, no instances
        for name in ["File", "TcpListener", "TcpStream"] {
            env.set_var(name.into(), Type::Custom(name.into(), Vec::new()));
        }

        // Range builtin class — includes Iterable, used by `..` and `..=` syntax
        env.set_class("Range".into(), {
            let mut info = ClassInfo::new(
                Type::Custom("Range".into(), Vec::new()),
                IndexMap::from([
                    ("start".into(), Type::Int),
                    ("end".into(), Type::Int),
                    ("inclusive".into(), Type::Bool),
                ]),
                HashMap::from([
                    (
                        "each".into(),
                        Type::func(
                            vec!["f".into()],
                            vec![Type::func(vec!["_0".into()], vec![Type::Int], Type::Void)],
                            Type::Void,
                        ),
                    ),
                    ("random".into(), Type::func(vec![], vec![], Type::Int)),
                ]),
            );
            info.includes = vec!["Iterable".into()];
            info.parametric_includes = vec![("Iterable".to_string(), vec![Type::Int])];
            info
        });

        // Build protocol traits and supporting enums — stored in builtin maps.
        // In prelude mode (no loader), also installed in env.
        let mut builtin_traits: HashMap<String, TraitInfo> = HashMap::new();
        let mut builtin_enums: HashMap<String, EnumInfo> = HashMap::new();

        builtin_enums.insert(
            "Ordering".into(),
            EnumInfo {
                name: "Ordering".into(),
                variants: vec!["Less".into(), "Equal".into(), "Greater".into()],
                includes: vec!["Eq".into()],
            },
        );

        builtin_traits.insert(
            "Eq".into(),
            TraitInfo {
                name: "Eq".into(),
                methods: HashMap::from([(
                    "eq".into(),
                    Type::func(
                        vec!["other".into()],
                        vec![Type::Custom("Self".into(), Vec::new())],
                        Type::Bool,
                    ),
                )]),
                required_methods: vec!["eq".into()],
                generic_params: None,
            },
        );

        builtin_traits.insert(
            "Ord".into(),
            TraitInfo {
                name: "Ord".into(),
                methods: HashMap::from([(
                    "cmp".into(),
                    Type::func(
                        vec!["other".into()],
                        vec![Type::Custom("Self".into(), Vec::new())],
                        Type::Custom("Ordering".into(), Vec::new()),
                    ),
                )]),
                required_methods: vec!["cmp".into()],
                generic_params: None,
            },
        );

        builtin_traits.insert(
            "Printable".into(),
            TraitInfo {
                name: "Printable".into(),
                methods: HashMap::from([
                    ("to_string".into(), Type::func(vec![], vec![], Type::String)),
                    ("debug".into(), Type::func(vec![], vec![], Type::String)),
                ]),
                required_methods: vec!["to_string".into()],
                generic_params: None,
            },
        );

        builtin_traits.insert(
            "From".into(),
            TraitInfo {
                name: "From".into(),
                methods: HashMap::from([(
                    "from".into(),
                    Type::func(
                        vec!["value".into()],
                        vec![Type::TypeVar("T".into(), vec![])],
                        Type::Custom("Self".into(), Vec::new()),
                    ),
                )]),
                required_methods: vec!["from".into()],
                generic_params: Some(vec!["T".into()]),
            },
        );

        builtin_traits.insert(
            "Into".into(),
            TraitInfo {
                name: "Into".into(),
                methods: HashMap::from([(
                    "into".into(),
                    Type::func(vec![], vec![], Type::TypeVar("T".into(), vec![])),
                )]),
                required_methods: vec!["into".into()],
                generic_params: Some(vec!["T".into()]),
            },
        );

        builtin_traits.insert(
            "Iterator".into(),
            TraitInfo {
                name: "Iterator".into(),
                methods: HashMap::from([(
                    "next".into(),
                    Type::func(
                        vec![],
                        vec![],
                        Type::Nullable(Box::new(Type::TypeVar("T".into(), vec![]))),
                    ),
                )]),
                required_methods: vec!["next".into()],
                generic_params: Some(vec!["T".into()]),
            },
        );

        builtin_traits.insert(
            "Drop".into(),
            TraitInfo {
                name: "Drop".into(),
                methods: HashMap::from([("drop".into(), Type::func(vec![], vec![], Type::Void))]),
                required_methods: vec!["drop".into()],
                generic_params: None,
            },
        );

        builtin_traits.insert(
            "Close".into(),
            TraitInfo {
                name: "Close".into(),
                methods: HashMap::from([(
                    "close".into(),
                    Type::Function {
                        param_names: vec![],
                        params: vec![],
                        ret: Box::new(Type::Void),
                        throws: Some(Box::new(Type::Custom("Error".into(), Vec::new()))),
                        suspendable: false,
                    },
                )]),
                required_methods: vec!["close".into()],
                generic_params: None,
            },
        );

        builtin_traits.insert(
            "Iterable".into(),
            TraitInfo {
                name: "Iterable".into(),
                methods: HashMap::from([(
                    "each".into(),
                    Type::func(
                        vec!["f".into()],
                        vec![Type::func(
                            vec!["_0".into()],
                            vec![Type::TypeVar("T".into(), vec![])],
                            Type::Void,
                        )],
                        Type::Void,
                    ),
                )]),
                required_methods: vec!["each".into()],
                generic_params: Some(vec!["T".into()]),
            },
        );

        builtin_traits.insert(
            "Random".into(),
            TraitInfo {
                name: "Random".into(),
                methods: HashMap::from([(
                    "random".into(),
                    Type::func(vec![], vec![], Type::TypeVar("Self".into(), vec![])),
                )]),
                required_methods: vec!["random".into()],
                generic_params: None,
            },
        );

        // Prelude mode: install all protocol traits and enums in env
        for (name, info) in &builtin_traits {
            env.set_trait(name.clone(), info.clone());
        }
        for (name, info) in &builtin_enums {
            env.set_enum(name.clone(), info.clone());
        }

        (Rc::new(builtin_traits), Rc::new(builtin_enums))
    }

    /// Create a TypeChecker with a module loader for resolving `use` imports.
    /// Protocol traits are NOT in scope — they must be imported via `use std { ... }`.
    pub fn with_loader(loader: Rc<RefCell<ModuleLoader>>) -> Self {
        let mut tc = Self::new();
        tc.module_loader = Some(loader);
        // Remove protocol traits from env — require `use std { ... }` import
        for name in [
            "Eq",
            "Ord",
            "Printable",
            "From",
            "Into",
            "Iterable",
            "Iterator",
            "Random",
            // Drop and Close stay in prelude — they're fundamental lifecycle traits
        ] {
            tc.env.remove_trait(name);
        }
        tc.env.remove_enum("Ordering");
        tc
    }

    /// Create a child TypeChecker that inherits context flags and a child scope.
    /// Uses clone — prefer `with_child_scope` for better performance.
    pub(crate) fn child_checker(&self) -> TypeChecker {
        TypeChecker {
            env: self.env.child(),
            loop_depth: self.loop_depth,
            expected_return_type: self.expected_return_type.clone(),
            current_function: self.current_function.clone(),
            throws_type: self.throws_type.clone(),
            diagnostics: Vec::new(),
            module_loader: self.module_loader.clone(),
            builtin_traits: self.builtin_traits.clone(),
            builtin_enums: self.builtin_enums.clone(),
            expected_type: self.expected_type.clone(),
            const_names: self.const_names.clone(),
            default_params: self.default_params.clone(),
            consumed_tasks: self.consumed_tasks.clone(),
            task_bindings: self.task_bindings.clone(),
            boundary_crossed: HashMap::new(),
            type_table: TypeTable::new(),
            last_call_suspendable: false,
        }
    }

    /// Emit a W003 warning if `name` already exists in a parent scope.
    pub(crate) fn warn_if_shadowed(&mut self, name: &str, span: Span) {
        if self.env.parent_has_var(name) {
            self.diagnostics.push(
                Diagnostic::warning(format!("variable '{}' shadows a previous binding", name))
                    .with_code("W003")
                    .with_label(span, "shadows earlier binding"),
            );
        }
    }

    fn save_scope_state(&mut self) -> ScopeState {
        ScopeState {
            loop_depth: self.loop_depth,
            expected_return_type: self.expected_return_type.clone(),
            current_function: self.current_function.clone(),
            throws_type: self.throws_type.clone(),
            diagnostics: std::mem::take(&mut self.diagnostics),
            expected_type: self.expected_type.clone(),
            const_names: self.const_names.clone(),
            consumed_tasks: self.consumed_tasks.clone(),
            task_bindings: self.task_bindings.clone(),
        }
    }

    fn restore_scope_state(&mut self, saved: ScopeState) {
        let child_diagnostics = std::mem::take(&mut self.diagnostics);
        let child_task_bindings = std::mem::take(&mut self.task_bindings);

        self.loop_depth = saved.loop_depth;
        self.expected_return_type = saved.expected_return_type;
        self.current_function = saved.current_function;
        self.throws_type = saved.throws_type;
        self.diagnostics = saved.diagnostics;
        self.expected_type = saved.expected_type;
        self.const_names = saved.const_names;
        self.consumed_tasks = saved.consumed_tasks;
        self.task_bindings = saved.task_bindings;

        self.task_bindings.extend(child_task_bindings);
        self.diagnostics.extend(child_diagnostics);
    }

    /// Execute `f` in a child scope. The env is scoped via enter/exit (zero-copy),
    /// and TypeChecker state (loop_depth, throws, etc.) is saved and restored.
    pub(crate) fn with_child_scope<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        let saved = self.save_scope_state();
        self.env.enter_scope();

        let result = f(self);

        self.env.exit_scope();
        self.restore_scope_state(saved);

        result
    }

    pub fn check_module(&mut self, m: &ast::Module) -> Result<(), Diagnostic> {
        let diags = self.check_module_all(m);
        let mut first_error = None;
        for d in diags {
            if d.severity == ast::Severity::Error {
                if first_error.is_none() {
                    first_error = Some(d);
                }
            } else {
                self.diagnostics.push(d);
            }
        }
        match first_error {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    pub fn check_module_all(&mut self, m: &ast::Module) -> Vec<Diagnostic> {
        // First pass: pre-register all top-level function signatures so that
        // recursive and mutually recursive calls resolve during the second pass.
        for s in &m.body {
            if let Stmt::Let {
                name,
                value:
                    Expr::Lambda {
                        params,
                        ret_type,
                        generic_params,
                        throws,
                        type_constraints,
                        ..
                    },
                ..
            } = s
            {
                // Skip lambdas with inferred param types — they need context to resolve.
                if params.iter().any(|(_, t)| *t == Type::Inferred) {
                    continue;
                }

                // Determine generic type params (explicit or auto-detected).
                let inferred_type_params = if generic_params.is_some() {
                    generic_params.clone().unwrap_or_default()
                } else {
                    let mut type_param_names: Vec<String> = Vec::new();
                    for (_, t) in params {
                        self.collect_unknown_type_names(t, &mut type_param_names);
                    }
                    type_param_names
                };

                let param_types: Vec<Type> = params.iter().map(|(_, t)| t.clone()).collect();

                // Convert inferred type params from Custom to TypeVar in the signature.
                let (final_params, final_ret) = if inferred_type_params.is_empty() {
                    (param_types, ret_type.clone())
                } else {
                    let fp = param_types
                        .iter()
                        .map(|t| {
                            Self::replace_custom_with_typevar(
                                t,
                                &inferred_type_params,
                                type_constraints,
                            )
                        })
                        .collect();
                    let fr = Self::replace_custom_with_typevar(
                        ret_type,
                        &inferred_type_params,
                        type_constraints,
                    );
                    (fp, fr)
                };

                let fn_type = Type::Function {
                    param_names: params.iter().map(|(n, _)| n.clone()).collect(),
                    params: final_params,
                    ret: Box::new(final_ret),
                    throws: throws.as_deref().cloned().map(Box::new),
                    suspendable: false,
                };
                self.env.set_var(name.clone(), fn_type);
            }
        }

        self.infer_suspendable_functions(m);

        // Second pass: typecheck all statements (function bodies can now see all signatures).
        for s in &m.body {
            match self.check_stmt(s) {
                Ok(_) => {}
                Err(diag) => {
                    self.diagnostics.push(diag);
                    // For let bindings that failed, assign Type::Error so later code doesn't cascade
                    if let ast::Stmt::Let { name, .. } = s {
                        self.env.set_var(name.clone(), Type::Error);
                    }
                }
            }
        }
        let all = std::mem::take(&mut self.diagnostics);
        let mut errors = Vec::new();
        for d in all {
            if d.severity == ast::Severity::Error {
                errors.push(d);
            } else {
                self.diagnostics.push(d);
            }
        }
        errors
    }

    fn infer_suspendable_functions(&mut self, m: &ast::Module) {
        loop {
            let mut changed = false;
            for stmt in &m.body {
                let Stmt::Let {
                    name,
                    value: Expr::Lambda { body, .. },
                    ..
                } = stmt
                else {
                    continue;
                };
                if !self.body_is_suspendable(body) {
                    continue;
                }
                let Some(Type::Function {
                    param_names,
                    params,
                    ret,
                    throws,
                    suspendable,
                }) = self.env.get_var(name)
                else {
                    continue;
                };
                if suspendable {
                    continue;
                }
                self.env.set_var(
                    name.clone(),
                    Type::Function {
                        param_names,
                        params,
                        ret,
                        throws,
                        suspendable: true,
                    },
                );
                changed = true;
            }
            if !changed {
                break;
            }
        }
    }

    fn body_is_suspendable(&self, body: &[Stmt]) -> bool {
        body.iter().any(|stmt| self.stmt_is_suspendable(stmt))
    }

    fn stmt_is_suspendable(&self, stmt: &Stmt) -> bool {
        match stmt {
            Stmt::Let { value, .. } => self.expr_is_suspendable(value),
            Stmt::Return(expr, _) | Stmt::Expr(expr, _) => self.expr_is_suspendable(expr),
            Stmt::If {
                cond,
                then_body,
                elif_branches,
                else_body,
                ..
            } => {
                self.expr_is_suspendable(cond)
                    || self.body_is_suspendable(then_body)
                    || elif_branches.iter().any(|(expr, body)| {
                        self.expr_is_suspendable(expr) || self.body_is_suspendable(body)
                    })
                    || self.body_is_suspendable(else_body)
            }
            Stmt::While { cond, body, .. } => {
                self.expr_is_suspendable(cond) || self.body_is_suspendable(body)
            }
            Stmt::For { iter, body, .. } => {
                self.expr_is_suspendable(iter) || self.body_is_suspendable(body)
            }
            Stmt::Assignment { target, value, .. } => {
                self.expr_is_suspendable(target) || self.expr_is_suspendable(value)
            }
            _ => false,
        }
    }

    fn expr_is_suspendable(&self, expr: &Expr) -> bool {
        match expr {
            Expr::AsyncCall { .. } | Expr::BlockingCall { .. } | Expr::DetachedCall { .. } => true,
            Expr::Resolve { .. } => true,
            Expr::Call { func, args, .. } => {
                self.expr_refers_to_suspendable_function(func)
                    || self.expr_is_suspendable(func)
                    || args.iter().any(|(_, _, arg)| self.expr_is_suspendable(arg))
            }
            Expr::Member { object, .. } => self.expr_is_suspendable(object),
            Expr::BinaryOp { left, right, .. } => {
                self.expr_is_suspendable(left) || self.expr_is_suspendable(right)
            }
            Expr::UnaryOp { operand, .. } => self.expr_is_suspendable(operand),
            Expr::ListLiteral(items, _) => items.iter().any(|item| self.expr_is_suspendable(item)),
            Expr::Index { object, index, .. } => {
                self.expr_is_suspendable(object) || self.expr_is_suspendable(index)
            }
            Expr::Match {
                scrutinee, arms, ..
            } => {
                self.expr_is_suspendable(scrutinee)
                    || arms.iter().any(|(pattern, expr)| {
                        self.pattern_is_suspendable(pattern) || self.expr_is_suspendable(expr)
                    })
            }
            Expr::Propagate(inner, _) | Expr::Throw(inner, _) => self.expr_is_suspendable(inner),
            Expr::ErrorOr { expr, default, .. } => {
                self.expr_is_suspendable(expr) || self.expr_is_suspendable(default)
            }
            Expr::ErrorOrElse { expr, handler, .. } => {
                self.expr_is_suspendable(expr) || self.expr_is_suspendable(handler)
            }
            Expr::ErrorCatch { expr, arms, .. } => {
                self.expr_is_suspendable(expr)
                    || arms.iter().any(|(_, arm)| self.expr_is_suspendable(arm))
            }
            Expr::StringInterpolation { parts, .. } => parts.iter().any(|part| match part {
                ast::StringPart::Literal(_) => false,
                ast::StringPart::Expr(expr) => self.expr_is_suspendable(expr),
            }),
            Expr::Map { entries, .. } => entries.iter().any(|(key, value)| {
                self.expr_is_suspendable(key) || self.expr_is_suspendable(value)
            }),
            Expr::Lambda { body, .. } => self.body_is_suspendable(body),
            Expr::Range { start, end, .. } => {
                self.expr_is_suspendable(start) || self.expr_is_suspendable(end)
            }
            Expr::Int(..)
            | Expr::Float(..)
            | Expr::Str(..)
            | Expr::Bool(..)
            | Expr::Nil(_)
            | Expr::Ident(..) => false,
        }
    }

    fn pattern_is_suspendable(&self, pattern: &MatchPattern) -> bool {
        match pattern {
            MatchPattern::Literal(expr, _) => self.expr_is_suspendable(expr),
            MatchPattern::Ident(..)
            | MatchPattern::Wildcard(_)
            | MatchPattern::EnumVariant { .. } => false,
        }
    }

    fn expr_refers_to_suspendable_function(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Ident(name, _) => self
                .env
                .get_var(name)
                .is_some_and(|ty| ty.is_suspendable_function()),
            Expr::Member { .. } => false,
            _ => false,
        }
    }

    pub fn check_stmt(&mut self, stmt: &Stmt) -> Result<Type, Diagnostic> {
        let stmt_span = stmt.span();
        match stmt {
            Stmt::Let {
                name,
                type_ann,
                value,
                ..
            } => {
                let prev_fn = self.current_function.clone();
                if matches!(value, Expr::Lambda { .. }) {
                    self.current_function = Some(name.clone());
                }
                // If the value is a lambda with inferred types and we have a type annotation,
                // propagate the expected type for inference.
                // Also set expected_type for parametric trait resolution (e.g., .into())
                let prev_expected = self.expected_type.take();
                if let Some(ann) = type_ann {
                    self.expected_type = Some(ann.clone());
                }
                let mut ty = if matches!(value, Expr::Lambda { .. }) {
                    self.check_lambda_with_expected(value, type_ann.as_ref())?
                } else {
                    self.check_expr(value)?
                };
                if matches!(value, Expr::Lambda { .. })
                    && let (
                        Type::Function {
                            param_names,
                            params,
                            ret,
                            throws,
                            suspendable: false,
                        },
                        Some(Type::Function {
                            suspendable: true, ..
                        }),
                    ) = (&ty, self.env.get_var(name))
                {
                    ty = Type::Function {
                        param_names: param_names.clone(),
                        params: params.clone(),
                        ret: ret.clone(),
                        throws: throws.clone(),
                        suspendable: true,
                    };
                }
                self.expected_type = prev_expected;
                self.current_function = prev_fn;
                if ty.is_error() {
                    self.env.set_var(name.clone(), Type::Error);
                    return Ok(Type::Error);
                }
                if let Some(ann) = type_ann {
                    // Empty list takes on the annotated type
                    if ty == Type::List(Box::new(Type::Nil)) && matches!(ann, Type::List(_)) {
                        self.env.set_var(name.clone(), ann.clone());
                        return Ok(ann.clone());
                    }
                    // Empty map takes on the annotated type
                    if ty == Type::Map(Box::new(Type::Error), Box::new(Type::Error))
                        && matches!(ann, Type::Map(_, _))
                    {
                        self.env.set_var(name.clone(), ann.clone());
                        return Ok(ann.clone());
                    }
                    // Nullable auto-wrap: T or Nil assigned to T?
                    if let Type::Nullable(inner) = ann {
                        if ty == *ann || ty == **inner || ty == Type::Nil {
                            self.env.set_var(name.clone(), ann.clone());
                            return Ok(ann.clone());
                        }
                        return Err(Diagnostic::error(format!(
                            "Type annotation mismatch for '{}': declared {}, got {}",
                            name, ann, ty
                        ))
                        .with_code("E001")
                        .with_label(stmt_span, format!("expected {}", ann)));
                    }
                    // Nil cannot be assigned to non-nullable types
                    if ty == Type::Nil && !matches!(ann, Type::Nil) {
                        return Err(Diagnostic::error(format!(
                            "Cannot assign nil to non-nullable type {}",
                            ann
                        ))
                        .with_code("E001")
                        .with_label(stmt_span, format!("expected {}", ann)));
                    }
                    if !Self::types_compatible_with_env(ann, &ty, &self.env) {
                        return Err(Diagnostic::error(format!(
                            "Type annotation mismatch for '{}': declared {}, got {}",
                            name, ann, ty
                        ))
                        .with_code("E001")
                        .with_label(stmt_span, format!("expected {}", ann)));
                    }
                    // W001: warn when a type annotation is redundant (matches inferred type)
                    if Self::is_obviously_typed(value, &self.env) && *ann == ty {
                        self.diagnostics.push(
                            Diagnostic::warning(format!(
                                "redundant type annotation: type `{}` can be inferred",
                                ann
                            ))
                            .with_code("W001"),
                        );
                    }
                }
                // Track default params for the function if it has any
                if let Expr::Lambda {
                    params, defaults, ..
                } = value
                {
                    let mut default_set = std::collections::HashSet::new();
                    for (i, d) in defaults.iter().enumerate() {
                        if d.is_some()
                            && let Some((pname, _)) = params.get(i)
                        {
                            default_set.insert(pname.clone());
                        }
                    }
                    if !default_set.is_empty() {
                        self.default_params.insert(name.clone(), default_set);
                    }
                }
                self.warn_if_shadowed(name, stmt_span);
                self.env.set_var(name.clone(), ty.clone());
                // Track Task[T] bindings for must-consume enforcement
                if matches!(ty, Type::Task(_)) {
                    self.task_bindings.insert(name.clone(), stmt_span);
                }
                Ok(ty)
            }
            Stmt::Class {
                name,
                fields,
                methods,
                generic_params,
                extends,
                includes,
                ..
            } => self.check_class_stmt(name, fields, methods, generic_params, extends, includes),
            Stmt::Trait {
                name,
                methods,
                generic_params,
                ..
            } => {
                // Push type params into scope so method types can reference them
                if let Some(gp) = generic_params {
                    for p in gp {
                        self.env.set_var(
                            format!("__type_param_{}", p),
                            Type::TypeVar(p.clone(), vec![]),
                        );
                    }
                }

                let mut method_map = HashMap::new();
                let mut required_methods = Vec::new();
                for m in methods {
                    if let Stmt::Let {
                        name: mname, value, ..
                    } = m
                    {
                        let mty = self.check_expr(value)?;
                        // Store with unqualified name for trait matching
                        let short_name = mname
                            .strip_prefix(&format!("{}.", name))
                            .unwrap_or(mname)
                            .to_string();
                        // Check if this is an abstract method (empty body)
                        if let Expr::Lambda { body, .. } = value
                            && body.is_empty()
                        {
                            required_methods.push(short_name.clone());
                        }
                        method_map.insert(short_name, mty);
                    } else {
                        return Err(Diagnostic::error(format!(
                            "Unexpected stmt in trait methods: {:?}",
                            m
                        ))
                        .with_code("E014")
                        .with_label(m.span(), "expected method definition"));
                    }
                }

                let info = TraitInfo {
                    name: name.clone(),
                    methods: method_map,
                    required_methods,
                    generic_params: generic_params.clone(),
                };
                self.env.set_trait(name.clone(), info);
                Ok(Type::Void)
            }
            Stmt::Return(expr, span) => {
                let ty = self.check_expr(expr)?;
                if ty.is_error() {
                    return Ok(Type::Error);
                }
                // Mark returned task idents as consumed (caller takes responsibility)
                self.mark_task_ident_consumed(expr);
                if let Some(expected) = &self.expected_return_type
                    && ty != *expected
                    && !Self::is_nullable_compatible(expected, &ty)
                    && !Self::is_subtype_compatible(&ty, expected, &self.env)
                {
                    let ctx = self.current_function.as_deref().unwrap_or("<anonymous>");
                    return Err(Diagnostic::error(format!(
                        "Return type mismatch in '{}': expected {}, got {}",
                        ctx, expected, ty
                    ))
                    .with_code("E004")
                    .with_label(*span, format!("expected {}", expected)));
                }
                Ok(ty)
            }
            Stmt::Expr(expr, _) => self.check_expr(expr),
            Stmt::If {
                cond,
                then_body,
                elif_branches,
                else_body,
                ..
            } => {
                let cond_ty = self.check_expr(cond)?;
                if cond_ty != Type::Bool && !cond_ty.is_error() {
                    return Err(Diagnostic::error(format!(
                        "If condition must be Bool, got {}",
                        cond_ty
                    ))
                    .with_code("E015")
                    .with_label(cond.span(), "expected Bool"));
                }

                self.with_child_scope(|tc| tc.check_body(then_body))?;
                for (elif_cond, elif_body) in elif_branches {
                    let elif_cond_ty = self.check_expr(elif_cond)?;
                    if elif_cond_ty != Type::Bool && !elif_cond_ty.is_error() {
                        return Err(Diagnostic::error(format!(
                            "Elif condition must be Bool, got {}",
                            elif_cond_ty
                        ))
                        .with_code("E015")
                        .with_label(elif_cond.span(), "expected Bool"));
                    }
                    self.with_child_scope(|tc| tc.check_body(elif_body))?;
                }
                self.with_child_scope(|tc| tc.check_body(else_body))
            }
            Stmt::While { cond, body, .. } => {
                let cond_ty = self.check_expr(cond)?;
                if cond_ty != Type::Bool && !cond_ty.is_error() {
                    return Err(Diagnostic::error(format!(
                        "While condition must be Bool, got {}",
                        cond_ty
                    ))
                    .with_code("E015")
                    .with_label(cond.span(), "expected Bool"));
                }
                self.with_child_scope(|tc| {
                    tc.loop_depth += 1;
                    tc.check_body(body)
                })
            }
            Stmt::For {
                var, iter, body, ..
            } => {
                let iter_ty = self.check_expr(iter)?;
                if iter_ty.is_error() {
                    return self.with_child_scope(|tc| {
                        tc.loop_depth += 1;
                        tc.env.set_var(var.clone(), Type::Error);
                        tc.check_body(body)?;
                        Ok(Type::Void)
                    });
                }
                let elem_ty = match iter_ty {
                    Type::List(inner) => *inner,
                    Type::Custom(ref class_name, _) => {
                        if let Some(class_info) = self.env.get_class(class_name) {
                            if class_info.includes.contains(&"Iterable".to_string()) {
                                Self::get_iterable_element_type_from_class(&class_info)
                                    .ok_or_else(|| {
                                        Diagnostic::error(format!(
                                            "Class '{}' includes Iterable but has no valid each() method",
                                            class_name
                                        ))
                                        .with_code("E007")
                                        .with_label(iter.span(), "missing each() method")
                                    })?
                            } else if class_info.includes.contains(&"Iterator".to_string()) {
                                Self::get_iterator_element_type_from_class(&class_info)
                                    .ok_or_else(|| {
                                        Diagnostic::error(format!(
                                            "Class '{}' includes Iterator but has no valid next() method",
                                            class_name
                                        ))
                                        .with_code("E007")
                                        .with_label(iter.span(), "missing next() method")
                                    })?
                            } else {
                                return Err(Diagnostic::error(format!(
                                    "Cannot iterate over '{}': class does not include Iterable or Iterator",
                                    class_name
                                ))
                                .with_code("E007")
                                .with_label(iter.span(), "does not include Iterable or Iterator"));
                            }
                        } else {
                            return Err(Diagnostic::error(format!(
                                "Cannot iterate over {}, expected List, Iterable, or Iterator class",
                                iter_ty
                            ))
                            .with_code("E007")
                            .with_label(iter.span(), "expected List, Iterable, or Iterator"));
                        }
                    }
                    _ => {
                        return Err(Diagnostic::error(format!(
                            "Cannot iterate over {}, expected List, Iterable, or Iterator class",
                            iter_ty
                        ))
                        .with_code("E007")
                        .with_label(iter.span(), "expected List, Iterable, or Iterator"));
                    }
                };
                self.with_child_scope(|tc| {
                    tc.loop_depth += 1;
                    tc.warn_if_shadowed(var, stmt_span);
                    tc.env.set_var(var.clone(), elem_ty);
                    tc.check_body(body)
                })
            }
            Stmt::Assignment { target, value, .. } => {
                let val_ty = self.check_expr(value)?;
                if val_ty.is_error() {
                    return Ok(Type::Error);
                }
                match target {
                    Expr::Ident(name, ident_span) => {
                        // Check if the variable is a const binding
                        if self.const_names.contains(name) {
                            return Err(Diagnostic::error(format!(
                                "Cannot reassign const '{}'",
                                name
                            ))
                            .with_code("E026")
                            .with_label(*ident_span, "const binding cannot be reassigned"));
                        }
                        let target_ty = self.env.get_var(name).ok_or_else(|| {
                            let mut diag = Diagnostic::error(format!(
                                "Assignment to undeclared variable '{}'",
                                name
                            ))
                            .with_code("E009")
                            .with_label(*ident_span, "not found in this scope");
                            if let Some(suggestion) = self.suggest_similar_name(name) {
                                diag = diag.with_note(format!("did you mean '{}'?", suggestion));
                            }
                            diag
                        })?;
                        if target_ty.is_error() {
                            return Ok(Type::Error);
                        }
                        if target_ty != val_ty {
                            // Nullable auto-wrap: allow T or Nil assigned to T?
                            if let Type::Nullable(inner) = &target_ty
                                && (val_ty == **inner || val_ty == Type::Nil)
                            {
                                return Ok(target_ty);
                            }
                            // Subtype compatibility: allow Dog assigned to Animal
                            if Self::is_subtype_compatible(&val_ty, &target_ty, &self.env) {
                                return Ok(target_ty);
                            }
                            return Err(Diagnostic::error(format!(
                                "Assignment type mismatch: variable '{}' is {}, got {}",
                                name, target_ty, val_ty
                            ))
                            .with_code("E001")
                            .with_label(stmt_span, format!("expected {}", target_ty)));
                        }
                        // Reassignment clears boundary-crossed status (new value)
                        self.boundary_crossed.remove(name);
                        Ok(val_ty)
                    }
                    Expr::Member { object, field, .. } => {
                        let obj_ty = self.check_expr(object)?;
                        if obj_ty.is_error() {
                            return Ok(Type::Error);
                        }
                        if let Type::Custom(class_name, _) = &obj_ty {
                            if let Some(info) = self.env.get_class(class_name) {
                                if let Some(field_ty) = info.fields.get(field) {
                                    if *field_ty != val_ty {
                                        // Nullable auto-wrap: allow T or Nil assigned to T?
                                        if let Type::Nullable(inner) = field_ty
                                            && (val_ty == **inner || val_ty == Type::Nil)
                                        {
                                            return Ok(field_ty.clone());
                                        }
                                        return Err(Diagnostic::error(format!(
                                            "Cannot assign {} to field '{}' of type {}",
                                            val_ty, field, field_ty
                                        ))
                                        .with_code("E001")
                                        .with_label(stmt_span, format!("expected {}", field_ty)));
                                    }
                                } else {
                                    return Err(Diagnostic::error(format!(
                                        "Class '{}' has no field '{}'",
                                        class_name, field
                                    ))
                                    .with_code("E010")
                                    .with_label(target.span(), "unknown field"));
                                }
                            } else {
                                return Err(Diagnostic::error(format!(
                                    "Unknown class '{}'",
                                    class_name
                                ))
                                .with_code("E010")
                                .with_label(object.span(), "unknown class"));
                            }
                        } else {
                            return Err(Diagnostic::error(format!(
                                "Cannot assign to member on non-class type {}",
                                obj_ty
                            ))
                            .with_code("E010")
                            .with_label(object.span(), "not a class type"));
                        }
                        Ok(val_ty)
                    }
                    Expr::Index { object, index, .. } => {
                        let obj_ty = self.check_expr(object)?;
                        let idx_ty = self.check_expr(index)?;
                        if obj_ty.is_error() || idx_ty.is_error() {
                            return Ok(Type::Error);
                        }
                        match &obj_ty {
                            Type::List(inner) => {
                                if idx_ty != Type::Int {
                                    return Err(Diagnostic::error(format!(
                                        "List index must be Int, got {}",
                                        idx_ty
                                    ))
                                    .with_code("E016")
                                    .with_label(index.span(), "expected Int"));
                                }
                                if **inner != val_ty {
                                    return Err(Diagnostic::error(format!(
                                        "Cannot assign {} to List[{}] element",
                                        val_ty, inner
                                    ))
                                    .with_code("E001")
                                    .with_label(stmt_span, format!("expected {}", inner)));
                                }
                                Ok(val_ty)
                            }
                            Type::Map(key_ty, map_val_ty) => {
                                if idx_ty != **key_ty {
                                    return Err(Diagnostic::error(format!(
                                        "Map key must be {}, got {}",
                                        key_ty, idx_ty
                                    ))
                                    .with_code("E016")
                                    .with_label(index.span(), format!("expected {}", key_ty)));
                                }
                                if **map_val_ty != val_ty {
                                    return Err(Diagnostic::error(format!(
                                        "Cannot assign {} to Map value type {}",
                                        val_ty, map_val_ty
                                    ))
                                    .with_code("E001")
                                    .with_label(stmt_span, format!("expected {}", map_val_ty)));
                                }
                                Ok(val_ty)
                            }
                            _ => Err(Diagnostic::error(format!(
                                "Cannot index-assign into {}",
                                obj_ty
                            ))
                            .with_code("E016")
                            .with_label(object.span(), "not a list or map")),
                        }
                    }
                    _ => Err(Diagnostic::error("Invalid assignment target".to_string())
                        .with_code("E008")
                        .with_label(target.span(), "invalid target")),
                }
            }
            Stmt::Break(span) => {
                if self.loop_depth == 0 {
                    return Err(
                        Diagnostic::error("'break' used outside of a loop".to_string())
                            .with_code("E003")
                            .with_label(*span, "not inside a loop"),
                    );
                }
                Ok(Type::Void)
            }
            Stmt::Continue(span) => {
                if self.loop_depth == 0 {
                    return Err(
                        Diagnostic::error("'continue' used outside of a loop".to_string())
                            .with_code("E003")
                            .with_label(*span, "not inside a loop"),
                    );
                }
                Ok(Type::Void)
            }
            Stmt::Use {
                path,
                names,
                alias,
                span,
                ..
            } => self.resolve_use(path, names, alias, span),
            Stmt::Enum {
                name,
                variants,
                includes,
                ..
            } => {
                let variant_names: Vec<String> = variants.iter().map(|v| v.name.clone()).collect();

                // Validate includes — extract base trait names
                let mut include_names = Vec::new();
                for (trait_name, type_args) in includes {
                    let trait_info = self.env.get_trait(trait_name).ok_or_else(|| {
                        Diagnostic::error(format!(
                            "Unknown trait '{}' in includes for enum '{}'",
                            trait_name, name
                        ))
                        .with_code("E014")
                    })?;
                    // Validate type argument arity for parametric traits
                    if let Some(ref gp) = trait_info.generic_params
                        && type_args.len() != gp.len()
                    {
                        return Err(Diagnostic::error(format!(
                            "Trait '{}' expects {} type parameter(s), got {}",
                            trait_name,
                            gp.len(),
                            type_args.len()
                        ))
                        .with_code("E014"));
                    }
                    include_names.push(trait_name.clone());
                }

                let info = EnumInfo {
                    name: name.clone(),
                    variants: variant_names,
                    includes: include_names,
                };
                self.env.set_enum(name.clone(), info);
                Ok(Type::Void)
            }
            Stmt::Const {
                name,
                type_ann,
                value,
                ..
            } => {
                // Validate that the value is a compile-time constant expression
                if !Self::is_const_expr(value) {
                    return Err(Diagnostic::error(format!(
                        "Const '{}' must be initialized with a compile-time constant",
                        name
                    ))
                    .with_code("E026")
                    .with_label(stmt_span, "not a constant expression"));
                }
                let val_ty = self.check_expr(value)?;
                if let Some(ann) = type_ann {
                    if !Self::types_compatible_with_env(ann, &val_ty, &self.env) {
                        return Err(Diagnostic::error(format!(
                            "Type annotation mismatch for const '{}': declared {}, got {}",
                            name, ann, val_ty
                        ))
                        .with_code("E001")
                        .with_label(stmt_span, format!("expected {}", ann)));
                    }
                    self.env.set_var(name.clone(), ann.clone());
                } else {
                    self.env.set_var(name.clone(), val_ty);
                }
                self.const_names.insert(name.clone());
                Ok(Type::Void)
            }
        }
    }

    /// Walk the ancestor chain for a class, returning ClassInfos in order (parent first).
    /// Stops on cycle detection. Does NOT include the class itself.
    pub(crate) fn walk_ancestors(&self, start_class: &str) -> Vec<ClassInfo> {
        let mut result = Vec::new();
        let mut visited = std::collections::HashSet::new();
        visited.insert(start_class.to_string());
        let mut current_name = self
            .env
            .get_class(start_class)
            .and_then(|info| info.extends.clone());
        while let Some(ref cname) = current_name {
            if !visited.insert(cname.clone()) {
                break; // cycle
            }
            if let Some(ancestor) = self.env.get_class(cname) {
                let next = ancestor.extends.clone();
                result.push(ancestor);
                current_name = next;
            } else {
                break;
            }
        }
        result
    }

    /// Check if `child_ty` is a subtype of `parent_ty` via the extends hierarchy.
    pub(crate) fn is_error_subtype(&self, child_ty: &Type, parent_ty: &Type) -> bool {
        if child_ty == parent_ty {
            return true;
        }
        let child_name = match child_ty {
            Type::Custom(n, _) => n,
            _ => return false,
        };
        let parent_name = match parent_ty {
            Type::Custom(n, _) => n,
            _ => return false,
        };
        for ancestor in self.walk_ancestors(child_name) {
            if let Type::Custom(ref n, _) = ancestor.ty
                && n == parent_name
            {
                return true;
            }
        }
        false
    }

    /// Compare types for compatibility, ignoring param_names on Function types.
    pub(crate) fn types_compatible_with_env(a: &Type, b: &Type, env: &TypeEnv) -> bool {
        match (a, b) {
            (
                Type::Function {
                    params: ap,
                    ret: ar,
                    throws: at,
                    ..
                },
                Type::Function {
                    params: bp,
                    ret: br,
                    throws: bt,
                    ..
                },
            ) => ap == bp && ar == br && at == bt,
            // S3: Check subtype relationship for Custom types
            (Type::Custom(an, _), Type::Custom(bn, _)) if an != bn => {
                Self::is_subtype_of(bn, an, env)
            }
            _ => a == b,
        }
    }

    /// Build a ModuleExports containing only the named builtin traits and enums.
    fn builtin_exports_from(
        &self,
        trait_names: &[&str],
        enum_names: &[&str],
    ) -> crate::module_loader::ModuleExports {
        let mut exports = crate::module_loader::ModuleExports {
            variables: HashMap::new(),
            classes: HashMap::new(),
            traits: HashMap::new(),
            enums: HashMap::new(),
        };
        for &name in trait_names {
            if let Some(t) = self.builtin_traits.get(name) {
                exports.traits.insert(name.to_string(), t.clone());
            }
        }
        for &name in enum_names {
            if let Some(e) = self.builtin_enums.get(name) {
                exports.enums.insert(name.to_string(), e.clone());
            }
        }
        exports
    }

    /// Build exports for a std submodule. Returns None if submodule name is unknown.
    fn builtin_std_submodule_exports(
        &self,
        submodule: &str,
    ) -> Option<crate::module_loader::ModuleExports> {
        match submodule {
            "cmp" => Some(self.builtin_exports_from(&["Eq", "Ord"], &["Ordering"])),
            "fmt" => Some(self.builtin_exports_from(&["Printable"], &[])),
            "collections" => Some(self.builtin_exports_from(&["Iterable", "Iterator"], &[])),
            "convert" => Some(self.builtin_exports_from(&["From", "Into"], &[])),
            "random" => Some(self.builtin_exports_from(&["Random"], &[])),
            _ => None,
        }
    }

    /// Build exports for the entire "std" module (all submodules merged).
    fn builtin_std_exports(&self) -> crate::module_loader::ModuleExports {
        crate::module_loader::ModuleExports {
            variables: HashMap::new(),
            classes: HashMap::new(),
            traits: (*self.builtin_traits).clone(),
            enums: (*self.builtin_enums).clone(),
        }
    }

    /// Resolve a `use` statement by loading the target module and injecting exports.
    fn resolve_use(
        &mut self,
        path: &[String],
        names: &Option<Vec<String>>,
        alias: &Option<String>,
        span: &ast::Span,
    ) -> Result<Type, Diagnostic> {
        // Handle built-in std modules — always available, no module loader needed
        if !path.is_empty() && path[0] == "std" {
            if path.len() == 1 {
                // `use std` or `use std { ... }` — all submodules merged
                let exports = self.builtin_std_exports();
                return self.apply_imports(&exports, "std", names, alias, span);
            }
            if path.len() == 2 {
                // `use std/cmp { Eq }` etc.
                let submodule = &path[1];
                if let Some(exports) = self.builtin_std_submodule_exports(submodule) {
                    let module_key = format!("std/{}", submodule);
                    return self.apply_imports(&exports, &module_key, names, alias, span);
                }
                // Unknown std submodule — fall through to module loader
            }
        }

        let loader_rc = match &self.module_loader {
            Some(loader) => Rc::clone(loader),
            None => return Ok(Type::Void), // No loader — ignore use (prelude mode)
        };

        let exports = ModuleLoader::load_module(&loader_rc, path, *span)?;
        let module_key = path.join("/");
        self.apply_imports(&exports, &module_key, names, alias, span)
    }

    /// Apply imports from a ModuleExports into the current environment.
    fn apply_imports(
        &mut self,
        exports: &crate::module_loader::ModuleExports,
        module_key: &str,
        names: &Option<Vec<String>>,
        alias: &Option<String>,
        span: &ast::Span,
    ) -> Result<Type, Diagnostic> {
        match (names, alias) {
            (Some(_), Some(_)) => {
                // Selective + alias is not allowed
                Err(Diagnostic::error(
                    "Cannot combine selective imports { ... } with 'as' alias".to_string(),
                )
                .with_code("M004")
                .with_label(*span, "use either { names } or 'as alias', not both"))
            }
            (Some(selected_names), None) => {
                // Selective import: use foo { Bar, baz }
                for name in selected_names {
                    if !self.inject_export(name, exports) {
                        return Err(Diagnostic::error(format!(
                            "'{}' is not exported by module '{}'",
                            name, module_key
                        ))
                        .with_code("M002")
                        .with_label(*span, format!("'{}' not found in module", name)));
                    }
                }
                Ok(Type::Void)
            }
            (None, Some(alias_name)) => {
                // Namespace import: use foo as ns
                let ns = ast::NamespaceInfo {
                    variables: exports.variables.clone(),
                    classes: exports.classes.clone(),
                    traits: exports.traits.clone(),
                    enums: exports.enums.clone(),
                };
                self.env.set_namespace(alias_name.clone(), ns);
                Ok(Type::Void)
            }
            (None, None) => {
                // Wildcard import: use foo — import all pub items
                self.inject_all_exports(exports);
                Ok(Type::Void)
            }
        }
    }

    /// Inject all exports from a module into the current environment.
    fn inject_all_exports(&mut self, exports: &crate::module_loader::ModuleExports) {
        for (name, ty) in &exports.variables {
            self.env.set_var(name.clone(), ty.clone());
        }
        for (name, info) in &exports.classes {
            self.env.set_class(name.clone(), info.clone());
        }
        for (name, info) in &exports.traits {
            self.env.set_trait(name.clone(), info.clone());
        }
        for (name, info) in &exports.enums {
            self.env.set_enum(name.clone(), info.clone());
        }
    }

    /// Try to inject a single named export into the current environment.
    /// Returns false if the name wasn't found in any export category.
    fn inject_export(&mut self, name: &str, exports: &crate::module_loader::ModuleExports) -> bool {
        let mut found = false;
        if let Some(info) = exports.classes.get(name) {
            self.env.set_class(name.to_string(), info.clone());
            found = true;
        }
        if let Some(info) = exports.traits.get(name) {
            self.env.set_trait(name.to_string(), info.clone());
            found = true;
        }
        if let Some(info) = exports.enums.get(name) {
            self.env.set_enum(name.to_string(), info.clone());
            found = true;
        }
        if let Some(ty) = exports.variables.get(name) {
            self.env.set_var(name.to_string(), ty.clone());
            found = true;
        }
        found
    }

    pub(crate) fn check_body(&mut self, body: &[Stmt]) -> Result<Type, Diagnostic> {
        let mut last = Type::Void;
        for s in body {
            last = self.check_stmt(s)?;
        }
        Ok(last)
    }

    pub(crate) fn check_match_pattern(
        &self,
        pattern: &MatchPattern,
        scrutinee_ty: &Type,
    ) -> Result<(), Diagnostic> {
        match pattern {
            MatchPattern::Wildcard(_) | MatchPattern::Ident(..) => Ok(()),
            MatchPattern::Literal(expr, span) => {
                let pat_ty = match &**expr {
                    Expr::Int(..) => Type::Int,
                    Expr::Float(..) => Type::Float,
                    Expr::Str(..) => Type::String,
                    Expr::Bool(..) => Type::Bool,
                    Expr::Nil(_) => Type::Nil,
                    _ => {
                        return Err(Diagnostic::error(
                            "Invalid literal in match pattern".to_string(),
                        )
                        .with_code("E005")
                        .with_label(*span, "invalid pattern"));
                    }
                };
                if matches!(scrutinee_ty, Type::Nullable(_)) {
                    if pat_ty == Type::Nil {
                        return Ok(());
                    }
                    if let Type::Nullable(inner) = scrutinee_ty
                        && pat_ty == **inner
                    {
                        return Ok(());
                    }
                }
                if pat_ty != *scrutinee_ty {
                    return Err(Diagnostic::error(format!(
                        "Pattern type {} does not match scrutinee type {}",
                        pat_ty, scrutinee_ty
                    ))
                    .with_code("E001")
                    .with_label(*span, format!("expected {}", scrutinee_ty)));
                }
                Ok(())
            }
            MatchPattern::EnumVariant {
                enum_name,
                variant,
                span,
            } => {
                // Check the enum exists
                let enum_info = self.env.get_enum(enum_name).ok_or_else(|| {
                    Diagnostic::error(format!("Unknown enum '{}'", enum_name))
                        .with_code("E002")
                        .with_label(*span, "unknown enum")
                })?;
                // Check the variant exists
                if !enum_info.variants.contains(&variant.to_string()) {
                    return Err(Diagnostic::error(format!(
                        "Unknown variant '{}' on enum '{}'",
                        variant, enum_name
                    ))
                    .with_code("E002")
                    .with_label(*span, format!("unknown variant on {}", enum_name)));
                }
                // Check enum type matches scrutinee type (unwrap Nullable if present)
                let expected_enum_ty = Type::Custom(enum_name.clone(), Vec::new());
                let scrutinee_unwrapped = match scrutinee_ty {
                    Type::Nullable(inner) => inner.as_ref(),
                    other => other,
                };
                if *scrutinee_unwrapped != expected_enum_ty {
                    return Err(Diagnostic::error(format!(
                        "Pattern type mismatch: expected {}, got {}",
                        scrutinee_ty, enum_name
                    ))
                    .with_code("E001")
                    .with_label(*span, format!("expected {}", scrutinee_ty)));
                }
                Ok(())
            }
        }
    }

    /// Returns true if the expression is a valid compile-time constant.
    fn is_const_expr(expr: &Expr) -> bool {
        match expr {
            Expr::Int(..) | Expr::Float(..) | Expr::Str(..) | Expr::Bool(..) | Expr::Nil(_) => true,
            Expr::UnaryOp { operand, .. } => Self::is_const_expr(operand),
            Expr::BinaryOp { left, right, .. } => {
                Self::is_const_expr(left) && Self::is_const_expr(right)
            }
            Expr::ListLiteral(elems, _) => elems.iter().all(Self::is_const_expr),
            Expr::StringInterpolation { parts, .. } => parts.iter().all(|p| match p {
                ast::StringPart::Literal(_) => true,
                ast::StringPart::Expr(e) => Self::is_const_expr(e),
            }),
            _ => false,
        }
    }

    pub(crate) fn suggest_similar_name(&self, name: &str) -> Option<String> {
        let mut best: Option<(usize, &str)> = None;
        for known in self.env.all_var_names() {
            let dist = Self::levenshtein(name, known);
            let dominated = best.as_ref().is_none_or(|(d, _)| dist < *d);
            if dist <= 2 && dist < name.len() && dominated {
                best = Some((dist, known));
            }
        }
        best.map(|(_, s)| s.to_string())
    }

    pub(crate) fn levenshtein(a: &str, b: &str) -> usize {
        let a = a.as_bytes();
        let b = b.as_bytes();
        let n = b.len();
        let mut prev: Vec<usize> = (0..=n).collect();
        let mut curr = vec![0usize; n + 1];
        for (i, &a_byte) in a.iter().enumerate() {
            curr[0] = i + 1;
            for (j, &b_byte) in b.iter().enumerate() {
                let cost = if a_byte == b_byte { 0 } else { 1 };
                curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
            }
            std::mem::swap(&mut prev, &mut curr);
        }
        prev[n]
    }

    /// Returns true if the expression has an obvious, unambiguous type from its literal form.
    fn is_obviously_typed(expr: &Expr, env: &TypeEnv) -> bool {
        match expr {
            Expr::Int(..) | Expr::Float(..) | Expr::Str(..) | Expr::Bool(..) => true,
            Expr::ListLiteral(elems, _) => {
                !elems.is_empty() && elems.iter().all(|e| Self::is_obviously_typed(e, env))
            }
            Expr::UnaryOp {
                op: ast::UnaryOp::Neg,
                operand,
                ..
            } => matches!(**operand, Expr::Int(..) | Expr::Float(..)),
            Expr::Call { func, .. } => {
                if let Expr::Ident(name, _) = func.as_ref() {
                    env.get_class(name).is_some()
                } else {
                    false
                }
            }
            _ => false,
        }
    }
}
