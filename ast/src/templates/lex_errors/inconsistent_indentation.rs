use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InconsistentIndentation;

impl InconsistentIndentation {
    pub fn code(&self) -> &'static str {
        "L010"
    }
    pub fn render(&self) -> String {
        "indentation does not match any previous indentation level".to_string()
    }
}
