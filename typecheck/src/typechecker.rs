use ast::{ClassInfo, Expr, MatchPattern, Stmt, TraitInfo, Type, TypeEnv};
use std::collections::HashMap;


pub struct TypeChecker {
    pub env: TypeEnv,
    pub is_async_context: bool,
    pub loop_depth: usize,
    pub expected_return_type: Option<Type>,
    /// Current function name for better error messages.
    pub current_function: Option<String>,
    /// The error type this function declares via `throws`.
    pub throws_type: Option<Type>,
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
                params: vec![Type::String],
                ret: Box::new(Type::Void),
                is_async: false,
                throws: None,
            },
        );
        env.set_var(
            "print".into(),
            Type::Function {
                params: vec![Type::String],
                ret: Box::new(Type::Void),
                is_async: false,
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
                fields: HashMap::from([("message".into(), Type::String)]),
                methods: HashMap::new(),
                generic_params: None,
                extends: None,
            },
        );
        env.set_var(
            "Exception".into(),
            Type::Function {
                params: vec![Type::String],
                ret: Box::new(Type::Custom("Exception".into(), Vec::new())),
                is_async: false,
                throws: None,
            },
        );
        env.set_class(
            "Error".into(),
            ClassInfo {
                ty: Type::Custom("Error".into(), Vec::new()),
                fields: HashMap::new(), // inherits message from Exception
                methods: HashMap::new(),
                generic_params: None,
                extends: Some("Exception".into()),
            },
        );
        env.set_var(
            "Error".into(),
            Type::Function {
                params: vec![Type::String], // inherited message field
                ret: Box::new(Type::Custom("Error".into(), Vec::new())),
                is_async: false,
                throws: None,
            },
        );
        Self {
            env,
            is_async_context: false,
            loop_depth: 0,
            expected_return_type: None,
            current_function: None,
            throws_type: None,
        }
    }

    /// Create a child TypeChecker that inherits context flags and a child scope.
    pub(crate) fn child_checker(&self) -> TypeChecker {
        TypeChecker {
            env: self.env.child(),
            is_async_context: self.is_async_context,
            loop_depth: self.loop_depth,
            expected_return_type: self.expected_return_type.clone(),
            current_function: self.current_function.clone(),
            throws_type: self.throws_type.clone(),
        }
    }

    pub fn check_module(&mut self, m: &ast::Module) -> Result<(), String> {
        for s in &m.body {
            self.check_stmt(s)?;
        }
        Ok(())
    }

    pub fn check_stmt(&mut self, stmt: &Stmt) -> Result<Type, String> {
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
                let ty = self.check_expr(value)?;
                self.current_function = prev_fn;
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
                        return Err(format!(
                            "Type annotation mismatch for '{}': declared {:?}, got {:?}",
                            name, ann, ty
                        ));
                    }
                    // Nil cannot be assigned to non-nullable types
                    if ty == Type::Nil && !matches!(ann, Type::Nil) {
                        return Err(format!(
                            "Cannot assign nil to non-nullable type {:?}",
                            ann
                        ));
                    }
                    if *ann != ty {
                        return Err(format!(
                            "Type annotation mismatch for '{}': declared {:?}, got {:?}",
                            name, ann, ty
                        ));
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
            Stmt::Trait {
                name,
                methods,
                ..
            } => {
                let mut method_map = HashMap::new();
                let mut required_methods = Vec::new();
                for m in methods {
                    if let Stmt::Let {
                        name: mname, value, ..
                    } = m
                    {
                        let mty = self.check_expr(value)?;
                        // Store with unqualified name for trait matching
                        let short_name = mname.strip_prefix(&format!("{}.", name))
                            .unwrap_or(mname)
                            .to_string();
                        // Check if this is an abstract method (empty body)
                        if let Expr::Lambda { body, .. } = value {
                            if body.is_empty() {
                                required_methods.push(short_name.clone());
                            }
                        }
                        method_map.insert(short_name, mty);
                    } else {
                        return Err(format!("Unexpected stmt in trait methods: {:?}", m));
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
            Stmt::Return(expr) => {
                let ty = self.check_expr(expr)?;
                if let Some(expected) = &self.expected_return_type {
                    if ty != *expected {
                        let ctx = self.current_function.as_deref().unwrap_or("<anonymous>");
                        return Err(format!(
                            "Return type mismatch in '{}': expected {:?}, got {:?}",
                            ctx, expected, ty
                        ));
                    }
                }
                Ok(ty)
            }
            Stmt::Expr(expr) => self.check_expr(expr),
            Stmt::If {
                cond,
                then_body,
                elif_branches,
                else_body,
            } => {
                let cond_ty = self.check_expr(cond)?;
                if cond_ty != Type::Bool {
                    return Err(format!("If condition must be Bool, got {:?}", cond_ty));
                }

                self.child_checker().check_body(then_body)?;
                for (elif_cond, elif_body) in elif_branches {
                    let elif_cond_ty = self.check_expr(elif_cond)?;
                    if elif_cond_ty != Type::Bool {
                        return Err(format!(
                            "Elif condition must be Bool, got {:?}",
                            elif_cond_ty
                        ));
                    }
                    self.child_checker().check_body(elif_body)?;
                }
                self.child_checker().check_body(else_body)
            }
            Stmt::While { cond, body } => {
                let cond_ty = self.check_expr(cond)?;
                if cond_ty != Type::Bool {
                    return Err(format!("While condition must be Bool, got {:?}", cond_ty));
                }
                let mut sub = self.child_checker();
                sub.loop_depth += 1;
                sub.check_body(body)
            }
            Stmt::For { var, iter, body } => {
                let iter_ty = self.check_expr(iter)?;
                let elem_ty = match iter_ty {
                    Type::List(inner) => *inner,
                    _ => return Err(format!("Cannot iterate over {:?}, expected List", iter_ty)),
                };
                let mut sub = self.child_checker();
                sub.loop_depth += 1;
                sub.env.set_var(var.clone(), elem_ty);
                sub.check_body(body)
            }
            Stmt::Assignment { target, value } => {
                let val_ty = self.check_expr(value)?;
                match target {
                    Expr::Ident(name) => {
                        let target_ty = self.env.get_var(name).ok_or_else(|| {
                            format!("Assignment to undeclared variable '{}'", name)
                        })?;
                        if target_ty != val_ty {
                            // Nullable auto-wrap: allow T or Nil assigned to T?
                            if let Type::Nullable(inner) = &target_ty {
                                if val_ty == **inner || val_ty == Type::Nil {
                                    return Ok(target_ty);
                                }
                            }
                            return Err(format!(
                                "Assignment type mismatch: variable '{}' is {:?}, got {:?}",
                                name, target_ty, val_ty
                            ));
                        }
                        Ok(val_ty)
                    }
                    Expr::Member { object, field } => {
                        let obj_ty = self.check_expr(object)?;
                        if let Type::Custom(class_name, _) = &obj_ty {
                            if let Some(info) = self.env.get_class(class_name) {
                                if let Some(field_ty) = info.fields.get(field) {
                                    if *field_ty != val_ty {
                                        return Err(format!(
                                            "Cannot assign {:?} to field '{}' of type {:?}",
                                            val_ty, field, field_ty
                                        ));
                                    }
                                } else {
                                    return Err(format!(
                                        "Class '{}' has no field '{}'",
                                        class_name, field
                                    ));
                                }
                            } else {
                                return Err(format!("Unknown class '{}'", class_name));
                            }
                        } else {
                            return Err(format!(
                                "Cannot assign to member on non-class type {:?}",
                                obj_ty
                            ));
                        }
                        Ok(val_ty)
                    }
                    Expr::Index { object, index } => {
                        let obj_ty = self.check_expr(object)?;
                        let idx_ty = self.check_expr(index)?;
                        if idx_ty != Type::Int {
                            return Err(format!("Index must be Int, got {:?}", idx_ty));
                        }
                        match &obj_ty {
                            Type::List(inner) => {
                                if **inner != val_ty {
                                    return Err(format!(
                                        "Cannot assign {:?} to List[{:?}] element",
                                        val_ty, inner
                                    ));
                                }
                                Ok(val_ty)
                            }
                            _ => Err(format!("Cannot index-assign into {:?}", obj_ty)),
                        }
                    }
                    _ => Err("Invalid assignment target".to_string()),
                }
            }
            Stmt::Break => {
                if self.loop_depth == 0 {
                    return Err("'break' used outside of a loop".to_string());
                }
                Ok(Type::Void)
            }
            Stmt::Continue => {
                if self.loop_depth == 0 {
                    return Err("'continue' used outside of a loop".to_string());
                }
                Ok(Type::Void)
            }
            Stmt::Use { .. } => Ok(Type::Void),
        }
    }

    /// Check if `child_ty` is a subtype of `parent_ty` via the extends hierarchy.
    /// Both types must be Custom types (class names). Returns true if they're
    /// the same type or if child transitively extends parent.
    pub(crate) fn is_error_subtype(&self, child_ty: &Type, parent_ty: &Type) -> bool {
        if child_ty == parent_ty {
            return true;
        }
        // Both must be Custom types
        let child_name = match child_ty {
            Type::Custom(n, _) => n,
            _ => return false,
        };
        let parent_name = match parent_ty {
            Type::Custom(n, _) => n,
            _ => return false,
        };
        // Walk the extends chain from child up (with cycle protection)
        let mut current = child_name.clone();
        let mut visited = std::collections::HashSet::new();
        loop {
            if !visited.insert(current.clone()) {
                return false; // cycle detected
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

    pub(crate) fn is_in_async_context(&self) -> bool {
        self.is_async_context
    }

    /// Iterate over `body` statements and return the type of the last one
    /// (or `Type::Void` for an empty body). Operates on `self` directly —
    /// callers are responsible for creating a child scope when needed.
    pub(crate) fn check_body(&mut self, body: &[Stmt]) -> Result<Type, String> {
        let mut last = Type::Void;
        for s in body {
            last = self.check_stmt(s)?;
        }
        Ok(last)
    }

    pub(crate) fn check_match_pattern(&self, pattern: &MatchPattern, scrutinee_ty: &Type) -> Result<(), String> {
        match pattern {
            MatchPattern::Wildcard | MatchPattern::Ident(_) => Ok(()),
            MatchPattern::Literal(expr) => {
                let pat_ty = match expr {
                    Expr::Int(_) => Type::Int,
                    Expr::Float(_) => Type::Float,
                    Expr::Str(_) => Type::String,
                    Expr::Bool(_) => Type::Bool,
                    Expr::Nil => Type::Nil,
                    _ => return Err("Invalid literal in match pattern".to_string()),
                };
                // Allow nil pattern and inner-type patterns to match nullable types
                if matches!(scrutinee_ty, Type::Nullable(_)) {
                    if pat_ty == Type::Nil {
                        return Ok(());
                    }
                    if let Type::Nullable(inner) = scrutinee_ty {
                        if pat_ty == **inner {
                            return Ok(());
                        }
                    }
                }
                if pat_ty != *scrutinee_ty {
                    return Err(format!(
                        "Pattern type {:?} does not match scrutinee type {:?}",
                        pat_ty, scrutinee_ty
                    ));
                }
                Ok(())
            }
        }
    }
}
