use crate::types::Type;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConditionTypeError {
    pub actual: Type,
}

impl ConditionTypeError {
    pub fn code(&self) -> &'static str {
        "E015"
    }
    pub fn render(&self) -> String {
        format!("Condition must be Bool, got {}", self.actual)
    }
}
