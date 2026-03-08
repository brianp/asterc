use ast::{ClassInfo, Stmt, Type};
use std::collections::HashMap;

use crate::typechecker::TypeChecker;

impl TypeChecker {
    /// Handle all type-checking logic for a `class` statement.
    /// Called from the `Stmt::Class` arm of `check_stmt`.
    pub(crate) fn check_class_stmt(
        &mut self,
        name: &str,
        fields: &[(String, Type)],
        methods: &[Stmt],
        generic_params: &Option<Vec<String>>,
        extends: &Option<String>,
        includes: &Option<Vec<String>>,
    ) -> Result<Type, String> {
        let mut field_map = HashMap::new();
        for (fname, fty) in fields {
            field_map.insert(fname.clone(), fty.clone());
        }

        let mut method_map = HashMap::new();
        for m in methods {
            if let Stmt::Let {
                name: mname, value, ..
            } = m
            {
                let mty = self.check_expr(value)?;
                // Store methods with unqualified name for member access lookup
                let short_name = mname
                    .strip_prefix(&format!("{}.", name))
                    .unwrap_or(mname)
                    .to_string();
                method_map.insert(short_name, mty);
            } else {
                return Err(format!("Unexpected stmt in class methods: {:?}", m));
            }
        }

        // Validate includes
        if let Some(trait_names) = includes {
            for trait_name in trait_names {
                let trait_info = self.env.get_trait(trait_name).ok_or_else(|| {
                    format!("Unknown trait '{}' in includes for class '{}'", trait_name, name)
                })?;
                // Check that the class implements all required (abstract) trait methods
                for method_name in &trait_info.required_methods {
                    if let Some(class_method_ty) = method_map.get(method_name) {
                        // Compare signatures using unification (handles TypeVars)
                        if let Some(trait_method_ty) = trait_info.methods.get(method_name) {
                            let mut bindings = HashMap::new();
                            if Self::unify_type(trait_method_ty, class_method_ty, &mut bindings).is_err() {
                                return Err(format!(
                                    "Method '{}' in class '{}' has signature {:?}, but trait '{}' requires {:?}",
                                    method_name, name, class_method_ty, trait_name, trait_method_ty
                                ));
                            }
                        }
                    } else {
                        return Err(format!(
                            "Class '{}' must implement method '{}' from trait '{}'",
                            name, method_name, trait_name
                        ));
                    }
                }
            }
        }

        // Register class first so cycle detection can follow the chain
        let info = ClassInfo {
            ty: Type::Custom(name.to_string(), Vec::new()),
            fields: field_map,
            methods: method_map,
            generic_params: generic_params.clone(),
            extends: extends.clone(),
        };
        self.env.set_class(name.to_string(), info);

        // Validate extends — parent class must exist, no cycles
        if let Some(parent_name) = extends {
            if self.env.get_class(parent_name).is_none() {
                return Err(format!(
                    "Class '{}' extends unknown class '{}'",
                    name, parent_name
                ));
            }
            // Detect circular inheritance by walking the parent chain
            let mut visited = std::collections::HashSet::new();
            visited.insert(name.to_string());
            let mut current = Some(parent_name.clone());
            while let Some(ref cname) = current {
                if !visited.insert(cname.clone()) {
                    return Err(format!(
                        "Circular inheritance detected: class '{}' forms a cycle through '{}'",
                        name, cname
                    ));
                }
                current = self.env.get_class(cname).and_then(|info| info.extends.clone());
            }
        }

        // Check for field shadowing — child cannot redeclare inherited fields
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
                    return Err(format!(
                        "Field '{}' in class '{}' shadows inherited field from parent chain",
                        fname, name
                    ));
                }
            }
        }

        // Synthesize constructor: ClassName(parent_fields..., own_fields...) -> Custom(ClassName, [TypeVars])
        // Collect inherited fields from parent chain (in order)
        let mut inherited_field_types: Vec<Type> = Vec::new();
        if let Some(parent_name) = extends {
            let mut current = Some(parent_name.clone());
            let mut chain = Vec::new();
            while let Some(ref cname) = current {
                if let Some(parent_info) = self.env.get_class(cname) {
                    chain.push(parent_info.clone());
                    current = parent_info.extends.clone();
                } else {
                    break;
                }
            }
            // Reverse so root parent fields come first
            for ancestor in chain.into_iter().rev() {
                for (_, fty) in &ancestor.fields {
                    inherited_field_types.push(fty.clone());
                }
            }
        }
        let mut all_field_types = inherited_field_types;
        all_field_types.extend(fields.iter().map(|(_, t)| t.clone()));
        let field_types = all_field_types;
        let generic_type_args: Vec<Type> = generic_params
            .as_ref()
            .map(|gp| gp.iter().map(|p| Type::TypeVar(p.clone())).collect())
            .unwrap_or_default();
        self.env.set_var(
            name.to_string(),
            Type::Function {
                params: field_types,
                ret: Box::new(Type::Custom(name.to_string(), generic_type_args)),
                is_async: false,
                throws: None,
            },
        );

        Ok(Type::Void)
    }
}
