use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MissingIterable {
    pub type_name: String,
}

impl MissingIterable {
    pub fn code(&self) -> &'static str {
        "E007"
    }
    pub fn render(&self) -> String {
        format!(
            "Class '{}' does not have required each() method",
            self.type_name
        )
    }
}
