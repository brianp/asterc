use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NotCompilable {
    pub message: String,
}

impl NotCompilable {
    pub fn code(&self) -> &'static str {
        "E028"
    }
    pub fn render(&self) -> String {
        self.message.clone()
    }
}
