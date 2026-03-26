use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvalidEscape {
    pub sequence: String,
}

impl InvalidEscape {
    pub fn code(&self) -> &'static str {
        "L004"
    }
    pub fn render(&self) -> String {
        format!("Invalid escape sequence '\\{}'", self.sequence)
    }
}
