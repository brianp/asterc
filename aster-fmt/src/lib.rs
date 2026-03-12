pub mod config;
pub mod doc;
pub mod rules;

#[cfg(test)]
mod tests;

use config::FormatConfig;

/// Errors that can occur during formatting.
#[derive(Debug)]
pub enum FormatError {
    /// The source failed to lex.
    LexError(String),
    /// The source failed to parse.
    ParseError(String),
}

impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FormatError::LexError(msg) => write!(f, "lex error: {}", msg),
            FormatError::ParseError(msg) => write!(f, "parse error: {}", msg),
        }
    }
}

impl std::error::Error for FormatError {}

/// Format Aster source code according to the given configuration.
///
/// Parses the source into an AST, then pretty-prints it back using
/// the Wadler-Lindig algorithm.
pub fn format_source(source: &str, config: &FormatConfig) -> Result<String, FormatError> {
    // 1. Lex
    let tokens = lexer::lex(source).map_err(|d| FormatError::LexError(d.message))?;

    // 2. Parse
    let mut parser = parser::Parser::new(tokens);
    let module = parser
        .parse_module("<fmt>")
        .map_err(|d| FormatError::ParseError(d.message))?;

    // 3. Format
    let doc = rules::format_module(&module, config);
    let output = doc::pretty(config.line_width, config.indent_size, &doc);

    Ok(output)
}
