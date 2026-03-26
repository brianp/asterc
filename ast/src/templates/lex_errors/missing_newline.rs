use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MissingNewline;

impl MissingNewline {
    pub fn code(&self) -> &'static str {
        "L008"
    }
    pub fn render(&self) -> String {
        "File must end with newline".to_string()
    }
}
