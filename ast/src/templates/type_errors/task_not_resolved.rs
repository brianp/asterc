use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskNotResolved {
    pub name: String,
}

impl TaskNotResolved {
    pub fn code(&self) -> &'static str {
        "E027"
    }
    pub fn render(&self) -> String {
        format!("Task '{}' created but never resolved", self.name)
    }
}
