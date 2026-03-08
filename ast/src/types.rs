#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Int,
    Float,
    Bool,
    String,
    Nil,
    Void,
    Never,
    List(Box<Type>),
    /// Map type scaffolding — parser support exists but no literal syntax or runtime yet.
    Map(Box<Type>, Box<Type>),
    Custom(String, Vec<Type>),
    TypeVar(String),
    Function {
        params: Vec<Type>,
        ret: Box<Type>,
        is_async: bool,
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
}
