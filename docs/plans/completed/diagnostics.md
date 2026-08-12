# Plan: Better Diagnostics

## Context

The Aster compiler currently reports errors as plain `String` values with no source location, no context, and no suggestions. Tokens have `line` and `col` fields but these are not propagated to AST nodes or error messages. The user sees messages like `"Type error: expected Int, got String"` with no indication of *where* in their code the problem is.

Good diagnostics are foundational -- the REPL, LSP, formatter, and codegen plans all depend on spans and structured errors. **This plan should be implemented first.**

## Community Research

**What developers love (Rust, Elm):**
- Rust's diagnostics are widely considered the gold standard: labeled source spans, multi-line context, `help:` and `note:` sub-diagnostics, error codes linking to detailed explanations
- Elm pioneered "compiler errors for humans" -- first-person tone ("I found a problem"), plain English, concrete suggestions, no jargon
- Developers consistently rank Rust and Elm as the best for error messages; Java/Go/TypeScript are criticized for cryptic, unhelpful output

**What developers hate:**
- Error messages that say *what* went wrong but not *where* or *why*
- Cascading errors from a single root cause (one typo = 50 errors)
- Jargon-heavy messages that assume compiler internals knowledge
- No suggestions for how to fix the problem

**Library ecosystem (Rust crates):**
- `ariadne` -- most beautiful output, inline + multi-line labels, colors, arbitrary span configurations. Best for maximum visual quality.
- `miette` -- derives from `std::error::Error`, integrates with `?` operator, good for library authors. Uses some `ariadne` code internally.
- `codespan-reporting` -- stable, well-tested, inspired `ariadne`. Slightly less pretty but battle-tested.

**Recommendation:** Use `ariadne` for rendering. It produces the prettiest output and Aster should have best-in-class error messages from day one.

## Current State

```
Token { kind: TokenKind, line: usize, col: usize }  // has position, no byte offset
Stmt/Expr enums -- no span fields
TypeChecker returns Result<Type, String>              // no structured errors
Parser returns Result<T, String>                      // no structured errors
Lexer returns Result<Vec<Token>, String>              // no structured errors
```

## Design

### Phase 1: Span Infrastructure

**1A. Define `Span` (in `ast/src/span.rs`)**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,  // byte offset
    pub end: usize,    // byte offset
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self { ... }
    pub fn merge(self, other: Span) -> Span { ... }  // union of two spans
}
```

Keep it simple -- byte offsets only. Line/column computed on demand from the source string when rendering. This is what Rust, Elm, and most modern compilers do.

**1B. Add byte offset to `Token`**

```rust
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,       // byte offset range
    pub line: usize,      // keep for backward compat during migration
    pub col: usize,
}
```

Update the lexer to track byte position as it scans. The lexer already tracks `line` and `col`, so adding byte offset is straightforward.

**1C. Add `Span` to AST nodes**

Two options:
1. Add `span: Span` field to every `Stmt` and `Expr` variant
2. Wrap nodes: `Spanned<T> { node: T, span: Span }`

**Decision: Option 1 (field per variant).** It's more explicit and avoids wrapping/unwrapping. Elm, Rust (rustc), and most production compilers use this approach. The `Spanned<T>` wrapper is elegant but adds friction everywhere you pattern-match.

Add `span: Span` to:
- Every `Stmt` variant
- Every `Expr` variant
- `Module`

### Phase 2: Structured Diagnostics

**2A. Define `Diagnostic` (in `ast/src/diagnostic.rs`)**

```rust
#[derive(Debug, Clone)]
pub enum Severity {
    Error,
    Warning,
    Hint,
}

#[derive(Debug, Clone)]
pub struct Label {
    pub span: Span,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,        // primary message
    pub labels: Vec<Label>,     // source locations with context
    pub notes: Vec<String>,     // help text, suggestions
    pub code: Option<String>,   // e.g. "E001" for error index
}
```

**2B. Replace `String` errors throughout the pipeline**

| Component | Before | After |
|-----------|--------|-------|
| Lexer | `Result<Vec<Token>, String>` | `Result<Vec<Token>, Diagnostic>` |
| Parser | `Result<T, String>` | `Result<T, Diagnostic>` |
| TypeChecker | `Result<Type, String>` | `Result<Type, Diagnostic>` |

**2C. Error rendering with `ariadne`**

Add `ariadne` as a dependency of the top-level `asterc` crate (not the library crates). The library crates produce `Diagnostic` values; `main.rs` renders them.

```rust
// main.rs -- render diagnostics
fn render_diagnostic(source: &str, filename: &str, diag: &Diagnostic) {
    use ariadne::{Report, ReportKind, Label, Source};
    let kind = match diag.severity {
        Severity::Error => ReportKind::Error,
        Severity::Warning => ReportKind::Warning,
        Severity::Hint => ReportKind::Advice,
    };
    let mut report = Report::build(kind, filename, diag.labels[0].span.start);
    report.set_message(&diag.message);
    for label in &diag.labels {
        report.add_label(
            Label::new((filename, label.span.start..label.span.end))
                .with_message(&label.message)
        );
    }
    for note in &diag.notes {
        report.set_note(note);
    }
    report.finish().print((filename, Source::from(source))).unwrap();
}
```

### Phase 3: Rich Error Messages

Follow Elm's philosophy: **errors are a conversation, not a stack trace.**

**3A. Type error messages**

Before:
```
Type error: expected Int, got String
```

After:
```
Error: type mismatch
   --> examples/03_simple_function.aster:5:12
   |
 5 |     let x = "hello" + 1
   |             ^^^^^^^ this is a String
   |                       ^ but (+) expects both sides to be the same type
   |
   = help: try converting with to_string() or parse()
```

**3B. Suggestion system**

Common suggestions to implement:
- Misspelled identifiers → "did you mean `foo`?" (Levenshtein distance)
- Missing return type → "add `-> Type` to the function signature"
- Type mismatch in assignment → show both types clearly
- Unknown field on class → list available fields
- Calling non-function → "this is a `Int`, not a function"

**3C. Error codes**

Assign stable codes (E001, E002, ...) to each error class. This enables:
- `asterc --explain E001` for detailed explanation + examples
- Linking to online documentation
- Searching for help

### Phase 4: Error Recovery

**4A. Parser error recovery**

Instead of stopping at the first error, the parser should:
1. Record the error as a `Diagnostic`
2. Skip tokens until a synchronization point (newline at base indent, `def`, `class`, `let`)
3. Continue parsing

This enables reporting multiple errors per compilation and is essential for the LSP.

```rust
pub struct ParseResult {
    pub module: Module,              // best-effort AST
    pub diagnostics: Vec<Diagnostic>, // all errors found
}
```

**4B. TypeChecker error accumulation**

Similarly, the type checker should continue past errors:
- Assign `Type::Error` to nodes with type errors
- `Type::Error` is compatible with everything (prevents cascading errors)
- Collect all diagnostics, report at the end

Add to `ast/src/types.rs`:
```rust
pub enum Type {
    // ... existing variants ...
    Error,  // sentinel for error recovery
}
```

## Files Modified

| File | Changes |
|------|---------|
| `ast/src/span.rs` | NEW -- Span type |
| `ast/src/diagnostic.rs` | NEW -- Diagnostic, Label, Severity |
| `ast/src/lib.rs` | Re-export span and diagnostic modules |
| `ast/src/expr.rs` | Add `span: Span` to every Stmt/Expr variant |
| `ast/src/types.rs` | Add `Type::Error` |
| `lexer/src/token.rs` | Add `span: Span` to Token |
| `lexer/src/lib.rs` | Track byte offsets, return Diagnostic errors |
| `parser/src/lib.rs` | Propagate spans, error recovery, return Diagnostic |
| `parser/src/expr.rs` | Propagate spans, return Diagnostic |
| `typecheck/src/typechecker.rs` | Return Diagnostic, accumulate errors, suggestions |
| `typecheck/src/check_expr.rs` | Return Diagnostic, span propagation |
| `Cargo.toml` | Add `ariadne` dependency |
| `src/main.rs` | Render diagnostics with ariadne |
| `tests/integration.rs` | Update error assertions to check Diagnostic fields |

## Dependency Graph

```
Phase 1 (Spans) -- foundational, everything depends on this
    |
Phase 2 (Structured Diagnostics)
    |
Phase 3 (Rich Messages) <-> Phase 4 (Error Recovery)  [parallel]
```

## Migration Strategy

This is a large refactor touching every file. Approach:

1. Add `Span` and `Diagnostic` types (non-breaking)
2. Add `span` to `Token` (update lexer, keep `line`/`col` temporarily)
3. Add `span` to AST nodes one variant at a time -- each variant is a small PR
4. Switch error types from `String` to `Diagnostic` one component at a time
5. Add ariadne rendering in main.rs
6. Add error recovery
7. Remove legacy `line`/`col` from Token once everything uses `span`

### Phase 5: TOONS -- Machine-Native Diagnostic Format

**Core philosophy:** Aster is designed for a three-actor ecosystem:

1. **Human writes code** -- in clean, low-entropy Aster syntax
2. **Compiler determines truth** -- types, constraints, violations, everything proven
3. **LLM explains truth** -- translates compiler facts into human understanding

The compiler is the source of correctness. The LLM is the adaptive explanation layer. **The LLM should never infer from source alone if TOONS exists.**

Priority: `TOONS truth > AST truth > source text`

TOONS is not "JSON error output." It is a **full semantic interchange format** -- the contract between the compiler and any machine consumer (MCP server, AI agent, IDE plugin, CI tool).

**5A. Stable Node IDs**

Every AST node gets a deterministic ID. This lets diagnostics, semantic info, and repair candidates reference specific nodes without ambiguity.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct NodeId(pub u32);

// Added to every Stmt and Expr variant alongside span
pub struct StmtNode {
    pub id: NodeId,
    pub span: Span,
    pub kind: Stmt,
}
```

Node IDs are stable within a single compilation. They allow diagnostics to say "node_184 has a type mismatch" and repair candidates to say "replace node_184 with this expression."

**5B. TOONS Diagnostic Object**

Diagnostics are not prose-first. They are structured facts with machine-actionable data:

```json
{
  "diagnostic_id": "E0412",
  "severity": "error",
  "message_template": "type_mismatch_assignment",
  "primary_node": "node_184",
  "primary_span": {"file": "main.aster", "start": 120, "end": 141, "line": 5, "col": 12},
  "source_line": "    let x = \"hello\" + 1",
  "expected_type": "Int",
  "actual_type": "String",
  "constraint": "Assignable(actual, expected)",
  "related_nodes": [
    {"node": "node_122", "role": "binding_declaration", "span": {"start": 10, "end": 25}},
    {"node": "node_184", "role": "assigned_expression", "span": {"start": 120, "end": 141}}
  ],
  "scope_context": {
    "function": "main",
    "block_depth": 1,
    "parent_stmt": "node_180"
  },
  "candidate_fixes": [
    {"kind": "convert", "from": "String", "to": "Int", "via": "parse()", "confidence": 0.61,
     "edit": {"node": "node_184", "replacement": "parse(\"hello\")"}},
    {"kind": "change_binding_type", "from": "Int", "to": "String", "confidence": 0.48,
     "edit": {"node": "node_122", "replacement": "let x: String = \"hello\" + 1"}}
  ]
}
```

Key differences from plain JSON diagnostics:
- **`message_template`** not prose -- a template ID the LLM can render for beginner/expert/child/Rust-programmer differently
- **`primary_node`** + **`related_nodes`** -- node IDs, not just spans. The LLM can traverse the AST around the failure.
- **`constraint`** -- the actual type rule that was violated, as a formal expression
- **`scope_context`** -- where in the program structure the error occurred
- **`candidate_fixes`** with **`confidence`** -- machine-actionable repairs the compiler computed, with likelihood scores. The LLM picks the best one, not guesses from scratch.
- **`edit`** -- concrete replacement text for each fix. The agent can apply it directly.

**5C. TOONS Four Layers**

A TOONS artifact contains four distinct sections:

**Layer 1: Syntax** -- what the parser saw
```json
{
  "layer": "syntax",
  "tokens": [...],
  "ast": { "nodes": [...], "root": "node_0" },
  "source_map": { "file": "main.aster", "content_hash": "sha256:..." }
}
```
- Token stream with spans
- AST with stable node IDs
- Source provenance (content hash for cache invalidation)

**Layer 2: Semantics** -- what the compiler concluded
```json
{
  "layer": "semantics",
  "symbol_table": [
    {"name": "x", "node": "node_122", "type": "Int", "scope": "main", "kind": "variable"},
    {"name": "greet", "node": "node_50", "type": "(String) -> Void", "scope": "module", "kind": "function"}
  ],
  "resolved_types": {
    "node_184": {"inferred": "String", "expected": "Int"},
    "node_122": {"declared": "Int"}
  },
  "trait_satisfaction": [...],
  "scope_graph": [...]
}
```
- Resolved names, inferred and declared types
- Trait/interface satisfaction
- Scope graph with parent/child relationships

**Layer 3: Diagnostics** -- what failed or is suspicious
```json
{
  "layer": "diagnostics",
  "errors": [...],
  "warnings": [...],
  "lints": [...]
}
```
Each item uses the full diagnostic object format from 5B.

**Layer 4: Repairs** -- how the compiler thinks the code could be fixed
```json
{
  "layer": "repairs",
  "candidate_fixes": [...],
  "desugared_form": {...},
  "normalized_form": {...}
}
```
- Edit scripts for autofix
- Desugared/normalized forms for comparison
- Alternative valid interpretations if any

**5D. Artifact Output Directory**

Every compilation writes a TOONS bundle to `.aster/`:

```
.aster/
  last-run/
    source-map.json      # file paths, content hashes
    tokens.json          # full token stream
    ast.json             # AST with node IDs
    semantics.json       # symbol table, resolved types, scopes
    diagnostics.toons    # TOONS diagnostic objects
    repairs.json         # candidate fixes
    human.txt            # pretty-printed ariadne output
    envelope.json        # compilation result summary
  history/
    run-0001/            # previous runs (configurable retention)
    run-0002/
```

The split serves two audiences:
- **IDE / human** reads `human.txt`
- **MCP server / AI agent** reads `diagnostics.toons` + `ast.json` + `semantics.json`

**5E. CLI Flags**

```
asterc file.aster                             # human output (ariadne)
asterc file.aster --toons                     # write TOONS to .aster/last-run/
asterc file.aster --toons --emit all          # write TOONS + all IR dumps
asterc file.aster --output-format json        # JSON to stdout (for piping)
asterc file.aster --emit ast                  # dump AST to stdout
asterc file.aster --emit tokens               # dump tokens to stdout
asterc file.aster --emit types                # dump type env to stdout
asterc file.aster --emit symbols              # dump symbol index to stdout
asterc file.aster --emit all                  # everything to stdout
```

`--toons` writes to disk (for MCP server to watch).
`--output-format json` writes to stdout (for piping).
`--emit` selects which layers to include.

**5F. Compilation Result Envelope**

```json
{
  "version": "0.1.0",
  "success": false,
  "file": "main.aster",
  "content_hash": "sha256:abc123...",
  "timestamp": "2026-03-08T14:23:01Z",
  "stages": {
    "lex": {"ok": true, "token_count": 47, "duration_us": 200},
    "parse": {"ok": true, "node_count": 23, "duration_us": 800},
    "typecheck": {"ok": false, "error_count": 2, "warning_count": 0, "duration_us": 1200}
  },
  "files": {
    "tokens": "tokens.json",
    "ast": "ast.json",
    "semantics": "semantics.json",
    "diagnostics": "diagnostics.toons",
    "repairs": "repairs.json",
    "human": "human.txt"
  }
}
```

**5G. Derive Serialize on all types**

Add `serde` + `serde_json` as dependencies. Derive `Serialize` (and `Deserialize` for tool consumption) on:
- `TokenKind`, `Token`
- `Stmt`, `Expr`, `MatchPattern`, `BinOp`, `UnaryOp`
- `Type`
- `Span`, `NodeId`, `Diagnostic`, `Label`, `Severity`
- `Module`

```rust
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Stmt { ... }
```

**5H. Message Templates (not prose)**

Diagnostics use template IDs, not hardcoded English strings:

```rust
pub enum DiagnosticTemplate {
    TypeMismatchAssignment,     // "expected {expected}, got {actual}"
    UndefinedVariable,          // "cannot find '{name}' in this scope"
    FieldNotFound,              // "type {type} has no field '{field}'"
    ArgumentCountMismatch,      // "expected {expected} arguments, got {actual}"
    NotCallable,                // "'{name}' is a {type}, not a function"
    // ...
}
```

The compiler stores the template + structured parameters. The human renderer fills in English prose. The LLM gets the template ID + parameters and can render for any audience:
- beginner
- expert
- Rust programmer
- JS programmer
- IDE quick-fix panel
- child

**Same truth, different presentation.**

## Files Modified

| File | Changes |
|------|---------|
| `ast/src/span.rs` | NEW -- Span type, derive Serialize |
| `ast/src/node_id.rs` | NEW -- NodeId type, derive Serialize |
| `ast/src/diagnostic.rs` | NEW -- Diagnostic, Label, Severity, DiagnosticTemplate, CandidateFix |
| `ast/src/lib.rs` | Re-export span, node_id, diagnostic modules |
| `ast/src/expr.rs` | Add `id: NodeId, span: Span` to every Stmt/Expr variant, derive Serialize |
| `ast/src/types.rs` | Add `Type::Error`, derive Serialize |
| `lexer/src/token.rs` | Add `span: Span` to Token, derive Serialize |
| `lexer/src/lib.rs` | Track byte offsets, return Diagnostic errors |
| `parser/src/lib.rs` | Assign NodeIds, propagate spans, error recovery, return Diagnostic |
| `parser/src/expr.rs` | Assign NodeIds, propagate spans, return Diagnostic |
| `typecheck/src/typechecker.rs` | Return Diagnostic with node refs, accumulate errors, candidate fixes |
| `typecheck/src/check_expr.rs` | Return Diagnostic with node refs, span propagation |
| `Cargo.toml` | Add `ariadne`, `serde`, `serde_json` dependencies |
| `src/main.rs` | `--toons`, `--output-format json`, `--emit` flags, dual-channel output |
| `src/toons.rs` | NEW -- TOONS writer: artifact directory, layers, envelope |
| `src/output.rs` | NEW -- Human output (ariadne renderer) |
| `src/templates.rs` | NEW -- DiagnosticTemplate definitions + human rendering |
| `tests/integration.rs` | Update error assertions to check Diagnostic fields + TOONS output |

## Dependency Graph

```
Phase 1 (Spans + NodeIds) -- foundational
    |
Phase 2 (Structured Diagnostics with templates + constraints)
    |
Phase 3 (Rich Messages) <-> Phase 4 (Error Recovery)  [parallel]
    |
Phase 5 (TOONS: four layers, artifact dir, candidate fixes, Serialize)
    |
mcp-server.md (consumes TOONS artifacts)  -- separate plan
```

## Migration Strategy

1. Add `Span`, `NodeId`, `Diagnostic` types (non-breaking)
2. Add `span` + `id` to `Token` and AST nodes (update lexer/parser)
3. Switch error types from `String` to `Diagnostic` one component at a time
4. Add ariadne rendering for human channel
5. Add error recovery
6. Add `DiagnosticTemplate` + constraint expressions
7. Add `candidate_fixes` computation in type checker
8. Add `serde::Serialize` derives on all types
9. Add `--toons` artifact writer and `--output-format json` / `--emit` flags
10. Remove legacy `line`/`col` from Token once everything uses `span`

## Verification

- `cargo test` passes at every step
- Human output: file path, line number, source context, caret, suggestions (ariadne)
- TOONS output: valid JSON, all four layers, stable node IDs
- Every diagnostic has: template ID, primary node, constraint, related nodes
- Type errors include candidate fixes with confidence scores
- At least 5 common errors have machine-actionable repair candidates
- `--toons` writes complete artifact bundle to `.aster/last-run/`
- `--output-format json` produces valid JSON parseable by `jq`
- `--emit ast` outputs complete AST with node IDs matching TOONS references
- TOONS diagnostics round-trip: an external tool can read `diagnostics.toons`, apply `candidate_fixes[0].edit`, and the result compiles
- `human.txt` and `diagnostics.toons` describe the same errors (dual-channel consistency)
