use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CircularImport {
    pub module: String,
}

impl CircularImport {
    pub fn code(&self) -> &'static str {
        "M003"
    }
    pub fn render(&self) -> String {
        format!("Circular import detected involving '{}'", self.module)
    }
}
