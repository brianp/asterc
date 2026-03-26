use crate::types::Type;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnaryOpError {
    pub op: String,
    pub actual: Type,
}

impl UnaryOpError {
    pub fn code(&self) -> &'static str {
        "E018"
    }
    pub fn render(&self) -> String {
        format!(
            "Cannot apply '{}' to {} (expected Bool)",
            self.op, self.actual
        )
    }
}
