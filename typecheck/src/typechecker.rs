use ast::{ClassInfo, Diagnostic, EnumInfo, Expr, MatchPattern, Stmt, TraitInfo, Type, TypeEnv};
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
    /// When None, `use` statements are ignored (backward compatible).
    pub module_loader: Option<Rc<RefCell<ModuleLoader>>>,
}

impl Default for TypeChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeChecker {
    pub fn new() -> Self {
        let mut env = TypeEnv::new();
        // Builtins
        env.set_var(
            "log".into(),
            Type::Function {
                param_names: vec!["message".into()],
                params: vec![Type::String],
                ret: Box::new(Type::Void),
                throws: None,
            },
        );
        env.set_var(
            "print".into(),
            Type::Function {
                param_names: vec!["message".into()],
                params: vec![Type::String],
                ret: Box::new(Type::Void),
                throws: None,
            },
        );
        // Note: `len` and `to_string` are handled as polymorphic builtins
        // in check_call_inner rather than registered here, because their
        // type signatures depend on the argument type.

        // Built-in error hierarchy: Exception (root) -> Error (app base)
        env.set_class(
            "Exception".into(),
            ClassInfo {
                ty: Type::Custom("Exception".into(), Vec::new()),
                fields: IndexMap::from([("message".into(), Type::String)]),
                methods: HashMap::new(),
                generic_params: None,
                extends: None,
                includes: Vec::new(),
            },
        );
        env.set_var(
            "Exception".into(),
            Type::Function {
                param_names: vec!["message".into()],
                params: vec![Type::String],
                ret: Box::new(Type::Custom("Exception".into(), Vec::new())),
                throws: None,
            },
        );
        env.set_class(
            "Error".into(),
            ClassInfo {
                ty: Type::Custom("Error".into(), Vec::new()),
                fields: IndexMap::new(), // inherits message from Exception
                methods: HashMap::new(),
                generic_params: None,
                extends: Some("Exception".into()),
                includes: Vec::new(),
            },
        );
        env.set_var(
            "Error".into(),
            Type::Function {
                param_names: vec!["message".into()], // inherited message field
                params: vec![Type::String],
                ret: Box::new(Type::Custom("Error".into(), Vec::new())),
                throws: None,
            },
        );
        // Built-in CancelledError for async task cancellation
        env.set_class(
            "CancelledError".into(),
            ClassInfo {
                ty: Type::Custom("CancelledError".into(), Vec::new()),
                fields: IndexMap::new(),
                methods: HashMap::new(),
                generic_params: None,
                extends: Some("Error".into()),
                includes: Vec::new(),
            },
        );
        env.set_var(
            "CancelledError".into(),
            Type::Function {
                param_names: vec!["message".into()],
                params: vec![Type::String],
                ret: Box::new(Type::Custom("CancelledError".into(), Vec::new())),
                throws: None,
            },
        );

        // Built-in Ordering enum
        env.set_enum(
            "Ordering".into(),
            EnumInfo {
                name: "Ordering".into(),
                variants: vec!["Less".into(), "Equal".into(), "Greater".into()],
                includes: vec!["Eq".into()],
            },
        );

        // Built-in Eq trait: def eq(other: Self) -> Bool
        env.set_trait(
            "Eq".into(),
            TraitInfo {
                name: "Eq".into(),
                methods: HashMap::from([(
                    "eq".into(),
                    Type::Function {
                        param_names: vec!["other".into()],
                        params: vec![Type::Custom("Self".into(), Vec::new())],
                        ret: Box::new(Type::Bool),
                        throws: None,
                    },
                )]),
                required_methods: vec!["eq".into()],
            },
        );

        // Built-in Ord trait: def cmp(other: Self) -> Ordering
        // Ord includes Eq — including Ord auto-includes Eq.
        env.set_trait(
            "Ord".into(),
            TraitInfo {
                name: "Ord".into(),
                methods: HashMap::from([(
                    "cmp".into(),
                    Type::Function {
                        param_names: vec!["other".into()],
                        params: vec![Type::Custom("Self".into(), Vec::new())],
                        ret: Box::new(Type::Custom("Ordering".into(), Vec::new())),
                        throws: None,
                    },
                )]),
                required_methods: vec!["cmp".into()],
            },
        );

        // Built-in Printable trait: def to_string() -> String, def debug() -> String
        // to_string() is required (or auto-derived). debug() defaults to to_string().
        env.set_trait(
            "Printable".into(),
            TraitInfo {
                name: "Printable".into(),
                methods: HashMap::from([
                    (
                        "to_string".into(),
                        Type::Function {
                            param_names: vec![],
                            params: vec![],
                            ret: Box::new(Type::String),
                            throws: None,
                        },
                    ),
                    (
                        "debug".into(),
                        Type::Function {
                            param_names: vec![],
                            params: vec![],
                            ret: Box::new(Type::String),
                            throws: None,
                        },
                    ),
                ]),
                // Only to_string is required — debug has a default (delegates to to_string)
                required_methods: vec!["to_string".into()],
            },
        );

        Self {
            env,
            loop_depth: 0,
            expected_return_type: None,
            current_function: None,
            throws_type: None,
            diagnostics: Vec::new(),
            module_loader: None,
        }
    }

    /// Create a TypeChecker with a module loader for resolving `use` imports.
    pub fn with_loader(loader: Rc<RefCell<ModuleLoader>>) -> Self {
        let mut tc = Self::new();
        tc.module_loader = Some(loader);
        tc
    }

    /// Create a child TypeChecker that inherits context flags and a child scope.
    pub(crate) fn child_checker(&self) -> TypeChecker {
        TypeChecker {
            env: self.env.child(),
            loop_depth: self.loop_depth,
            expected_return_type: self.expected_return_type.clone(),
            current_function: self.current_function.clone(),
            throws_type: self.throws_type.clone(),
            diagnostics: Vec::new(),
            module_loader: self.module_loader.clone(),
        }
    }

    pub fn check_module(&mut self, m: &ast::Module) -> Result<(), Diagnostic> {
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
        if self.diagnostics.is_empty() {
            Ok(())
        } else {
            // Return the first diagnostic for backward compatibility
            Err(self.diagnostics[0].clone())
        }
    }

    pub fn check_module_all(&mut self, m: &ast::Module) -> Vec<Diagnostic> {
        for s in &m.body {
            match self.check_stmt(s) {
                Ok(_) => {}
                Err(diag) => {
                    self.diagnostics.push(diag);
                    if let ast::Stmt::Let { name, .. } = s {
                        self.env.set_var(name.clone(), Type::Error);
                    }
                }
            }
        }
        std::mem::take(&mut self.diagnostics)
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
                let ty = if matches!(value, Expr::Lambda { .. }) {
                    self.check_lambda_with_expected(value, type_ann.as_ref())?
                } else {
                    self.check_expr(value)?
                };
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
                    // Nullable auto-wrap: T or Nil assigned to T?
                    if let Type::Nullable(inner) = ann {
                        if ty == *ann || ty == **inner || ty == Type::Nil {
                            self.env.set_var(name.clone(), ann.clone());
                            return Ok(ann.clone());
                        }
                        return Err(Diagnostic::error(format!(
                            "Type annotation mismatch for '{}': declared {:?}, got {:?}",
                            name, ann, ty
                        ))
                        .with_code("E001")
                        .with_label(stmt_span, format!("expected {:?}", ann)));
                    }
                    // Nil cannot be assigned to non-nullable types
                    if ty == Type::Nil && !matches!(ann, Type::Nil) {
                        return Err(Diagnostic::error(format!(
                            "Cannot assign nil to non-nullable type {:?}",
                            ann
                        ))
                        .with_code("E001")
                        .with_label(stmt_span, format!("expected {:?}", ann)));
                    }
                    if !Self::types_compatible(ann, &ty) {
                        return Err(Diagnostic::error(format!(
                            "Type annotation mismatch for '{}': declared {:?}, got {:?}",
                            name, ann, ty
                        ))
                        .with_code("E001")
                        .with_label(stmt_span, format!("expected {:?}", ann)));
                    }
                }
                self.env.set_var(name.clone(), ty.clone());
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
            Stmt::Trait { name, methods, .. } => {
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
                };
                self.env.set_trait(name.clone(), info);
                Ok(Type::Void)
            }
            Stmt::Return(expr, span) => {
                let ty = self.check_expr(expr)?;
                if ty.is_error() {
                    return Ok(Type::Error);
                }
                if let Some(expected) = &self.expected_return_type
                    && ty != *expected
                {
                    let ctx = self.current_function.as_deref().unwrap_or("<anonymous>");
                    return Err(Diagnostic::error(format!(
                        "Return type mismatch in '{}': expected {:?}, got {:?}",
                        ctx, expected, ty
                    ))
                    .with_code("E004")
                    .with_label(*span, format!("expected {:?}", expected)));
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
                        "If condition must be Bool, got {:?}",
                        cond_ty
                    ))
                    .with_code("E015")
                    .with_label(cond.span(), "expected Bool"));
                }

                self.child_checker().check_body(then_body)?;
                for (elif_cond, elif_body) in elif_branches {
                    let elif_cond_ty = self.check_expr(elif_cond)?;
                    if elif_cond_ty != Type::Bool && !elif_cond_ty.is_error() {
                        return Err(Diagnostic::error(format!(
                            "Elif condition must be Bool, got {:?}",
                            elif_cond_ty
                        ))
                        .with_code("E015")
                        .with_label(elif_cond.span(), "expected Bool"));
                    }
                    self.child_checker().check_body(elif_body)?;
                }
                self.child_checker().check_body(else_body)
            }
            Stmt::While { cond, body, .. } => {
                let cond_ty = self.check_expr(cond)?;
                if cond_ty != Type::Bool && !cond_ty.is_error() {
                    return Err(Diagnostic::error(format!(
                        "While condition must be Bool, got {:?}",
                        cond_ty
                    ))
                    .with_code("E015")
                    .with_label(cond.span(), "expected Bool"));
                }
                let mut sub = self.child_checker();
                sub.loop_depth += 1;
                sub.check_body(body)
            }
            Stmt::For {
                var, iter, body, ..
            } => {
                let iter_ty = self.check_expr(iter)?;
                if iter_ty.is_error() {
                    let mut sub = self.child_checker();
                    sub.loop_depth += 1;
                    sub.env.set_var(var.clone(), Type::Error);
                    sub.check_body(body)?;
                    return Ok(Type::Void);
                }
                let elem_ty = match iter_ty {
                    Type::List(inner) => *inner,
                    _ => {
                        return Err(Diagnostic::error(format!(
                            "Cannot iterate over {:?}, expected List",
                            iter_ty
                        ))
                        .with_code("E007")
                        .with_label(iter.span(), "expected List"));
                    }
                };
                let mut sub = self.child_checker();
                sub.loop_depth += 1;
                sub.env.set_var(var.clone(), elem_ty);
                sub.check_body(body)
            }
            Stmt::Assignment { target, value, .. } => {
                let val_ty = self.check_expr(value)?;
                if val_ty.is_error() {
                    return Ok(Type::Error);
                }
                match target {
                    Expr::Ident(name, ident_span) => {
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
                            return Err(Diagnostic::error(format!(
                                "Assignment type mismatch: variable '{}' is {:?}, got {:?}",
                                name, target_ty, val_ty
                            ))
                            .with_code("E001")
                            .with_label(stmt_span, format!("expected {:?}", target_ty)));
                        }
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
                                        return Err(Diagnostic::error(format!(
                                            "Cannot assign {:?} to field '{}' of type {:?}",
                                            val_ty, field, field_ty
                                        ))
                                        .with_code("E001")
                                        .with_label(
                                            stmt_span,
                                            format!("expected {:?}", field_ty),
                                        ));
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
                                "Cannot assign to member on non-class type {:?}",
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
                        if idx_ty != Type::Int {
                            return Err(Diagnostic::error(format!(
                                "Index must be Int, got {:?}",
                                idx_ty
                            ))
                            .with_code("E016")
                            .with_label(index.span(), "expected Int"));
                        }
                        match &obj_ty {
                            Type::List(inner) => {
                                if **inner != val_ty {
                                    return Err(Diagnostic::error(format!(
                                        "Cannot assign {:?} to List[{:?}] element",
                                        val_ty, inner
                                    ))
                                    .with_code("E001")
                                    .with_label(stmt_span, format!("expected {:?}", inner)));
                                }
                                Ok(val_ty)
                            }
                            _ => Err(Diagnostic::error(format!(
                                "Cannot index-assign into {:?}",
                                obj_ty
                            ))
                            .with_code("E016")
                            .with_label(object.span(), "not a list")),
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

                // Validate includes
                for trait_name in includes {
                    if self.env.get_trait(trait_name).is_none() {
                        return Err(Diagnostic::error(format!(
                            "Unknown trait '{}' in includes for enum '{}'",
                            trait_name, name
                        ))
                        .with_code("E014"));
                    }
                }

                let info = EnumInfo {
                    name: name.clone(),
                    variants: variant_names,
                    includes: includes.clone(),
                };
                self.env.set_enum(name.clone(), info);
                Ok(Type::Void)
            }
        }
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
        let mut current = child_name.clone();
        let mut visited = std::collections::HashSet::new();
        loop {
            if !visited.insert(current.clone()) {
                return false;
            }
            if let Some(info) = self.env.get_class(&current) {
                if let Some(extends) = &info.extends {
                    if extends == parent_name {
                        return true;
                    }
                    current = extends.clone();
                } else {
                    return false;
                }
            } else {
                return false;
            }
        }
    }

    /// Compare types for compatibility, ignoring param_names on Function types.
    pub(crate) fn types_compatible(a: &Type, b: &Type) -> bool {
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
            _ => a == b,
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
        let loader_rc = match &self.module_loader {
            Some(loader) => Rc::clone(loader),
            None => return Ok(Type::Void), // No loader — ignore use (backward compatible)
        };

        let exports = ModuleLoader::load_module(&loader_rc, path, *span)?;
        let module_key = path.join("/");

        match (names, alias) {
            (Some(_), Some(_)) => {
                // Selective + alias is not allowed
                return Err(Diagnostic::error(
                    "Cannot combine selective imports { ... } with 'as' alias".to_string(),
                )
                .with_code("M004")
                .with_label(*span, "use either { names } or 'as alias', not both"));
            }
            (Some(selected_names), None) => {
                // Selective import: use foo { Bar, baz }
                for name in selected_names {
                    if !self.inject_export(name, &exports) {
                        return Err(Diagnostic::error(format!(
                            "'{}' is not exported by module '{}'",
                            name, module_key
                        ))
                        .with_code("M002")
                        .with_label(*span, format!("'{}' not found in module", name)));
                    }
                }
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
            }
            (None, None) => {
                // Wildcard import: use foo — import all pub items
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
        }

        Ok(Type::Void)
    }

    /// Try to inject a single named export into the current environment.
    /// Returns false if the name wasn't found in any export category.
    fn inject_export(
        &mut self,
        name: &str,
        exports: &crate::module_loader::ModuleExports,
    ) -> bool {
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
                        .with_code("E001")
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
                        "Pattern type {:?} does not match scrutinee type {:?}",
                        pat_ty, scrutinee_ty
                    ))
                    .with_code("E001")
                    .with_label(*span, format!("expected {:?}", scrutinee_ty)));
                }
                Ok(())
            }
        }
    }

    pub(crate) fn suggest_similar_name(&self, name: &str) -> Option<String> {
        let mut best: Option<(usize, &str)> = None;
        for known in self.env.all_var_names() {
            let dist = Self::levenshtein(name, known);
            if dist <= 2 && dist < name.len() && (best.is_none() || dist < best.unwrap().0) {
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
}
