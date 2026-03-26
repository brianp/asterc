use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TabIndentation;

impl TabIndentation {
    pub fn code(&self) -> &'static str {
        "L003"
    }
    pub fn render(&self) -> String {
        "Tab character found (use spaces for indentation)".to_string()
    }
}
