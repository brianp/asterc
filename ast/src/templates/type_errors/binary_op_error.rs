use crate::types::Type;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BinaryOpError {
    pub op: String,
    pub left: Type,
    pub right: Type,
}

impl BinaryOpError {
    pub fn code(&self) -> &'static str {
        "E003"
    }
    pub fn render(&self) -> String {
        format!(
            "'{}' used outside of a valid context or with incompatible types {} and {}",
            self.op, self.left, self.right
        )
    }
}
