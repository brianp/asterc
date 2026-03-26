use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UndeclaredAssignment {
    pub name: String,
}

impl UndeclaredAssignment {
    pub fn code(&self) -> &'static str {
        "E009"
    }
    pub fn render(&self) -> String {
        format!("Assignment to undeclared variable '{}'", self.name)
    }
}
