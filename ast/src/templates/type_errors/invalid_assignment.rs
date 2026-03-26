use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvalidAssignment;

impl InvalidAssignment {
    pub fn code(&self) -> &'static str {
        "E008"
    }
    pub fn render(&self) -> String {
        "Invalid assignment target".to_string()
    }
}
