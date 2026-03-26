use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConstraintError {
    pub message: String,
}

impl ConstraintError {
    pub fn code(&self) -> &'static str {
        "E021"
    }
    pub fn render(&self) -> String {
        self.message.clone()
    }
}
