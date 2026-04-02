use ast::templates::DiagnosticTemplate;
use ast::templates::module_errors::{
    CircularImport, InvalidImportAlias, JitRequired, SymbolNotExported, UnstableRequired,
};
use ast::templates::type_errors::{
    ArgumentTypeMismatch, ConditionTypeError, ConstReassignment, ConstraintError, ControlFlowError,
    IndexTypeError, InvalidAssignment, MissingIterable, ReturnTypeMismatch, TraitError,
    TypeMismatch, UndeclaredAssignment, UndefinedVariable, UnknownField,
};
use ast::templates::warnings::{JitNotEnabled, RedundantTypeAnnotation, ShadowedVariable};
use ast::{
    ClassInfo, Diagnostic, EnumInfo, Expr, MatchPattern, Span, Stmt, SymbolIndex, SymbolInfo,
    SymbolKind, TraitInfo, Type, TypeEnv, TypeTable,
};
use indexmap::IndexMap;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::module_loader::ModuleLoader;

pub struct ScopeContext {
    pub loop_depth: usize,
    pub expected_return_type: Option<Type>,
    /// Current function name for better error messages.
    pub current_function: Option<String>,
    /// The error type this function declares via `throws`.
    pub throws_type: Option<Type>,
    /// Expected type from context (e.g., let binding type annotation, function arg type).
    /// Used to resolve ambiguous parametric trait methods like `.into()`.
    pub(crate) expected_type: Option<Type>,
    /// Names of const bindings — cannot be reassigned.
    pub(crate) const_names: std::collections::HashSet<String>,
    /// Detectable single-consumer tracking for task bindings resolved in the current checker.
    pub(crate) consumed_tasks: std::collections::HashSet<String>,
    /// Task bindings created in the current scope via `let t = async f()`.
    /// Maps variable name to creation span for must-consume enforcement.
    pub(crate) task_bindings: HashMap<String, Span>,
    /// Set by `check_call_inner` to indicate whether the resolved callee was suspendable.
    /// Read by `check_blocking_call` to avoid re-evaluating the func expression.
    pub(crate) last_call_suspendable: bool,
    /// Variables that have crossed a thread boundary (passed to `async f()`).
    /// Maps variable name to the span where the crossing happened.
    /// Used for data sharing warnings (W002).
    pub(crate) boundary_crossed: HashMap<String, Span>,
    /// Current class being checked (set during check_class_methods).
    /// Used for DynamicReceiver bare-call routing.
    pub(crate) current_class: Option<String>,
}

pub struct TypeRegistry {
    /// Accumulated diagnostics from error recovery.
    pub diagnostics: Vec<Diagnostic>,
    /// Built-in protocol traits (Eq, Ord, Printable, etc.) — source of truth for `use std`.
    /// In prelude mode (no loader), these are also copied to env.
    /// Wrapped in Rc since these are read-only after initialization; avoids cloning on every child scope.
    pub(crate) builtin_traits: Rc<HashMap<String, TraitInfo>>,
    /// Built-in enum types (Ordering) — source of truth for `use std`.
    /// Wrapped in Rc since these are read-only after initialization.
    pub(crate) builtin_enums: Rc<HashMap<String, EnumInfo>>,
    /// For functions with default parameters: maps function name -> set of param names that have defaults.
    pub(crate) default_params: HashMap<String, std::collections::HashSet<String>>,
    /// Tracks List[Nil] → List[T] promotions from `.push()` calls.
    /// Persists across child scopes (not saved/restored by ScopeState),
    /// so promotions inside if/while/for are visible after the block exits.
    pub(crate) nil_promotions: HashMap<String, Type>,
    /// Class names imported from other modules. Used to enforce field/method visibility.
    pub(crate) imported_classes: std::collections::HashSet<String>,
}

pub struct TypeChecker {
    pub env: TypeEnv,
    pub sc: ScopeContext,
    pub reg: TypeRegistry,
    /// Maps expression spans to their resolved types. Consumed by FIR lowerer.
    pub type_table: TypeTable,
    /// Maps use-site spans to their resolved symbol information.
    /// Populated during typechecking; consumed by LSP server for hover, go-to-def, etc.
    pub symbol_index: SymbolIndex,
    /// Optional module loader for resolving `use` imports.
    /// When None, `use` statements are ignored (prelude mode).
    pub module_loader: Option<Rc<RefCell<ModuleLoader>>>,
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
            sc: ScopeContext {
                loop_depth: 0,
                expected_return_type: None,
                current_function: None,
                throws_type: None,
                expected_type: None,
                const_names: std::collections::HashSet::new(),
                consumed_tasks: std::collections::HashSet::new(),
                task_bindings: HashMap::new(),
                boundary_crossed: HashMap::new(),
                last_call_suspendable: false,
                current_class: None,
            },
            reg: TypeRegistry {
                diagnostics: Vec::new(),
                builtin_traits,
                builtin_enums,
                default_params: HashMap::new(),
                nil_promotions: HashMap::new(),
                imported_classes: std::collections::HashSet::new(),
            },
            module_loader: None,
            type_table: TypeTable::new(),
            symbol_index: SymbolIndex::new(),
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
        env.set_var_type(
            "log".into(),
            Type::func(vec!["message".into()], vec![Type::String], Type::Void),
        );
        env.set_var_type(
            "say".into(),
            Type::func(vec!["message".into()], vec![Type::String], Type::Void),
        );
        // Note: `len`, `to_string`, and `random` are handled as polymorphic
        // builtins in check_call_inner rather than registered here, because
        // their type signatures depend on context.
        env.set_var_type("random".into(), Type::func(vec![], vec![], Type::Int));

        // Built-in error hierarchy: Exception (root) -> Error (app base)
        env.set_class(
            "Exception".into(),
            ClassInfo::new(
                Type::Custom("Exception".into(), Vec::new()),
                IndexMap::from([("message".into(), Type::String)]),
                HashMap::new(),
            ),
        );
        env.set_var_type(
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
        env.set_var_type(
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
        env.set_var_type(
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
            env.set_var_type(
                name.into(),
                Type::func(
                    vec!["message".into()],
                    vec![Type::String],
                    Type::Custom(name.into(), Vec::new()),
                ),
            );
        }

        // FunctionNotFound — built-in error thrown by method_missing to signal a closed set
        env.set_class("FunctionNotFound".into(), {
            let mut info = ClassInfo::new(
                Type::Custom("FunctionNotFound".into(), Vec::new()),
                IndexMap::from([("name".into(), Type::String)]),
                HashMap::new(),
            );
            info.extends = Some("Error".into());
            info
        });
        env.set_var_type(
            "FunctionNotFound".into(),
            Type::func(
                vec!["message".into(), "name".into()],
                vec![Type::String, Type::String],
                Type::Custom("FunctionNotFound".into(), Vec::new()),
            ),
        );

        // EvalError — thrown by std/runtime evaluate() on compile or runtime failure
        env.set_class("EvalError".into(), {
            let mut info = ClassInfo::new(
                Type::Custom("EvalError".into(), Vec::new()),
                IndexMap::from([
                    ("kind".into(), Type::String),
                    ("message".into(), Type::String),
                ]),
                HashMap::new(),
            );
            info.extends = Some("Error".into());
            info.pub_fields = HashSet::from(["kind".into(), "message".into()]);
            info
        });
        env.set_var_type(
            "EvalError".into(),
            Type::func(
                vec!["kind".into(), "message".into()],
                vec![Type::String, Type::String],
                Type::Custom("EvalError".into(), Vec::new()),
            ),
        );

        // ProcessError — thrown by std/process run() on spawn failure
        env.set_class("ProcessError".into(), {
            let mut info = ClassInfo::new(
                Type::Custom("ProcessError".into(), Vec::new()),
                IndexMap::from([("command".into(), Type::String)]),
                HashMap::new(),
            );
            info.extends = Some("Error".into());
            info
        });
        env.set_var_type(
            "ProcessError".into(),
            Type::func(
                vec!["message".into(), "command".into()],
                vec![Type::String, Type::String],
                Type::Custom("ProcessError".into(), Vec::new()),
            ),
        );

        // ProcessResult — returned by std/process run()
        env.set_class(
            "ProcessResult".into(),
            ClassInfo::new(
                Type::Custom("ProcessResult".into(), Vec::new()),
                IndexMap::from([
                    ("exit_code".into(), Type::Int),
                    ("stdout".into(), Type::String),
                    ("stderr".into(), Type::String),
                ]),
                HashMap::new(),
            ),
        );

        // I/O namespaces — static methods only, no instances
        for name in ["File", "TcpListener", "TcpStream"] {
            env.set_var_type(name.into(), Type::Custom(name.into(), Vec::new()));
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

        // ── Introspection built-in types ────────────────────────────────
        // Type: represents a class/type as a runtime value (comparable, stringifiable)
        env.set_class(
            "Type".into(),
            ClassInfo::new(
                Type::Custom("Type".into(), Vec::new()),
                IndexMap::new(),
                HashMap::from([("to_string".into(), Type::func(vec![], vec![], Type::String))]),
            ),
        );

        // FieldInfo: describes a field on a class instance
        env.set_class(
            "FieldInfo".into(),
            ClassInfo::new(
                Type::Custom("FieldInfo".into(), Vec::new()),
                IndexMap::from([
                    ("name".into(), Type::String),
                    ("type_name".into(), Type::Custom("Type".into(), Vec::new())),
                    ("is_public".into(), Type::Bool),
                ]),
                HashMap::new(),
            ),
        );

        // ParamInfo: describes a parameter on a method
        env.set_class(
            "ParamInfo".into(),
            ClassInfo::new(
                Type::Custom("ParamInfo".into(), Vec::new()),
                IndexMap::from([
                    ("name".into(), Type::String),
                    ("param_type".into(), Type::Custom("Type".into(), Vec::new())),
                    ("has_default".into(), Type::Bool),
                ]),
                HashMap::new(),
            ),
        );

        // MethodInfo: describes a method on a class instance
        env.set_class(
            "MethodInfo".into(),
            ClassInfo::new(
                Type::Custom("MethodInfo".into(), Vec::new()),
                IndexMap::from([
                    ("name".into(), Type::String),
                    (
                        "params".into(),
                        Type::List(Box::new(Type::Custom("ParamInfo".into(), Vec::new()))),
                    ),
                    (
                        "return_type".into(),
                        Type::Custom("Type".into(), Vec::new()),
                    ),
                    ("is_public".into(), Type::Bool),
                ]),
                HashMap::new(),
            ),
        );

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
                variant_fields: HashMap::new(),
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

        // DynamicReceiver — opt-in trait for dynamic dispatch via method_missing.
        // The required method signature is validated structurally in check_class.rs
        // (first param must be String, second must be Map[String, T]).
        builtin_traits.insert(
            "DynamicReceiver".into(),
            TraitInfo {
                name: "DynamicReceiver".into(),
                methods: HashMap::from([(
                    "method_missing".into(),
                    // Placeholder type: actual validation is structural (check_class.rs)
                    Type::func(
                        vec!["fn_name".into(), "args".into()],
                        vec![
                            Type::String,
                            Type::Map(Box::new(Type::String), Box::new(Type::String)),
                        ],
                        Type::Void,
                    ),
                )]),
                required_methods: vec!["method_missing".into()],
                generic_params: None,
            },
        );

        // FieldAccessible — unstable trait for dynamic field access by name.
        // Auto-generates a FieldValue enum and field_value(name: String) -> FieldValue? method.
        // Validated structurally in check_class.rs (like DynamicReceiver).
        builtin_traits.insert(
            "FieldAccessible".into(),
            TraitInfo {
                name: "FieldAccessible".into(),
                methods: HashMap::from([(
                    "field_value".into(),
                    // Placeholder type: actual return type is class-specific (auto-generated)
                    Type::func(vec!["name".into()], vec![Type::String], Type::Void),
                )]),
                required_methods: vec!["field_value".into()],
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

    /// Create a TypeChecker pre-populated from a [`ContextSnapshot`].
    ///
    /// When a runtime `evaluate()` call compiles a code string, this
    /// constructor sets up the typechecker so that the evaluated code
    /// sees the class context, local variables, and functions that were
    /// in scope at the call site.
    pub fn from_snapshot(snapshot: &ast::ContextSnapshot) -> Self {
        let mut tc = Self::new();

        // Pre-populate class context
        if let Some(class_name) = &snapshot.current_class {
            tc.sc.current_class = Some(class_name.clone());

            if let Some(ci) = &snapshot.class_info {
                use indexmap::IndexMap;

                let field_map: IndexMap<String, Type> = ci.fields.iter().cloned().collect();
                let method_map: HashMap<String, Type> = ci.methods.clone();

                let mut class_info = ClassInfo::new(
                    Type::Custom(class_name.clone(), Vec::new()),
                    field_map,
                    method_map.clone(),
                );

                if let Some(dr) = &ci.dynamic_receiver {
                    class_info.dynamic_receiver = Some(ast::DynamicReceiverInfo {
                        args_value_ty: dr.args_value_ty.clone(),
                        return_ty: dr.return_ty.clone(),
                        known_names: dr
                            .known_names
                            .as_ref()
                            .map(|names| names.iter().cloned().collect()),
                    });
                }

                tc.env.set_class(class_name.clone(), class_info);

                // Register "self" so evaluated code can access self.field
                tc.env.set_var_type(
                    "self".to_string(),
                    Type::Custom(class_name.clone(), Vec::new()),
                );

                // Register class fields as local variables (mirroring check_class_stmt)
                for (fname, fty) in &ci.fields {
                    tc.env.set_var_type(fname.clone(), fty.clone());
                }

                // Register class methods as callable bare functions
                for (mname, mty) in &method_map {
                    tc.env.set_var_type(mname.clone(), mty.clone());
                }
            }

            // Register method default params so the typechecker accepts
            // calls with missing args that have defaults.
            for (qualified, defaults) in &snapshot.method_defaults {
                let default_set: std::collections::HashSet<String> = defaults
                    .iter()
                    .filter(|(_, d)| d.is_some())
                    .map(|(name, _)| name.clone())
                    .collect();
                if !default_set.is_empty() {
                    // Register with both qualified name (Seedfile.override) and
                    // short name (override) so bare calls find defaults too.
                    tc.reg
                        .default_params
                        .insert(qualified.clone(), default_set.clone());
                    if let Some(short) = qualified.split('.').next_back() {
                        tc.reg.default_params.insert(short.to_string(), default_set);
                    }
                }
            }
        }

        // Pre-populate local variables
        for (name, ty) in &snapshot.variables {
            tc.env.set_var_type(name.clone(), ty.clone());
        }

        // Pre-populate functions
        for (name, ty) in &snapshot.functions {
            tc.env.set_var_type(name.clone(), ty.clone());
        }

        tc
    }

    /// Create a TypeChecker with a module loader for resolving `use` imports.
    /// Protocol traits are NOT in scope -- they must be imported via `use std/cmp { Eq }` etc.
    pub fn with_loader(loader: Rc<RefCell<ModuleLoader>>) -> Self {
        let mut tc = Self::new();
        tc.enable_module_loader(loader);
        tc
    }

    /// Attach a module loader and remove protocol traits that require explicit import.
    /// Used by both `with_loader()` and `from_snapshot()` when `allow_imports` is set.
    pub fn enable_module_loader(&mut self, loader: Rc<RefCell<ModuleLoader>>) {
        self.module_loader = Some(loader);
        // Remove protocol traits from env — require `use std/<submodule>` import
        for name in [
            "Eq",
            "Ord",
            "Printable",
            "From",
            "Into",
            "Iterable",
            "Iterator",
            "Random",
            "FieldAccessible",
            // Drop and Close stay in prelude — they're fundamental lifecycle traits
        ] {
            self.env.remove_trait(name);
        }
        self.env.remove_enum("Ordering");
    }

    /// Create a child TypeChecker that inherits context flags and a child scope.
    /// Moves the parent env into the child (zero-copy). The caller MUST call
    /// `restore_from_child` when the child checker is no longer needed to
    /// move the env back.
    pub(crate) fn child_checker(&mut self) -> TypeChecker {
        TypeChecker {
            env: std::mem::take(&mut self.env).into_child(),
            sc: ScopeContext {
                loop_depth: self.sc.loop_depth,
                expected_return_type: self.sc.expected_return_type.clone(),
                current_function: self.sc.current_function.clone(),
                throws_type: self.sc.throws_type.clone(),
                expected_type: self.sc.expected_type.clone(),
                const_names: self.sc.const_names.clone(),
                consumed_tasks: self.sc.consumed_tasks.clone(),
                task_bindings: self.sc.task_bindings.clone(),
                boundary_crossed: HashMap::new(),
                last_call_suspendable: false,
                current_class: self.sc.current_class.clone(),
            },
            reg: TypeRegistry {
                diagnostics: Vec::new(),
                builtin_traits: self.reg.builtin_traits.clone(),
                builtin_enums: self.reg.builtin_enums.clone(),
                default_params: self.reg.default_params.clone(),
                nil_promotions: HashMap::new(),
                imported_classes: self.reg.imported_classes.clone(),
            },
            module_loader: self.module_loader.clone(),
            type_table: TypeTable::new(),
            symbol_index: SymbolIndex::new(),
        }
    }

    /// Restore the parent env from a child checker after it is done.
    /// Merges diagnostics, type_table, and imported_classes, then recovers the parent env.
    pub(crate) fn restore_from_child(&mut self, mut child: TypeChecker) {
        self.reg
            .diagnostics
            .extend(std::mem::take(&mut child.reg.diagnostics));
        self.type_table
            .extend(std::mem::take(&mut child.type_table));
        self.symbol_index
            .extend(std::mem::take(&mut child.symbol_index));
        self.reg
            .imported_classes
            .extend(std::mem::take(&mut child.reg.imported_classes));
        child.env.exit_scope();
        self.env = child.env;
    }

    /// Emit a W003 warning if `name` already exists in a parent scope.
    pub(crate) fn warn_if_shadowed(&mut self, name: &str, span: Span) {
        if self.env.parent_has_var(name) {
            self.reg.diagnostics.push(
                Diagnostic::warning(format!("variable '{}' shadows a previous binding", name))
                    .with_template(DiagnosticTemplate::ShadowedVariable(ShadowedVariable {
                        name: name.to_string(),
                    }))
                    .with_label(span, "shadows earlier binding"),
            );
        }
    }

    fn save_scope_state(&mut self) -> ScopeState {
        ScopeState {
            loop_depth: self.sc.loop_depth,
            expected_return_type: self.sc.expected_return_type.clone(),
            current_function: self.sc.current_function.clone(),
            throws_type: self.sc.throws_type.clone(),
            diagnostics: std::mem::take(&mut self.reg.diagnostics),
            expected_type: self.sc.expected_type.clone(),
            const_names: self.sc.const_names.clone(),
            consumed_tasks: self.sc.consumed_tasks.clone(),
            task_bindings: self.sc.task_bindings.clone(),
        }
    }

    fn restore_scope_state(&mut self, saved: ScopeState) {
        let child_diagnostics = std::mem::take(&mut self.reg.diagnostics);
        let child_task_bindings = std::mem::take(&mut self.sc.task_bindings);

        self.sc.loop_depth = saved.loop_depth;
        self.sc.expected_return_type = saved.expected_return_type;
        self.sc.current_function = saved.current_function;
        self.sc.throws_type = saved.throws_type;
        self.reg.diagnostics = saved.diagnostics;
        self.sc.expected_type = saved.expected_type;
        self.sc.const_names = saved.const_names;
        self.sc.consumed_tasks = saved.consumed_tasks;
        self.sc.task_bindings = saved.task_bindings;

        self.sc.task_bindings.extend(child_task_bindings);
        self.reg.diagnostics.extend(child_diagnostics);
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
                self.reg.diagnostics.push(d);
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
                self.env.set_var_type(name.clone(), fn_type);
            }
        }

        self.infer_suspendable_functions(m);

        // Intermediate pass: resolve return types for functions with inferred returns.
        // Uses fixpoint iteration — keeps re-checking until all return types stabilize,
        // handling cases where function A calls function B and both have inferred returns.
        let mut resolved_fns = std::collections::HashSet::new();
        {
            let inferred_fns: Vec<usize> = m
                .body
                .iter()
                .enumerate()
                .filter_map(|(i, s)| {
                    if let Stmt::Let {
                        name,
                        value: Expr::Lambda { .. },
                        ..
                    } = s
                        && let Some(Type::Function { ret, .. }) = self.env.get_var_type(name)
                        && matches!(ret.as_ref(), Type::Inferred)
                    {
                        return Some(i);
                    }
                    None
                })
                .collect();

            if !inferred_fns.is_empty() {
                // Save diagnostics accumulated before the fixpoint loop so they
                // are not lost when we clear intermediate fixpoint results.
                let saved_diagnostics = std::mem::take(&mut self.reg.diagnostics);
                let saved_nil_promotions = std::mem::take(&mut self.reg.nil_promotions);

                // Fixpoint: keep checking until no return types change
                for _ in 0..inferred_fns.len() + 1 {
                    let mut changed = false;
                    self.reg.diagnostics.clear();
                    self.reg.nil_promotions.clear();
                    for &idx in &inferred_fns {
                        let s = &m.body[idx];
                        let name = match s {
                            Stmt::Let { name, .. } => name,
                            _ => continue,
                        };
                        match self.check_stmt(s) {
                            Ok(_) => {}
                            Err(diag) => {
                                self.reg.diagnostics.push(diag);
                            }
                        }
                        // Check if the return type was resolved from Inferred
                        if let Some(Type::Function { ret, .. }) = self.env.get_var_type(name)
                            && !matches!(ret.as_ref(), Type::Inferred)
                        {
                            changed = true;
                        }
                        resolved_fns.insert(name.clone());
                    }
                    if !changed {
                        break;
                    }
                }
                // Restore pre-fixpoint diagnostics; discard intermediate fixpoint
                // diagnostics since the final pass will re-check everything.
                self.reg.diagnostics = saved_diagnostics;
                self.reg.nil_promotions = saved_nil_promotions;
            }
        }

        // Second pass: typecheck all statements (function bodies can now see all signatures).
        for s in &m.body {
            match self.check_stmt(s) {
                Ok(_) => {}
                Err(diag) => {
                    self.reg.diagnostics.push(diag);
                    // For let bindings that failed, assign Type::Error so later code doesn't cascade
                    if let ast::Stmt::Let { name, .. } = s {
                        self.env.set_var_type(name.clone(), Type::Error);
                    }
                }
            }
        }
        let all = std::mem::take(&mut self.reg.diagnostics);
        let mut errors = Vec::new();
        for d in all {
            if d.severity == ast::Severity::Error {
                errors.push(d);
            } else {
                self.reg.diagnostics.push(d);
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
                }) = self.env.get_var_type(name).cloned()
                else {
                    continue;
                };
                if suspendable {
                    continue;
                }
                self.env.set_var_type(
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
                .get_var_type(name)
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
            } => self.check_let_stmt(name, type_ann.as_ref(), value, stmt_span),
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
                        self.env.set_var_type(
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
                        return Err(Diagnostic::from_template(DiagnosticTemplate::TraitError(
                            TraitError {
                                message: format!("Unexpected stmt in trait methods: {:?}", m),
                            },
                        ))
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
            Stmt::Return(expr, span) => self.check_return_stmt(expr, *span),
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
                    return Err(
                        Diagnostic::from_template(DiagnosticTemplate::ConditionTypeError(
                            ConditionTypeError {
                                actual: cond_ty.clone(),
                            },
                        ))
                        .with_label(cond.span(), "expected Bool"),
                    );
                }

                self.with_child_scope(|tc| tc.check_body(then_body))?;
                for (elif_cond, elif_body) in elif_branches {
                    let elif_cond_ty = self.check_expr(elif_cond)?;
                    if elif_cond_ty != Type::Bool && !elif_cond_ty.is_error() {
                        return Err(Diagnostic::from_template(
                            DiagnosticTemplate::ConditionTypeError(ConditionTypeError {
                                actual: elif_cond_ty.clone(),
                            }),
                        )
                        .with_label(elif_cond.span(), "expected Bool"));
                    }
                    self.with_child_scope(|tc| tc.check_body(elif_body))?;
                }
                self.with_child_scope(|tc| tc.check_body(else_body))
            }
            Stmt::While { cond, body, .. } => self.check_while_stmt(cond, body),
            Stmt::For {
                var, iter, body, ..
            } => self.check_for_stmt(var, iter, body, stmt_span),
            Stmt::Assignment { target, value, .. } => {
                self.check_assignment_stmt(target, value, stmt_span)
            }
            Stmt::Break(span) => {
                if self.sc.loop_depth == 0 {
                    return Err(
                        Diagnostic::from_template(DiagnosticTemplate::ControlFlowError(
                            ControlFlowError {
                                keyword: "break".to_string(),
                            },
                        ))
                        .with_label(*span, "not inside a loop"),
                    );
                }
                Ok(Type::Void)
            }
            Stmt::Continue(span) => {
                if self.sc.loop_depth == 0 {
                    return Err(
                        Diagnostic::from_template(DiagnosticTemplate::ControlFlowError(
                            ControlFlowError {
                                keyword: "continue".to_string(),
                            },
                        ))
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
                let variant_fields: HashMap<String, Vec<(String, Type)>> = variants
                    .iter()
                    .filter(|v| !v.fields.is_empty())
                    .map(|v| (v.name.clone(), v.fields.clone()))
                    .collect();

                // Validate includes — extract base trait names
                let mut include_names = Vec::new();
                for (trait_name, type_args) in includes {
                    let trait_info = self.env.get_trait(trait_name).ok_or_else(|| {
                        Diagnostic::from_template(DiagnosticTemplate::TraitError(TraitError {
                            message: format!(
                                "Unknown trait '{}' in includes for enum '{}'",
                                trait_name, name
                            ),
                        }))
                    })?;
                    // Validate type argument arity for parametric traits
                    if let Some(ref gp) = trait_info.generic_params
                        && type_args.len() != gp.len()
                    {
                        return Err(Diagnostic::from_template(DiagnosticTemplate::TraitError(
                            TraitError {
                                message: format!(
                                    "Trait '{}' expects {} type parameter(s), got {}",
                                    trait_name,
                                    gp.len(),
                                    type_args.len()
                                ),
                            },
                        )));
                    }
                    include_names.push(trait_name.clone());
                }

                let info = EnumInfo {
                    name: name.clone(),
                    variants: variant_names,
                    includes: include_names,
                    variant_fields,
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
                    return Err(
                        Diagnostic::from_template(DiagnosticTemplate::ConstReassignment(
                            ConstReassignment { name: name.clone() },
                        ))
                        .with_label(stmt_span, "not a constant expression"),
                    );
                }
                let val_ty = self.check_expr(value)?;
                if let Some(ann) = type_ann {
                    if !Self::types_compatible_with_env(ann, &val_ty, &self.env) {
                        return Err(Diagnostic::from_template(DiagnosticTemplate::TypeMismatch(
                            TypeMismatch {
                                expected: ann.clone(),
                                actual: val_ty.clone(),
                            },
                        ))
                        .with_label(stmt_span, format!("expected {}", ann)));
                    }
                    self.env.set_var_type(name.clone(), ann.clone());
                } else {
                    self.env.set_var_type(name.clone(), val_ty);
                }
                self.sc.const_names.insert(name.clone());
                Ok(Type::Void)
            }
        }
    }

    // -------------------------------------------------------------------------
    // check_stmt helpers — extracted from the match arms of check_stmt
    // -------------------------------------------------------------------------

    fn check_let_stmt(
        &mut self,
        name: &str,
        type_ann: Option<&Type>,
        value: &Expr,
        stmt_span: Span,
    ) -> Result<Type, Diagnostic> {
        let prev_fn = self.sc.current_function.clone();
        if matches!(value, Expr::Lambda { .. }) {
            self.sc.current_function = Some(name.to_string());
        }
        // If the value is a lambda with inferred types and we have a type annotation,
        // propagate the expected type for inference.
        // Also set expected_type for parametric trait resolution (e.g., .into())
        let prev_expected = self.sc.expected_type.take();
        if let Some(ann) = type_ann {
            self.validate_collection_eq_constraint(ann, stmt_span)?;
            self.sc.expected_type = Some(ann.clone());
        }
        let mut ty = if matches!(value, Expr::Lambda { .. }) {
            self.check_lambda_with_expected(value, type_ann)?
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
            ) = (&ty, self.env.get_var_type(name))
        {
            ty = Type::Function {
                param_names: param_names.clone(),
                params: params.clone(),
                ret: ret.clone(),
                throws: throws.clone(),
                suspendable: true,
            };
        }
        self.sc.expected_type = prev_expected;
        self.sc.current_function = prev_fn;
        if ty.is_error() {
            self.env.set_var_type(name.to_string(), Type::Error);
            return Ok(Type::Error);
        }
        if let Some(ann) = type_ann {
            // Empty list takes on the annotated type
            if ty == Type::List(Box::new(Type::Nil)) && matches!(ann, Type::List(_)) {
                self.env.set_var_type(name.to_string(), ann.clone());
                return Ok(ann.clone());
            }
            // Empty map takes on the annotated type
            if ty == Type::Map(Box::new(Type::Error), Box::new(Type::Error))
                && matches!(ann, Type::Map(_, _))
            {
                self.env.set_var_type(name.to_string(), ann.clone());
                return Ok(ann.clone());
            }
            // Nullable auto-wrap: T or Nil assigned to T?
            if let Type::Nullable(inner) = ann {
                if ty == *ann || ty == **inner || ty == Type::Nil {
                    self.env.set_var_type(name.to_string(), ann.clone());
                    return Ok(ann.clone());
                }
                return Err(Diagnostic::from_template(DiagnosticTemplate::TypeMismatch(
                    TypeMismatch {
                        expected: ann.clone(),
                        actual: ty.clone(),
                    },
                ))
                .with_label(stmt_span, format!("expected {}", ann)));
            }
            // Nil cannot be assigned to non-nullable types
            if ty == Type::Nil && !matches!(ann, Type::Nil) {
                return Err(Diagnostic::from_template(DiagnosticTemplate::TypeMismatch(
                    TypeMismatch {
                        expected: ann.clone(),
                        actual: ty.clone(),
                    },
                ))
                .with_label(stmt_span, format!("expected {}", ann)));
            }
            if !Self::types_compatible_with_env(ann, &ty, &self.env) {
                return Err(Diagnostic::from_template(DiagnosticTemplate::TypeMismatch(
                    TypeMismatch {
                        expected: ann.clone(),
                        actual: ty.clone(),
                    },
                ))
                .with_label(stmt_span, format!("expected {}", ann)));
            }
            // W001: warn when a type annotation is redundant (matches inferred type)
            if Self::is_obviously_typed(value, &self.env) && *ann == ty {
                self.reg.diagnostics.push(
                    Diagnostic::warning(format!(
                        "redundant type annotation: type `{}` can be inferred",
                        ann
                    ))
                    .with_template(
                        DiagnosticTemplate::RedundantTypeAnnotation(RedundantTypeAnnotation {
                            type_name: ann.to_string(),
                        }),
                    ),
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
                self.reg
                    .default_params
                    .insert(name.to_string(), default_set);
            }
        }
        self.warn_if_shadowed(name, stmt_span);
        self.env
            .set_var_with_span(name.to_string(), ty.clone(), stmt_span);
        // Record the definition site in the symbol index.
        let kind = if matches!(ty, Type::Function { .. }) {
            SymbolKind::Function
        } else {
            SymbolKind::Variable
        };
        self.symbol_index.insert(
            stmt_span,
            SymbolInfo {
                name: name.to_string(),
                ty: ty.clone(),
                def_span: Some(stmt_span),
                kind,
            },
        );
        // Track Task[T] bindings for must-consume enforcement
        if matches!(ty, Type::Task(_)) {
            self.sc.task_bindings.insert(name.to_string(), stmt_span);
        }
        Ok(ty)
    }

    fn check_return_stmt(&mut self, expr: &Expr, span: Span) -> Result<Type, Diagnostic> {
        // Set expected_type from return type for inference (e.g., empty list literals)
        let prev_expected = self.sc.expected_type.take();
        if let Some(ret) = &self.sc.expected_return_type {
            self.sc.expected_type = Some(ret.clone());
        }
        let ty = self.check_expr(expr)?;
        self.sc.expected_type = prev_expected;
        if ty.is_error() {
            return Ok(Type::Error);
        }
        // Mark returned task idents as consumed (caller takes responsibility)
        self.mark_task_ident_consumed(expr);
        // List[Nil] is compatible with any List[T] (empty list)
        let is_nil_list_compat = matches!(
            (&ty, &self.sc.expected_return_type),
            (Type::List(inner), Some(Type::List(_))) if **inner == Type::Nil
        );
        if let Some(expected) = &self.sc.expected_return_type
            && ty != *expected
            && !is_nil_list_compat
            && !Self::is_nullable_compatible(expected, &ty)
            && !Self::is_subtype_compatible(&ty, expected, &self.env)
        {
            let ctx = self.sc.current_function.as_deref().unwrap_or("<anonymous>");
            return Err(
                Diagnostic::from_template(DiagnosticTemplate::ReturnTypeMismatch(
                    ReturnTypeMismatch {
                        function: ctx.to_string(),
                        expected: expected.clone(),
                        actual: ty.clone(),
                    },
                ))
                .with_label(span, format!("expected {}", expected)),
            );
        }
        Ok(ty)
    }

    fn check_while_stmt(&mut self, cond: &Expr, body: &[Stmt]) -> Result<Type, Diagnostic> {
        let cond_ty = self.check_expr(cond)?;
        if cond_ty != Type::Bool && !cond_ty.is_error() {
            return Err(
                Diagnostic::from_template(DiagnosticTemplate::ConditionTypeError(
                    ConditionTypeError {
                        actual: cond_ty.clone(),
                    },
                ))
                .with_label(cond.span(), "expected Bool"),
            );
        }
        self.with_child_scope(|tc| {
            tc.sc.loop_depth += 1;
            tc.check_body(body)
        })
    }

    fn check_for_stmt(
        &mut self,
        var: &str,
        iter: &Expr,
        body: &[Stmt],
        stmt_span: Span,
    ) -> Result<Type, Diagnostic> {
        let iter_ty = self.check_expr(iter)?;
        if iter_ty.is_error() {
            return self.with_child_scope(|tc| {
                tc.sc.loop_depth += 1;
                tc.env.set_var_type(var.to_string(), Type::Error);
                tc.check_body(body)?;
                Ok(Type::Void)
            });
        }
        let elem_ty = match iter_ty {
            Type::List(inner) | Type::Set(inner) => *inner,
            Type::Custom(ref class_name, _) => {
                if let Some(class_info) = self.env.get_class(class_name) {
                    if class_info.includes.contains(&"Iterable".to_string()) {
                        Self::get_iterable_element_type_from_class(class_info).ok_or_else(|| {
                            Diagnostic::from_template(DiagnosticTemplate::MissingIterable(
                                MissingIterable {
                                    type_name: class_name.clone(),
                                },
                            ))
                            .with_label(iter.span(), "missing each() method")
                        })?
                    } else if class_info.includes.contains(&"Iterator".to_string()) {
                        Self::get_iterator_element_type_from_class(class_info).ok_or_else(|| {
                            Diagnostic::from_template(DiagnosticTemplate::MissingIterable(
                                MissingIterable {
                                    type_name: class_name.clone(),
                                },
                            ))
                            .with_label(iter.span(), "missing next() method")
                        })?
                    } else {
                        return Err(Diagnostic::from_template(
                            DiagnosticTemplate::MissingIterable(MissingIterable {
                                type_name: class_name.clone(),
                            }),
                        )
                        .with_label(iter.span(), "does not include Iterable or Iterator"));
                    }
                } else {
                    return Err(
                        Diagnostic::from_template(DiagnosticTemplate::MissingIterable(
                            MissingIterable {
                                type_name: iter_ty.to_string(),
                            },
                        ))
                        .with_label(iter.span(), "expected List, Iterable, or Iterator"),
                    );
                }
            }
            _ => {
                return Err(
                    Diagnostic::from_template(DiagnosticTemplate::MissingIterable(
                        MissingIterable {
                            type_name: iter_ty.to_string(),
                        },
                    ))
                    .with_label(iter.span(), "expected List, Iterable, or Iterator"),
                );
            }
        };
        self.with_child_scope(|tc| {
            tc.sc.loop_depth += 1;
            tc.warn_if_shadowed(var, stmt_span);
            tc.env.set_var_type(var.to_string(), elem_ty);
            tc.check_body(body)
        })
    }

    fn check_assignment_stmt(
        &mut self,
        target: &Expr,
        value: &Expr,
        stmt_span: Span,
    ) -> Result<Type, Diagnostic> {
        let val_ty = self.check_expr(value)?;
        if val_ty.is_error() {
            return Ok(Type::Error);
        }
        match target {
            Expr::Ident(name, ident_span) => {
                // Check if the variable is a const binding
                if self.sc.const_names.contains(name) {
                    return Err(
                        Diagnostic::from_template(DiagnosticTemplate::ConstReassignment(
                            ConstReassignment { name: name.clone() },
                        ))
                        .with_label(*ident_span, "const binding cannot be reassigned"),
                    );
                }
                let target_ty = self.env.get_var_type(name).cloned().ok_or_else(|| {
                    let mut diag =
                        Diagnostic::from_template(DiagnosticTemplate::UndeclaredAssignment(
                            UndeclaredAssignment { name: name.clone() },
                        ))
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
                    return Err(Diagnostic::from_template(DiagnosticTemplate::TypeMismatch(
                        TypeMismatch {
                            expected: target_ty.clone(),
                            actual: val_ty.clone(),
                        },
                    ))
                    .with_label(stmt_span, format!("expected {}", target_ty)));
                }
                // Reassignment clears boundary-crossed status (new value)
                self.sc.boundary_crossed.remove(name);
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
                                return Err(Diagnostic::from_template(
                                    DiagnosticTemplate::TypeMismatch(TypeMismatch {
                                        expected: field_ty.clone(),
                                        actual: val_ty.clone(),
                                    }),
                                )
                                .with_label(stmt_span, format!("expected {}", field_ty)));
                            }
                        } else {
                            return Err(Diagnostic::from_template(
                                DiagnosticTemplate::UnknownField(UnknownField {
                                    field: field.clone(),
                                    type_name: class_name.clone(),
                                }),
                            )
                            .with_label(target.span(), "unknown field"));
                        }
                    } else {
                        return Err(Diagnostic::from_template(DiagnosticTemplate::UnknownField(
                            UnknownField {
                                field: class_name.clone(),
                                type_name: class_name.clone(),
                            },
                        ))
                        .with_label(object.span(), "unknown class"));
                    }
                } else {
                    return Err(Diagnostic::from_template(DiagnosticTemplate::UnknownField(
                        UnknownField {
                            field: obj_ty.to_string(),
                            type_name: obj_ty.to_string(),
                        },
                    ))
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
                            return Err(Diagnostic::from_template(
                                DiagnosticTemplate::IndexTypeError(IndexTypeError {
                                    actual: idx_ty.clone(),
                                }),
                            )
                            .with_label(index.span(), "expected Int"));
                        }
                        if **inner != val_ty {
                            return Err(Diagnostic::from_template(
                                DiagnosticTemplate::TypeMismatch(TypeMismatch {
                                    expected: *inner.clone(),
                                    actual: val_ty.clone(),
                                }),
                            )
                            .with_label(stmt_span, format!("expected {}", inner)));
                        }
                        Ok(val_ty)
                    }
                    Type::Map(key_ty, map_val_ty) => {
                        if idx_ty != **key_ty {
                            return Err(Diagnostic::from_template(
                                DiagnosticTemplate::IndexTypeError(IndexTypeError {
                                    actual: idx_ty.clone(),
                                }),
                            )
                            .with_label(index.span(), format!("expected {}", key_ty)));
                        }
                        if **map_val_ty != val_ty {
                            return Err(Diagnostic::from_template(
                                DiagnosticTemplate::TypeMismatch(TypeMismatch {
                                    expected: *map_val_ty.clone(),
                                    actual: val_ty.clone(),
                                }),
                            )
                            .with_label(stmt_span, format!("expected {}", map_val_ty)));
                        }
                        Ok(val_ty)
                    }
                    _ => Err(
                        Diagnostic::from_template(DiagnosticTemplate::IndexTypeError(
                            IndexTypeError {
                                actual: obj_ty.clone(),
                            },
                        ))
                        .with_label(object.span(), "not a list or map"),
                    ),
                }
            }
            _ => Err(
                Diagnostic::from_template(DiagnosticTemplate::InvalidAssignment(
                    InvalidAssignment {},
                ))
                .with_label(target.span(), "invalid target"),
            ),
        }
    }

    // -------------------------------------------------------------------------
    // End of check_stmt helpers
    // -------------------------------------------------------------------------

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
                result.push(ancestor.clone());
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
        Self::is_subtype_of(child_name, parent_name, &self.env)
    }

    /// Check if a type is hashable (can be used as Set element or Map key).
    /// Primitives and custom types with Eq are hashable.
    /// Containers (List, Set, Map) are NOT hashable because there is no runtime
    /// hash implementation for them. This is distinct from Eq protocol inclusion,
    /// which allows auto-derive through container fields.
    pub(crate) fn type_is_hashable(&self, ty: &Type) -> bool {
        match ty {
            Type::Int | Type::Float | Type::String | Type::Bool => true,
            Type::Nil => true,
            Type::Custom(..) => self.type_includes_eq(ty),
            Type::List(_) | Type::Set(_) | Type::Map(_, _) => false,
            Type::Task(_) | Type::Nullable(_) | Type::Function { .. } => false,
            Type::Error => true,
            _ => false,
        }
    }

    /// Validate that Set[T] and Map[K, V] element/key types are hashable.
    fn validate_collection_eq_constraint(&self, ty: &Type, span: Span) -> Result<(), Diagnostic> {
        match ty {
            Type::Set(inner) => {
                if !self.type_is_hashable(inner) {
                    return Err(
                        Diagnostic::from_template(DiagnosticTemplate::ConstraintError(
                            ConstraintError {
                                message: format!(
                                    "Set element type {} does not include Eq. \
                             Add 'includes Eq' to use as a Set element",
                                    inner
                                ),
                            },
                        ))
                        .with_label(span, "Set requires Eq on element type"),
                    );
                }
            }
            Type::Map(key, _) => {
                if !self.type_is_hashable(key) {
                    return Err(
                        Diagnostic::from_template(DiagnosticTemplate::ConstraintError(
                            ConstraintError {
                                message: format!(
                                    "Map key type {} does not include Eq. \
                             Add 'includes Eq' to use as a Map key",
                                    key
                                ),
                            },
                        ))
                        .with_label(span, "Map requires Eq on key type"),
                    );
                }
            }
            _ => {}
        }
        Ok(())
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
            // S3: Check subtype relationship for Custom types.
            // Generic containers are invariant: Box[Dog] is NOT compatible with
            // Box[Animal] even if Dog extends Animal. Only bare (non-generic)
            // custom types participate in subtype compatibility.
            (Type::Custom(an, a_args), Type::Custom(bn, b_args)) if an != bn => {
                if !a_args.is_empty() || !b_args.is_empty() {
                    return false;
                }
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
            if let Some(t) = self.reg.builtin_traits.get(name) {
                exports.traits.insert(name.to_string(), t.clone());
            }
        }
        for &name in enum_names {
            if let Some(e) = self.reg.builtin_enums.get(name) {
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
            "unstable" => Some(self.builtin_exports_from(&["FieldAccessible"], &[])),
            "sys" => Some(self.builtin_function_exports(&[
                (
                    "args",
                    Type::func(vec![], vec![], Type::List(Box::new(Type::String))),
                ),
                (
                    "env",
                    Type::Function {
                        param_names: vec!["key".into()],
                        params: vec![Type::String],
                        ret: Box::new(Type::Nullable(Box::new(Type::String))),
                        throws: None,
                        suspendable: false,
                    },
                ),
                (
                    "set_env",
                    Type::func(
                        vec!["key".into(), "value".into()],
                        vec![Type::String, Type::String],
                        Type::Void,
                    ),
                ),
                (
                    "exit",
                    Type::func(vec!["code".into()], vec![Type::Int], Type::Void),
                ),
            ])),
            "fs" => {
                let io_err = || Some(Box::new(Type::Custom("IOError".into(), Vec::new())));
                Some(self.builtin_function_exports(&[
                    (
                        "read_file",
                        Type::Function {
                            param_names: vec!["path".into()],
                            params: vec![Type::String],
                            ret: Box::new(Type::String),
                            throws: io_err(),
                            suspendable: false,
                        },
                    ),
                    (
                        "write_file",
                        Type::Function {
                            param_names: vec!["path".into(), "content".into()],
                            params: vec![Type::String, Type::String],
                            ret: Box::new(Type::Void),
                            throws: io_err(),
                            suspendable: false,
                        },
                    ),
                    (
                        "append_file",
                        Type::Function {
                            param_names: vec!["path".into(), "content".into()],
                            params: vec![Type::String, Type::String],
                            ret: Box::new(Type::Void),
                            throws: io_err(),
                            suspendable: false,
                        },
                    ),
                    (
                        "exists",
                        Type::func(vec!["path".into()], vec![Type::String], Type::Bool),
                    ),
                    (
                        "is_dir",
                        Type::func(vec!["path".into()], vec![Type::String], Type::Bool),
                    ),
                    (
                        "mkdir",
                        Type::Function {
                            param_names: vec!["path".into()],
                            params: vec![Type::String],
                            ret: Box::new(Type::Void),
                            throws: io_err(),
                            suspendable: false,
                        },
                    ),
                    (
                        "remove",
                        Type::Function {
                            param_names: vec!["path".into()],
                            params: vec![Type::String],
                            ret: Box::new(Type::Void),
                            throws: io_err(),
                            suspendable: false,
                        },
                    ),
                    (
                        "list_dir",
                        Type::Function {
                            param_names: vec!["path".into()],
                            params: vec![Type::String],
                            ret: Box::new(Type::List(Box::new(Type::String))),
                            throws: io_err(),
                            suspendable: false,
                        },
                    ),
                    (
                        "copy",
                        Type::Function {
                            param_names: vec!["src".into(), "dst".into()],
                            params: vec![Type::String, Type::String],
                            ret: Box::new(Type::Void),
                            throws: io_err(),
                            suspendable: false,
                        },
                    ),
                    (
                        "rename",
                        Type::Function {
                            param_names: vec!["src".into(), "dst".into()],
                            params: vec![Type::String, Type::String],
                            ret: Box::new(Type::Void),
                            throws: io_err(),
                            suspendable: false,
                        },
                    ),
                ]))
            }
            "process" => {
                let proc_err = || Some(Box::new(Type::Custom("ProcessError".into(), Vec::new())));
                Some(self.builtin_function_exports(&[(
                    "run",
                    Type::Function {
                        param_names: vec!["cmd".into(), "args".into()],
                        params: vec![Type::String, Type::List(Box::new(Type::String))],
                        ret: Box::new(Type::Custom("ProcessResult".into(), Vec::new())),
                        throws: proc_err(),
                        suspendable: false,
                    },
                )]))
            }
            "crypto" => Some(self.builtin_function_exports(&[(
                "sha256",
                Type::func(vec!["data".into()], vec![Type::String], Type::String),
            )])),
            "runtime" => {
                let eval_err = || Some(Box::new(Type::Custom("EvalError".into(), Vec::new())));
                let eval_sig = |throws_fn: &dyn Fn() -> Option<Box<Type>>| Type::Function {
                    param_names: vec!["code".into()],
                    params: vec![Type::String],
                    ret: Box::new(Type::Void),
                    throws: throws_fn(),
                    suspendable: false,
                };
                let mut exports = self.builtin_function_exports(&[
                    (
                        "jit_run",
                        Type::func(vec!["code".into()], vec![Type::String], Type::Int),
                    ),
                    ("evaluate", eval_sig(&eval_err)),
                    ("evaluate_unrestricted", eval_sig(&eval_err)),
                ]);
                // Export EvalError class so callers can use it in catch arms
                exports.classes.insert(
                    "EvalError".into(),
                    self.env.get_class("EvalError").cloned().unwrap(),
                );
                Some(exports)
            }
            _ => None,
        }
    }

    /// Build a ModuleExports containing only function variables (no traits/enums/classes).
    fn builtin_function_exports(
        &self,
        functions: &[(&str, Type)],
    ) -> crate::module_loader::ModuleExports {
        let mut exports = crate::module_loader::ModuleExports {
            variables: HashMap::new(),
            classes: HashMap::new(),
            traits: HashMap::new(),
            enums: HashMap::new(),
        };
        for (name, ty) in functions {
            exports.variables.insert((*name).to_string(), ty.clone());
        }
        exports
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
                // Bare `use std` is no longer supported — require submodule paths
                let hint = match names {
                    Some(ns) => {
                        let suggestions: Vec<String> = ns
                            .iter()
                            .map(|n| {
                                let sub = match n.as_str() {
                                    "Eq" | "Ord" | "Ordering" => "cmp",
                                    "Printable" => "fmt",
                                    "Iterable" | "Iterator" => "collections",
                                    "From" | "Into" => "convert",
                                    "Random" => "random",
                                    _ => "cmp",
                                };
                                format!("use std/{} {{ {} }}", sub, n)
                            })
                            .collect();
                        format!("Import from a submodule instead: {}", suggestions.join(", "))
                    }
                    None => "Import from a submodule: use std/cmp { Eq }, use std/fmt { Printable }, use std/collections { Iterable }, use std/convert { From }, use std/random { Random }, use std/sys { args }, use std/fs { read_file }, use std/process { run }, use std/crypto { sha256 }".to_string(),
                };
                let mut diag =
                    Diagnostic::from_template(DiagnosticTemplate::CircularImport(CircularImport {
                        module: "std".to_string(),
                    }))
                    .with_label(*span, "bare `use std` is not supported");
                diag.message = hint;
                return Err(diag);
            }
            // Gate std/unstable imports behind the --unstable flag.
            if path.len() >= 2 && path[1] == "unstable" {
                let unstable_enabled = self
                    .module_loader
                    .as_ref()
                    .is_some_and(|loader| loader.borrow().unstable);
                if !unstable_enabled {
                    return Err(
                        Diagnostic::from_template(DiagnosticTemplate::UnstableRequired(
                            UnstableRequired {},
                        ))
                        .with_label(*span, "requires --unstable flag or ASTER_UNSTABLE=1"),
                    );
                }
            }
            // Gate std/runtime JIT functions behind the --jit flag.
            if path.len() >= 2 && path[1] == "runtime" {
                let jit_enabled = self
                    .module_loader
                    .as_ref()
                    .is_some_and(|loader| loader.borrow().jit);
                if !jit_enabled {
                    // Check if the user selectively imports JIT-requiring functions
                    let jit_functions = ["evaluate", "evaluate_unrestricted", "jit_run"];
                    if let Some(selected) = names {
                        for name in selected {
                            if jit_functions.contains(&name.as_str()) {
                                return Err(Diagnostic::from_template(
                                    DiagnosticTemplate::JitRequired(JitRequired {}),
                                )
                                .with_label(*span, format!("'{}' requires --jit flag", name)));
                            }
                        }
                    } else {
                        // Wildcard import of std/runtime without --jit: warn
                        self.reg.diagnostics.push(
                            Diagnostic::from_template(DiagnosticTemplate::JitNotEnabled(
                                JitNotEnabled {},
                            ))
                            .with_label(*span, "std/runtime imported without --jit"),
                        );
                    }
                }
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
                Err(
                    Diagnostic::from_template(DiagnosticTemplate::InvalidImportAlias(
                        InvalidImportAlias {},
                    ))
                    .with_label(*span, "use either { names } or 'as alias', not both"),
                )
            }
            (Some(selected_names), None) => {
                // Selective import: use foo { Bar, baz }
                for name in selected_names {
                    if !self.inject_export(name, exports) {
                        return Err(Diagnostic::from_template(
                            DiagnosticTemplate::SymbolNotExported(SymbolNotExported {
                                symbol: name.clone(),
                                module: module_key.to_string(),
                            }),
                        )
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
            self.env.set_var_type(name.clone(), ty.clone());
        }
        for (name, info) in &exports.classes {
            self.env.set_class(name.clone(), info.clone());
            self.reg.imported_classes.insert(name.clone());
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
            self.reg.imported_classes.insert(name.to_string());
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
            self.env.set_var_type(name.to_string(), ty.clone());
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
                        return Err(Diagnostic::from_template(
                            DiagnosticTemplate::ArgumentTypeMismatch(ArgumentTypeMismatch {
                                param: "pattern".to_string(),
                                expected: Type::Int,
                                actual: Type::Void,
                            }),
                        )
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
                    return Err(Diagnostic::from_template(DiagnosticTemplate::TypeMismatch(
                        TypeMismatch {
                            expected: scrutinee_ty.clone(),
                            actual: pat_ty.clone(),
                        },
                    ))
                    .with_label(*span, format!("expected {}", scrutinee_ty)));
                }
                Ok(())
            }
            MatchPattern::EnumVariant {
                enum_name,
                variant,
                span,
                bindings,
            } => {
                // Check the enum exists
                let enum_info = self.env.get_enum(enum_name).ok_or_else(|| {
                    Diagnostic::from_template(DiagnosticTemplate::UndefinedVariable(
                        UndefinedVariable {
                            name: enum_name.clone(),
                        },
                    ))
                    .with_label(*span, "unknown enum")
                })?;
                // Check the variant exists
                if !enum_info.variants.contains(&variant.to_string()) {
                    return Err(
                        Diagnostic::from_template(DiagnosticTemplate::UndefinedVariable(
                            UndefinedVariable {
                                name: format!("{}::{}", enum_name, variant),
                            },
                        ))
                        .with_label(*span, format!("unknown variant on {}", enum_name)),
                    );
                }
                // Validate bindings against the variant's fields
                if !bindings.is_empty() {
                    let variant_fields = enum_info
                        .variant_fields
                        .get(variant.as_str())
                        .map(|v| v.as_slice())
                        .unwrap_or(&[]);
                    if variant_fields.is_empty() {
                        return Err(Diagnostic::from_template(DiagnosticTemplate::TypeMismatch(
                            TypeMismatch {
                                expected: Type::Custom(
                                    format!("{}.{}", enum_name, variant),
                                    Vec::new(),
                                ),
                                actual: Type::Custom(
                                    format!("{}.{} (no fields to destructure)", enum_name, variant),
                                    Vec::new(),
                                ),
                            },
                        ))
                        .with_label(
                            *span,
                            format!("variant '{}' has no fields to destructure", variant),
                        ));
                    }
                    if bindings.len() > variant_fields.len() {
                        return Err(Diagnostic::from_template(DiagnosticTemplate::TypeMismatch(
                            TypeMismatch {
                                expected: Type::Custom(
                                    format!("{} binding(s)", variant_fields.len()),
                                    Vec::new(),
                                ),
                                actual: Type::Custom(
                                    format!("{} binding(s)", bindings.len()),
                                    Vec::new(),
                                ),
                            },
                        ))
                        .with_label(
                            *span,
                            format!(
                                "variant '{}' has {} field(s) but {} binding(s) provided",
                                variant,
                                variant_fields.len(),
                                bindings.len()
                            ),
                        ));
                    }
                }
                // Check enum type matches scrutinee type (unwrap Nullable if present)
                let expected_enum_ty = Type::Custom(enum_name.clone(), Vec::new());
                let scrutinee_unwrapped = match scrutinee_ty {
                    Type::Nullable(inner) => inner.as_ref(),
                    other => other,
                };
                if *scrutinee_unwrapped != expected_enum_ty {
                    return Err(Diagnostic::from_template(DiagnosticTemplate::TypeMismatch(
                        TypeMismatch {
                            expected: scrutinee_ty.clone(),
                            actual: Type::Custom(enum_name.clone(), Vec::new()),
                        },
                    ))
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
