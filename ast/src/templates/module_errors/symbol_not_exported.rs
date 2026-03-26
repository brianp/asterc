use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SymbolNotExported {
    pub symbol: String,
    pub module: String,
}

impl SymbolNotExported {
    pub fn code(&self) -> &'static str {
        "M002"
    }
    pub fn render(&self) -> String {
        format!("'{}' is not exported by module '{}'", self.symbol, self.module)
    }
}
