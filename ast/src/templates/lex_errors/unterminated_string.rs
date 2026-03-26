use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnterminatedString;

impl UnterminatedString {
    pub fn code(&self) -> &'static str {
        "L002"
    }
    pub fn render(&self) -> String {
        "Unterminated string or escape sequence".to_string()
    }
}
