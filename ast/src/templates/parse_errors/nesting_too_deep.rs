use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NestingTooDeep;

impl NestingTooDeep {
    pub fn code(&self) -> &'static str {
        "P003"
    }
    pub fn render(&self) -> String {
        "Maximum nesting depth exceeded".to_string()
    }
}
