use crate::types::Type;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReturnTypeMismatch {
    pub function: String,
    pub expected: Type,
    pub actual: Type,
}

impl ReturnTypeMismatch {
    pub fn code(&self) -> &'static str {
        "E004"
    }
    pub fn render(&self) -> String {
        format!(
            "Return type mismatch in '{}': expected {}, got {}",
            self.function, self.expected, self.actual
        )
    }
}
