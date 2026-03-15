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
                overloaded_methods: HashMap::new(),
                parametric_includes: Vec::new(),
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

        let class_type = Type::Custom(
            name.to_string(),
            generic_params
                .as_ref()
                .map(|gp| {
                    gp.iter()
                        .map(|p| Type::TypeVar(p.clone(), vec![]))
                        .collect()
                })
                .unwrap_or_default(),
        );

        // Create a child checker with class fields in scope for method body checking
        let mut method_checker = self.child_checker();
        for (fname, fty) in fields {
            method_checker.env.set_var(fname.clone(), fty.clone());
        }
        // Also inject inherited fields from parent classes
        if extends.is_some() {
            for ancestor in self.walk_ancestors(name) {
                for (fname, fty) in ancestor.fields.iter() {
                    method_checker.env.set_var(fname.clone(), fty.clone());
                }
            }
        }

        // Count how many times each parametric trait is included (for allowing method overloads)
        let parametric_trait_counts: HashMap<String, usize> = {
            let mut counts = HashMap::new();
            for (tname, targs) in includes.clone().unwrap_or_default() {
                if !targs.is_empty() {
                    *counts.entry(tname).or_insert(0) += 1;
                }
            }
            counts
        };
        let has_parametric_overloads = parametric_trait_counts.values().any(|&c| c > 1);

        let mut method_map = HashMap::new();
        let mut overloaded_methods: HashMap<String, Vec<Type>> = HashMap::new();
        for m in methods {
            if let Stmt::Let {
                name: mname, value, ..
            } = m
            {
                // Substitute Self -> class type in method lambda types before checking
                let resolved_value = Self::substitute_self_in_lambda(value, &class_type);
                let mty = method_checker.check_expr(&resolved_value)?;

                // Register method defaults for call-site resolution
                if let ast::Expr::Lambda {
                    params: lp,
                    defaults: ld,
                    ..
                } = value
                {
                    let mut default_set = std::collections::HashSet::new();
                    for (i, d) in ld.iter().enumerate() {
                        if d.is_some()
                            && let Some((pname, _)) = lp.get(i)
                        {
                            default_set.insert(pname.clone());
                        }
                    }
                    if !default_set.is_empty() {
                        // Use the qualified name (e.g., "Calc.add")
                        self.default_params.insert(mname.clone(), default_set);
                    }
                }

                let short_name = mname
                    .strip_prefix(&format!("{}.", name))
                    .unwrap_or(mname)
                    .to_string();
                #[allow(clippy::map_entry)]
                if method_map.contains_key(&short_name) {
                    // Allow duplicate if class has multiple parametric trait inclusions
                    // (e.g., Into[Fahrenheit], Into[Kelvin] both produce into())
                    if has_parametric_overloads {
                        let existing = method_map
                            .remove(&short_name)
                            .expect("invariant: contains_key checked above");
                        let overloads = overloaded_methods.entry(short_name.clone()).or_default();
                        if overloads.is_empty() {
                            overloads.push(existing);
                        }
                        overloads.push(mty);
                    } else {
                        return Err(Diagnostic::error(format!(
                            "Duplicate method '{}' in class '{}'",
                            short_name, name
                        ))
                        .with_code("E014")
                        .with_label(m.span(), "duplicate method"));
                    }
                } else if overloaded_methods.contains_key(&short_name) {
                    // Already moved to overloaded — add another
                    overloaded_methods
                        .get_mut(&short_name)
                        .expect("invariant: contains_key checked above")
                        .push(mty);
                } else {
                    method_map.insert(short_name, mty);
                }
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
            let elem_ty = iterable_element_type
                .clone()
                .expect("invariant: set when Iterable detected above");
            self.inject_iterable_vocabulary(&mut method_map, &elem_ty);
        }

        // Build parametric_includes list preserving duplicates with type args
        let parametric_includes: Vec<(String, Vec<Type>)> = includes_refs
            .iter()
            .filter(|(_, targs)| !targs.is_empty())
            .cloned()
            .collect();

        // For Iterable, inject inferred type arg so parametric trait validation passes
        let mut iterable_parametric = parametric_includes.clone();
        if let Some(ref elem_ty) = iterable_element_type {
            iterable_parametric.push(("Iterable".to_string(), vec![elem_ty.clone()]));
        }

        // Build set of already-validated trait+args combos to avoid re-validating
        // when includes_list deduplicates trait names
        let mut validated_traits: std::collections::HashSet<String> =
            std::collections::HashSet::new();

        // Validate includes — iterate over each (trait_name, type_args) inclusion individually
        // This handles multiple inclusions of the same parametric trait (e.g., Into[A], Into[B])
        let all_inclusions: Vec<(String, Vec<Type>)> = {
            let mut v: Vec<(String, Vec<Type>)> = includes_refs.clone();
            // Add auto-added Eq if Ord is included
            if includes_list.contains(&"Eq".to_string())
                && !includes_refs.iter().any(|(n, _)| n == "Eq")
            {
                v.push(("Eq".to_string(), Vec::new()));
            }
            // Add inferred Iterable type args
            if let Some(ref elem_ty) = iterable_element_type {
                for item in v.iter_mut() {
                    if item.0 == "Iterable" && item.1.is_empty() {
                        item.1 = vec![elem_ty.clone()];
                    }
                }
            }
            v
        };

        for (trait_name, targs) in &all_inclusions {
            let trait_info = self.env.get_trait(trait_name).ok_or_else(|| {
                // Check if it's a known stdlib trait — give helpful import suggestion
                if self.builtin_traits.contains_key(trait_name) {
                    let submod = match trait_name.as_str() {
                        "Eq" | "Ord" => "cmp",
                        "Printable" => "fmt",
                        "Iterable" | "Iterator" => "collections",
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
            if let Some(ref gp) = trait_info.generic_params
                && targs.len() != gp.len()
            {
                return Err(Diagnostic::error(format!(
                    "Trait '{}' expects {} type parameter(s), got {}",
                    trait_name,
                    gp.len(),
                    targs.len()
                ))
                .with_code("E014"));
            }

            for method_name in &trait_info.required_methods {
                // For overloaded methods, find the matching overload by return type
                let class_method_ty = if let Some(overloads) = overloaded_methods.get(method_name) {
                    // Resolve expected return type from trait + type args
                    if let Some(trait_method_ty) = trait_info.methods.get(method_name)
                        && let Some(ref gp) = trait_info.generic_params
                        && !targs.is_empty()
                    {
                        let param_bindings: HashMap<String, Type> = gp
                            .iter()
                            .zip(targs.iter())
                            .map(|(p, t)| (p.clone(), t.clone()))
                            .collect();
                        let resolved_trait_ty = Self::substitute_typevars(
                            &Self::substitute_self(trait_method_ty, &class_type),
                            &param_bindings,
                        );
                        // Find the overload whose signature matches
                        overloads.iter().find(|oty| {
                            let oty: &Type = oty;
                            let mut bindings = HashMap::new();
                            Self::unify_type(&resolved_trait_ty, oty, &mut bindings).is_ok() || {
                                // Try ignoring throws
                                let no_throws = if let Type::Function {
                                    param_names,
                                    params,
                                    ret,
                                    ..
                                } = oty
                                {
                                    Type::Function {
                                        param_names: param_names.clone(),
                                        params: params.clone(),
                                        ret: ret.clone(),
                                        throws: None,
                                        suspendable: false,
                                    }
                                } else {
                                    oty.clone()
                                };
                                let mut bindings2 = HashMap::new();
                                Self::unify_type(&resolved_trait_ty, &no_throws, &mut bindings2)
                                    .is_ok()
                            }
                        })
                    } else {
                        overloads.first()
                    }
                } else {
                    method_map.get(method_name)
                };

                if let Some(class_method_ty) = class_method_ty {
                    if let Some(trait_method_ty) = trait_info.methods.get(method_name) {
                        // Substitute Self -> class type in trait method signature
                        let mut resolved_trait_ty =
                            Self::substitute_self(trait_method_ty, &class_type);

                        // Substitute trait type params -> concrete types
                        if let Some(ref gp) = trait_info.generic_params
                            && !targs.is_empty()
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
                        let mut bindings = HashMap::new();
                        if Self::unify_type(&resolved_trait_ty, class_method_ty, &mut bindings)
                            .is_err()
                        {
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
                                    suspendable: false,
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
                    // Skip if we already validated this non-parametric trait
                    if validated_traits.contains(trait_name) {
                        continue;
                    }
                    if let Some((mname, mty)) = self.try_auto_derive_method(
                        name,
                        trait_name,
                        method_name,
                        &field_map,
                        &class_type,
                    )? {
                        method_map.insert(mname, mty);
                    } else {
                        return Err(Diagnostic::error(format!(
                            "Class '{}' must implement method '{}' from trait '{}'",
                            name, method_name, trait_name
                        ))
                        .with_code("E014"));
                    }
                }
            }
            validated_traits.insert(trait_name.clone());
        }

        // Printable: auto-add debug() defaulting to to_string() signature if not defined
        if includes_list.contains(&"Printable".to_string()) && !method_map.contains_key("debug") {
            let debug_ty = Type::Function {
                param_names: vec![],
                params: vec![],
                ret: Box::new(Type::String),
                throws: None,
                suspendable: false,
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
            overloaded_methods,
            parametric_includes,
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
            self.validate_circular_inheritance(name)?;
        }

        // Check for field shadowing
        if extends.is_some() {
            self.detect_field_shadowing(name, fields)?;
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
        if extends.is_some() {
            let ancestors = self.walk_ancestors(name);
            for ancestor in ancestors.into_iter().rev() {
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
                suspendable: false,
            },
        );
    }

    /// Validate that a class does not form a circular inheritance chain.
    fn validate_circular_inheritance(&self, name: &str) -> Result<(), Diagnostic> {
        let mut visited = std::collections::HashSet::new();
        visited.insert(name.to_string());
        let mut current = self
            .env
            .get_class(name)
            .and_then(|info| info.extends.clone());
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
        Ok(())
    }

    /// Check that no field in the class shadows an inherited field from the parent chain.
    fn detect_field_shadowing(
        &self,
        name: &str,
        fields: &[(String, Type)],
    ) -> Result<(), Diagnostic> {
        let mut inherited_fields = std::collections::HashSet::new();
        for ancestor in self.walk_ancestors(name) {
            for fname in ancestor.fields.keys() {
                inherited_fields.insert(fname.clone());
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
        Ok(())
    }

    /// Try to auto-derive a trait method for a class. Returns `Some((method_name, method_type))`
    /// if the trait/method combo is auto-derivable (Eq/eq, Ord/cmp, Printable/to_string),
    /// or `None` if not recognized. Returns `Err` if auto-derive validation fails.
    fn try_auto_derive_method(
        &self,
        class_name: &str,
        trait_name: &str,
        method_name: &str,
        fields: &IndexMap<String, Type>,
        class_type: &Type,
    ) -> Result<Option<(String, Type)>, Diagnostic> {
        match (trait_name, method_name) {
            ("Eq", "eq") => {
                self.check_auto_derive(class_name, fields, "Eq", "E021", Self::type_includes_eq)?;
                Ok(Some((
                    "eq".into(),
                    Type::Function {
                        param_names: vec!["other".into()],
                        params: vec![class_type.clone()],
                        ret: Box::new(Type::Bool),
                        throws: None,
                        suspendable: false,
                    },
                )))
            }
            ("Ord", "cmp") => {
                self.check_auto_derive(class_name, fields, "Ord", "E022", Self::type_includes_ord)?;
                Ok(Some((
                    "cmp".into(),
                    Type::Function {
                        param_names: vec!["other".into()],
                        params: vec![class_type.clone()],
                        ret: Box::new(Type::Custom("Ordering".into(), Vec::new())),
                        throws: None,
                        suspendable: false,
                    },
                )))
            }
            ("Printable", "to_string") => {
                self.check_auto_derive(
                    class_name,
                    fields,
                    "Printable",
                    "E023",
                    Self::type_includes_printable,
                )?;
                Ok(Some((
                    "to_string".into(),
                    Type::Function {
                        param_names: vec![],
                        params: vec![],
                        ret: Box::new(Type::String),
                        throws: None,
                        suspendable: false,
                    },
                )))
            }
            _ => Ok(None),
        }
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
            defaults,
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
                throws: resolved_throws.map(Box::new),
                type_constraints: type_constraints.clone(),
                defaults: defaults.clone(),
                span: *span,
            }
        } else {
            expr.clone()
        }
    }

    /// Substitute `Self` (represented as `Type::Custom("Self", [])`) with the concrete class type.
    fn substitute_self(ty: &Type, class_type: &Type) -> Type {
        ty.map_type(&|t| {
            if let Type::Custom(name, args) = t
                && name == "Self"
                && args.is_empty()
            {
                return Some(class_type.clone());
            }
            None
        })
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

    /// Check if a type includes the given protocol.
    ///
    /// Primitive types satisfy protocols based on a static mapping:
    /// - Printable: Int, Float, String, Bool, Nil
    /// - Eq: Int, Float, String, Bool, Nil (Ord implies Eq)
    /// - Ord: Int, Float, String, Bool
    ///
    /// Custom types satisfy a protocol if their `includes` list contains it
    /// (checked on both classes and enums). Ord implies Eq.
    /// List types recurse on the element type. Error is always compatible.
    pub(crate) fn type_includes_protocol(&self, ty: &Type, protocol: &str) -> bool {
        match ty {
            Type::Int | Type::Float | Type::String | Type::Bool => true,
            Type::Nil => matches!(protocol, "Printable" | "Eq"),
            Type::Custom(name, _) => {
                let traits_to_check: &[&str] = match protocol {
                    "Eq" => &["Eq", "Ord"],
                    _ => &[protocol],
                };
                let check_includes = |includes: &[String]| {
                    traits_to_check
                        .iter()
                        .any(|t| includes.contains(&t.to_string()))
                };
                if let Some(info) = self.env.get_class(name) {
                    return check_includes(&info.includes);
                }
                if let Some(info) = self.env.get_enum(name) {
                    return check_includes(&info.includes);
                }
                false
            }
            Type::List(inner) => self.type_includes_protocol(inner, protocol),
            Type::Error => true,
            _ => false,
        }
    }

    /// Check if a type includes Printable.
    pub(crate) fn type_includes_printable(&self, ty: &Type) -> bool {
        self.type_includes_protocol(ty, "Printable")
    }

    /// Check if a type includes Eq (Ord implies Eq).
    pub(crate) fn type_includes_eq(&self, ty: &Type) -> bool {
        self.type_includes_protocol(ty, "Eq")
    }

    /// Check if a type includes Ord.
    pub(crate) fn type_includes_ord(&self, ty: &Type) -> bool {
        self.type_includes_protocol(ty, "Ord")
    }

    /// Inject Iterable vocabulary methods into a class's method map.
    /// Methods are only added if not already defined by the class (allowing overrides).
    /// Conditional methods (min, max, sort) are added here;
    /// the Ord check happens at call site in check_member.
    fn inject_iterable_vocabulary(&self, method_map: &mut HashMap<String, Type>, elem_ty: &Type) {
        for (name, ty) in iterable_vocabulary_methods(elem_ty) {
            method_map.entry(name).or_insert(ty);
        }
    }

    /// Extract the Iterable element type from a class's each() method signature.
    /// Validates the signature matches the Iterable protocol: each(f: (T) -> Void) -> Void
    pub(crate) fn get_iterable_element_type_from_class(info: &ClassInfo) -> Option<Type> {
        if let Some(Type::Function { params, ret, .. }) = info.methods.get("each")
            && params.len() == 1
            && **ret == Type::Void
            && let Some(Type::Function {
                params: cb_params,
                ret: cb_ret,
                ..
            }) = params.first()
            && cb_params.len() == 1
            && **cb_ret == Type::Void
        {
            return cb_params.first().cloned();
        }
        None
    }

    /// Extract the element type T from a class that includes Iterator[T].
    /// Looks at the parametric_includes to find the concrete type argument.
    /// Falls back to inspecting the next() method return type.
    pub(crate) fn get_iterator_element_type_from_class(info: &ClassInfo) -> Option<Type> {
        // First try parametric_includes for the concrete type arg
        for (tname, targs) in &info.parametric_includes {
            if tname == "Iterator" && targs.len() == 1 {
                return Some(targs[0].clone());
            }
        }
        // Fallback: inspect next() return type (should be T?)
        if let Some(Type::Function { ret, .. }) = info.methods.get("next")
            && let Type::Nullable(inner) = ret.as_ref()
        {
            return Some(*inner.clone());
        }
        None
    }
}

/// Returns the 13 Iterable vocabulary method signatures for a given element type.
/// Used by both List[T] builtins and classes that include Iterable.
pub(crate) fn iterable_vocabulary_methods(elem_ty: &Type) -> Vec<(String, Type)> {
    let u_var = || Type::TypeVar("U".into(), vec![]);
    let callback_predicate = || Type::Function {
        param_names: vec!["_0".into()],
        params: vec![elem_ty.clone()],
        ret: Box::new(Type::Bool),
        throws: None,
        suspendable: false,
    };

    vec![
        // map: (f: (T) -> U) -> List[U]
        (
            "map".into(),
            Type::Function {
                param_names: vec!["f".into()],
                params: vec![Type::Function {
                    param_names: vec!["_0".into()],
                    params: vec![elem_ty.clone()],
                    ret: Box::new(u_var()),
                    throws: None,
                    suspendable: false,
                }],
                ret: Box::new(Type::List(Box::new(u_var()))),
                throws: None,
                suspendable: false,
            },
        ),
        // filter: (f: (T) -> Bool) -> List[T]
        (
            "filter".into(),
            Type::Function {
                param_names: vec!["f".into()],
                params: vec![callback_predicate()],
                ret: Box::new(Type::List(Box::new(elem_ty.clone()))),
                throws: None,
                suspendable: false,
            },
        ),
        // reduce: (init: U, f: (U, T) -> U) -> U
        (
            "reduce".into(),
            Type::Function {
                param_names: vec!["init".into(), "f".into()],
                params: vec![
                    u_var(),
                    Type::Function {
                        param_names: vec!["_0".into(), "_1".into()],
                        params: vec![u_var(), elem_ty.clone()],
                        ret: Box::new(u_var()),
                        throws: None,
                        suspendable: false,
                    },
                ],
                ret: Box::new(u_var()),
                throws: None,
                suspendable: false,
            },
        ),
        // find: (f: (T) -> Bool) -> T?
        (
            "find".into(),
            Type::Function {
                param_names: vec!["f".into()],
                params: vec![callback_predicate()],
                ret: Box::new(Type::Nullable(Box::new(elem_ty.clone()))),
                throws: None,
                suspendable: false,
            },
        ),
        // any: (f: (T) -> Bool) -> Bool
        (
            "any".into(),
            Type::Function {
                param_names: vec!["f".into()],
                params: vec![callback_predicate()],
                ret: Box::new(Type::Bool),
                throws: None,
                suspendable: false,
            },
        ),
        // all: (f: (T) -> Bool) -> Bool
        (
            "all".into(),
            Type::Function {
                param_names: vec!["f".into()],
                params: vec![callback_predicate()],
                ret: Box::new(Type::Bool),
                throws: None,
                suspendable: false,
            },
        ),
        // count: () -> Int
        (
            "count".into(),
            Type::Function {
                param_names: vec![],
                params: vec![],
                ret: Box::new(Type::Int),
                throws: None,
                suspendable: false,
            },
        ),
        // first: () -> T?
        (
            "first".into(),
            Type::Function {
                param_names: vec![],
                params: vec![],
                ret: Box::new(Type::Nullable(Box::new(elem_ty.clone()))),
                throws: None,
                suspendable: false,
            },
        ),
        // last: () -> T?
        (
            "last".into(),
            Type::Function {
                param_names: vec![],
                params: vec![],
                ret: Box::new(Type::Nullable(Box::new(elem_ty.clone()))),
                throws: None,
                suspendable: false,
            },
        ),
        // to_list: () -> List[T]
        (
            "to_list".into(),
            Type::Function {
                param_names: vec![],
                params: vec![],
                ret: Box::new(Type::List(Box::new(elem_ty.clone()))),
                throws: None,
                suspendable: false,
            },
        ),
        // min: () -> T? (requires T includes Ord -- checked at call site)
        (
            "min".into(),
            Type::Function {
                param_names: vec![],
                params: vec![],
                ret: Box::new(Type::Nullable(Box::new(elem_ty.clone()))),
                throws: None,
                suspendable: false,
            },
        ),
        // max: () -> T? (requires T includes Ord -- checked at call site)
        (
            "max".into(),
            Type::Function {
                param_names: vec![],
                params: vec![],
                ret: Box::new(Type::Nullable(Box::new(elem_ty.clone()))),
                throws: None,
                suspendable: false,
            },
        ),
        // sort: () -> List[T] (requires T includes Ord -- checked at call site)
        (
            "sort".into(),
            Type::Function {
                param_names: vec![],
                params: vec![],
                ret: Box::new(Type::List(Box::new(elem_ty.clone()))),
                throws: None,
                suspendable: false,
            },
        ),
    ]
}
