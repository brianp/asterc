use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComparisonError {
    pub message: String,
}

impl ComparisonError {
    pub fn code(&self) -> &'static str {
        "E019"
    }
    pub fn render(&self) -> String {
        self.message.clone()
    }
}
