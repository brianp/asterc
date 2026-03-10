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

        let class_type = Type::Custom(name.to_string(), Vec::new());
        let mut includes_list = includes.clone().unwrap_or_default();

        // Ord includes Eq — auto-add Eq if Ord is included
        if includes_list.contains(&"Ord".to_string()) && !includes_list.contains(&"Eq".to_string())
        {
            includes_list.push("Eq".to_string());
        }

        // Validate includes — check trait satisfaction with Self substitution
        for trait_name in &includes_list {
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
                        // Substitute Self -> class type in trait method signature
                        let resolved_trait_ty = Self::substitute_self(trait_method_ty, &class_type);
                        let mut bindings = HashMap::new();
                        if Self::unify_type(&resolved_trait_ty, class_method_ty, &mut bindings)
                            .is_err()
                        {
                            return Err(Diagnostic::error(format!(
                                "Method '{}' in class '{}' has signature {:?}, but trait '{}' requires {:?}",
                                method_name, name, class_method_ty, trait_name, resolved_trait_ty
                            ))
                            .with_code("E014"));
                        }
                    }
                } else {
                    // Method not defined — check if auto-derive applies
                    if trait_name == "Eq" && method_name == "eq" {
                        // Auto-derive: verify all fields include Eq
                        self.check_auto_derive_eq(name, &field_map)?;
                        let eq_method_ty = Type::Function {
                            param_names: vec!["other".into()],
                            params: vec![class_type.clone()],
                            ret: Box::new(Type::Bool),
                            throws: None,
                        };
                        method_map.insert("eq".into(), eq_method_ty);
                    } else if trait_name == "Ord" && method_name == "cmp" {
                        // Auto-derive: verify all fields include Ord
                        self.check_auto_derive_ord(name, &field_map)?;
                        let cmp_method_ty = Type::Function {
                            param_names: vec!["other".into()],
                            params: vec![class_type.clone()],
                            ret: Box::new(Type::Custom("Ordering".into(), Vec::new())),
                            throws: None,
                        };
                        method_map.insert("cmp".into(), cmp_method_ty);
                    } else if trait_name == "Printable" && method_name == "to_string" {
                        // Auto-derive: verify all fields include Printable
                        self.check_auto_derive_printable(name, &field_map)?;
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

        // Synthesize constructor with named args
        let mut inherited_field_names: Vec<String> = Vec::new();
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
                for (fname, fty) in ancestor.fields.iter() {
                    inherited_field_names.push(fname.clone());
                    inherited_field_types.push(fty.clone());
                }
            }
        }
        let mut all_field_names = inherited_field_names;
        let mut all_field_types = inherited_field_types;
        all_field_names.extend(fields.iter().map(|(n, _)| n.clone()));
        all_field_types.extend(fields.iter().map(|(_, t)| t.clone()));
        let generic_type_args: Vec<Type> = generic_params
            .as_ref()
            .map(|gp| gp.iter().map(|p| Type::TypeVar(p.clone())).collect())
            .unwrap_or_default();
        self.env.set_var(
            name.to_string(),
            Type::Function {
                param_names: all_field_names,
                params: all_field_types,
                ret: Box::new(Type::Custom(name.to_string(), generic_type_args)),
                throws: None,
            },
        );

        Ok(Type::Void)
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

    /// Check that all fields of a class include Eq for auto-derive.
    fn check_auto_derive_eq(
        &self,
        class_name: &str,
        fields: &IndexMap<String, Type>,
    ) -> Result<(), Diagnostic> {
        for (fname, fty) in fields {
            if !self.type_includes_eq(fty) {
                return Err(Diagnostic::error(format!(
                    "Cannot derive Eq for '{}': field '{}' of type {:?} does not include Eq",
                    class_name, fname, fty
                ))
                .with_code("E021"));
            }
        }
        Ok(())
    }

    /// Check that all fields of a class include Ord for auto-derive.
    fn check_auto_derive_ord(
        &self,
        class_name: &str,
        fields: &IndexMap<String, Type>,
    ) -> Result<(), Diagnostic> {
        for (fname, fty) in fields {
            if !self.type_includes_ord(fty) {
                return Err(Diagnostic::error(format!(
                    "Cannot derive Ord for '{}': field '{}' of type {:?} does not include Ord",
                    class_name, fname, fty
                ))
                .with_code("E022"));
            }
        }
        Ok(())
    }

    /// Check that all fields of a class include Printable for auto-derive.
    fn check_auto_derive_printable(
        &self,
        class_name: &str,
        fields: &IndexMap<String, Type>,
    ) -> Result<(), Diagnostic> {
        for (fname, fty) in fields {
            if !self.type_includes_printable(fty) {
                return Err(Diagnostic::error(format!(
                    "Cannot derive Printable for '{}': field '{}' of type {:?} does not include Printable",
                    class_name, fname, fty
                ))
                .with_code("E023"));
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
