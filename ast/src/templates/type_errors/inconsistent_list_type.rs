use crate::types::Type;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InconsistentListType {
    pub expected: Type,
    pub actual: Type,
}

impl InconsistentListType {
    pub fn code(&self) -> &'static str {
        "E017"
    }
    pub fn render(&self) -> String {
        format!(
            "List elements have inconsistent types: expected {}, got {}",
            self.expected, self.actual
        )
    }
}
