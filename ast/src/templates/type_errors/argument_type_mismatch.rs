use crate::types::Type;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArgumentTypeMismatch {
    pub param: String,
    pub expected: Type,
    pub actual: Type,
}

impl ArgumentTypeMismatch {
    pub fn code(&self) -> &'static str {
        "E005"
    }
    pub fn render(&self) -> String {
        format!(
            "Argument '{}' expects {}, got {}",
            self.param, self.expected, self.actual
        )
    }
}
