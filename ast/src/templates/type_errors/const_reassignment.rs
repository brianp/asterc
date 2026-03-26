use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConstReassignment {
    pub name: String,
}

impl ConstReassignment {
    pub fn code(&self) -> &'static str {
        "E026"
    }
    pub fn render(&self) -> String {
        format!("const binding '{}' cannot be reassigned", self.name)
    }
}
