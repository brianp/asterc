use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StringTooLong;

impl StringTooLong {
    pub fn code(&self) -> &'static str {
        "L005"
    }
    pub fn render(&self) -> String {
        "String exceeds maximum length".to_string()
    }
}
