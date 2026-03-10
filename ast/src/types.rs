use serde::{Deserialize, Serialize};

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
    List(Box<Type>),
    /// Map type scaffolding — parser support exists but no literal syntax or runtime yet.
    Map(Box<Type>, Box<Type>),
    Custom(std::string::String, Vec<Type>),
    TypeVar(std::string::String),
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
}
