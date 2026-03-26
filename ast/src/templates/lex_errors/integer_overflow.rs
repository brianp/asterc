use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IntegerOverflow;

impl IntegerOverflow {
    pub fn code(&self) -> &'static str {
        "L007"
    }
    pub fn render(&self) -> String {
        "Integer literal overflows i64 range".to_string()
    }
}
