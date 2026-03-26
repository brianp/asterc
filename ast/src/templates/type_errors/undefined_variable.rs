use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UndefinedVariable {
    pub name: String,
}

impl UndefinedVariable {
    pub fn code(&self) -> &'static str {
        "E002"
    }
    pub fn render(&self) -> String {
        format!("Unknown identifier '{}'", self.name)
    }
}
