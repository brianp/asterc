use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ControlFlowError {
    pub keyword: String,
}

impl ControlFlowError {
    pub fn code(&self) -> &'static str {
        "E029"
    }
    pub fn render(&self) -> String {
        format!("`{}` can only be used inside a loop", self.keyword)
    }
}
