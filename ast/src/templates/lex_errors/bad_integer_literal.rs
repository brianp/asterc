use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BadIntegerLiteral {
    pub literal: String,
}

impl BadIntegerLiteral {
    pub fn code(&self) -> &'static str {
        "L012"
    }
    pub fn render(&self) -> String {
        format!("invalid integer literal '{}'", self.literal)
    }
}
