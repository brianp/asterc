use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnexpectedCharacter {
    pub ch: char,
}

impl UnexpectedCharacter {
    pub fn code(&self) -> &'static str {
        "L011"
    }
    pub fn render(&self) -> String {
        format!("unexpected character '{}'", self.ch)
    }
}
