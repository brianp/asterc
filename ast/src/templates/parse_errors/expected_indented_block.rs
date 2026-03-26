use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExpectedIndentedBlock;

impl ExpectedIndentedBlock {
    pub fn code(&self) -> &'static str {
        "P002"
    }
    pub fn render(&self) -> String {
        "Expected indented block after colon".to_string()
    }
}
