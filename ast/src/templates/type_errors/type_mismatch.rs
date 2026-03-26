use crate::types::Type;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypeMismatch {
    pub expected: Type,
    pub actual: Type,
}

impl TypeMismatch {
    pub fn code(&self) -> &'static str {
        "E001"
    }
    pub fn render(&self) -> String {
        format!(
            "Type annotation mismatch: expected {}, got {}",
            self.expected, self.actual
        )
    }
}
