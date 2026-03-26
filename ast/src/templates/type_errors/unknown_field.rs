use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnknownField {
    pub field: String,
    pub type_name: String,
}

impl UnknownField {
    pub fn code(&self) -> &'static str {
        "E010"
    }
    pub fn render(&self) -> String {
        format!(
            "Unknown field '{}' on type '{}'",
            self.field, self.type_name
        )
    }
}
