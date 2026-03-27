use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SuspensionError {
    pub message: String,
}

impl SuspensionError {
    pub fn code(&self) -> &'static str {
        "E030"
    }
    pub fn render(&self) -> String {
        self.message.clone()
    }
}
