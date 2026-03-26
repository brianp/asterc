use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraitError {
    pub message: String,
}

impl TraitError {
    pub fn code(&self) -> &'static str {
        "E014"
    }
    pub fn render(&self) -> String {
        self.message.clone()
    }
}
