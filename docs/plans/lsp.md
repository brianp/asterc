# Plan: LSP Server

## Context

A Language Server Protocol (LSP) server gives Aster first-class editor support: inline errors, go-to-definition, hover type info, autocomplete. This is what makes a language *feel* real in day-to-day use.

**Depends on:** `diagnostics.md` (spans + structured errors). Spans are non-negotiable for LSP -- every feature maps source positions to semantic information.

## Community Research

**Most-used LSP features (by developer surveys):**
1. **Diagnostics** -- inline errors/warnings as you type (the killer feature)
2. **Go-to-definition** -- click to jump to where something is defined
3. **Hover** -- show type signature on mouse hover
4. **Autocomplete** -- context-aware code completion
5. **Find references** -- find all usages of a symbol

Features 1-4 cover ~90% of daily LSP usage. Features like rename, code actions, and formatting are "nice to have" initially.

**What developers hate in LSP servers:**
- Slow responsiveness (>200ms for completions feels laggy)
- Stale diagnostics that don't update until save
- Completions that suggest irrelevant items
- Missing go-to-definition for obvious things (imports, variables)
- No hover information at all

**Rust frameworks for building LSP servers:**
- `tower-lsp` -- the community standard. Async, based on tower middleware. Implements the full LSP protocol. Provides `LanguageServer` trait you fill in.
- `lsp-server` (from rust-analyzer) -- lower-level, synchronous. More control, more boilerplate.
- `lsp-types` -- protocol type definitions, used by both.

**Recommendation:** Use `tower-lsp`. It's async, well-documented, actively maintained, and the most commonly used framework for custom language servers in Rust.

**Key architectural insight:** Implementing incremental lexing/parsing is hard to retrofit. The LSP server should work with **full re-lex/re-parse on each change** initially (fast enough for small-to-medium files), with incremental parsing as a future optimization.

## Design

### Architecture

```
Editor (VS Code, Neovim, etc.)
    |
    | LSP Protocol (JSON-RPC over stdio)
    |
+---v-----------------------------------+
| Aster Language Server                  |
|                                        |
|  DocumentState {                       |
|    source: String,                     |
|    tokens: Vec<Token>,                 |
|    ast: Module,                        |
|    type_env: TypeEnv,                  |
|    diagnostics: Vec<Diagnostic>,       |
|    symbol_index: SymbolIndex,          |
|  }                                     |
|                                        |
|  On change: re-lex -> re-parse ->      |
|             re-typecheck -> publish    |
+----------------------------------------+
```

### New Crate: `aster-lsp/`

Add to workspace as a separate binary crate:

```toml
# Cargo.toml workspace members
members = ["ast", "lexer", "parser", "typecheck", "aster-lsp"]
```

### Phase 1: Diagnostics (the MVP)

The single most valuable LSP feature. Get this working first.

**1A. Document synchronization**

```rust
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

struct AsterLanguageServer {
    client: Client,
    documents: DashMap<Url, DocumentState>,
}

struct DocumentState {
    source: String,
    version: i32,
}
```

**1B. On-change analysis pipeline**

When a document changes (`textDocument/didChange`):

```rust
async fn on_change(&self, uri: Url, text: String, version: i32) {
    // 1. Store new source
    self.documents.insert(uri.clone(), DocumentState { source: text.clone(), version });

    // 2. Full pipeline: lex -> parse -> typecheck
    let diagnostics = self.analyze(&text);

    // 3. Publish diagnostics to editor
    self.client.publish_diagnostics(
        uri,
        diagnostics.into_iter().map(|d| to_lsp_diagnostic(&text, d)).collect(),
        Some(version),
    ).await;
}

fn analyze(&self, source: &str) -> Vec<Diagnostic> {
    let mut diagnostics = vec![];

    let tokens = match lex(source) {
        Ok(t) => t,
        Err(d) => { diagnostics.push(d); return diagnostics; }
    };

    let module = match Parser::new(tokens).parse_module("Main") {
        Ok(m) => m,
        Err(d) => { diagnostics.push(d); return diagnostics; }
    };

    let mut checker = TypeChecker::new();
    if let Err(d) = checker.check_module(&module) {
        diagnostics.push(d);
    }

    diagnostics
}
```

With error recovery (from diagnostics plan), the parser and typechecker will return *multiple* diagnostics and a best-effort AST, making this much more useful.

**1C. Convert Aster diagnostics to LSP diagnostics**

```rust
fn to_lsp_diagnostic(source: &str, diag: AsterDiagnostic) -> LspDiagnostic {
    let range = span_to_range(source, diag.labels[0].span);
    LspDiagnostic {
        range,
        severity: Some(match diag.severity {
            Severity::Error => DiagnosticSeverity::ERROR,
            Severity::Warning => DiagnosticSeverity::WARNING,
            Severity::Hint => DiagnosticSeverity::HINT,
        }),
        message: diag.message,
        ..Default::default()
    }
}
```

### Phase 2: Hover + Go-to-Definition

**2A. Symbol Index**

Build during typechecking -- map each identifier's span to its type and definition location:

```rust
pub struct SymbolInfo {
    pub name: String,
    pub ty: Type,
    pub def_span: Span,           // where it was defined
    pub kind: SymbolKind,          // Variable, Function, Class, Trait, Field, Method
}

pub struct SymbolIndex {
    /// Map from use-site span to definition info
    pub references: HashMap<Span, SymbolInfo>,
    /// Map from definition span to symbol info
    pub definitions: HashMap<String, SymbolInfo>,
}
```

The typechecker builds this index as a side effect of type checking. When it resolves a name, it records: "identifier at span X refers to symbol Y defined at span Z with type T."

**2B. Hover**

```rust
async fn hover(&self, params: HoverParams) -> Option<Hover> {
    let uri = params.text_document_position_params.text_document.uri;
    let pos = params.text_document_position_params.position;
    let doc = self.documents.get(&uri)?;
    let offset = position_to_offset(&doc.source, pos);

    // Find symbol at offset
    let symbol = doc.symbol_index.find_at(offset)?;

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: format!("```aster\n{}: {}\n```", symbol.name, symbol.ty),
        }),
        range: Some(span_to_range(&doc.source, symbol.span)),
    })
}
```

**2C. Go-to-Definition**

```rust
async fn goto_definition(&self, params: GotoDefinitionParams) -> Option<GotoDefinitionResponse> {
    let offset = position_to_offset(&doc.source, pos);
    let symbol = doc.symbol_index.find_at(offset)?;

    Some(GotoDefinitionResponse::Scalar(Location {
        uri: uri.clone(),  // same file for now; multi-file later
        range: span_to_range(&doc.source, symbol.def_span),
    }))
}
```

### Phase 3: Autocomplete

**3A. Completion sources:**
- All names in current `TypeEnv` scope
- Class fields/methods after `.`
- Keywords in appropriate contexts
- Type names after `:` or `->` annotations

**3B. Context-aware filtering:**

```rust
async fn completion(&self, params: CompletionParams) -> Option<CompletionResponse> {
    let offset = position_to_offset(&doc.source, pos);
    let context = analyze_completion_context(&doc.source, offset);

    let items = match context {
        CompletionContext::AfterDot(expr_type) => {
            // Suggest fields and methods of the type
            get_type_members(&doc.type_env, &expr_type)
        }
        CompletionContext::TypeAnnotation => {
            // Suggest type names
            get_type_names(&doc.type_env)
        }
        CompletionContext::General(prefix) => {
            // Suggest all names matching prefix
            get_all_names(&doc.type_env, &prefix)
        }
    };

    Some(CompletionResponse::Array(items))
}
```

### Phase 4: Find References + Document Symbols

**4A. Find References** -- reverse lookup from the symbol index. For a given definition, find all spans that reference it.

**4B. Document Symbols** -- return an outline of all definitions in the file (functions, classes, traits, let bindings). This powers the "outline" sidebar in editors.

### Phase 5: VS Code Extension

A thin VS Code extension that launches the LSP binary:

```
aster-vscode/
  package.json     -- extension manifest
  src/
    extension.ts   -- just starts the LSP server process
```

```json
{
  "name": "aster-lang",
  "activationEvents": ["onLanguage:aster"],
  "contributes": {
    "languages": [{
      "id": "aster",
      "extensions": [".aster"],
      "configuration": "./language-configuration.json"
    }],
    "grammars": [{
      "language": "aster",
      "scopeName": "source.aster",
      "path": "./syntaxes/aster.tmGrammar.json"
    }]
  }
}
```

The extension also includes a TextMate grammar for basic syntax highlighting (independent of the LSP).

## Files Created/Modified

| File | Changes |
|------|---------|
| `aster-lsp/Cargo.toml` | NEW -- deps: tower-lsp, tokio, dashmap, serde_json |
| `aster-lsp/src/main.rs` | NEW -- LSP server entry point |
| `aster-lsp/src/analysis.rs` | NEW -- document analysis pipeline |
| `aster-lsp/src/symbols.rs` | NEW -- SymbolIndex, SymbolInfo |
| `aster-lsp/src/conversions.rs` | NEW -- span/position/range conversions |
| `typecheck/src/typechecker.rs` | Build SymbolIndex during typecheck |
| `Cargo.toml` | Add aster-lsp to workspace members |
| `aster-vscode/` | NEW -- VS Code extension directory |

## Performance Considerations

- **Full re-analysis on every keystroke is fine** for files under ~5K lines. The lex+parse+typecheck pipeline in Rust should complete in <10ms for typical files.
- **Debounce** changes -- don't re-analyze until the user pauses typing (~150ms delay).
- **Future: incremental parsing** with tree-sitter or a custom incremental lexer. Only needed if performance becomes an issue with large files.
- **Future: salsa-style query framework** for incremental computation. This is what rust-analyzer uses.

## Verification

- LSP server starts and connects to VS Code
- Syntax errors appear as inline squiggles as you type
- Type errors appear with correct source locations
- Hover shows type information for variables, functions, classes
- Go-to-definition jumps to the correct location
- Autocomplete suggests relevant names after typing a prefix
- Autocomplete suggests fields/methods after `.`
- Document outline shows all definitions

### Phase 6: Agent-Readable Hooks (MCP / LLM Integration)

The LSP already speaks JSON-RPC, but AI agents connected via an Aster MCP server need additional capabilities beyond standard LSP.

**6A. Custom LSP requests for agent tooling**

Extend the LSP server with custom methods (prefixed `aster/`) that an MCP server can call:

| Method | Request | Response |
|--------|---------|----------|
| `aster/dumpAst` | `{ uri, range? }` | Full AST as JSON for the file or range |
| `aster/dumpTokens` | `{ uri, range? }` | Token stream as JSON |
| `aster/dumpTypes` | `{ uri }` | All type bindings in the file |
| `aster/typeAt` | `{ uri, position }` | Type of expression at cursor position |
| `aster/symbolsFlat` | `{ uri }` | Flat list of all symbols with types and spans |
| `aster/explain` | `{ code: "E003" }` | Detailed explanation of error code |
| `aster/suggest` | `{ uri, position }` | Context-aware fix suggestions for diagnostic at position |

**6B. Diagnostic notifications with full context**

When the LSP publishes diagnostics, the Aster MCP server needs more context than standard LSP provides. The custom notification `aster/diagnosticsRich` includes:

```json
{
  "uri": "file:///project/main.aster",
  "diagnostics": [
    {
      "severity": "error",
      "code": "E003",
      "message": "type mismatch",
      "range": { "start": {"line": 4, "character": 12}, "end": {"line": 4, "character": 19} },
      "source_line": "    let x = \"hello\" + 1",
      "labels": [
        { "range": {...}, "message": "this is a String" },
        { "range": {...}, "message": "but (+) expects both sides to be the same type" }
      ],
      "notes": ["try converting with to_string() or parse()"],
      "ast_context": {
        "node": "BinaryOp",
        "parent": "Let { name: \"x\" }",
        "scope": "function main"
      }
    }
  ]
}
```

The `ast_context` field gives agents the surrounding AST structure so they can understand *where* in the program structure the error occurs without re-parsing.

**6C. Compilation event stream**

The LSP emits `aster/compilationResult` notifications after each analysis pass:

```json
{
  "uri": "file:///project/main.aster",
  "success": false,
  "stages": {
    "lex": {"ok": true, "token_count": 47, "duration_ms": 0.2},
    "parse": {"ok": true, "duration_ms": 0.8},
    "typecheck": {"ok": false, "duration_ms": 1.2}
  },
  "diagnostic_count": 2,
  "symbol_count": 15
}
```

This lets agents know the overall health of a file at a glance without querying individual diagnostics.

## Dependency Chain

```
diagnostics.md (spans, structured errors, error recovery, JSON output)  -- hard prerequisite
    |
lsp.md Phase 1 (diagnostics)  -- MVP, most valuable
    |
lsp.md Phase 2 (hover, go-to-def)  -- requires SymbolIndex in typechecker
    |
lsp.md Phase 3 (autocomplete)  -- requires scope analysis
    |
lsp.md Phase 4-5 (references, VS Code extension)
    |
lsp.md Phase 6 (agent-readable hooks)  -- custom aster/ methods for MCP
```
