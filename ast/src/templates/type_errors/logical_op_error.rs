use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogicalOpError;

impl LogicalOpError {
    pub fn code(&self) -> &'static str {
        "E020"
    }
    pub fn render(&self) -> String {
        "'and'/'or' operands must be Bool".to_string()
    }
}
