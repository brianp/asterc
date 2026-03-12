/// Style for string quote characters in formatted output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuoteStyle {
    /// Use double quotes: `"hello"`
    Double,
    /// Use single quotes: `'hello'`
    Single,
}

/// Configuration for the Aster formatter.
#[derive(Debug, Clone)]
pub struct FormatConfig {
    /// Maximum line width before breaking. Default: 88.
    pub line_width: usize,
    /// Number of spaces per indentation level. Default: 4.
    pub indent_size: usize,
    /// Quote style for string literals. Default: Double.
    pub quote_style: QuoteStyle,
}

impl Default for FormatConfig {
    fn default() -> Self {
        Self {
            line_width: 88,
            indent_size: 4,
            quote_style: QuoteStyle::Double,
        }
    }
}
