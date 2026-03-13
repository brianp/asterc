---
status: executed
executed: 2026-03-13
---

# Plan: Formatter

## Context

A formatter canonicalizes Aster source code to a single consistent style. For an indent-based language, this is especially important -- inconsistent indentation is a semantic error, not just a style issue.

**Depends on:** `diagnostics.md` (spans). Partially independent of other plans.

## Community Research

**The opinionated vs. configurable debate:**

- **Opinionated (Black, gofmt, deno fmt):** One true style, zero config. Eliminates bike-shedding. Black's motto: "Any color you like, as long as it's black." Community consensus: most developers *prefer* this. It ends formatting debates permanently.
- **Configurable (rustfmt, clang-format):** Multiple options for indent size, brace placement, etc. Gives teams autonomy. But creates fragmentation -- every project looks different.
- **Community verdict:** The trend strongly favors opinionated formatters. Prettier, Black, and gofmt are all massively popular *because* they're opinionated. Developers initially resist losing control, then love it once they stop thinking about formatting.

**Recommendation for Aster:** **Opinionated, minimal configuration.** Allow only:
- Line width (default: 88, matching Black)
- Indent size (default: 4 spaces -- Aster is indent-based, must be consistent)
- Quote style (single vs double, default: double)

Everything else is decided by the formatter. One style for all Aster code.

**Formatter algorithm -- Wadler-Lindig pretty printer:**
- The standard algorithm for language formatters since 1997
- Used by Prettier, Ruff, Elixir's formatter, and many others
- Core idea: describe the document as a tree of `Doc` nodes (text, line breaks, groups, indentation). The printer greedily fits as much as possible on each line.
- Ruff (Python formatter) uses a two-layer architecture: language-agnostic `Doc` IR + language-specific formatting rules. This is the state of the art.
- Runs in O(n) time, O(width * depth) space

**Indent-based language considerations (Python/Black/Ruff):**
- Indentation IS syntax -- the formatter must never change the semantic meaning
- Ruff preserves trivia (comments, whitespace) by attaching them to tokens
- Black normalizes everything: trailing commas trigger vertical formatting, consistent string quoting, magic trailing comma
- The formatter must understand Aster's indent/dedent structure natively

**What developers hate in formatters:**
- Formatters that break code (change semantics)
- Formatters that don't preserve comments
- Formatters that produce ugly, unreadable output for edge cases
- Inconsistent handling of trailing commas and line breaks
- Slow formatters (>100ms for a file is too slow)

## Design

### Architecture

```
Source Code
    |
    v
Lexer (with trivia)  -->  Token Stream (tokens + comments + whitespace)
    |
    v
Parser  -->  CST or Annotated AST (preserves all tokens)
    |
    v
Formatting Rules  -->  Doc IR (Wadler-Lindig document)
    |
    v
Printer  -->  Formatted Source Code
```

### The Trivia Problem

The current lexer drops comments and whitespace. The current AST is semantic-only -- it doesn't preserve the concrete syntax. A formatter needs both.

**Two approaches:**

1. **CST (Concrete Syntax Tree):** Build a lossless tree that preserves every token, comment, and whitespace. The formatter works on this tree. This is what rust-analyzer and Roslyn do.

2. **Token stream with trivia attachment:** Keep the current AST, but lex comments/whitespace as "trivia" tokens attached to their adjacent real tokens. The formatter uses the AST for structure and trivia tokens for comments. This is what Ruff and Prettier do.

**Decision: Option 2 (trivia attachment).** Building a full CST is a major architectural overhaul. Trivia attachment works with the existing AST and is how the most successful modern formatters (Ruff, Prettier, Biome) work.

### Phase 1: Trivia-Aware Lexer

**1A. Add trivia to Token**

```rust
#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
    pub leading_trivia: Vec<Trivia>,   // comments/whitespace before this token
    pub trailing_trivia: Vec<Trivia>,  // comments/whitespace after this token (same line)
}

#[derive(Debug, Clone)]
pub enum Trivia {
    Whitespace(String),
    Comment(String),    // # single-line comment
    Newline,
}
```

**1B. Lexer changes**

Add a `lex_with_trivia` function (keep existing `lex` for the compiler pipeline). This function:
- Emits `Trivia::Comment` for `#` to end-of-line
- Emits `Trivia::Whitespace` for runs of spaces/tabs
- Attaches leading trivia to the *next* token
- Attaches trailing trivia (on same line) to the *previous* token

### Phase 2: Doc IR (Wadler-Lindig)

**2A. Define the Doc type**

```rust
pub enum Doc {
    /// Literal text
    Text(String),

    /// A line break. If the enclosing Group fits on one line, replaced by `flat`.
    /// Otherwise, replaced by a newline + current indentation.
    Line { flat: String },  // flat is usually " " or ""

    /// Concatenation
    Concat(Vec<Doc>),

    /// Increase indent level for contents
    Indent(Box<Doc>),

    /// Group: try to fit on one line; if not, break inner Line nodes
    Group(Box<Doc>),

    /// Hard line break (always breaks, even in a flat group)
    HardLine,

    /// Empty
    Nil,
}
```

Helper constructors:

```rust
fn text(s: &str) -> Doc { Doc::Text(s.to_string()) }
fn line() -> Doc { Doc::Line { flat: " ".to_string() } }
fn softline() -> Doc { Doc::Line { flat: "".to_string() } }
fn hardline() -> Doc { Doc::HardLine }
fn group(doc: Doc) -> Doc { Doc::Group(Box::new(doc)) }
fn indent(doc: Doc) -> Doc { Doc::Indent(Box::new(doc)) }
fn concat(docs: Vec<Doc>) -> Doc { Doc::Concat(docs) }
```

**2B. The Printer**

```rust
pub fn print_doc(doc: &Doc, width: usize) -> String {
    let mut output = String::new();
    let mut stack: Vec<(usize, Mode, &Doc)> = vec![(0, Mode::Break, doc)];
    let mut pos = 0;  // current column position

    while let Some((indent, mode, doc)) = stack.pop() {
        match doc {
            Doc::Text(s) => { output.push_str(s); pos += s.len(); }
            Doc::Line { flat } => match mode {
                Mode::Flat => { output.push_str(flat); pos += flat.len(); }
                Mode::Break => {
                    output.push('\n');
                    output.push_str(&" ".repeat(indent));
                    pos = indent;
                }
            },
            Doc::HardLine => {
                output.push('\n');
                output.push_str(&" ".repeat(indent));
                pos = indent;
            }
            Doc::Concat(docs) => {
                for d in docs.iter().rev() {
                    stack.push((indent, mode, d));
                }
            }
            Doc::Indent(inner) => {
                stack.push((indent + INDENT_SIZE, mode, inner));
            }
            Doc::Group(inner) => {
                if fits(inner, width - pos) {
                    stack.push((indent, Mode::Flat, inner));
                } else {
                    stack.push((indent, Mode::Break, inner));
                }
            }
            Doc::Nil => {}
        }
    }
    output
}
```

### Phase 3: Formatting Rules

One function per AST node type, converting to `Doc`:

**3A. Statements**

```rust
fn format_stmt(stmt: &Stmt) -> Doc {
    match stmt {
        Stmt::Let { name, type_ann, value, is_public } => {
            let pub_prefix = if *is_public { "pub " } else { "" };
            let ann = type_ann.as_ref().map(|t| format!(": {}", t)).unwrap_or_default();
            group(concat(vec![
                text(&format!("{}let {}{} = ", pub_prefix, name, ann)),
                format_expr(value),
            ]))
        }
        Stmt::If { cond, then_body, elif_branches, else_body } => {
            let mut parts = vec![
                text("if "),
                format_expr(cond),
                text(":"),
                hardline(),
                indent(format_body(then_body)),
            ];
            for (cond, body) in elif_branches {
                parts.extend(vec![
                    hardline(),
                    text("elif "),
                    format_expr(cond),
                    text(":"),
                    hardline(),
                    indent(format_body(body)),
                ]);
            }
            if !else_body.is_empty() {
                parts.extend(vec![
                    hardline(),
                    text("else:"),
                    hardline(),
                    indent(format_body(else_body)),
                ]);
            }
            concat(parts)
        }
        // ... other statement types
    }
}
```

**3B. Expressions**

```rust
fn format_expr(expr: &Expr) -> Doc {
    match expr {
        Expr::BinaryOp { left, op, right } => {
            group(concat(vec![
                format_expr(left),
                line(),  // break here if line is too long
                text(&format!("{} ", op)),
                format_expr(right),
            ]))
        }
        Expr::Call { func, args } => {
            let formatted_args = args.iter()
                .map(format_expr)
                .collect::<Vec<_>>();
            group(concat(vec![
                format_expr(func),
                text("("),
                indent(concat(intersperse(formatted_args, concat(vec![text(","), line()])))),
                text(")"),
            ]))
        }
        Expr::Lambda { params, ret_type, body, .. } => {
            // def (params) -> ret_type:
            //     body
            concat(vec![
                format_params(params),
                text(&format!(" -> {}:", ret_type)),
                hardline(),
                indent(format_body(body)),
            ])
        }
        // ... other expression types
    }
}
```

**3C. Comment handling**

Comments are attached as trivia to tokens. The formatter must:
1. Preserve all comments in the output
2. Place leading comments on the line before their associated node
3. Place trailing comments at the end of their line
4. Reflow comment indentation to match the new formatting

### Phase 4: CLI Integration

```
asterc fmt                    # format all .aster files in current directory
asterc fmt file.aster         # format a single file
asterc fmt --check            # check if files are formatted (exit 1 if not)
asterc fmt --diff             # show what would change
asterc fmt --stdin            # read from stdin, write to stdout
```

**Configuration (`aster.toml`):**

```toml
[format]
line_width = 88
indent_size = 4
quote_style = "double"   # "single" or "double"
```

### Phase 5: Magic Trailing Comma

Adopt Black/Ruff's "magic trailing comma" convention:

```aster
# No trailing comma -> formatter may collapse to one line
let items = [1, 2, 3]

# Trailing comma -> formatter keeps vertical layout
let items = [
    1,
    2,
    3,
]
```

This gives developers explicit control over line breaking for collections and argument lists.

## New Crate: `aster-fmt/`

```
aster-fmt/
  Cargo.toml
  src/
    lib.rs          -- public API: format_source(source: &str, config: &Config) -> String
    doc.rs          -- Doc IR types and printer
    rules.rs        -- AST-to-Doc formatting rules
    trivia.rs       -- comment/whitespace handling
    config.rs       -- Config struct (line_width, indent_size, quote_style)
```

## Files Created/Modified

| File | Changes |
|------|---------|
| `aster-fmt/` | NEW -- entire formatter crate |
| `lexer/src/lib.rs` | Add `lex_with_trivia` function |
| `lexer/src/token.rs` | Add `Trivia` enum, trivia fields on Token |
| `src/main.rs` | Add `fmt` subcommand |
| `Cargo.toml` | Add aster-fmt to workspace |

## Verification

- `asterc fmt` produces valid Aster code (round-trip: format then parse succeeds)
- Comments are preserved in output
- Idempotent: formatting already-formatted code produces identical output
- All example files format without error
- `asterc fmt --check` returns 0 on formatted files, 1 on unformatted
- Long lines are broken at sensible points
- Indentation is always consistent (4 spaces)
- Magic trailing comma controls vertical vs. horizontal layout

## Idempotency Testing

The most critical property: `format(format(x)) == format(x)`. Test this by:
1. Format every example file
2. Format the result again
3. Assert the two outputs are identical

This catches bugs where the formatter's output isn't in its own canonical form.

### Phase 6: Agent-Readable Output (MCP / LLM Integration)

The formatter should support machine-readable output so agents can programmatically check formatting and apply fixes.

**6A. JSON diff output**

```
asterc fmt --check --output-format json file.aster
```

```json
{
  "file": "file.aster",
  "formatted": false,
  "diff": [
    {
      "line": 5,
      "original": "let x=1+2",
      "formatted": "let x = 1 + 2",
      "span": { "start": 42, "end": 51 }
    },
    {
      "line": 12,
      "original": "    if   x>0:",
      "formatted": "    if x > 0:",
      "span": { "start": 120, "end": 133 }
    }
  ]
}
```

This gives agents a structured diff they can present as suggestions or auto-apply, rather than needing to parse unified diff output.

**6B. Formatted output with AST**

```
asterc fmt --emit ast --output-format json file.aster
```

Returns the formatted source alongside the AST, so agents can verify the formatter didn't change semantics:

```json
{
  "file": "file.aster",
  "formatted_source": "let x = 1 + 2\n...",
  "ast_before": { ... },
  "ast_after": { ... },
  "ast_equal": true
}
```

The `ast_equal: true` field is a built-in semantic safety check -- the formatter asserts that the AST before and after formatting is identical.

## Dependency Chain

```
diagnostics.md (spans on tokens)  -- soft prerequisite (for trivia spans)
    |
formatter Phase 1 (trivia lexer)  -- can start independently
    |
formatter Phase 2 (Doc IR + printer)  -- pure algorithm, no deps
    |
formatter Phase 3 (formatting rules)  -- needs AST knowledge
    |
formatter Phase 4-5 (CLI, magic comma)
    |
formatter Phase 6 (agent-readable JSON output)
```
