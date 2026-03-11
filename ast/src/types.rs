use serde::{Deserialize, Serialize};

/// Constraint on a generic type parameter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TypeConstraint {
    /// T extends ClassName — must be a subclass of the named class.
    Extends(String),
    /// T includes TraitName[Args] — must include the named trait.
    Includes(String, Vec<Type>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Type {
    Int,
    Float,
    Bool,
    String,
    Nil,
    Void,
    Never,
    /// Sentinel for error recovery — compatible with everything to prevent cascading errors.
    Error,
    /// Placeholder for lambda parameters whose types will be inferred from call context.
    Inferred,
    List(Box<Type>),
    /// Map type scaffolding — parser support exists but no literal syntax or runtime yet.
    Map(Box<Type>, Box<Type>),
    Custom(std::string::String, Vec<Type>),
    TypeVar(std::string::String, Vec<TypeConstraint>),
    Function {
        param_names: Vec<std::string::String>,
        params: Vec<Type>,
        ret: Box<Type>,
        throws: Option<Box<Type>>,
    },
    Task(Box<Type>),
    Nullable(Box<Type>),
}

impl Type {
    pub fn from_ident(name: &str) -> Self {
        match name {
            "Int" => Type::Int,
            "Float" => Type::Float,
            "Bool" => Type::Bool,
            "String" => Type::String,
            "Nil" => Type::Nil,
            "Void" => Type::Void,
            "Never" => Type::Never,
            _ => Type::Custom(name.to_string(), Vec::new()),
        }
    }

    /// Returns true if this type is the error recovery sentinel.
    pub fn is_error(&self) -> bool {
        matches!(self, Type::Error)
    }

    /// Recursively transform a type by applying `f` to each node bottom-up.
    /// The closure receives each type after its children have been transformed.
    /// Leaf types (Int, Float, Bool, etc.) are passed through unchanged unless `f` transforms them.
    pub fn map_type<F>(&self, f: &F) -> Type
    where
        F: Fn(&Type) -> Option<Type>,
    {
        // First, try the custom transformation
        if let Some(result) = f(self) {
            return result;
        }
        // Otherwise, recurse into children
        match self {
            Type::List(inner) => Type::List(Box::new(inner.map_type(f))),
            Type::Map(k, v) => Type::Map(Box::new(k.map_type(f)), Box::new(v.map_type(f))),
            Type::Nullable(inner) => Type::Nullable(Box::new(inner.map_type(f))),
            Type::Task(inner) => Type::Task(Box::new(inner.map_type(f))),
            Type::Function {
                param_names,
                params,
                ret,
                throws,
            } => Type::Function {
                param_names: param_names.clone(),
                params: params.iter().map(|p| p.map_type(f)).collect(),
                ret: Box::new(ret.map_type(f)),
                throws: throws.as_ref().map(|t| Box::new(t.map_type(f))),
            },
            Type::Custom(name, args) => {
                Type::Custom(name.clone(), args.iter().map(|a| a.map_type(f)).collect())
            }
            // Leaf types and TypeVar — return as-is
            _ => self.clone(),
        }
    }

    /// Check if a predicate holds for any node in the type tree.
    pub fn any_type<F>(&self, f: &F) -> bool
    where
        F: Fn(&Type) -> bool,
    {
        if f(self) {
            return true;
        }
        match self {
            Type::List(inner) | Type::Nullable(inner) | Type::Task(inner) => inner.any_type(f),
            Type::Map(k, v) => k.any_type(f) || v.any_type(f),
            Type::Function {
                params,
                ret,
                throws,
                ..
            } => {
                params.iter().any(|p| p.any_type(f))
                    || ret.any_type(f)
                    || throws.as_ref().is_some_and(|t| t.any_type(f))
            }
            Type::Custom(_, args) => args.iter().any(|a| a.any_type(f)),
            _ => false,
        }
    }

    /// Collect all items matching a predicate from the type tree.
    pub fn collect_types<F>(&self, f: &F, results: &mut Vec<Type>)
    where
        F: Fn(&Type) -> bool,
    {
        if f(self) {
            results.push(self.clone());
        }
        match self {
            Type::List(inner) | Type::Nullable(inner) | Type::Task(inner) => {
                inner.collect_types(f, results)
            }
            Type::Map(k, v) => {
                k.collect_types(f, results);
                v.collect_types(f, results);
            }
            Type::Function {
                params,
                ret,
                throws,
                ..
            } => {
                for p in params {
                    p.collect_types(f, results);
                }
                ret.collect_types(f, results);
                if let Some(t) = throws {
                    t.collect_types(f, results);
                }
            }
            Type::Custom(_, args) => {
                for a in args {
                    a.collect_types(f, results);
                }
            }
            _ => {}
        }
    }
}
