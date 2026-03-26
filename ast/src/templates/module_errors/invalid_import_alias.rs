use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvalidImportAlias;

impl InvalidImportAlias {
    pub fn code(&self) -> &'static str {
        "M004"
    }
    pub fn render(&self) -> String {
        "Cannot use both selective import and 'as' alias".to_string()
    }
}
