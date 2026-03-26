use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskAlreadyConsumed {
    pub name: String,
}

impl TaskAlreadyConsumed {
    pub fn code(&self) -> &'static str {
        "E012"
    }
    pub fn render(&self) -> String {
        format!("Task '{}' is already consumed", self.name)
    }
}
