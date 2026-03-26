use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShadowedVariable {
    pub name: String,
}

impl ShadowedVariable {
    pub fn code(&self) -> &'static str {
        "W003"
    }
    pub fn render(&self) -> String {
        format!("Variable '{}' shadows a previous binding", self.name)
    }
}
