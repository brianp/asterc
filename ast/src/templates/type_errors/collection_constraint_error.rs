use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CollectionConstraintError {
    pub message: String,
}

impl CollectionConstraintError {
    pub fn code(&self) -> &'static str {
        "E025"
    }
    pub fn render(&self) -> String {
        self.message.clone()
    }
}
