use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnusedDefaultParam {
    pub name: String,
}

impl UnusedDefaultParam {
    pub fn code(&self) -> &'static str {
        "W001"
    }
    pub fn render(&self) -> String {
        format!("Variable '{}' has default parameter", self.name)
    }
}
