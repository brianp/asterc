use crate::types::Type;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexTypeError {
    pub actual: Type,
}

impl IndexTypeError {
    pub fn code(&self) -> &'static str {
        "E016"
    }
    pub fn render(&self) -> String {
        format!("Index must be Int, got {}", self.actual)
    }
}
