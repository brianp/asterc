use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RedundantTypeAnnotation {
    pub type_name: String,
}

impl RedundantTypeAnnotation {
    pub fn code(&self) -> &'static str {
        "W004"
    }
    pub fn render(&self) -> String {
        format!(
            "redundant type annotation: type `{}` can be inferred",
            self.type_name
        )
    }
}
