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

        let runtime_name = match field {
            "class_name" => "aster_introspect_class_name",
            "fields" => "aster_introspect_fields",
            "methods" => "aster_introspect_methods",
            "ancestors" => "aster_introspect_ancestors",
            "children" => "aster_introspect_children",
            _ => unreachable!(),
        };

        Ok(Some(FirExpr::RuntimeCall {
            name: runtime_name.to_string(),
            args: vec![FirExpr::StringLit(type_name_str)],
            ret_ty: FirType::Ptr,
        }))
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

                Ok(Some(FirExpr::RuntimeCall {
                    name: "aster_introspect_is_a".to_string(),
                    args: vec![
                        FirExpr::StringLit(type_name_str),
                        FirExpr::StringLit(target_type),
                    ],
                    ret_ty: FirType::Bool,
                }))
            }
            "responds_to" => {
                let type_name = self.resolve_static_type_name(object);
                let type_name_str = type_name.unwrap_or_else(|| "Unknown".to_string());

                // Consume the object expression
                let _ = self.lower_expr(object)?;

                // The argument is a string expression
                let method_name_expr = if let Some((_, _, arg_expr)) = args.first() {
                    self.lower_expr(arg_expr)?
                } else {
                    FirExpr::StringLit(String::new())
                };

                Ok(Some(FirExpr::RuntimeCall {
                    name: "aster_introspect_responds_to".to_string(),
                    args: vec![FirExpr::StringLit(type_name_str), method_name_expr],
                    ret_ty: FirType::Bool,
                }))
            }
            _ => Ok(None),
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
        if let Expr::Ident(name, _) = expr
            && let Some(ty) = self.scope.local_ast_types.get(name.as_str())
        {
            return Some(Self::type_to_name(ty));
        }
        // Try literal types
        match expr {
            Expr::Int(..) => Some("Int".to_string()),
            Expr::Float(..) => Some("Float".to_string()),
            Expr::Str(..) => Some("String".to_string()),
            Expr::Bool(..) => Some("Bool".to_string()),
            Expr::Nil(..) => Some("Nil".to_string()),
            _ => None,
        }
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
