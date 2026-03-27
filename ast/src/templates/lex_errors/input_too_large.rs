use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputTooLarge {
    pub size: usize,
    pub limit: usize,
}

impl InputTooLarge {
    pub fn code(&self) -> &'static str {
        "L009"
    }
    pub fn render(&self) -> String {
        format!(
            "input is {} bytes, exceeding the maximum of {} bytes",
            self.size, self.limit
        )
    }
}
