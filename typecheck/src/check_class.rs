use ast::templates::DiagnosticTemplate;
use ast::templates::type_errors::{
    CollectionConstraintError, ConstraintError, DynamicMethodConflict, PrintableError, TraitError,
};
use ast::{ClassInfo, Diagnostic, Expr, Span, Stmt, Type};
use indexmap::IndexMap;
use std::collections::HashMap;

use crate::typechecker::TypeChecker;

impl TypeChecker {
    pub(crate) fn check_class_stmt(
        &mut self,
        name: &str,
        fields: &[(String, Type, bool)],
        methods: &[Stmt],
        generic_params: &Option<Vec<String>>,
        extends: &Option<String>,
        includes: &Option<Vec<(String, Vec<Type>)>>,
    ) -> Result<Type, Diagnostic> {
        // Pre-register the class so that method type checking recognizes
        // the class name as a known type (needed for inline generic inference).
        self.env.set_class(name.to_string(), {
            let mut info = ClassInfo::new(
                Type::Custom(name.to_string(), Vec::new()),
                IndexMap::new(),
                HashMap::new(),
            );
            info.generic_params = generic_params.clone();
            info.extends = extends.clone();
            info
        });

        let mut field_map = IndexMap::new();
        let mut pub_fields = std::collections::HashSet::new();
        for (fname, fty, is_pub) in fields {
            if field_map.contains_key(fname) {
                return Err(Diagnostic::from_template(DiagnosticTemplate::TraitError(
                    TraitError {
                        message: format!("Duplicate field '{}' in class '{}'", fname, name),
                    },
                )));
            }
            field_map.insert(fname.clone(), fty.clone());
            if *is_pub {
                pub_fields.insert(fname.clone());
            }
        }

        // Pre-register constructor so method bodies can call ClassName(field: val)
        self.register_class_constructor(name, fields, generic_params, extends);

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

        // Pre-scan: if class includes DynamicReceiver, extract preliminary info from
        // method_missing AST so that bare calls inside other methods can resolve.
        let includes_dynamic_receiver = includes
            .as_ref()
            .map(|inc| inc.iter().any(|(n, _)| n == "DynamicReceiver"))
            .unwrap_or(false);
        if includes_dynamic_receiver
            && let Some(pre_info) = Self::prescan_dynamic_receiver(name, methods)
            && let Some(info) = self.env.get_class(name).cloned()
        {
            let mut updated = info;
            updated.dynamic_receiver = Some(pre_info);
            self.env.set_class(name.to_string(), updated);
        }

        // Create a child checker with class fields in scope for method body checking
        let mut method_checker = self.child_checker();
        for (fname, fty, _) in fields {
            method_checker.env.set_var(fname.clone(), fty.clone());
        }
        // Also inject inherited fields from parent classes
        if extends.is_some() {
            for ancestor in method_checker.walk_ancestors(name) {
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
        let mut pub_methods = std::collections::HashSet::new();

        // Set current_class so bare calls inside methods can resolve via DynamicReceiver
        method_checker.sc.current_class = Some(name.to_string());

        let methods_result = self.check_class_methods(
            name,
            &class_type,
            methods,
            has_parametric_overloads,
            &mut method_checker,
            &mut method_map,
            &mut overloaded_methods,
            &mut pub_methods,
        );

        // Always restore parent env, then propagate any error
        self.restore_from_child(method_checker);
        methods_result?;

        let (includes_list, parametric_includes) = self.validate_trait_inclusions(
            name,
            &class_type,
            &field_map,
            includes,
            &mut method_map,
            &mut overloaded_methods,
            &mut pub_methods,
        )?;

        self.inject_auto_derive(&mut method_map, &mut pub_methods, &includes_list);

        // ── DynamicReceiver validation ──────────────────────────────────
        let dynamic_receiver = if includes_list.contains(&"DynamicReceiver".to_string()) {
            Some(self.validate_dynamic_receiver(name, methods, &method_map)?)
        } else {
            None
        };

        let mut info = ClassInfo::new(class_type, field_map, method_map);
        info.generic_params = generic_params.clone();
        info.extends = extends.clone();
        info.includes = includes_list;
        info.overloaded_methods = overloaded_methods;
        info.parametric_includes = parametric_includes;
        info.pub_fields = pub_fields;
        info.pub_methods = pub_methods;
        info.dynamic_receiver = dynamic_receiver;
        self.env.set_class(name.to_string(), info);

        // Validate extends
        if let Some(parent_name) = extends {
            if self.env.get_class(parent_name).is_none() {
                return Err(Diagnostic::from_template(DiagnosticTemplate::TraitError(
                    TraitError {
                        message: format!(
                            "Class '{}' extends unknown class '{}'",
                            name, parent_name
                        ),
                    },
                )));
            }
            self.validate_circular_inheritance(name)?;
        }

        // Check for field shadowing
        if extends.is_some() {
            self.detect_field_shadowing(name, fields)?;
        }

        // Final constructor registration (may differ from pre-registration if
        // field shadowing check above modified the picture, but keeps them in sync)
        self.register_class_constructor(name, fields, generic_params, extends);

        Ok(Type::Void)
    }

    /// Iterate through class method statements, type-check each one, and populate
    /// `method_map`, `overloaded_methods`, and `pub_methods`.
    ///
    /// Runs inside a closure so the caller can always restore the child checker's
    /// environment even when an error occurs.
    #[allow(clippy::too_many_arguments)]
    fn check_class_methods(
        &mut self,
        name: &str,
        class_type: &Type,
        methods: &[Stmt],
        has_parametric_overloads: bool,
        method_checker: &mut TypeChecker,
        method_map: &mut HashMap<String, Type>,
        overloaded_methods: &mut HashMap<String, Vec<Type>>,
        pub_methods: &mut std::collections::HashSet<String>,
    ) -> Result<(), Diagnostic> {
        for m in methods {
            if let Stmt::Let {
                name: mname,
                value,
                is_public: m_is_public,
                ..
            } = m
            {
                // Substitute Self -> class type in method lambda types before checking
                let resolved_value = Self::substitute_self_in_lambda(value, class_type);
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
                        self.reg.default_params.insert(mname.clone(), default_set);
                    }
                }

                let short_name = mname
                    .strip_prefix(&format!("{}.", name))
                    .unwrap_or(mname)
                    .to_string();
                if *m_is_public {
                    pub_methods.insert(short_name.clone());
                }
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
                        return Err(Diagnostic::from_template(DiagnosticTemplate::TraitError(
                            TraitError {
                                message: format!(
                                    "Duplicate method '{}' in class '{}'",
                                    short_name, name
                                ),
                            },
                        ))
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
                return Err(Diagnostic::from_template(DiagnosticTemplate::TraitError(
                    TraitError {
                        message: format!("Unexpected stmt in class methods: {:?}", m),
                    },
                ))
                .with_label(m.span(), "expected method definition"));
            }
        }
        Ok(())
    }

    /// Check that the class satisfies all trait inclusions and, where possible,
    /// auto-derive missing required methods. Also handles Iterable inference and
    /// the Ord-implies-Eq auto-inclusion rule.
    ///
    /// Mutates `method_map` (may inject auto-derived methods and Iterable vocabulary),
    /// `overloaded_methods`, and `pub_methods`.
    ///
    /// Returns the computed `(includes_list, parametric_includes)` so the caller can
    /// store them in `ClassInfo` (includes_list may include auto-added traits such as Eq).
    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    fn validate_trait_inclusions(
        &mut self,
        name: &str,
        class_type: &Type,
        field_map: &IndexMap<String, Type>,
        includes: &Option<Vec<(String, Vec<Type>)>>,
        method_map: &mut HashMap<String, Type>,
        overloaded_methods: &mut HashMap<String, Vec<Type>>,
        pub_methods: &mut std::collections::HashSet<String>,
    ) -> Result<(Vec<String>, Vec<(String, Vec<Type>)>), Diagnostic> {
        let includes_refs = includes.clone().unwrap_or_default();

        // Build includes list of base trait names
        let mut includes_list: Vec<String> = includes_refs.iter().map(|(n, _)| n.clone()).collect();

        // Ord includes Eq — auto-add Eq if Ord is included
        if includes_list.contains(&"Ord".to_string()) && !includes_list.contains(&"Eq".to_string())
        {
            includes_list.push("Eq".to_string());
            // Auto-import Eq from builtins if not already in scope
            if self.env.get_trait("Eq").is_none()
                && let Some(eq_info) = self.reg.builtin_traits.get("Eq")
            {
                self.env.set_trait("Eq".into(), eq_info.clone());
            }
        }

        // Iterable: infer element type from each() and inject vocabulary methods.
        // `includes Iterable` (no type args) — infer T from each(f: Fn(T) -> Void).
        // `includes Iterable[T]` — error, must use bare form.
        let mut iterable_element_type: Option<Type> = None;
        if includes_list.contains(&"Iterable".to_string()) {
            // Check for explicit type args (not allowed)
            for (tname, targs) in &includes_refs {
                if tname == "Iterable" && !targs.is_empty() {
                    return Err(Diagnostic::from_template(DiagnosticTemplate::CollectionConstraintError(CollectionConstraintError {
                        message: "Iterable does not take type parameters. The element type is inferred from your each() method signature".to_string(),
                    })));
                }
            }

            // Find each() in the class methods to infer element type
            let each_method = method_map.get("each");
            if let Some(Type::Function { params, .. }) = each_method {
                // each(f: Fn(T) -> Void) — extract T from the callback parameter
                if let Some(Type::Function {
                    params: cb_params, ..
                }) = params.first()
                    && let Some(element_ty) = cb_params.first()
                {
                    iterable_element_type = Some(element_ty.clone());
                }
            }

            if iterable_element_type.is_none() {
                return Err(Diagnostic::from_template(
                    DiagnosticTemplate::CollectionConstraintError(CollectionConstraintError {
                        message: format!(
                            "Class '{}' includes Iterable but has no each(f: Fn(T) -> Void) -> Void method",
                            name
                        ),
                    }),
                ));
            }

            // Inject vocabulary methods (only if not already defined by the class)
            let elem_ty = iterable_element_type
                .clone()
                .expect("invariant: set when Iterable detected above");
            // Mark all vocabulary methods as public (they come from trait inclusion)
            for (vname, _) in iterable_vocabulary_methods(&elem_ty) {
                if !method_map.contains_key(&vname) {
                    pub_methods.insert(vname);
                }
            }
            self.inject_iterable_vocabulary(method_map, &elem_ty);
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
            // DynamicReceiver is validated separately in validate_dynamic_receiver
            if trait_name == "DynamicReceiver" {
                validated_traits.insert(trait_name.clone());
                continue;
            }
            let trait_info = self.env.get_trait(trait_name).ok_or_else(|| {
                // Check if it's a known stdlib trait — give helpful import suggestion
                if self.reg.builtin_traits.contains_key(trait_name) {
                    let submod = match trait_name.as_str() {
                        "Eq" | "Ord" => "cmp",
                        "Printable" => "fmt",
                        "Iterable" | "Iterator" => "collections",
                        "From" | "Into" => "convert",
                        "Drop" | "Close" => "lifecycle",
                        _ => "std",
                    };
                    Diagnostic::from_template(DiagnosticTemplate::TraitError(TraitError {
                        message: format!(
                            "Unknown trait '{}'. Add `use std/{} {{ {} }}` to import it",
                            trait_name, submod, trait_name
                        ),
                    }))
                } else {
                    Diagnostic::from_template(DiagnosticTemplate::TraitError(TraitError {
                        message: format!(
                            "Unknown trait '{}' in includes for class '{}'",
                            trait_name, name
                        ),
                    }))
                }
            })?;

            // Validate type argument arity for parametric traits
            if let Some(ref gp) = trait_info.generic_params
                && targs.len() != gp.len()
            {
                return Err(Diagnostic::from_template(DiagnosticTemplate::TraitError(
                    TraitError {
                        message: format!(
                            "Trait '{}' expects {} type parameter(s), got {}",
                            trait_name,
                            gp.len(),
                            targs.len()
                        ),
                    },
                )));
            }

            for method_name in &trait_info.required_methods {
                // Trait-satisfying methods are part of the public interface
                pub_methods.insert(method_name.clone());
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
                            &Self::substitute_self(trait_method_ty, class_type),
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
                                    Type::func(param_names.clone(), params.clone(), *ret.clone())
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
                            Self::substitute_self(trait_method_ty, class_type);

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
                                } => Type::func(param_names.clone(), params.clone(), *ret.clone()),
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
                                return Err(Diagnostic::from_template(
                                    DiagnosticTemplate::TraitError(TraitError {
                                        message: format!(
                                            "Method '{}' in class '{}' has signature {}, but trait '{}' requires {}",
                                            method_name,
                                            name,
                                            class_method_ty,
                                            trait_name,
                                            resolved_trait_ty
                                        ),
                                    }),
                                ));
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
                        field_map,
                        class_type,
                    )? {
                        pub_methods.insert(mname.clone());
                        method_map.insert(mname, mty);
                    } else {
                        return Err(Diagnostic::from_template(DiagnosticTemplate::TraitError(
                            TraitError {
                                message: format!(
                                    "Class '{}' must implement method '{}' from trait '{}'",
                                    name, method_name, trait_name
                                ),
                            },
                        )));
                    }
                }
            }
            validated_traits.insert(trait_name.clone());
        }

        // Build parametric_includes list preserving duplicates with type args
        let parametric_includes: Vec<(String, Vec<Type>)> = includes_refs
            .iter()
            .filter(|(_, targs)| !targs.is_empty())
            .cloned()
            .collect();

        Ok((includes_list, parametric_includes))
    }

    /// Inject auto-derived methods for traits that provide default implementations
    /// when the class does not define them explicitly.
    ///
    /// Currently handles: Printable's `debug()` method (defaults to `to_string()` signature).
    fn inject_auto_derive(
        &self,
        method_map: &mut HashMap<String, Type>,
        pub_methods: &mut std::collections::HashSet<String>,
        includes_list: &[String],
    ) {
        // Printable: auto-add debug() defaulting to to_string() signature if not defined
        if includes_list.contains(&"Printable".to_string()) && !method_map.contains_key("debug") {
            method_map.insert("debug".into(), Type::func(vec![], vec![], Type::String));
            pub_methods.insert("debug".into());
        }
    }

    /// Build and register the constructor function for a class, including inherited fields.
    fn register_class_constructor(
        &mut self,
        name: &str,
        fields: &[(String, Type, bool)],
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
        all_field_names.extend(fields.iter().map(|(n, _, _)| n.clone()));
        all_field_types.extend(fields.iter().map(|(_, t, _)| t.clone()));
        self.env.set_var(
            name.to_string(),
            Type::func(
                all_field_names,
                all_field_types,
                Type::Custom(name.to_string(), generic_type_args),
            ),
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
                return Err(Diagnostic::from_template(DiagnosticTemplate::TraitError(
                    TraitError {
                        message: format!(
                            "Circular inheritance detected: class '{}' forms a cycle through '{}'",
                            name, cname
                        ),
                    },
                )));
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
        fields: &[(String, Type, bool)],
    ) -> Result<(), Diagnostic> {
        let mut inherited_fields = std::collections::HashSet::new();
        for ancestor in self.walk_ancestors(name) {
            for fname in ancestor.fields.keys() {
                inherited_fields.insert(fname.clone());
            }
        }
        for (fname, _, _) in fields {
            if inherited_fields.contains(fname) {
                return Err(Diagnostic::from_template(DiagnosticTemplate::TraitError(
                    TraitError {
                        message: format!(
                            "Field '{}' in class '{}' shadows inherited field from parent chain",
                            fname, name
                        ),
                    },
                )));
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
                self.check_auto_derive(
                    class_name,
                    fields,
                    "Eq",
                    |msg| DiagnosticTemplate::ConstraintError(ConstraintError { message: msg }),
                    Self::type_includes_eq,
                )?;
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
                self.check_auto_derive(
                    class_name,
                    fields,
                    "Ord",
                    |msg| DiagnosticTemplate::ConstraintError(ConstraintError { message: msg }),
                    Self::type_includes_ord,
                )?;
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
                    |_msg| DiagnosticTemplate::PrintableError(PrintableError {}),
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
        make_template: fn(String) -> DiagnosticTemplate,
        checker: fn(&TypeChecker, &Type) -> bool,
    ) -> Result<(), Diagnostic> {
        for (fname, fty) in fields {
            if !checker(self, fty) {
                return Err(Diagnostic::from_template(make_template(format!(
                    "Cannot derive {} for '{}': field '{}' of type {} does not include {}",
                    trait_name, class_name, fname, fty, trait_name
                ))));
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
            Type::List(inner) | Type::Set(inner) => self.type_includes_protocol(inner, protocol),
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
    /// Validates the signature matches the Iterable protocol: each(f: Fn(T) -> Void) -> Void
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

    /// Pre-scan method_missing from raw AST (before type-checking) to extract
    /// DynamicReceiverInfo. This enables bare calls in other methods to resolve.
    fn prescan_dynamic_receiver(
        class_name: &str,
        methods: &[Stmt],
    ) -> Option<ast::DynamicReceiverInfo> {
        let mm_key = format!("{}.method_missing", class_name);
        let mm_stmt = methods.iter().find(
            |m| matches!(m, Stmt::Let { name, .. } if name == &mm_key || name == "method_missing"),
        )?;
        let Stmt::Let { value, .. } = mm_stmt else {
            return None;
        };
        let Expr::Lambda {
            params, ret_type, ..
        } = value
        else {
            return None;
        };
        if params.len() < 2 {
            return None;
        }
        if params[0].1 != Type::String {
            return None;
        }
        let value_ty = match &params[1].1 {
            Type::Map(k, v) if **k == Type::String => *v.clone(),
            _ => return None,
        };
        let return_ty = ret_type.clone();

        // Extract known dynamic method names (strip spans for the pre-scan info)
        let known_names = Self::extract_dynamic_method_names(class_name, methods)
            .map(|m| m.into_keys().collect::<std::collections::HashSet<String>>());

        Some(ast::DynamicReceiverInfo {
            args_value_ty: value_ty,
            return_ty,
            known_names,
        })
    }

    /// Validate a DynamicReceiver inclusion: check method_missing signature,
    /// extract known dynamic method names from the body, and check for conflicts.
    fn validate_dynamic_receiver(
        &self,
        class_name: &str,
        methods: &[Stmt],
        method_map: &HashMap<String, Type>,
    ) -> Result<ast::DynamicReceiverInfo, Diagnostic> {
        // Find method_missing in the method_map (typechecked type)
        let mm_type = method_map.get("method_missing").ok_or_else(|| {
            Diagnostic::from_template(DiagnosticTemplate::TraitError(TraitError {
                message: format!(
                    "Class '{}' includes DynamicReceiver but does not define method_missing",
                    class_name
                ),
            }))
        })?;

        // Validate signature shape: (String, Map[String, T]) -> R
        let (args_value_ty, return_ty) = match mm_type {
            Type::Function { params, ret, .. } => {
                if params.len() < 2 {
                    return Err(Diagnostic::from_template(DiagnosticTemplate::TraitError(
                        TraitError {
                            message: "method_missing must take at least 2 parameters: (fn_name: String, args: Map[String, T])".to_string(),
                        },
                    )));
                }
                if params[0] != Type::String {
                    return Err(Diagnostic::from_template(DiagnosticTemplate::TraitError(
                        TraitError {
                            message: format!(
                                "method_missing first parameter must be String, got {}",
                                params[0]
                            ),
                        },
                    )));
                }
                let value_ty = match &params[1] {
                    Type::Map(k, v) => {
                        if **k != Type::String {
                            return Err(Diagnostic::from_template(DiagnosticTemplate::TraitError(
                                TraitError {
                                    message: format!(
                                        "method_missing second parameter must be Map[String, T], got Map[{}, {}]",
                                        k, v
                                    ),
                                },
                            )));
                        }
                        *v.clone()
                    }
                    other => {
                        return Err(Diagnostic::from_template(DiagnosticTemplate::TraitError(
                            TraitError {
                                message: format!(
                                    "method_missing second parameter must be Map[String, T], got {}",
                                    other
                                ),
                            },
                        )));
                    }
                };
                (value_ty, *ret.clone())
            }
            _ => {
                return Err(Diagnostic::from_template(DiagnosticTemplate::TraitError(
                    TraitError {
                        message: "method_missing must be a function".to_string(),
                    },
                )));
            }
        };

        // Extract known dynamic method names from the method_missing AST body
        let known_names_with_spans = Self::extract_dynamic_method_names(class_name, methods);

        // If closed set, check for conflicts with real methods
        if let Some(ref names) = known_names_with_spans {
            for (dyn_name, dyn_span) in names {
                if dyn_name == "method_missing" {
                    continue;
                }
                if method_map.contains_key(dyn_name) {
                    // Find the span of the real method definition
                    let real_method_span = Self::find_method_span(class_name, dyn_name, methods);
                    let mut diag = Diagnostic::from_template(
                        DiagnosticTemplate::DynamicMethodConflict(DynamicMethodConflict {
                            method_name: dyn_name.clone(),
                            class_name: class_name.to_string(),
                        }),
                    )
                    .with_label(
                        *dyn_span,
                        format!("'{}' declared as dynamic method here", dyn_name),
                    );
                    if let Some(real_span) = real_method_span {
                        diag = diag.with_label(
                            real_span,
                            format!("'{}' already defined as a real method here", dyn_name),
                        );
                    }
                    return Err(diag);
                }
            }
        }

        let known_names = known_names_with_spans
            .map(|m| m.into_keys().collect::<std::collections::HashSet<String>>());

        Ok(ast::DynamicReceiverInfo {
            args_value_ty,
            return_ty,
            known_names,
        })
    }

    /// Inspect the method_missing body to extract known dynamic method names and their spans.
    /// Returns Some(map) if the catch-all throws FunctionNotFound (closed set), None otherwise.
    fn extract_dynamic_method_names(
        class_name: &str,
        methods: &[Stmt],
    ) -> Option<HashMap<String, Span>> {
        // Find the method_missing Stmt::Let in the AST
        let mm_key = format!("{}.method_missing", class_name);
        let mm_stmt = methods.iter().find(
            |m| matches!(m, Stmt::Let { name, .. } if name == &mm_key || name == "method_missing"),
        )?;

        let Stmt::Let { value, .. } = mm_stmt else {
            return None;
        };
        let ast::Expr::Lambda { body, params, .. } = value else {
            return None;
        };

        // The first parameter name (fn_name)
        let fn_name_param = params.first().map(|(n, _)| n.as_str()).unwrap_or("fn_name");

        // Look for a match expression on the fn_name parameter in the body
        for stmt in body {
            if let Stmt::Expr(
                ast::Expr::Match {
                    scrutinee, arms, ..
                },
                _,
            )
            | Stmt::Return(
                ast::Expr::Match {
                    scrutinee, arms, ..
                },
                _,
            ) = stmt
            {
                // Check if the scrutinee is the fn_name parameter
                if let ast::Expr::Ident(ref sname, _) = **scrutinee {
                    if sname != fn_name_param {
                        continue;
                    }
                } else {
                    continue;
                }

                // Collect string literal arms and check the catch-all
                let mut names: HashMap<String, Span> = HashMap::new();
                let mut has_throwing_catchall = false;

                for (pattern, arm_body) in arms {
                    match pattern {
                        ast::MatchPattern::Literal(expr, pat_span) => {
                            if let Expr::Str(s, _) = expr.as_ref() {
                                names.insert(s.clone(), *pat_span);
                            }
                        }
                        ast::MatchPattern::Wildcard(_) | ast::MatchPattern::Ident(_, _) => {
                            // Check if the catch-all throws FunctionNotFound
                            has_throwing_catchall = Self::body_throws_function_not_found(arm_body);
                        }
                        _ => {}
                    }
                }

                if has_throwing_catchall && !names.is_empty() {
                    return Some(names);
                }
                // Catch-all doesn't throw FunctionNotFound, so any name is valid
                return None;
            }
        }

        // No match on fn_name found, so any name is valid
        None
    }

    /// Find the span of a method definition in the class AST.
    fn find_method_span(class_name: &str, method_name: &str, methods: &[Stmt]) -> Option<Span> {
        let qualified = format!("{}.{}", class_name, method_name);
        for m in methods {
            if let Stmt::Let { name, span, .. } = m {
                let short = name
                    .strip_prefix(&format!("{}.", class_name))
                    .unwrap_or(name);
                if short == method_name || name == &qualified {
                    return Some(*span);
                }
            }
        }
        None
    }

    /// Check if an expression (match arm body) contains a throw of FunctionNotFound.
    fn body_throws_function_not_found(expr: &Expr) -> bool {
        if let Expr::Throw(inner, _) = expr
            && let Expr::Call { func, .. } = inner.as_ref()
            && let Expr::Ident(name, _) = func.as_ref()
        {
            return name == "FunctionNotFound";
        }
        false
    }
}

/// Returns the 14 Iterable vocabulary method signatures for a given element type.
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
        // map: (f: Fn(T) -> U) -> List[U]
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
        // filter: (f: Fn(T) -> Bool) -> List[T]
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
        // reduce: (init: U, f: Fn(U, T) -> U) -> U
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
        // find: (f: Fn(T) -> Bool) -> T?
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
        // any: (f: Fn(T) -> Bool) -> Bool
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
        // all: (f: Fn(T) -> Bool) -> Bool
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
        // unique: () -> List[T] (requires T includes Eq -- checked at call site)
        (
            "unique".into(),
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
