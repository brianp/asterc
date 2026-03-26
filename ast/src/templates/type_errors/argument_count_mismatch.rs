use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArgumentCountMismatch {
    pub expected: usize,
    pub actual: usize,
}

impl ArgumentCountMismatch {
    pub fn code(&self) -> &'static str {
        "E006"
    }
    pub fn render(&self) -> String {
        format!(
            "Function parameter count mismatch: expected {}, got {}",
            self.expected, self.actual
        )
    }
}
