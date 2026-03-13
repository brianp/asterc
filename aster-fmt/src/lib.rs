pub mod config;
pub mod doc;
pub mod rules;
pub(crate) mod trivia;

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
/// the Wadler-Lindig algorithm. Comments are preserved by extracting
/// them from the source and re-inserting at the correct positions.
pub fn format_source(source: &str, config: &FormatConfig) -> Result<String, FormatError> {
    // 1. Lex
    let tokens = lexer::lex(source).map_err(|d| FormatError::LexError(d.message))?;

    // 2. Detect trailing commas for magic trailing comma support.
    //    Use a drop guard so state is always cleaned up, even on error or panic.
    rules::detect_trailing_commas(&tokens);
    struct ClearOnDrop;
    impl Drop for ClearOnDrop {
        fn drop(&mut self) {
            rules::clear_trailing_commas();
        }
    }
    let _guard = ClearOnDrop;

    // 3. Parse
    let mut parser = parser::Parser::new(tokens);
    let module = parser
        .parse_module("<fmt>")
        .map_err(|d| FormatError::ParseError(d.message))?;

    // 4. Extract comments for preservation
    let comments = trivia::extract_comments(source);

    // 5. Format with comment insertion
    let doc = rules::format_module_with_comments(&module, config, &comments, source);
    let output = doc::pretty(config.line_width, config.indent_size, &doc);

    Ok(output)
}

/// Format source and return a structured diff of changes.
/// Each entry is (line_number, original_line, formatted_line).
pub fn format_diff(source: &str, config: &FormatConfig) -> Result<Vec<DiffEntry>, FormatError> {
    let formatted = format_source(source, config)?;
    if source == formatted {
        return Ok(Vec::new());
    }

    let orig_lines: Vec<&str> = source.lines().collect();
    let fmt_lines: Vec<&str> = formatted.lines().collect();
    let mut diffs = Vec::new();

    let max_lines = orig_lines.len().max(fmt_lines.len());
    for i in 0..max_lines {
        let orig = orig_lines.get(i).copied().unwrap_or("");
        let fmt = fmt_lines.get(i).copied().unwrap_or("");
        if orig != fmt {
            diffs.push(DiffEntry {
                line: i + 1,
                original: orig.to_string(),
                formatted: fmt.to_string(),
            });
        }
    }

    Ok(diffs)
}

/// A single line difference between original and formatted source.
#[derive(Debug, Clone)]
pub struct DiffEntry {
    pub line: usize,
    pub original: String,
    pub formatted: String,
}
