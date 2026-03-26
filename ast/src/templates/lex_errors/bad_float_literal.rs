use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BadFloatLiteral {
    pub line: usize,
}

impl BadFloatLiteral {
    pub fn code(&self) -> &'static str {
        "L006"
    }
    pub fn render(&self) -> String {
        format!("Bad float literal at line {}", self.line)
    }
}
