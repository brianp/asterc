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

/// Result of lexing a string — either a plain string or an interpolated string
/// with embedded expression tokens.
enum StringResult {
    Plain(String, usize),
    Interpolated(Vec<Token>, usize),
}

/// Lex a double-quoted string literal. Called after the opening `"` has been
/// consumed. Returns the string contents and the updated column offset.
/// If the string contains `{expr}` interpolations, returns an Interpolated result
/// with StringStart/StringMid/StringEnd tokens and embedded expression tokens.
fn lex_string_full(
    chars: &mut std::iter::Peekable<std::str::Chars>,
    mut col: usize,
    line_no: usize,
    byte_offset: usize,
    ls: usize,
) -> Result<StringResult, Diagnostic> {
    let mut s = String::new();
    let mut has_interpolation = false;
    let mut segments: Vec<(String, Vec<Token>)> = Vec::new(); // (literal_before, expr_tokens)

    loop {
        match chars.peek() {
            Some(&'"') => {
                chars.next();
                col += 1;
                break;
            }
            Some(&'\\') => {
                chars.next();
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
                    Some('{') => {
                        s.push('{');
                        col += 1;
                    }
                    Some('}') => {
                        s.push('}');
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
            Some(&'{') => {
                chars.next();
                col += 1;
                has_interpolation = true;
                // Collect the expression text until matching '}'
                let literal_part = std::mem::take(&mut s);
                let mut expr_text = String::new();
                let mut brace_depth = 1;
                let expr_start_col = col;
                while let Some(&ch) = chars.peek() {
                    if ch == '{' {
                        brace_depth += 1;
                        expr_text.push(ch);
                        chars.next();
                        col += 1;
                    } else if ch == '}' {
                        brace_depth -= 1;
                        if brace_depth == 0 {
                            chars.next();
                            col += 1;
                            break;
                        }
                        expr_text.push(ch);
                        chars.next();
                        col += 1;
                    } else if ch == '"' {
                        // Don't allow unescaped quotes inside interpolation
                        return Err(Diagnostic::error(format!(
                            "Unexpected '\"' inside string interpolation at line {}",
                            line_no
                        ))
                        .with_code("L002")
                        .with_label(
                            Span::new(ls + col, ls + col + 1),
                            "unexpected quote in interpolation",
                        ));
                    } else {
                        expr_text.push(ch);
                        chars.next();
                        col += 1;
                    }
                }
                if brace_depth != 0 {
                    return Err(Diagnostic::error(format!(
                        "Unterminated string interpolation at line {}",
                        line_no
                    ))
                    .with_code("L002"));
                }
                // Lex the expression text into tokens
                // We need to produce tokens with correct positions
                let mut expr_chars = expr_text.chars().peekable();
                let mut expr_col = expr_start_col;
                let mut expr_tokens = Vec::new();
                while let Some(&ech) = expr_chars.peek() {
                    if ech == ' ' || ech == '\t' {
                        expr_chars.next();
                        expr_col += 1;
                        continue;
                    }
                    let tok_start = ls + expr_col;
                    expr_col += 1;
                    expr_chars.next();
                    match ech {
                        '(' => expr_tokens.push(Token {
                            kind: TokenKind::LParen,
                            line: line_no,
                            col: expr_col,
                            start: tok_start,
                            end: tok_start + 1,
                        }),
                        ')' => expr_tokens.push(Token {
                            kind: TokenKind::RParen,
                            line: line_no,
                            col: expr_col,
                            start: tok_start,
                            end: tok_start + 1,
                        }),
                        '+' => expr_tokens.push(Token {
                            kind: TokenKind::Plus,
                            line: line_no,
                            col: expr_col,
                            start: tok_start,
                            end: tok_start + 1,
                        }),
                        '-' => {
                            if expr_chars.peek() == Some(&'>') {
                                expr_chars.next();
                                expr_col += 1;
                                expr_tokens.push(Token {
                                    kind: TokenKind::Arrow,
                                    line: line_no,
                                    col: expr_col,
                                    start: tok_start,
                                    end: tok_start + 2,
                                });
                            } else {
                                expr_tokens.push(Token {
                                    kind: TokenKind::Minus,
                                    line: line_no,
                                    col: expr_col,
                                    start: tok_start,
                                    end: tok_start + 1,
                                });
                            }
                        }
                        '*' => {
                            if expr_chars.peek() == Some(&'*') {
                                expr_chars.next();
                                expr_col += 1;
                                expr_tokens.push(Token {
                                    kind: TokenKind::StarStar,
                                    line: line_no,
                                    col: expr_col,
                                    start: tok_start,
                                    end: tok_start + 2,
                                });
                            } else {
                                expr_tokens.push(Token {
                                    kind: TokenKind::Star,
                                    line: line_no,
                                    col: expr_col,
                                    start: tok_start,
                                    end: tok_start + 1,
                                });
                            }
                        }
                        '/' => expr_tokens.push(Token {
                            kind: TokenKind::Slash,
                            line: line_no,
                            col: expr_col,
                            start: tok_start,
                            end: tok_start + 1,
                        }),
                        '%' => expr_tokens.push(Token {
                            kind: TokenKind::Percent,
                            line: line_no,
                            col: expr_col,
                            start: tok_start,
                            end: tok_start + 1,
                        }),
                        '.' => expr_tokens.push(Token {
                            kind: TokenKind::Dot,
                            line: line_no,
                            col: expr_col,
                            start: tok_start,
                            end: tok_start + 1,
                        }),
                        ',' => expr_tokens.push(Token {
                            kind: TokenKind::Comma,
                            line: line_no,
                            col: expr_col,
                            start: tok_start,
                            end: tok_start + 1,
                        }),
                        ':' => expr_tokens.push(Token {
                            kind: TokenKind::Colon,
                            line: line_no,
                            col: expr_col,
                            start: tok_start,
                            end: tok_start + 1,
                        }),
                        '[' => expr_tokens.push(Token {
                            kind: TokenKind::LBracket,
                            line: line_no,
                            col: expr_col,
                            start: tok_start,
                            end: tok_start + 1,
                        }),
                        ']' => expr_tokens.push(Token {
                            kind: TokenKind::RBracket,
                            line: line_no,
                            col: expr_col,
                            start: tok_start,
                            end: tok_start + 1,
                        }),
                        '=' => {
                            if expr_chars.peek() == Some(&'=') {
                                expr_chars.next();
                                expr_col += 1;
                                expr_tokens.push(Token {
                                    kind: TokenKind::EqualEqual,
                                    line: line_no,
                                    col: expr_col,
                                    start: tok_start,
                                    end: tok_start + 2,
                                });
                            } else {
                                expr_tokens.push(Token {
                                    kind: TokenKind::Equals,
                                    line: line_no,
                                    col: expr_col,
                                    start: tok_start,
                                    end: tok_start + 1,
                                });
                            }
                        }
                        '!' => {
                            if expr_chars.peek() == Some(&'=') {
                                expr_chars.next();
                                expr_col += 1;
                                expr_tokens.push(Token {
                                    kind: TokenKind::BangEqual,
                                    line: line_no,
                                    col: expr_col,
                                    start: tok_start,
                                    end: tok_start + 2,
                                });
                            } else {
                                expr_tokens.push(Token {
                                    kind: TokenKind::Bang,
                                    line: line_no,
                                    col: expr_col,
                                    start: tok_start,
                                    end: tok_start + 1,
                                });
                            }
                        }
                        '<' => {
                            if expr_chars.peek() == Some(&'=') {
                                expr_chars.next();
                                expr_col += 1;
                                expr_tokens.push(Token {
                                    kind: TokenKind::LessEqual,
                                    line: line_no,
                                    col: expr_col,
                                    start: tok_start,
                                    end: tok_start + 2,
                                });
                            } else {
                                expr_tokens.push(Token {
                                    kind: TokenKind::Less,
                                    line: line_no,
                                    col: expr_col,
                                    start: tok_start,
                                    end: tok_start + 1,
                                });
                            }
                        }
                        '>' => {
                            if expr_chars.peek() == Some(&'=') {
                                expr_chars.next();
                                expr_col += 1;
                                expr_tokens.push(Token {
                                    kind: TokenKind::GreaterEqual,
                                    line: line_no,
                                    col: expr_col,
                                    start: tok_start,
                                    end: tok_start + 2,
                                });
                            } else {
                                expr_tokens.push(Token {
                                    kind: TokenKind::Greater,
                                    line: line_no,
                                    col: expr_col,
                                    start: tok_start,
                                    end: tok_start + 1,
                                });
                            }
                        }
                        '?' => expr_tokens.push(Token {
                            kind: TokenKind::Question,
                            line: line_no,
                            col: expr_col,
                            start: tok_start,
                            end: tok_start + 1,
                        }),
                        '0'..='9' => {
                            let (kind, new_col) =
                                lex_number(&mut expr_chars, ech, expr_col, line_no, tok_start)?;
                            expr_col = new_col;
                            expr_tokens.push(Token {
                                kind,
                                line: line_no,
                                col: expr_col,
                                start: tok_start,
                                end: ls + expr_col,
                            });
                        }
                        _ if ech.is_ascii_alphabetic() || ech == '_' => {
                            let (kind, new_col) =
                                lex_ident_or_keyword(&mut expr_chars, ech, expr_col);
                            expr_col = new_col;
                            expr_tokens.push(Token {
                                kind,
                                line: line_no,
                                col: expr_col,
                                start: tok_start,
                                end: ls + expr_col,
                            });
                        }
                        _ => {
                            return Err(Diagnostic::error(format!(
                                "Unexpected character '{}' in string interpolation at line {}",
                                ech, line_no
                            ))
                            .with_code("L001")
                            .with_label(
                                Span::new(tok_start, tok_start + 1),
                                "unexpected character in interpolation",
                            ));
                        }
                    }
                }
                segments.push((literal_part, expr_tokens));
            }
            Some(&c) => {
                chars.next();
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

    if !has_interpolation {
        return Ok(StringResult::Plain(s, col));
    }

    // Build interpolation tokens
    let mut tokens = Vec::new();
    let total_segments = segments.len();
    for (i, (literal, expr_tokens)) in segments.into_iter().enumerate() {
        if i == 0 {
            tokens.push(Token {
                kind: TokenKind::StringStart(literal),
                line: line_no,
                col,
                start: byte_offset,
                end: ls + col,
            });
        } else {
            tokens.push(Token {
                kind: TokenKind::StringMid(literal),
                line: line_no,
                col,
                start: byte_offset,
                end: ls + col,
            });
        }
        tokens.extend(expr_tokens);
    }
    // Final trailing literal after last interpolation
    if total_segments > 0 {
        tokens.push(Token {
            kind: TokenKind::StringEnd(s),
            line: line_no,
            col,
            start: byte_offset,
            end: ls + col,
        });
    }

    Ok(StringResult::Interpolated(tokens, col))
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
        "blocking" => Blocking,
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
        "const" => Const,
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
    // Bracket depth tracking: suppress INDENT/DEDENT inside brackets.
    // This follows Python's approach — implicit line continuation inside
    // (), [], and {} means indentation changes don't create new blocks.
    let mut bracket_depth: usize = 0;

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

        // Emit Indent / Dedent tokens, but only when NOT inside brackets.
        // Inside (), [], or {}, indentation changes are ignored (implicit
        // line continuation), matching Python's behavior.
        if bracket_depth == 0 {
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
                    return Err(Diagnostic::error(format!(
                        "Indentation error at line {}",
                        line_no
                    ))
                    .with_code("L003")
                    .with_label(Span::new(ls, ls + indent_width), "unexpected indent level"));
                }
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

                '(' => {
                    bracket_depth += 1;
                    push!(LParen);
                }
                ')' => {
                    bracket_depth = bracket_depth.saturating_sub(1);
                    push!(RParen);
                }
                ',' => push!(Comma),
                ':' => push!(Colon),
                '.' => {
                    if chars.peek() == Some(&'.') {
                        chars.next();
                        col += 1;
                        if chars.peek() == Some(&'=') {
                            chars.next();
                            col += 1;
                            tokens.push(Token {
                                kind: DotDotEq,
                                line: line_no,
                                col,
                                start: tok_start,
                                end: tok_start + 3,
                            });
                        } else {
                            tokens.push(Token {
                                kind: DotDot,
                                line: line_no,
                                col,
                                start: tok_start,
                                end: tok_start + 2,
                            });
                        }
                    } else {
                        push!(Dot);
                    }
                }
                '[' => {
                    bracket_depth += 1;
                    push!(LBracket);
                }
                ']' => {
                    bracket_depth = bracket_depth.saturating_sub(1);
                    push!(RBracket);
                }
                '{' => {
                    bracket_depth += 1;
                    push!(LBrace);
                }
                '}' => {
                    bracket_depth = bracket_depth.saturating_sub(1);
                    push!(RBrace);
                }
                '+' => push!(Plus),
                '/' => push!(Slash),
                '#' => break, // trailing comment — skip rest of line
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

                '"' => match lex_string_full(&mut chars, col, line_no, tok_start, ls)? {
                    StringResult::Plain(s, new_col) => {
                        col = new_col;
                        tokens.push(Token {
                            kind: Str(s),
                            line: line_no,
                            col,
                            start: tok_start,
                            end: ls + col,
                        });
                    }
                    StringResult::Interpolated(interp_tokens, new_col) => {
                        col = new_col;
                        tokens.extend(interp_tokens);
                    }
                },

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

// ---------------------------------------------------------------------------
// Comment extraction (for the formatter)
// ---------------------------------------------------------------------------

/// Extract comments from source code with their line numbers and byte offsets.
///
/// Returns `(line_number_1based, byte_offset, comment_text_including_hash)` for
/// each comment found. Supports both full-line comments (`# ...`) and trailing
/// comments (`code  # ...`).
///
/// This is the formatter's primary interface for comment preservation. It runs
/// in O(n) over the source, independent of the compiler's `lex()` pipeline.
pub fn extract_comments(input: &str) -> Vec<(usize, usize, String)> {
    let line_starts = compute_line_starts(input);
    let mut comments = Vec::new();

    for (line_idx, raw) in input.lines().enumerate() {
        let line_no = line_idx + 1;
        let ls = line_starts[line_idx];
        let trimmed = raw.trim();
        if trimmed.starts_with('#') {
            // Full-line comment
            let indent: String = raw.chars().take_while(|c| *c == ' ').collect();
            let comment_text = format!("{}{}", indent, trimmed);
            comments.push((line_no, ls, comment_text));
        } else if let Some(hash_pos) = find_comment_start(raw) {
            // Trailing comment — extract from '#' onward
            let comment_text = raw[hash_pos..].trim_end().to_string();
            comments.push((line_no, ls + hash_pos, comment_text));
        }
    }

    comments
}

/// Find the byte position of a trailing `#` comment in a line, skipping `#`
/// inside string literals.
fn find_comment_start(line: &str) -> Option<usize> {
    let mut in_string = false;
    let mut escape = false;
    for (i, ch) in line.char_indices() {
        if escape {
            escape = false;
            continue;
        }
        if ch == '\\' && in_string {
            escape = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            continue;
        }
        if ch == '#' && !in_string {
            // Only count as trailing if there's code before it
            let before = line[..i].trim();
            if !before.is_empty() {
                return Some(i);
            }
            return None; // full-line comment, handled elsewhere
        }
    }
    None
}
