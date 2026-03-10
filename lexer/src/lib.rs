mod token;

#[cfg(test)]
mod tests;

pub use token::{Token, TokenKind};

use ast::{Diagnostic, Span};

const MAX_INPUT_SIZE: usize = 10 * 1024 * 1024; // 10 MB
const MAX_STRING_LENGTH: usize = 1_000_000;

// ---------------------------------------------------------------------------
// Helpers called from lex()
// ---------------------------------------------------------------------------

/// Lex a double-quoted string literal. Called after the opening `"` has been
/// consumed. Returns the string contents and the updated column offset.
fn lex_string(
    chars: &mut std::iter::Peekable<std::str::Chars>,
    mut col: usize,
    line_no: usize,
    byte_offset: usize,
) -> Result<(String, usize), Diagnostic> {
    let mut s = String::new();
    loop {
        match chars.next() {
            Some('"') => {
                col += 1;
                break;
            }
            Some('\\') => {
                col += 1;
                match chars.next() {
                    Some('n') => {
                        s.push('\n');
                        col += 1;
                    }
                    Some('t') => {
                        s.push('\t');
                        col += 1;
                    }
                    Some('\\') => {
                        s.push('\\');
                        col += 1;
                    }
                    Some('"') => {
                        s.push('"');
                        col += 1;
                    }
                    Some('r') => {
                        s.push('\r');
                        col += 1;
                    }
                    Some('0') => {
                        s.push('\0');
                        col += 1;
                    }
                    Some(c) => {
                        return Err(Diagnostic::error(format!(
                            "Unknown escape sequence '\\{}' at line {}",
                            c, line_no
                        ))
                        .with_code("L004")
                        .with_label(
                            Span::new(byte_offset + col - 1, byte_offset + col + 1),
                            format!("invalid escape '\\{}'", c),
                        ));
                    }
                    None => {
                        return Err(Diagnostic::error(format!(
                            "Unterminated escape sequence at line {}",
                            line_no
                        ))
                        .with_code("L002"));
                    }
                }
            }
            Some(c) => {
                s.push(c);
                col += 1;
                if s.len() > MAX_STRING_LENGTH {
                    return Err(Diagnostic::error(format!(
                        "String literal exceeds maximum length of {} at line {}",
                        MAX_STRING_LENGTH, line_no
                    ))
                    .with_code("L005"));
                }
            }
            None => {
                return Err(
                    Diagnostic::error(format!("Unterminated string at line {}", line_no))
                        .with_code("L002")
                        .with_label(
                            Span::new(byte_offset, byte_offset + col),
                            "string starts here but is never closed",
                        ),
                );
            }
        }
    }
    Ok((s, col))
}

/// Lex an integer or float literal. Called with the first digit `first`.
/// Returns the `TokenKind` (Int or Float) and the updated column offset.
fn lex_number(
    chars: &mut std::iter::Peekable<std::str::Chars>,
    first: char,
    mut col: usize,
    line_no: usize,
    byte_offset: usize,
) -> Result<(TokenKind, usize), Diagnostic> {
    use TokenKind::*;
    let mut num = first.to_string();
    let mut is_float = false;
    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() {
            num.push(c);
            chars.next();
            col += 1;
        } else if c == '.' && !is_float {
            // Only treat dot as decimal point if followed by a digit.
            let mut lookahead = chars.clone();
            lookahead.next(); // skip the '.'
            if lookahead.peek().is_some_and(|ch| ch.is_ascii_digit()) {
                is_float = true;
                num.push('.');
                chars.next();
                col += 1;
            } else {
                break;
            }
        } else {
            break;
        }
    }
    if is_float {
        let v = num.parse::<f64>().map_err(|_| {
            Diagnostic::error(format!("bad float at line {}", line_no))
                .with_code("L006")
                .with_label(
                    Span::new(byte_offset, byte_offset + num.len()),
                    "invalid float literal",
                )
        })?;
        Ok((Float(v), col))
    } else {
        let v = num.parse::<i64>().map_err(|e| {
            let msg = e.to_string();
            if msg.contains("too large") || msg.contains("too small") || msg.contains("overflow") {
                Diagnostic::error(format!(
                    "Integer literal '{}' overflows i64 range at line {}",
                    num, line_no
                ))
                .with_code("L007")
                .with_label(
                    Span::new(byte_offset, byte_offset + num.len()),
                    "overflows i64",
                )
            } else {
                Diagnostic::error(format!(
                    "Invalid integer literal '{}' at line {}",
                    num, line_no
                ))
                .with_code("L006")
                .with_label(
                    Span::new(byte_offset, byte_offset + num.len()),
                    "invalid integer",
                )
            }
        })?;
        Ok((Int(v), col))
    }
}

/// Lex an identifier or keyword. Called with the first character `first`.
/// Returns the `TokenKind` and the updated column offset.
fn lex_ident_or_keyword(
    chars: &mut std::iter::Peekable<std::str::Chars>,
    first: char,
    mut col: usize,
) -> (TokenKind, usize) {
    use TokenKind::*;
    let mut word = first.to_string();
    while let Some(&c) = chars.peek() {
        if c.is_ascii_alphanumeric() || c == '_' {
            word.push(c);
            chars.next();
            col += 1;
        } else {
            break;
        }
    }
    let kind = match word.as_str() {
        "def" => Def,
        "class" => Class,
        "async" => Async,
        "return" => Return,
        "if" => If,
        "else" => Else,
        "true" => True,
        "false" => False,
        "nil" => Nil,
        "let" => Let,
        "elif" => Elif,
        "while" => While,
        "for" => For,
        "in" => In,
        "break" => Break,
        "continue" => Continue,
        "and" => And,
        "or" => Or,
        "not" => Not,
        "use" => Use,
        "as" => As,
        "pub" => Pub,
        "trait" => Trait,
        "enum" => Enum,
        "includes" => Includes,
        "extends" => Extends,
        "throw" => Throw,
        "throws" => Throws,
        "match" => Match,
        "catch" => Catch,
        "resolve" => Resolve,
        "detached" => Detached,
        "scope" => Scope,
        _ => Ident(word),
    };
    (kind, col)
}

// ---------------------------------------------------------------------------
// Compute byte offset of each line start
// ---------------------------------------------------------------------------

fn compute_line_starts(input: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(
            input
                .bytes()
                .enumerate()
                .filter_map(|(i, b)| if b == b'\n' { Some(i + 1) } else { None }),
        )
        .collect()
}

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

pub fn lex(input: &str) -> Result<Vec<Token>, Diagnostic> {
    use TokenKind::*;

    if input.len() > MAX_INPUT_SIZE {
        return Err(Diagnostic::error(format!(
            "Input too large: {} bytes exceeds maximum of {} bytes",
            input.len(),
            MAX_INPUT_SIZE
        ))
        .with_code("L008"));
    }

    // Reject non-ASCII source for now — byte offsets assume 1 byte per char.
    // String literals may contain non-ASCII, but identifiers/keywords cannot.
    if let Some(pos) = input.bytes().position(|b| b > 127) {
        let line = input[..pos].matches('\n').count() + 1;
        return Err(Diagnostic::error(format!(
            "Non-ASCII character at byte offset {} (line {}). Aster currently requires ASCII source files",
            pos, line
        ))
        .with_code("L009")
        .with_label(Span::new(pos, pos + 1), "non-ASCII byte"));
    }

    let line_starts = compute_line_starts(input);
    let mut tokens: Vec<Token> = Vec::new();
    let mut indent_stack: Vec<usize> = vec![0];

    let mut last_line_idx = 0usize;
    for (line_idx, raw) in input.lines().enumerate() {
        last_line_idx = line_idx;
        let line_no = line_idx + 1;
        let ls = line_starts[line_idx]; // byte offset of line start

        // Reject tab indentation.
        if raw.starts_with('\t')
            || (raw.starts_with(' ') && raw.contains('\t') && {
                let leading: String = raw.chars().take_while(|c| c.is_whitespace()).collect();
                leading.contains('\t')
            })
        {
            return Err(Diagnostic::error(format!(
                "Tab indentation is not allowed at line {}",
                line_no
            ))
            .with_code("L003")
            .with_label(Span::new(ls, ls + 1), "tab character found here"));
        }

        let indent_width = raw.chars().take_while(|c| *c == ' ').count();
        let trimmed = raw.trim_end();
        let rest = trimmed.trim_start();

        if rest.is_empty() || rest.starts_with('#') {
            tokens.push(Token {
                kind: Newline,
                line: line_no,
                col: 0,
                start: ls,
                end: ls + raw.len(),
            });
            continue;
        }

        // Emit Indent / Dedent tokens.
        let prev = indent_stack.last().copied().unwrap_or(0);
        if indent_width > prev {
            indent_stack.push(indent_width);
            tokens.push(Token {
                kind: Indent,
                line: line_no,
                col: 1,
                start: ls,
                end: ls + indent_width,
            });
        } else if indent_width < prev {
            while let Some(&top) = indent_stack.last() {
                if indent_width < top {
                    indent_stack.pop();
                    tokens.push(Token {
                        kind: Dedent,
                        line: line_no,
                        col: 1,
                        start: ls,
                        end: ls,
                    });
                } else {
                    break;
                }
            }
            if indent_stack.last().copied().unwrap_or(0) != indent_width {
                return Err(
                    Diagnostic::error(format!("Indentation error at line {}", line_no))
                        .with_code("L003")
                        .with_label(Span::new(ls, ls + indent_width), "unexpected indent level"),
                );
            }
        }

        // Tokenize the line content.
        let mut col = indent_width;
        let mut chars = rest.chars().peekable();

        while let Some(ch) = chars.next() {
            col += 1;
            let tok_start = ls + col - 1; // byte offset of this character

            // D1: push a single-character token.
            macro_rules! push {
                ($kind:expr) => {
                    tokens.push(Token {
                        kind: $kind,
                        line: line_no,
                        col,
                        start: tok_start,
                        end: tok_start + 1,
                    })
                };
            }

            // D2: single-char with optional two-char compound lookahead.
            macro_rules! try_two_char {
                ($second:expr, $compound:expr, $single:expr) => {
                    if chars.peek() == Some(&$second) {
                        chars.next();
                        col += 1;
                        tokens.push(Token {
                            kind: $compound,
                            line: line_no,
                            col,
                            start: tok_start,
                            end: tok_start + 2,
                        });
                    } else {
                        push!($single);
                    }
                };
            }

            match ch {
                ' ' | '\t' => {}

                '(' => push!(LParen),
                ')' => push!(RParen),
                ',' => push!(Comma),
                ':' => push!(Colon),
                '.' => push!(Dot),
                '[' => push!(LBracket),
                ']' => push!(RBracket),
                '{' => push!(LBrace),
                '}' => push!(RBrace),
                '+' => push!(Plus),
                '/' => push!(Slash),
                '%' => push!(Percent),
                '?' => push!(Question),

                '-' => try_two_char!('>', Arrow, Minus),
                '*' => try_two_char!('*', StarStar, Star),
                '!' => try_two_char!('=', BangEqual, Bang),
                '<' => try_two_char!('=', LessEqual, Less),
                '>' => try_two_char!('=', GreaterEqual, Greater),

                // `=` can become `==` or `=>` or stay as `=`.
                '=' => {
                    if chars.peek() == Some(&'=') {
                        chars.next();
                        col += 1;
                        tokens.push(Token {
                            kind: EqualEqual,
                            line: line_no,
                            col,
                            start: tok_start,
                            end: tok_start + 2,
                        });
                    } else if chars.peek() == Some(&'>') {
                        chars.next();
                        col += 1;
                        tokens.push(Token {
                            kind: FatArrow,
                            line: line_no,
                            col,
                            start: tok_start,
                            end: tok_start + 2,
                        });
                    } else {
                        push!(Equals);
                    }
                }

                '"' => {
                    let (s, new_col) = lex_string(&mut chars, col, line_no, tok_start)?;
                    col = new_col;
                    tokens.push(Token {
                        kind: Str(s),
                        line: line_no,
                        col,
                        start: tok_start,
                        end: ls + col,
                    });
                }

                '0'..='9' => {
                    let (kind, new_col) = lex_number(&mut chars, ch, col, line_no, tok_start)?;
                    col = new_col;
                    tokens.push(Token {
                        kind,
                        line: line_no,
                        col,
                        start: tok_start,
                        end: ls + col,
                    });
                }

                _ if ch.is_ascii_alphabetic() || ch == '_' => {
                    let (kind, new_col) = lex_ident_or_keyword(&mut chars, ch, col);
                    col = new_col;
                    tokens.push(Token {
                        kind,
                        line: line_no,
                        col,
                        start: tok_start,
                        end: ls + col,
                    });
                }

                _ => {
                    return Err(Diagnostic::error(format!(
                        "Unexpected character '{}' at line {}",
                        ch, line_no
                    ))
                    .with_code("L001")
                    .with_label(
                        Span::new(tok_start, tok_start + ch.len_utf8()),
                        "unexpected character",
                    ));
                }
            }
        }

        tokens.push(Token {
            kind: Newline,
            line: line_no,
            col,
            start: ls + trimmed.len(),
            end: ls + raw.len(),
        });
    }

    while indent_stack.len() > 1 {
        indent_stack.pop();
        tokens.push(Token {
            kind: TokenKind::Dedent,
            line: last_line_idx + 1,
            col: 0,
            start: input.len(),
            end: input.len(),
        });
    }
    tokens.push(Token {
        kind: TokenKind::EOF,
        line: last_line_idx + 1 + 1,
        col: 0,
        start: input.len(),
        end: input.len(),
    });
    Ok(tokens)
}
