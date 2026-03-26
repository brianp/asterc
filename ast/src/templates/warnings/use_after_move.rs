use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UseAfterMove {
    pub name: String,
}

impl UseAfterMove {
    pub fn code(&self) -> &'static str {
        "W002"
    }
    pub fn render(&self) -> String {
        format!("Variable '{}' used after copy/move boundary", self.name)
    }
}
