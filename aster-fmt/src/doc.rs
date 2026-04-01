/// Document IR for the Wadler-Lindig pretty printer.
///
/// A `Doc` describes a layout-independent document. The printer decides
/// how to break groups based on available width.
#[derive(Debug, Clone)]
pub enum Doc {
    /// Literal text (no newlines).
    Text(String),
    /// Soft line break. In flat mode, renders as `flat`; in break mode,
    /// renders as a newline followed by the current indentation.
    Line { flat: String },
    /// Concatenation of multiple documents.
    Concat(Vec<Doc>),
    /// Increase indentation by one level for the inner document.
    Indent(Box<Doc>),
    /// A group that the printer tries to fit on one line.
    /// If it doesn't fit, contained `Line` nodes break.
    Group(Box<Doc>),
    /// Unconditional newline — always breaks regardless of mode.
    HardLine,
    /// A blank line separator — emits two newlines with indentation only on
    /// the second line (no trailing whitespace on the blank line).
    BlankLine,
    /// Empty document.
    Nil,
}

// ---------------------------------------------------------------------------
// Smart constructors
// ---------------------------------------------------------------------------

/// Literal text (must not contain newlines).
pub fn text(s: impl Into<String>) -> Doc {
    Doc::Text(s.into())
}

/// Soft line: in flat mode renders as `" "`, in break mode as newline+indent.
pub fn line() -> Doc {
    Doc::Line {
        flat: " ".to_string(),
    }
}

/// Soft line that renders as empty string in flat mode.
pub fn softline() -> Doc {
    Doc::Line {
        flat: String::new(),
    }
}

/// Unconditional hard newline.
pub fn hardline() -> Doc {
    Doc::HardLine
}

/// A blank line: two newlines, with indentation only on the second line.
pub fn blankline() -> Doc {
    Doc::BlankLine
}

/// Group: try to print on one line; break if it doesn't fit.
pub fn group(doc: Doc) -> Doc {
    Doc::Group(Box::new(doc))
}

/// Increase indent level by one for the inner doc.
pub fn indent(doc: Doc) -> Doc {
    Doc::Indent(Box::new(doc))
}

/// Concatenate a sequence of docs.
pub fn concat(docs: Vec<Doc>) -> Doc {
    Doc::Concat(docs)
}

/// Join docs with a separator between each pair.
pub fn join(docs: Vec<Doc>, sep: Doc) -> Doc {
    let mut result = Vec::new();
    for (i, d) in docs.into_iter().enumerate() {
        if i > 0 {
            result.push(sep.clone());
        }
        result.push(d);
    }
    Doc::Concat(result)
}

// ---------------------------------------------------------------------------
// Printer
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Flat,
    Break,
}

/// A command on the printer stack: (indent_level, mode, doc).
type Cmd<'a> = (usize, Mode, &'a Doc);

/// Render a `Doc` to a `String` given the formatting parameters.
pub fn pretty(width: usize, indent_size: usize, doc: &Doc) -> String {
    let mut out = String::new();
    let mut col: usize = 0;
    // Stack of (indent, mode, doc) — processed right-to-left so we push in
    // reverse order for Concat children.
    let mut stack: Vec<Cmd> = vec![(0, Mode::Break, doc)];

    while let Some((ind, mode, d)) = stack.pop() {
        match d {
            Doc::Nil => {}
            Doc::Text(s) => {
                out.push_str(s);
                col += s.len();
            }
            Doc::Line { flat } => match mode {
                Mode::Flat => {
                    out.push_str(flat);
                    col += flat.len();
                }
                Mode::Break => {
                    out.push('\n');
                    let spaces = ind * indent_size;
                    for _ in 0..spaces {
                        out.push(' ');
                    }
                    col = spaces;
                }
            },
            Doc::HardLine => {
                out.push('\n');
                let spaces = ind * indent_size;
                for _ in 0..spaces {
                    out.push(' ');
                }
                col = spaces;
            }
            Doc::BlankLine => {
                out.push_str("\n\n");
                let spaces = ind * indent_size;
                for _ in 0..spaces {
                    out.push(' ');
                }
                col = spaces;
            }
            Doc::Concat(docs) => {
                // Push in reverse so the first child is processed first.
                for child in docs.iter().rev() {
                    stack.push((ind, mode, child));
                }
            }
            Doc::Indent(inner) => {
                stack.push((ind + 1, mode, inner));
            }
            Doc::Group(inner) => {
                if fits(width as isize - col as isize, &[(ind, Mode::Flat, inner)]) {
                    stack.push((ind, Mode::Flat, inner));
                } else {
                    stack.push((ind, Mode::Break, inner));
                }
            }
        }
    }

    out
}

/// Check whether a document fits within `remaining` columns in flat mode.
fn fits(mut remaining: isize, cmds: &[Cmd]) -> bool {
    let mut stack: Vec<Cmd> = cmds.to_vec();
    while let Some((ind, mode, d)) = stack.pop() {
        if remaining < 0 {
            return false;
        }
        match d {
            Doc::Nil => {}
            Doc::Text(s) => {
                remaining -= s.len() as isize;
            }
            Doc::Line { flat } => match mode {
                Mode::Flat => {
                    remaining -= flat.len() as isize;
                }
                Mode::Break => {
                    // A break-mode line inside fits check means newline — always fits.
                    return true;
                }
            },
            Doc::HardLine | Doc::BlankLine => {
                return true;
            }
            Doc::Concat(docs) => {
                for child in docs.iter().rev() {
                    stack.push((ind, mode, child));
                }
            }
            Doc::Indent(inner) => {
                stack.push((ind + 1, mode, inner));
            }
            Doc::Group(inner) => {
                // In a fits check, try flat mode for nested groups.
                stack.push((ind, Mode::Flat, inner));
            }
        }
    }
    remaining >= 0
}
