use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypeConstraintError {
    pub message: String,
}

impl TypeConstraintError {
    pub fn code(&self) -> &'static str {
        "E024"
    }
    pub fn render(&self) -> String {
        self.message.clone()
    }
}
