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
        includes: &Option<Vec<String>>,
    ) -> Result<Type, Diagnostic> {
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

        let mut method_map = HashMap::new();
        for m in methods {
            if let Stmt::Let {
                name: mname, value, ..
            } = m
            {
                let mty = self.check_expr(value)?;
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

        // Validate includes
        if let Some(trait_names) = includes {
            for trait_name in trait_names {
                let trait_info = self.env.get_trait(trait_name).ok_or_else(|| {
                    Diagnostic::error(format!(
                        "Unknown trait '{}' in includes for class '{}'",
                        trait_name, name
                    ))
                    .with_code("E014")
                })?;
                for method_name in &trait_info.required_methods {
                    if let Some(class_method_ty) = method_map.get(method_name) {
                        if let Some(trait_method_ty) = trait_info.methods.get(method_name) {
                            let mut bindings = HashMap::new();
                            if Self::unify_type(trait_method_ty, class_method_ty, &mut bindings)
                                .is_err()
                            {
                                return Err(Diagnostic::error(format!(
                                    "Method '{}' in class '{}' has signature {:?}, but trait '{}' requires {:?}",
                                    method_name, name, class_method_ty, trait_name, trait_method_ty
                                ))
                                .with_code("E014"));
                            }
                        }
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

        let info = ClassInfo {
            ty: Type::Custom(name.to_string(), Vec::new()),
            fields: field_map,
            methods: method_map,
            generic_params: generic_params.clone(),
            extends: extends.clone(),
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

        // Synthesize constructor
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
            for ancestor in chain.into_iter().rev() {
                for fty in ancestor.fields.values() {
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
