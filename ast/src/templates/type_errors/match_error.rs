use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MatchError {
    pub message: String,
}

impl MatchError {
    pub fn code(&self) -> &'static str {
        "E011"
    }
    pub fn render(&self) -> String {
        self.message.clone()
    }
}
