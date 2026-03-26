use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnexpectedToken {
    pub expected: String,
    pub found: String,
}

impl UnexpectedToken {
    pub fn code(&self) -> &'static str {
        "P001"
    }
    pub fn render(&self) -> String {
        format!("Expected {}, found {}", self.expected, self.found)
    }
}
