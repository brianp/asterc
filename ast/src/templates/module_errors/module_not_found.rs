use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModuleNotFound {
    pub name: String,
}

impl ModuleNotFound {
    pub fn code(&self) -> &'static str {
        "M001"
    }
    pub fn render(&self) -> String {
        format!("Module '{}' not found", self.name)
    }
}
