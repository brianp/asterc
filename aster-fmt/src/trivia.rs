use ast::Span;

/// A comment extracted from the source with its position info.
#[derive(Debug, Clone)]
pub(crate) struct Comment {
    /// 1-based line number in the source.
    pub line: usize,
    /// The full comment text including leading whitespace and `#`.
    pub text: String,
}

/// Extract comments from source and return them sorted by line number.
pub(crate) fn extract_comments(source: &str) -> Vec<Comment> {
    lexer::extract_comments(source)
        .into_iter()
        .map(|(line, _offset, text)| Comment { line, text })
        .collect()
}

/// Given a list of statements with spans, find comments that belong before
/// each statement. Returns a vec of (stmt_index, comments_before) pairs.
///
/// A comment belongs to the next statement that starts after the comment's line.
pub(crate) fn assign_comments_to_stmts(
    comments: &[Comment],
    stmt_spans: &[Span],
    source: &str,
) -> Vec<Vec<String>> {
    let mut result: Vec<Vec<String>> = vec![Vec::new(); stmt_spans.len()];
    if comments.is_empty() || stmt_spans.is_empty() {
        return result;
    }

    // Precompute line numbers for each statement start.
    let line_starts = compute_line_starts(source);
    let stmt_lines: Vec<usize> = stmt_spans
        .iter()
        .map(|span| offset_to_line(&line_starts, span.start))
        .collect();

    // Precompute the "body end" line for each statement. We use the line
    // just before the next statement's first non-blank line as the boundary.
    // This avoids relying on span.end which can be overly broad for functions.
    let stmt_body_end: Vec<usize> = stmt_lines
        .iter()
        .enumerate()
        .map(|(i, _)| {
            if i + 1 < stmt_lines.len() {
                // Find the last non-blank line before the next statement.
                // Comments between statements should NOT be considered part
                // of the previous statement's body, so we use the line just
                // before the first comment or non-blank line after the body.
                let next_start = stmt_lines[i + 1];
                // Walk backwards from the next statement to find where the
                // previous statement's body actually ends (skip blank lines).
                let mut end = next_start.saturating_sub(1);
                while end > stmt_lines[i] {
                    let line_content = source_line(source, &line_starts, end);
                    if !line_content.trim().is_empty()
                        && !line_content.trim_start().starts_with('#')
                    {
                        break;
                    }
                    end = end.saturating_sub(1);
                }
                end
            } else {
                // Last statement: use the last line of the source.
                line_starts.len()
            }
        })
        .collect();

    let mut comment_idx = 0;
    for (i, &stmt_line) in stmt_lines.iter().enumerate() {
        while comment_idx < comments.len() && comments[comment_idx].line < stmt_line {
            // Only assign this comment if it doesn't fall inside a previous
            // statement's body (e.g., an inline comment inside a function).
            let inside_prev = i > 0 && comments[comment_idx].line <= stmt_body_end[i - 1];
            if !inside_prev {
                result[i].push(comments[comment_idx].text.clone());
            }
            comment_idx += 1;
        }
        // Comments on the same line as the statement also belong to it.
        while comment_idx < comments.len() && comments[comment_idx].line == stmt_line {
            result[i].push(comments[comment_idx].text.clone());
            comment_idx += 1;
        }
    }

    // Trailing comments after the last statement go to the last statement,
    // but skip comments inside the last statement's body (they'll be handled
    // by the nested comment assignment in format_block_inner).
    let last_body_end = stmt_body_end.last().copied().unwrap_or(0);
    let last_start = stmt_lines.last().copied().unwrap_or(0);
    if let Some(last) = result.last_mut() {
        while comment_idx < comments.len() {
            let cline = comments[comment_idx].line;
            // Only include if the comment is after the last statement's body,
            // not inside it.
            if cline <= last_start || cline > last_body_end {
                last.push(comments[comment_idx].text.clone());
            }
            comment_idx += 1;
        }
    }

    result
}

/// Context for threading comments through recursive formatting.
pub(crate) struct CommentCtx<'a> {
    pub comments: &'a [Comment],
    pub source: &'a str,
}

/// Assign comments to statements within a nested block body.
///
/// Unlike `assign_comments_to_stmts`, this only considers comments
/// whose line falls within the range of the given body statements.
/// Returns a vec with one entry per statement, containing comments
/// that should appear before that statement.
pub(crate) fn assign_comments_to_body(
    ctx: &CommentCtx<'_>,
    stmt_spans: &[Span],
) -> Vec<Vec<String>> {
    let mut result: Vec<Vec<String>> = vec![Vec::new(); stmt_spans.len()];
    if ctx.comments.is_empty() || stmt_spans.is_empty() {
        return result;
    }

    let line_starts = compute_line_starts(ctx.source);
    let stmt_lines: Vec<usize> = stmt_spans
        .iter()
        .map(|span| offset_to_line(&line_starts, span.start))
        .collect();

    // Find the line range of this body.
    let body_first_line = stmt_lines[0];
    let body_last_line = stmt_spans
        .last()
        .map(|s| offset_to_line(&line_starts, s.end))
        .unwrap_or(body_first_line);

    // Collect only comments within this body's line range.
    let body_comments: Vec<&Comment> = ctx
        .comments
        .iter()
        .filter(|c| c.line >= body_first_line.saturating_sub(1) && c.line <= body_last_line)
        .collect();

    if body_comments.is_empty() {
        return result;
    }

    // Compute body-end lines for each statement (same heuristic as module-level).
    let stmt_body_end: Vec<usize> = stmt_lines
        .iter()
        .enumerate()
        .map(|(i, _)| {
            if i + 1 < stmt_lines.len() {
                let next_start = stmt_lines[i + 1];
                let mut end = next_start.saturating_sub(1);
                while end > stmt_lines[i] {
                    let line_content = source_line(ctx.source, &line_starts, end);
                    if !line_content.trim().is_empty()
                        && !line_content.trim_start().starts_with('#')
                    {
                        break;
                    }
                    end = end.saturating_sub(1);
                }
                end
            } else {
                body_last_line
            }
        })
        .collect();

    let mut comment_idx = 0;
    for (i, &stmt_line) in stmt_lines.iter().enumerate() {
        while comment_idx < body_comments.len() && body_comments[comment_idx].line < stmt_line {
            // Skip comments inside a previous statement's body (they'll be
            // handled when that statement is recursively formatted).
            let inside_prev = i > 0 && body_comments[comment_idx].line <= stmt_body_end[i - 1];
            if !inside_prev {
                result[i].push(body_comments[comment_idx].text.clone());
            }
            comment_idx += 1;
        }
        // Comments on the same line as the statement belong to it.
        while comment_idx < body_comments.len() && body_comments[comment_idx].line == stmt_line {
            result[i].push(body_comments[comment_idx].text.clone());
            comment_idx += 1;
        }
    }

    // Trailing comments in the body go to the last statement,
    // but skip comments inside the last statement's body.
    let last_body_end = stmt_body_end.last().copied().unwrap_or(0);
    let last_start = stmt_lines.last().copied().unwrap_or(0);
    if let Some(last) = result.last_mut() {
        while comment_idx < body_comments.len() {
            let cline = body_comments[comment_idx].line;
            if cline <= last_start || cline > last_body_end {
                last.push(body_comments[comment_idx].text.clone());
            }
            comment_idx += 1;
        }
    }

    result
}

fn compute_line_starts(source: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(
            source
                .bytes()
                .enumerate()
                .filter_map(|(i, b)| if b == b'\n' { Some(i + 1) } else { None }),
        )
        .collect()
}

/// Get the content of a 1-based line number from the source.
fn source_line<'a>(source: &'a str, line_starts: &[usize], line: usize) -> &'a str {
    if line == 0 || line > line_starts.len() {
        return "";
    }
    let start = line_starts[line - 1];
    let end = if line < line_starts.len() {
        line_starts[line].saturating_sub(1) // exclude the newline
    } else {
        source.len()
    };
    &source[start..end]
}

fn offset_to_line(line_starts: &[usize], offset: usize) -> usize {
    match line_starts.binary_search(&offset) {
        Ok(idx) => idx + 1,
        Err(idx) => idx, // offset is in the middle of a line
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_comments_basic() {
        let src = "# hello\nlet x = 1\n# world\nlet y = 2\n";
        let comments = extract_comments(src);
        assert_eq!(comments.len(), 2);
        assert_eq!(comments[0].text, "# hello");
        assert_eq!(comments[0].line, 1);
        assert_eq!(comments[1].text, "# world");
        assert_eq!(comments[1].line, 3);
    }

    #[test]
    fn extract_comments_indented() {
        let src = "def f()\n  # comment\n  let x = 1\n";
        let comments = extract_comments(src);
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].text, "  # comment");
    }

    #[test]
    fn extract_comments_empty() {
        let src = "let x = 1\n";
        let comments = extract_comments(src);
        assert!(comments.is_empty());
    }

    #[test]
    fn assign_comments_pairs() {
        let src = "# first\nlet x = 1\n# second\nlet y = 2\n";
        let comments = extract_comments(src);
        // Simulate spans: "let x = 1" starts at byte 8, "let y = 2" starts at byte 28
        let spans = vec![Span::new(8, 17), Span::new(28, 37)];
        let assigned = assign_comments_to_stmts(&comments, &spans, src);
        assert_eq!(assigned[0], vec!["# first"]);
        assert_eq!(assigned[1], vec!["# second"]);
    }
}
