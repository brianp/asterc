use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrintableError;

impl PrintableError {
    pub fn code(&self) -> &'static str {
        "E023"
    }
    pub fn render(&self) -> String {
        "Expression must be Printable".to_string()
    }
}
