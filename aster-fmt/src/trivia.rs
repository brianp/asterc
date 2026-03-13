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

    let mut comment_idx = 0;
    for (i, &stmt_line) in stmt_lines.iter().enumerate() {
        while comment_idx < comments.len() && comments[comment_idx].line < stmt_line {
            result[i].push(comments[comment_idx].text.clone());
            comment_idx += 1;
        }
        // Comments on the same line as the statement also belong to it.
        while comment_idx < comments.len() && comments[comment_idx].line == stmt_line {
            result[i].push(comments[comment_idx].text.clone());
            comment_idx += 1;
        }
    }

    // Trailing comments after the last statement go to the last statement.
    if let Some(last) = result.last_mut() {
        while comment_idx < comments.len() {
            last.push(comments[comment_idx].text.clone());
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
