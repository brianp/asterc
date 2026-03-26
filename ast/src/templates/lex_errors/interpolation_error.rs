use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InterpolationError;

impl InterpolationError {
    pub fn code(&self) -> &'static str {
        "L001"
    }
    pub fn render(&self) -> String {
        "Unexpected character in string interpolation".to_string()
    }
}
