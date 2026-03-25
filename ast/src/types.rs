use std::fmt;

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
        suspendable: bool,
    },
    Task(Box<Type>),
    Nullable(Box<Type>),
}

impl Type {
    pub fn func(param_names: Vec<std::string::String>, params: Vec<Type>, ret: Type) -> Self {
        Type::Function {
            param_names,
            params,
            ret: Box::new(ret),
            throws: None,
            suspendable: false,
        }
    }

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

    /// Recursively transform a type by applying `f` top-down.
    /// Each node is offered to `f` first; if `f` returns `Some`, that result is used
    /// without visiting children. Otherwise children are transformed recursively.
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
                suspendable,
            } => Type::Function {
                param_names: param_names.clone(),
                params: params.iter().map(|p| p.map_type(f)).collect(),
                ret: Box::new(ret.map_type(f)),
                throws: throws.as_ref().map(|t| Box::new(t.map_type(f))),
                suspendable: *suspendable,
            },
            Type::Custom(name, args) => {
                Type::Custom(name.clone(), args.iter().map(|a| a.map_type(f)).collect())
            }
            Type::TypeVar(name, constraints) => {
                let new_constraints = constraints
                    .iter()
                    .map(|c| match c {
                        TypeConstraint::Includes(n, args) => TypeConstraint::Includes(
                            n.clone(),
                            args.iter().map(|a| a.map_type(f)).collect(),
                        ),
                        other => other.clone(),
                    })
                    .collect();
                Type::TypeVar(name.clone(), new_constraints)
            }
            // Remaining leaf types — return as-is
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
            Type::TypeVar(_, constraints) => constraints.iter().any(|c| match c {
                TypeConstraint::Includes(_, args) => args.iter().any(|a| a.any_type(f)),
                _ => false,
            }),
            _ => false,
        }
    }

    pub fn is_suspendable_function(&self) -> bool {
        match self {
            Type::Function { suspendable, .. } => *suspendable,
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
            Type::TypeVar(_, constraints) => {
                for c in constraints {
                    if let TypeConstraint::Includes(_, args) = c {
                        for a in args {
                            a.collect_types(f, results);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_type_transforms_typevar_constraint_args() {
        // TypeVar with Includes("Iterable", [Int]) should transform Int -> Float
        let ty = Type::TypeVar(
            "T".to_string(),
            vec![TypeConstraint::Includes(
                "Iterable".to_string(),
                vec![Type::Int],
            )],
        );
        let result = ty.map_type(&|t| {
            if matches!(t, Type::Int) {
                Some(Type::Float)
            } else {
                None
            }
        });
        match result {
            Type::TypeVar(_, constraints) => match &constraints[0] {
                TypeConstraint::Includes(_, args) => {
                    assert_eq!(
                        args[0],
                        Type::Float,
                        "Int inside Includes should be transformed to Float"
                    );
                }
                _ => panic!("expected Includes constraint"),
            },
            _ => panic!("expected TypeVar"),
        }
    }

    #[test]
    fn map_type_preserves_extends_constraint() {
        let ty = Type::TypeVar(
            "T".to_string(),
            vec![TypeConstraint::Extends("Animal".to_string())],
        );
        let result = ty.map_type(&|_| None);
        assert_eq!(result, ty);
    }

    #[test]
    fn any_type_finds_type_in_typevar_constraints() {
        let ty = Type::TypeVar(
            "T".to_string(),
            vec![TypeConstraint::Includes(
                "Iterable".to_string(),
                vec![Type::Int],
            )],
        );
        assert!(ty.any_type(&|t| matches!(t, Type::Int)));
    }

    #[test]
    fn any_type_returns_false_for_unmatched_typevar() {
        let ty = Type::TypeVar(
            "T".to_string(),
            vec![TypeConstraint::Includes(
                "Iterable".to_string(),
                vec![Type::Int],
            )],
        );
        assert!(!ty.any_type(&|t| matches!(t, Type::Float)));
    }

    #[test]
    fn collect_types_finds_types_in_typevar_constraints() {
        let ty = Type::TypeVar(
            "T".to_string(),
            vec![TypeConstraint::Includes(
                "Iterable".to_string(),
                vec![Type::Int],
            )],
        );
        let mut results = Vec::new();
        ty.collect_types(&|t| matches!(t, Type::Int), &mut results);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], Type::Int);
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Int => write!(f, "Int"),
            Type::Float => write!(f, "Float"),
            Type::Bool => write!(f, "Bool"),
            Type::String => write!(f, "String"),
            Type::Nil => write!(f, "Nil"),
            Type::Void => write!(f, "Void"),
            Type::Never => write!(f, "Never"),
            Type::Error => write!(f, "Error"),
            Type::Inferred => write!(f, "Inferred"),
            Type::List(inner) => write!(f, "List[{}]", inner),
            Type::Nullable(inner) => write!(f, "{}?", inner),
            Type::Custom(name, params) if params.is_empty() => write!(f, "{}", name),
            Type::Custom(name, params) => {
                let ps: Vec<std::string::String> = params.iter().map(|p| p.to_string()).collect();
                write!(f, "{}[{}]", name, ps.join(", "))
            }
            Type::Task(inner) => write!(f, "Task[{}]", inner),
            Type::Map(k, v) => write!(f, "Map[{}, {}]", k, v),
            Type::TypeVar(name, _) => write!(f, "{}", name),
            Type::Function {
                params,
                ret,
                throws,
                suspendable,
                ..
            } => {
                let ps: Vec<std::string::String> = params.iter().map(|p| p.to_string()).collect();
                write!(f, "Fn({}) -> {}", ps.join(", "), ret)?;
                if let Some(t) = throws {
                    write!(f, " throws {}", t)?;
                }
                if *suspendable {
                    write!(f, " suspendable")?;
                }
                Ok(())
            }
        }
    }
}
