use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VisibilityError {
    pub member: String,
    pub class_name: String,
}

impl VisibilityError {
    pub fn code(&self) -> &'static str {
        "E031"
    }
    pub fn render(&self) -> String {
        format!(
            "'{}' is private in '{}' and cannot be accessed from outside the module",
            self.member, self.class_name
        )
    }
}
