use super::*;

impl Lowerer {
    /// Try to lower an introspection property access (class_name, fields, methods, ancestors, children).
    /// Returns Ok(Some(expr)) if handled, Ok(None) to fall through to normal field access.
    pub(crate) fn lower_introspection_member(
        &mut self,
        object: &Expr,
        field: &str,
    ) -> Result<Option<FirExpr>, LowerError> {
        match field {
            "class_name" | "fields" | "methods" | "ancestors" | "children" => {}
            _ => return Ok(None),
        }

        let type_name = self.resolve_static_type_name(object);
        let type_name_str = type_name.unwrap_or_else(|| "Unknown".to_string());

        // Consume the object expression (for side effects), but we don't need its value
        // for static introspection.
        let _ = self.lower_expr(object)?;

        match field {
            "class_name" => Ok(Some(FirExpr::RuntimeCall {
                name: "aster_introspect_class_name".to_string(),
                args: vec![FirExpr::StringLit(type_name_str)],
                ret_ty: FirType::Ptr,
            })),
            "fields" => {
                let serialized = self.serialize_fields(&type_name_str);
                Ok(Some(FirExpr::RuntimeCall {
                    name: "aster_introspect_fields".to_string(),
                    args: vec![FirExpr::StringLit(serialized)],
                    ret_ty: FirType::Ptr,
                }))
            }
            "methods" => {
                let serialized = self.serialize_methods(&type_name_str);
                Ok(Some(FirExpr::RuntimeCall {
                    name: "aster_introspect_methods".to_string(),
                    args: vec![FirExpr::StringLit(serialized)],
                    ret_ty: FirType::Ptr,
                }))
            }
            "ancestors" => {
                let ancestors = self.collect_ancestors(&type_name_str);
                let serialized = ancestors.join("|");
                Ok(Some(FirExpr::RuntimeCall {
                    name: "aster_introspect_ancestors".to_string(),
                    args: vec![FirExpr::StringLit(serialized)],
                    ret_ty: FirType::Ptr,
                }))
            }
            "children" => {
                let children = self.type_env.find_children(&type_name_str);
                let serialized = children.join("|");
                Ok(Some(FirExpr::RuntimeCall {
                    name: "aster_introspect_children".to_string(),
                    args: vec![FirExpr::StringLit(serialized)],
                    ret_ty: FirType::Ptr,
                }))
            }
            _ => unreachable!(),
        }
    }

    /// Try to lower an introspection method call (is_a, responds_to).
    /// Returns Ok(Some(expr)) if handled, Ok(None) to fall through.
    pub(crate) fn lower_introspection_call(
        &mut self,
        object: &Expr,
        method: &str,
        args: &[(String, ast::Span, Expr)],
    ) -> Result<Option<FirExpr>, LowerError> {
        match method {
            "is_a" => {
                let type_name = self.resolve_static_type_name(object);
                let type_name_str = type_name.unwrap_or_else(|| "Unknown".to_string());

                // Consume the object expression
                let _ = self.lower_expr(object)?;

                // The argument is a bare identifier naming a type (validated by typechecker)
                let target_type = if let Some((_, _, Expr::Ident(name, _))) = args.first() {
                    name.clone()
                } else {
                    "Unknown".to_string()
                };

                // Resolve at compile time by walking ancestor chain
                let ancestors = self.collect_ancestors(&type_name_str);
                let result = ancestors.contains(&target_type);

                Ok(Some(FirExpr::BoolLit(result)))
            }
            "responds_to" => {
                let type_name = self.resolve_static_type_name(object);
                let type_name_str = type_name.unwrap_or_else(|| "Unknown".to_string());

                // Consume the object expression
                let _ = self.lower_expr(object)?;

                // Check if the argument is a string literal for compile-time resolution
                if let Some((_, _, Expr::Str(method_name, _))) = args.first() {
                    let members = self.collect_all_members(&type_name_str);
                    let result = members.contains(method_name);
                    Ok(Some(FirExpr::BoolLit(result)))
                } else {
                    // Variable argument: fall back to runtime check
                    let method_name_expr = if let Some((_, _, arg_expr)) = args.first() {
                        self.lower_expr(arg_expr)?
                    } else {
                        FirExpr::StringLit(String::new())
                    };
                    let members = self.collect_all_members(&type_name_str);
                    let serialized = members.join("|");
                    Ok(Some(FirExpr::RuntimeCall {
                        name: "aster_introspect_responds_to".to_string(),
                        args: vec![FirExpr::StringLit(serialized), method_name_expr],
                        ret_ty: FirType::Bool,
                    }))
                }
            }
            _ => Ok(None),
        }
    }

    /// Collect the ancestor chain for a type (self first, root last).
    fn collect_ancestors(&self, type_name: &str) -> Vec<String> {
        let mut ancestors = vec![type_name.to_string()];
        let mut current = type_name.to_string();
        loop {
            let parent = self
                .type_env
                .get_class(&current)
                .and_then(|ci| ci.extends.clone());
            if let Some(parent_name) = parent {
                ancestors.push(parent_name.clone());
                current = parent_name;
            } else {
                break;
            }
        }
        ancestors
    }

    /// Collect all member names (fields + methods + inherited + built-in) for responds_to.
    fn collect_all_members(&self, type_name: &str) -> Vec<String> {
        let mut members = Vec::new();

        // Primitive built-in methods
        match type_name {
            "Int" => {
                members.extend(
                    ["is_even", "is_odd", "abs", "clamp", "min", "max"]
                        .iter()
                        .map(|s| s.to_string()),
                );
            }
            "Float" => {
                members.extend(
                    ["abs", "round", "floor", "ceil", "clamp", "min", "max"]
                        .iter()
                        .map(|s| s.to_string()),
                );
            }
            "String" => {
                members.extend(
                    [
                        "len",
                        "contains",
                        "starts_with",
                        "ends_with",
                        "trim",
                        "to_upper",
                        "to_lower",
                        "slice",
                        "replace",
                        "split",
                    ]
                    .iter()
                    .map(|s| s.to_string()),
                );
            }
            "Bool" => {}
            "List" => {
                members.extend(
                    [
                        "push", "pop", "len", "get", "set", "insert", "remove", "contains", "map",
                        "filter", "find", "any", "all", "reduce", "each", "first", "last", "count",
                        "min", "max", "sort", "random",
                    ]
                    .iter()
                    .map(|s| s.to_string()),
                );
            }
            _ => {}
        }

        // Walk class hierarchy for user-defined classes
        let mut current = Some(type_name.to_string());
        while let Some(ref cname) = current.clone() {
            if let Some(ci) = self.type_env.get_class(cname) {
                for field_name in ci.fields.keys() {
                    if !members.contains(field_name) {
                        members.push(field_name.clone());
                    }
                }
                for method_name in ci.methods.keys() {
                    if !members.contains(method_name) {
                        members.push(method_name.clone());
                    }
                }
                current = ci.extends.clone();
            } else {
                break;
            }
        }

        // Universal methods available on all types
        for m in &[
            "to_string",
            "class_name",
            "fields",
            "methods",
            "ancestors",
            "children",
            "is_a",
            "responds_to",
        ] {
            let s = m.to_string();
            if !members.contains(&s) {
                members.push(s);
            }
        }

        members
    }

    /// Serialize field metadata for a type, including inherited fields.
    /// Format: "name:TypeName:1|name2:TypeName2:0" (1=public, 0=private)
    /// Empty string for types with no fields.
    fn serialize_fields(&self, type_name: &str) -> String {
        // Primitives have no fields
        if matches!(
            type_name,
            "Int" | "Float" | "String" | "Bool" | "Nil" | "List" | "Set" | "Map"
        ) {
            return String::new();
        }

        // Collect class chain from root to leaf
        let mut chain: Vec<&ast::type_env::ClassInfo> = Vec::new();
        let mut current = Some(type_name.to_string());
        while let Some(ref cname) = current.clone() {
            if let Some(ci) = self.type_env.get_class(cname) {
                chain.push(ci);
                current = ci.extends.clone();
            } else {
                break;
            }
        }

        let mut fields: Vec<(String, String, bool)> = Vec::new();
        // Walk from root to leaf (reverse) so parent fields come first
        for ci in chain.into_iter().rev() {
            for (field_name, field_type) in &ci.fields {
                if !fields.iter().any(|(name, _, _)| name == field_name) {
                    let is_public = ci.pub_fields.contains(field_name);
                    fields.push((
                        field_name.clone(),
                        Self::type_to_name(field_type),
                        is_public,
                    ));
                }
            }
        }

        fields
            .iter()
            .map(|(name, ty, is_pub)| {
                format!("{}:{}:{}", name, ty, if *is_pub { "1" } else { "0" })
            })
            .collect::<Vec<_>>()
            .join("|")
    }

    /// Serialize method metadata for a type, including inherited and built-in methods.
    /// Format: "name:RetType:is_public:p1/T1,p2/T2|name2:RetType2:is_public2:params"
    fn serialize_methods(&self, type_name: &str) -> String {
        let mut methods: Vec<(String, String, bool, String)> = Vec::new();

        // Primitive built-in methods
        Self::add_primitive_methods(type_name, &mut methods);

        // Walk class hierarchy for user-defined methods
        let mut chain: Vec<&ast::type_env::ClassInfo> = Vec::new();
        let mut current = Some(type_name.to_string());
        while let Some(ref cname) = current.clone() {
            if let Some(ci) = self.type_env.get_class(cname) {
                chain.push(ci);
                current = ci.extends.clone();
            } else {
                break;
            }
        }

        // Walk from root to leaf (reverse), child methods override parent
        for ci in chain.into_iter().rev() {
            for (method_name, method_type) in &ci.methods {
                // Remove existing entry if overridden
                methods.retain(|(name, _, _, _)| name != method_name);

                let is_public = ci.pub_methods.contains(method_name);
                if let Type::Function {
                    param_names,
                    params,
                    ret,
                    ..
                } = method_type
                {
                    let ret_name = Self::type_to_name(ret);
                    let params_str = param_names
                        .iter()
                        .zip(params.iter())
                        .map(|(name, ty)| format!("{}/{}", name, Self::type_to_name(ty)))
                        .collect::<Vec<_>>()
                        .join(",");
                    methods.push((method_name.clone(), ret_name, is_public, params_str));
                }
            }
        }

        methods
            .iter()
            .map(|(name, ret, is_pub, params)| {
                format!(
                    "{}:{}:{}:{}",
                    name,
                    ret,
                    if *is_pub { "1" } else { "0" },
                    params
                )
            })
            .collect::<Vec<_>>()
            .join("|")
    }

    /// Add primitive built-in method signatures.
    fn add_primitive_methods(type_name: &str, methods: &mut Vec<(String, String, bool, String)>) {
        let add = |methods: &mut Vec<(String, String, bool, String)>,
                   name: &str,
                   ret: &str,
                   params: &str| {
            methods.push((name.into(), ret.into(), true, params.into()));
        };

        match type_name {
            "Int" => {
                add(methods, "is_even", "Bool", "");
                add(methods, "is_odd", "Bool", "");
                add(methods, "abs", "Int", "");
                add(methods, "clamp", "Int", "min/Int,max/Int");
                add(methods, "min", "Int", "other/Int");
                add(methods, "max", "Int", "other/Int");
                add(methods, "to_string", "String", "");
            }
            "Float" => {
                add(methods, "abs", "Float", "");
                add(methods, "round", "Int", "");
                add(methods, "floor", "Int", "");
                add(methods, "ceil", "Int", "");
                add(methods, "clamp", "Float", "min/Float,max/Float");
                add(methods, "min", "Float", "other/Float");
                add(methods, "max", "Float", "other/Float");
                add(methods, "to_string", "String", "");
            }
            "String" => {
                add(methods, "len", "Int", "");
                add(methods, "contains", "Bool", "substr/String");
                add(methods, "starts_with", "Bool", "prefix/String");
                add(methods, "ends_with", "Bool", "suffix/String");
                add(methods, "trim", "String", "");
                add(methods, "to_upper", "String", "");
                add(methods, "to_lower", "String", "");
                add(methods, "slice", "String", "start/Int,end/Int");
                add(methods, "replace", "String", "from/String,to/String");
                add(methods, "split", "List", "delimiter/String");
                add(methods, "to_string", "String", "");
            }
            "Bool" => {
                add(methods, "to_string", "String", "");
            }
            "List" => {
                add(methods, "push", "List", "element/Unknown");
                add(methods, "pop", "Unknown", "");
                add(methods, "len", "Int", "");
                add(methods, "get", "Unknown", "index/Int");
                add(methods, "set", "Void", "index/Int,value/Unknown");
                add(methods, "insert", "Void", "index/Int,value/Unknown");
                add(methods, "remove", "Unknown", "index/Int");
                add(methods, "contains", "Bool", "element/Unknown");
                add(methods, "random", "Unknown", "");
                add(methods, "to_string", "String", "");
            }
            _ => {}
        }
    }

    /// Resolve the static type name of an expression for introspection.
    /// Returns the type name as a string (e.g., "User", "Int", "String").
    fn resolve_static_type_name(&self, expr: &Expr) -> Option<String> {
        // Try the type table first
        if let Some(ty) = self.type_table.get(&expr.span()) {
            return Some(Self::type_to_name(ty));
        }
        // Try local AST types for identifiers
        if let Expr::Ident(name, _) = expr {
            if let Some(ty) = self.scope.local_ast_types.get(name.as_str()) {
                return Some(Self::type_to_name(ty));
            }
            // Fallback: check typechecker's type environment for variables
            // (covers top-level bindings not yet in local_ast_types)
            if let Some(ty) = self.type_env.get_var(name) {
                return Some(Self::type_to_name(ty));
            }
        }
        // Try literal types
        match expr {
            Expr::Int(..) => Some("Int".to_string()),
            Expr::Float(..) => Some("Float".to_string()),
            Expr::Str(..) => Some("String".to_string()),
            Expr::Bool(..) => Some("Bool".to_string()),
            Expr::Nil(..) => Some("Nil".to_string()),
            Expr::ListLiteral(..) => Some("List".to_string()),
            _ => None,
        }
    }

    /// Check if an expression produces a Type value (string at runtime).
    /// Used to allow `.to_string()` on Type values in the lowerer.
    pub(crate) fn is_type_valued_expr(&self, expr: &Expr) -> bool {
        if let Some(Type::Custom(name, _)) = self.type_table.get(&expr.span())
            && name == "Type"
        {
            return true;
        }
        if let Expr::Ident(name, _) = expr
            && let Some(Type::Custom(tname, _)) = self.scope.local_ast_types.get(name.as_str())
            && tname == "Type"
        {
            return true;
        }
        if let Expr::Member { field, .. } = expr
            && matches!(
                field.as_str(),
                "class_name" | "type_name" | "return_type" | "param_type"
            )
        {
            return true;
        }
        if let Expr::Index { object, .. } = expr {
            if let Expr::Member { field, .. } = object.as_ref()
                && matches!(field.as_str(), "ancestors" | "children")
            {
                return true;
            }
            if let Expr::Ident(name, _) = object.as_ref()
                && let Some(Type::List(inner)) = self
                    .scope
                    .local_ast_types
                    .get(name.as_str())
                    .or_else(|| self.type_env.get_var(name))
                && let Type::Custom(tname, _) = inner.as_ref()
                && tname == "Type"
            {
                return true;
            }
        }
        false
    }

    /// Convert a Type to its name string for introspection.
    fn type_to_name(ty: &Type) -> String {
        match ty {
            Type::Int => "Int".to_string(),
            Type::Float => "Float".to_string(),
            Type::String => "String".to_string(),
            Type::Bool => "Bool".to_string(),
            Type::Nil => "Nil".to_string(),
            Type::Void => "Void".to_string(),
            Type::Custom(name, _) => name.clone(),
            Type::List(_) => "List".to_string(),
            Type::Set(_) => "Set".to_string(),
            Type::Map(_, _) => "Map".to_string(),
            Type::Task(_) => "Task".to_string(),
            Type::Nullable(inner) => format!("{}?", Self::type_to_name(inner)),
            _ => "Unknown".to_string(),
        }
    }
}
