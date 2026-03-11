use ast::{ClassInfo, Diagnostic, Stmt, Type};
use indexmap::IndexMap;
use std::collections::HashMap;

use crate::typechecker::TypeChecker;

impl TypeChecker {
    pub(crate) fn check_class_stmt(
        &mut self,
        name: &str,
        fields: &[(String, Type)],
        methods: &[Stmt],
        generic_params: &Option<Vec<String>>,
        extends: &Option<String>,
        includes: &Option<Vec<(String, Vec<Type>)>>,
    ) -> Result<Type, Diagnostic> {
        // Pre-register the class so that method type checking recognizes
        // the class name as a known type (needed for inline generic inference).
        self.env.set_class(
            name.to_string(),
            ClassInfo {
                ty: Type::Custom(name.to_string(), Vec::new()),
                fields: IndexMap::new(),
                methods: HashMap::new(),
                generic_params: generic_params.clone(),
                extends: extends.clone(),
                includes: Vec::new(),
            },
        );

        let mut field_map = IndexMap::new();
        for (fname, fty) in fields {
            if field_map.contains_key(fname) {
                return Err(Diagnostic::error(format!(
                    "Duplicate field '{}' in class '{}'",
                    fname, name
                ))
                .with_code("E014"));
            }
            field_map.insert(fname.clone(), fty.clone());
        }

        // Pre-register constructor so method bodies can call ClassName(field: val)
        self.register_constructor(name, fields, generic_params, extends);

        let class_type = Type::Custom(name.to_string(), Vec::new());

        // Create a child checker with class fields in scope for method body checking
        let mut method_checker = self.child_checker();
        for (fname, fty) in fields {
            method_checker.env.set_var(fname.clone(), fty.clone());
        }
        // Also inject inherited fields from parent classes
        if let Some(parent_name) = extends {
            let mut current = Some(parent_name.clone());
            let mut visited = std::collections::HashSet::new();
            visited.insert(name.to_string());
            while let Some(ref cname) = current {
                if !visited.insert(cname.clone()) {
                    break;
                }
                if let Some(ancestor) = self.env.get_class(cname) {
                    for (fname, fty) in ancestor.fields.iter() {
                        method_checker.env.set_var(fname.clone(), fty.clone());
                    }
                    current = ancestor.extends.clone();
                } else {
                    break;
                }
            }
        }

        let mut method_map = HashMap::new();
        for m in methods {
            if let Stmt::Let {
                name: mname, value, ..
            } = m
            {
                // Substitute Self -> class type in method lambda types before checking
                let resolved_value = Self::substitute_self_in_lambda(value, &class_type);
                let mty = method_checker.check_expr(&resolved_value)?;
                let short_name = mname
                    .strip_prefix(&format!("{}.", name))
                    .unwrap_or(mname)
                    .to_string();
                if method_map.contains_key(&short_name) {
                    return Err(Diagnostic::error(format!(
                        "Duplicate method '{}' in class '{}'",
                        short_name, name
                    ))
                    .with_code("E014")
                    .with_label(m.span(), "duplicate method"));
                }
                method_map.insert(short_name, mty);
            } else {
                return Err(Diagnostic::error(format!(
                    "Unexpected stmt in class methods: {:?}",
                    m
                ))
                .with_code("E014")
                .with_label(m.span(), "expected method definition"));
            }
        }

        let includes_refs = includes.clone().unwrap_or_default();

        // Build includes list of base trait names
        let mut includes_list: Vec<String> = includes_refs.iter().map(|(n, _)| n.clone()).collect();

        // Ord includes Eq — auto-add Eq if Ord is included
        if includes_list.contains(&"Ord".to_string()) && !includes_list.contains(&"Eq".to_string())
        {
            includes_list.push("Eq".to_string());
            // Auto-import Eq from builtins if not already in scope
            if self.env.get_trait("Eq").is_none()
                && let Some(eq_info) = self.builtin_traits.get("Eq")
            {
                self.env.set_trait("Eq".into(), eq_info.clone());
            }
        }

        // Iterable: infer element type from each() and inject vocabulary methods.
        // `includes Iterable` (no type args) — infer T from each(f: (T) -> Void).
        // `includes Iterable[T]` — error, must use bare form.
        let mut iterable_element_type: Option<Type> = None;
        if includes_list.contains(&"Iterable".to_string()) {
            // Check for explicit type args (not allowed)
            for (tname, targs) in &includes_refs {
                if tname == "Iterable" && !targs.is_empty() {
                    return Err(Diagnostic::error(
                        "Iterable does not take type parameters. The element type is inferred from your each() method signature"
                    )
                    .with_code("E025"));
                }
            }

            // Find each() in the class methods to infer element type
            let each_method = method_map.get("each");
            if let Some(Type::Function { params, .. }) = each_method {
                // each(f: (T) -> Void) — extract T from the callback parameter
                if let Some(Type::Function {
                    params: cb_params, ..
                }) = params.first()
                    && let Some(element_ty) = cb_params.first()
                {
                    iterable_element_type = Some(element_ty.clone());
                }
            }

            if iterable_element_type.is_none() {
                return Err(Diagnostic::error(format!(
                    "Class '{}' includes Iterable but has no each(f: (T) -> Void) -> Void method",
                    name
                ))
                .with_code("E025"));
            }

            // Inject vocabulary methods (only if not already defined by the class)
            let elem_ty = iterable_element_type.clone().unwrap();
            self.inject_iterable_vocabulary(&mut method_map, &elem_ty);
        }

        // Build map of trait type args for parametric trait substitution
        let mut trait_type_args: HashMap<String, Vec<Type>> = HashMap::new();
        for (tname, targs) in &includes_refs {
            if !targs.is_empty() {
                trait_type_args.insert(tname.clone(), targs.clone());
            }
        }
        // For Iterable, inject inferred type arg so parametric trait validation passes
        if let Some(ref elem_ty) = iterable_element_type {
            trait_type_args.insert("Iterable".to_string(), vec![elem_ty.clone()]);
        }

        // Validate includes — check trait satisfaction with Self + type param substitution
        for trait_name in &includes_list {
            let trait_info = self.env.get_trait(trait_name).ok_or_else(|| {
                // Check if it's a known stdlib trait — give helpful import suggestion
                if self.builtin_traits.contains_key(trait_name) {
                    let submod = match trait_name.as_str() {
                        "Eq" | "Ord" => "cmp",
                        "Printable" => "fmt",
                        "Iterable" => "collections",
                        "From" | "Into" => "convert",
                        _ => "std",
                    };
                    Diagnostic::error(format!(
                        "Unknown trait '{}'. Add `use std/{} {{ {} }}` to import it",
                        trait_name, submod, trait_name
                    ))
                    .with_code("E014")
                } else {
                    Diagnostic::error(format!(
                        "Unknown trait '{}' in includes for class '{}'",
                        trait_name, name
                    ))
                    .with_code("E014")
                }
            })?;

            // Validate type argument arity for parametric traits
            if let Some(ref gp) = trait_info.generic_params {
                let provided = trait_type_args.get(trait_name);
                let provided_count = provided.map(|v| v.len()).unwrap_or(0);
                if provided_count != gp.len() {
                    return Err(Diagnostic::error(format!(
                        "Trait '{}' expects {} type parameter(s), got {}",
                        trait_name,
                        gp.len(),
                        provided_count
                    ))
                    .with_code("E014"));
                }
            }

            for method_name in &trait_info.required_methods {
                if let Some(class_method_ty) = method_map.get(method_name) {
                    if let Some(trait_method_ty) = trait_info.methods.get(method_name) {
                        // Substitute Self -> class type in trait method signature
                        let mut resolved_trait_ty =
                            Self::substitute_self(trait_method_ty, &class_type);

                        // Substitute trait type params -> concrete types
                        if let Some(ref gp) = trait_info.generic_params
                            && let Some(targs) = trait_type_args.get(trait_name)
                        {
                            let param_bindings: HashMap<String, Type> = gp
                                .iter()
                                .zip(targs.iter())
                                .map(|(p, t)| (p.clone(), t.clone()))
                                .collect();
                            resolved_trait_ty =
                                Self::substitute_typevars(&resolved_trait_ty, &param_bindings);
                        }

                        // Allow throws mismatch: class method may add throws to a non-throws trait method
                        // Unify ignoring throws on the class side
                        let mut bindings = HashMap::new();
                        if Self::unify_type(&resolved_trait_ty, class_method_ty, &mut bindings)
                            .is_err()
                        {
                            // Try again ignoring throws (class method may declare throws)
                            let class_no_throws = match class_method_ty {
                                Type::Function {
                                    param_names,
                                    params,
                                    ret,
                                    ..
                                } => Type::Function {
                                    param_names: param_names.clone(),
                                    params: params.clone(),
                                    ret: ret.clone(),
                                    throws: None,
                                },
                                other => other.clone(),
                            };
                            let mut bindings2 = HashMap::new();
                            if Self::unify_type(
                                &resolved_trait_ty,
                                &class_no_throws,
                                &mut bindings2,
                            )
                            .is_err()
                            {
                                return Err(Diagnostic::error(format!(
                                    "Method '{}' in class '{}' has signature {:?}, but trait '{}' requires {:?}",
                                    method_name, name, class_method_ty, trait_name, resolved_trait_ty
                                ))
                                .with_code("E014"));
                            }
                        }
                    }
                } else {
                    // Method not defined — check if auto-derive applies
                    if trait_name == "Eq" && method_name == "eq" {
                        // Auto-derive: verify all fields include Eq
                        self.check_auto_derive(
                            name,
                            &field_map,
                            "Eq",
                            "E021",
                            Self::type_includes_eq,
                        )?;
                        let eq_method_ty = Type::Function {
                            param_names: vec!["other".into()],
                            params: vec![class_type.clone()],
                            ret: Box::new(Type::Bool),
                            throws: None,
                        };
                        method_map.insert("eq".into(), eq_method_ty);
                    } else if trait_name == "Ord" && method_name == "cmp" {
                        // Auto-derive: verify all fields include Ord
                        self.check_auto_derive(
                            name,
                            &field_map,
                            "Ord",
                            "E022",
                            Self::type_includes_ord,
                        )?;
                        let cmp_method_ty = Type::Function {
                            param_names: vec!["other".into()],
                            params: vec![class_type.clone()],
                            ret: Box::new(Type::Custom("Ordering".into(), Vec::new())),
                            throws: None,
                        };
                        method_map.insert("cmp".into(), cmp_method_ty);
                    } else if trait_name == "Printable" && method_name == "to_string" {
                        // Auto-derive: verify all fields include Printable
                        self.check_auto_derive(
                            name,
                            &field_map,
                            "Printable",
                            "E023",
                            Self::type_includes_printable,
                        )?;
                        let to_string_ty = Type::Function {
                            param_names: vec![],
                            params: vec![],
                            ret: Box::new(Type::String),
                            throws: None,
                        };
                        method_map.insert("to_string".into(), to_string_ty);
                    } else {
                        return Err(Diagnostic::error(format!(
                            "Class '{}' must implement method '{}' from trait '{}'",
                            name, method_name, trait_name
                        ))
                        .with_code("E014"));
                    }
                }
            }
        }

        // Printable: auto-add debug() defaulting to to_string() signature if not defined
        if includes_list.contains(&"Printable".to_string()) && !method_map.contains_key("debug") {
            let debug_ty = Type::Function {
                param_names: vec![],
                params: vec![],
                ret: Box::new(Type::String),
                throws: None,
            };
            method_map.insert("debug".into(), debug_ty);
        }

        let info = ClassInfo {
            ty: class_type,
            fields: field_map,
            methods: method_map,
            generic_params: generic_params.clone(),
            extends: extends.clone(),
            includes: includes_list,
        };
        self.env.set_class(name.to_string(), info);

        // Validate extends
        if let Some(parent_name) = extends {
            if self.env.get_class(parent_name).is_none() {
                return Err(Diagnostic::error(format!(
                    "Class '{}' extends unknown class '{}'",
                    name, parent_name
                ))
                .with_code("E014"));
            }
            let mut visited = std::collections::HashSet::new();
            visited.insert(name.to_string());
            let mut current = Some(parent_name.clone());
            while let Some(ref cname) = current {
                if !visited.insert(cname.clone()) {
                    return Err(Diagnostic::error(format!(
                        "Circular inheritance detected: class '{}' forms a cycle through '{}'",
                        name, cname
                    ))
                    .with_code("E014"));
                }
                current = self
                    .env
                    .get_class(cname)
                    .and_then(|info| info.extends.clone());
            }
        }

        // Check for field shadowing
        if let Some(parent_name) = extends {
            let mut inherited_fields = std::collections::HashSet::new();
            let mut current = Some(parent_name.clone());
            while let Some(ref cname) = current {
                if let Some(ancestor) = self.env.get_class(cname) {
                    for fname in ancestor.fields.keys() {
                        inherited_fields.insert(fname.clone());
                    }
                    current = ancestor.extends.clone();
                } else {
                    break;
                }
            }
            for (fname, _) in fields {
                if inherited_fields.contains(fname) {
                    return Err(Diagnostic::error(format!(
                        "Field '{}' in class '{}' shadows inherited field from parent chain",
                        fname, name
                    ))
                    .with_code("E014"));
                }
            }
        }

        // Final constructor registration (may differ from pre-registration if
        // field shadowing check above modified the picture, but keeps them in sync)
        self.register_constructor(name, fields, generic_params, extends);

        Ok(Type::Void)
    }

    /// Build and register the constructor function for a class, including inherited fields.
    fn register_constructor(
        &mut self,
        name: &str,
        fields: &[(String, Type)],
        generic_params: &Option<Vec<String>>,
        extends: &Option<String>,
    ) {
        let generic_type_args: Vec<Type> = generic_params
            .as_ref()
            .map(|gp| {
                gp.iter()
                    .map(|p| Type::TypeVar(p.clone(), vec![]))
                    .collect()
            })
            .unwrap_or_default();
        let mut all_field_names: Vec<String> = Vec::new();
        let mut all_field_types: Vec<Type> = Vec::new();
        // Include inherited fields (with cycle detection)
        if let Some(parent_name) = extends {
            let mut current = Some(parent_name.clone());
            let mut chain = Vec::new();
            let mut visited = std::collections::HashSet::new();
            visited.insert(name.to_string());
            while let Some(ref cname) = current {
                if !visited.insert(cname.clone()) {
                    break; // Cycle detected, stop traversal
                }
                if let Some(parent_info) = self.env.get_class(cname) {
                    chain.push(parent_info.clone());
                    current = parent_info.extends.clone();
                } else {
                    break;
                }
            }
            for ancestor in chain.into_iter().rev() {
                for (fname, fty) in ancestor.fields.iter() {
                    all_field_names.push(fname.clone());
                    all_field_types.push(fty.clone());
                }
            }
        }
        all_field_names.extend(fields.iter().map(|(n, _)| n.clone()));
        all_field_types.extend(fields.iter().map(|(_, t)| t.clone()));
        self.env.set_var(
            name.to_string(),
            Type::Function {
                param_names: all_field_names,
                params: all_field_types,
                ret: Box::new(Type::Custom(name.to_string(), generic_type_args)),
                throws: None,
            },
        );
    }

    /// Replace Self type in a Lambda expression's type annotations before checking.
    fn substitute_self_in_lambda(expr: &ast::Expr, class_type: &Type) -> ast::Expr {
        if let ast::Expr::Lambda {
            params,
            ret_type,
            body,
            generic_params,
            throws,
            type_constraints,
            span,
        } = expr
        {
            let resolved_params: Vec<(String, Type)> = params
                .iter()
                .map(|(n, t)| (n.clone(), Self::substitute_self(t, class_type)))
                .collect();
            let resolved_ret = Self::substitute_self(ret_type, class_type);
            let resolved_throws = throws
                .as_ref()
                .map(|t| Self::substitute_self(t, class_type));
            ast::Expr::Lambda {
                params: resolved_params,
                ret_type: resolved_ret,
                body: body.clone(),
                generic_params: generic_params.clone(),
                throws: resolved_throws,
                type_constraints: type_constraints.clone(),
                span: *span,
            }
        } else {
            expr.clone()
        }
    }

    /// Substitute `Self` (represented as `Type::Custom("Self", [])`) with the concrete class type.
    fn substitute_self(ty: &Type, class_type: &Type) -> Type {
        match ty {
            Type::Custom(name, args) if name == "Self" && args.is_empty() => class_type.clone(),
            Type::Function {
                param_names,
                params,
                ret,
                throws,
            } => Type::Function {
                param_names: param_names.clone(),
                params: params
                    .iter()
                    .map(|t| Self::substitute_self(t, class_type))
                    .collect(),
                ret: Box::new(Self::substitute_self(ret, class_type)),
                throws: throws
                    .as_ref()
                    .map(|t| Box::new(Self::substitute_self(t, class_type))),
            },
            Type::List(inner) => Type::List(Box::new(Self::substitute_self(inner, class_type))),
            Type::Map(k, v) => Type::Map(
                Box::new(Self::substitute_self(k, class_type)),
                Box::new(Self::substitute_self(v, class_type)),
            ),
            Type::Nullable(inner) => {
                Type::Nullable(Box::new(Self::substitute_self(inner, class_type)))
            }
            Type::Task(inner) => Type::Task(Box::new(Self::substitute_self(inner, class_type))),
            Type::Custom(name, args) => Type::Custom(
                name.clone(),
                args.iter()
                    .map(|t| Self::substitute_self(t, class_type))
                    .collect(),
            ),
            other => other.clone(),
        }
    }

    /// Check that all fields of a class include a given trait for auto-derive.
    fn check_auto_derive(
        &self,
        class_name: &str,
        fields: &IndexMap<String, Type>,
        trait_name: &str,
        error_code: &str,
        checker: fn(&TypeChecker, &Type) -> bool,
    ) -> Result<(), Diagnostic> {
        for (fname, fty) in fields {
            if !checker(self, fty) {
                return Err(Diagnostic::error(format!(
                    "Cannot derive {} for '{}': field '{}' of type {:?} does not include {}",
                    trait_name, class_name, fname, fty, trait_name
                ))
                .with_code(error_code));
            }
        }
        Ok(())
    }

    /// Check if a type includes Printable (all primitives do, custom types need explicit includes).
    pub(crate) fn type_includes_printable(&self, ty: &Type) -> bool {
        match ty {
            Type::Int | Type::Float | Type::String | Type::Bool | Type::Nil => true,
            Type::Custom(name, _) => {
                if let Some(info) = self.env.get_class(name) {
                    return info.includes.contains(&"Printable".to_string());
                }
                if let Some(info) = self.env.get_enum(name) {
                    return info.includes.contains(&"Printable".to_string());
                }
                false
            }
            Type::List(inner) => self.type_includes_printable(inner),
            Type::Error => true,
            _ => false,
        }
    }

    /// Check if a type includes Eq (primitives do, custom types need explicit includes).
    /// Ord implies Eq, so types including Ord also include Eq.
    pub(crate) fn type_includes_eq(&self, ty: &Type) -> bool {
        match ty {
            Type::Int | Type::Float | Type::String | Type::Bool | Type::Nil => true,
            Type::Custom(name, _) => {
                // Check classes
                if let Some(info) = self.env.get_class(name) {
                    return info.includes.contains(&"Eq".to_string())
                        || info.includes.contains(&"Ord".to_string());
                }
                // Check enums
                if let Some(info) = self.env.get_enum(name) {
                    return info.includes.contains(&"Eq".to_string())
                        || info.includes.contains(&"Ord".to_string());
                }
                false
            }
            Type::List(inner) => self.type_includes_eq(inner),
            Type::Error => true, // error sentinel is compatible with everything
            _ => false,
        }
    }

    /// Inject Iterable vocabulary methods into a class's method map.
    /// Methods are only added if not already defined by the class (allowing overrides).
    /// Conditional methods (min, max, sort) are added here;
    /// the Ord check happens at call site in check_member.
    fn inject_iterable_vocabulary(&self, method_map: &mut HashMap<String, Type>, elem_ty: &Type) {
        // map: (f: (T) -> U) -> List[U]
        if !method_map.contains_key("map") {
            method_map.insert(
                "map".into(),
                Type::Function {
                    param_names: vec!["f".into()],
                    params: vec![Type::Function {
                        param_names: vec!["_0".into()],
                        params: vec![elem_ty.clone()],
                        ret: Box::new(Type::TypeVar("U".into(), vec![])),
                        throws: None,
                    }],
                    ret: Box::new(Type::List(Box::new(Type::TypeVar("U".into(), vec![])))),
                    throws: None,
                },
            );
        }

        // filter: (f: (T) -> Bool) -> List[T]
        if !method_map.contains_key("filter") {
            method_map.insert(
                "filter".into(),
                Type::Function {
                    param_names: vec!["f".into()],
                    params: vec![Type::Function {
                        param_names: vec!["_0".into()],
                        params: vec![elem_ty.clone()],
                        ret: Box::new(Type::Bool),
                        throws: None,
                    }],
                    ret: Box::new(Type::List(Box::new(elem_ty.clone()))),
                    throws: None,
                },
            );
        }

        // reduce: (init: U, f: (U, T) -> U) -> U
        if !method_map.contains_key("reduce") {
            method_map.insert(
                "reduce".into(),
                Type::Function {
                    param_names: vec!["init".into(), "f".into()],
                    params: vec![
                        Type::TypeVar("U".into(), vec![]),
                        Type::Function {
                            param_names: vec!["_0".into(), "_1".into()],
                            params: vec![Type::TypeVar("U".into(), vec![]), elem_ty.clone()],
                            ret: Box::new(Type::TypeVar("U".into(), vec![])),
                            throws: None,
                        },
                    ],
                    ret: Box::new(Type::TypeVar("U".into(), vec![])),
                    throws: None,
                },
            );
        }

        // find: (f: (T) -> Bool) -> T?
        if !method_map.contains_key("find") {
            method_map.insert(
                "find".into(),
                Type::Function {
                    param_names: vec!["f".into()],
                    params: vec![Type::Function {
                        param_names: vec!["_0".into()],
                        params: vec![elem_ty.clone()],
                        ret: Box::new(Type::Bool),
                        throws: None,
                    }],
                    ret: Box::new(Type::Nullable(Box::new(elem_ty.clone()))),
                    throws: None,
                },
            );
        }

        // any: (f: (T) -> Bool) -> Bool
        if !method_map.contains_key("any") {
            method_map.insert(
                "any".into(),
                Type::Function {
                    param_names: vec!["f".into()],
                    params: vec![Type::Function {
                        param_names: vec!["_0".into()],
                        params: vec![elem_ty.clone()],
                        ret: Box::new(Type::Bool),
                        throws: None,
                    }],
                    ret: Box::new(Type::Bool),
                    throws: None,
                },
            );
        }

        // all: (f: (T) -> Bool) -> Bool
        if !method_map.contains_key("all") {
            method_map.insert(
                "all".into(),
                Type::Function {
                    param_names: vec!["f".into()],
                    params: vec![Type::Function {
                        param_names: vec!["_0".into()],
                        params: vec![elem_ty.clone()],
                        ret: Box::new(Type::Bool),
                        throws: None,
                    }],
                    ret: Box::new(Type::Bool),
                    throws: None,
                },
            );
        }

        // count: () -> Int
        if !method_map.contains_key("count") {
            method_map.insert(
                "count".into(),
                Type::Function {
                    param_names: vec![],
                    params: vec![],
                    ret: Box::new(Type::Int),
                    throws: None,
                },
            );
        }

        // first: () -> T?
        if !method_map.contains_key("first") {
            method_map.insert(
                "first".into(),
                Type::Function {
                    param_names: vec![],
                    params: vec![],
                    ret: Box::new(Type::Nullable(Box::new(elem_ty.clone()))),
                    throws: None,
                },
            );
        }

        // last: () -> T?
        if !method_map.contains_key("last") {
            method_map.insert(
                "last".into(),
                Type::Function {
                    param_names: vec![],
                    params: vec![],
                    ret: Box::new(Type::Nullable(Box::new(elem_ty.clone()))),
                    throws: None,
                },
            );
        }

        // to_list: () -> List[T]
        if !method_map.contains_key("to_list") {
            method_map.insert(
                "to_list".into(),
                Type::Function {
                    param_names: vec![],
                    params: vec![],
                    ret: Box::new(Type::List(Box::new(elem_ty.clone()))),
                    throws: None,
                },
            );
        }

        // min: () -> T? (requires T includes Ord — checked at call site)
        if !method_map.contains_key("min") {
            method_map.insert(
                "min".into(),
                Type::Function {
                    param_names: vec![],
                    params: vec![],
                    ret: Box::new(Type::Nullable(Box::new(elem_ty.clone()))),
                    throws: None,
                },
            );
        }

        // max: () -> T? (requires T includes Ord — checked at call site)
        if !method_map.contains_key("max") {
            method_map.insert(
                "max".into(),
                Type::Function {
                    param_names: vec![],
                    params: vec![],
                    ret: Box::new(Type::Nullable(Box::new(elem_ty.clone()))),
                    throws: None,
                },
            );
        }

        // sort: () -> List[T] (requires T includes Ord — checked at call site)
        if !method_map.contains_key("sort") {
            method_map.insert(
                "sort".into(),
                Type::Function {
                    param_names: vec![],
                    params: vec![],
                    ret: Box::new(Type::List(Box::new(elem_ty.clone()))),
                    throws: None,
                },
            );
        }
    }

    /// Extract the Iterable element type from a class's each() method signature.
    pub(crate) fn get_iterable_element_type_from_class(info: &ClassInfo) -> Option<Type> {
        if let Some(Type::Function { params, .. }) = info.methods.get("each")
            && let Some(Type::Function {
                params: cb_params, ..
            }) = params.first()
        {
            return cb_params.first().cloned();
        }
        None
    }

    /// Check if a type includes Ord (primitives do, custom types need explicit includes).
    pub(crate) fn type_includes_ord(&self, ty: &Type) -> bool {
        match ty {
            Type::Int | Type::Float | Type::String | Type::Bool => true,
            Type::Custom(name, _) => {
                if let Some(info) = self.env.get_class(name) {
                    info.includes.contains(&"Ord".to_string())
                } else {
                    false
                }
            }
            Type::List(inner) => self.type_includes_ord(inner),
            Type::Error => true,
            _ => false,
        }
    }
}
