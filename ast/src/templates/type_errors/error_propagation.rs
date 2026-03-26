use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorPropagation {
    pub message: String,
}

impl ErrorPropagation {
    pub fn code(&self) -> &'static str {
        "E013"
    }
    pub fn render(&self) -> String {
        self.message.clone()
    }
}
