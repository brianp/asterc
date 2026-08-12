# Architecture: Compiler Pipeline

Status: Current checked pipeline plus the narrower executable contract for `run` and `build`.

---

## 1. Overview

Aster uses a staged pipeline where each stage produces a well-defined
intermediate representation and is implemented as a separate Rust crate.

```
source.aster
    |
    v
+----------+     +----------+     +-----------+     +----------+
|  Lexer   | --> |  Parser  | --> | TypeCheck | --> |  Codegen | --> execution
| (tokens) |     |   (AST)  |     | (typed)   |     | (HIR->   |
|          |     |          |     |           |     |  CLIF)   |
+----------+     +----------+     +-----------+     +----------+
    |                 |                 |                 |
    v                 v                 v                 v
 tokens.json      ast.json       semantics.json      hir.json
                                                     clif.txt
    \__________________|__________________/________________/
                       |
                   .aster/last-run/  (TOONS artifacts)
                       |
                   human.txt  (ariadne rendering)
```

Every stage emits artifacts to the TOONS bundle when `--toons` is active.

The executable path is narrower than the checked path today:

```text
lexer -> parser -> typecheck -> FIR lowering -> Cranelift -> runtime
```

When FIR lowering hits a feature that still lacks executable support, `asterc run` and `asterc build` emit `E028` instead of an opaque lowering discriminant.

---

## 2. Crate Structure

```
asterc/                    # root workspace
  src/                     # compiler driver (main binary)
    main.rs                # CLI entry point, flag parsing
    toons.rs               # TOONS artifact writer
    output.rs              # human output (ariadne renderer)
    templates.rs           # diagnostic template registry
    session.rs             # CompilerSession (stateful, for REPL)
    repl.rs                # REPL driver

  ast/                     # AST data structures
    src/
      lib.rs               # re-exports
      expr.rs              # Stmt, Expr, Module, MatchPattern
      types.rs             # Type enum
      type_env.rs          # TypeEnv (scoped environment)
      span.rs              # Span (byte offsets)
      node_id.rs           # NodeId (stable AST node identifiers)
      diagnostic.rs        # Diagnostic, Label, Severity, CandidateFix

  lexer/                   # tokenization
    src/
      lib.rs               # lex() and lex_with_trivia()
      token.rs             # Token, TokenKind, Trivia

  parser/                  # parsing
    src/
      lib.rs               # statement parsing, parse_module, parse_repl_input
      expr.rs              # expression parsing (precedence climbing)

  typecheck/               # type checking
    src/
      typechecker.rs       # statement-level checking
      check_expr.rs        # expression-level checking

  codegen/                 # code generation
    src/
      lib.rs               # public API
      hir.rs               # HIR types
      lower.rs             # AST -> HIR lowering
      jit.rs               # Cranelift JIT setup
      translate.rs         # HIR -> CLIF translation
      runtime.rs           # JIT runtime symbols, allocator, GC
      runtime_source.rs    # shared AOT C runtime source

  aster-lsp/               # language server (planned)
    src/
      main.rs              # LSP entry point (tower-lsp)
      analysis.rs          # document analysis pipeline
      symbols.rs           # SymbolIndex
      conversions.rs       # span/range conversions

  aster-fmt/               # formatter (planned)
    src/
      lib.rs               # format_source() API
      doc.rs               # Doc IR (Wadler-Lindig)
      rules.rs             # AST-to-Doc formatting rules
      trivia.rs            # comment/whitespace handling
      config.rs            # Config (line_width, indent_size, quote_style)

  aster-mcp/               # MCP server (planned)
    src/
      main.rs              # MCP server entry point (stdio)
      server.rs            # JSON-RPC server
      state.rs             # workspace state
      watcher.rs           # file system watcher
      resources.rs         # MCP resource handlers
      tools.rs             # MCP tool handlers
      compiler.rs          # asterc subprocess invocation

  tests/                   # integration tests
    integration.rs         # check_ok/check_err helpers
    examples.rs            # file-based example tests

  examples/                # executable contracts and front-end spec programs
  plans/                   # implementation plans
  docs/                    # design docs, RFCs, architecture
```

---

## 3. Data Flow

### Tokens

```rust
// lexer/src/token.rs
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
    pub line: usize,
    pub col: usize,
}
```

The lexer produces `Vec<Token>`. Tokens carry `Span` (byte offsets)
for diagnostic rendering. The `line`/`col` fields are kept during
migration but will eventually be computed on demand from `Span`.

For the formatter, `lex_with_trivia()` produces tokens with attached
comments and whitespace.

### AST

```rust
// ast/src/expr.rs
pub struct Module {
    pub name: String,
    pub body: Vec<Stmt>,
}
```

The parser produces a `Module` containing `Stmt` and `Expr` nodes.
Each node carries `id: NodeId` and `span: Span`.

The AST is semantic, not syntactic -- it does not preserve comments,
whitespace, or exact token positions. For that, use the trivia-attached
token stream (formatter) or future CST (if needed).

### TypeEnv

```rust
// ast/src/type_env.rs
pub struct TypeEnv {
    bindings: HashMap<String, Type>,
    parent: Option<Rc<TypeEnv>>,
}
```

Scoped type environment using Rc-based parent chains. Created fresh
for each scope (function body, class body, block). The type checker
walks the AST and populates/queries TypeEnv.

### Diagnostics

```rust
// ast/src/diagnostic.rs
pub struct Diagnostic {
    pub severity: Severity,
    pub template: DiagnosticTemplate,
    pub primary_node: NodeId,
    pub primary_span: Span,
    pub labels: Vec<Label>,
    pub related_nodes: Vec<RelatedNode>,
    pub notes: Vec<String>,
    pub candidate_fixes: Vec<CandidateFix>,
    pub code: String,
}
```

Produced by any pipeline stage. Consumed by:
- `output.rs` -> ariadne rendering (human channel)
- `toons.rs` -> TOONS artifacts (machine channel)
- LSP -> LSP diagnostic conversion

### HIR (planned)

Lowered from AST after type checking. All generics monomorphized,
all method calls resolved, all class layouts computed. Types
annotated on every node.

### CLIF (planned)

Generated from HIR by the Cranelift translation layer.

---

## 4. Dual-Channel Output

Every compilation produces two outputs:

### Human channel

Pretty-printed via ariadne. Written to:
- stdout (default)
- `.aster/last-run/human.txt` (when `--toons` is active)

Designed for terminals and human reading.

### Machine channel

Structured JSON/TOONS. Written to:
- `.aster/last-run/*.json` / `.toons` (when `--toons` is active)
- stdout (when `--output-format json` is active)

Designed for MCP servers, AI agents, CI tools, IDE plugins.

Both channels describe the same compilation. They never disagree.

---

## 5. Binary Targets

| Binary | Purpose |
|--------|---------|
| `asterc` | One-shot compiler. Lex, parse, typecheck, codegen, run. |
| `aster-lsp` | Language server (tower-lsp, stdio). |
| `aster-mcp` | MCP bridge for AI agents (stdio). |
| `asterd` | Incremental daemon (future). Watches workspace. |

### CLI

```
asterc file.aster                     # compile and run
asterc check file.aster               # typecheck only
asterc fmt file.aster                 # format
asterc fmt --check file.aster         # check formatting
asterc --repl                         # interactive REPL
asterc -e "1 + 2"                     # eval expression
asterc file.aster --toons             # write TOONS artifacts
asterc file.aster --output-format json    # JSON to stdout
asterc file.aster --emit ast          # dump specific stage
asterc --explain E0412                # explain error code
```

---

## 6. Error Strategy

Errors flow through the pipeline as `Diagnostic` values:

1. **Lexer errors**: invalid characters, unterminated strings
2. **Parser errors**: unexpected tokens, malformed syntax
3. **Type errors**: type mismatches, undefined names, constraint violations

With error recovery:
- Parser: skip to next synchronization point, continue parsing
- TypeChecker: assign `Type::Error`, continue checking
- Result: best-effort AST + list of all diagnostics

Without error recovery (current):
- Pipeline stops at first error
- Returns single error

Error recovery is essential for the LSP (report all errors) and
the REPL (don't crash on bad input).

---

## 7. Dependency Map

```
ast (no deps)
  |
  +--- lexer (depends on: ast)
  |
  +--- parser (depends on: ast, lexer)
  |
  +--- typecheck (depends on: ast)
  |
  +--- codegen (depends on: ast) [planned]
  |
  +--- aster-fmt (depends on: ast, lexer) [planned]
  |
  +--- aster-lsp (depends on: ast, lexer, parser, typecheck) [planned]
  |
  +--- aster-mcp (depends on: ast) [planned]

asterc (depends on: all library crates)
```

The `ast` crate is the foundation. It defines all shared types
(Stmt, Expr, Type, Span, NodeId, Diagnostic) and has no dependencies
on other workspace crates.
