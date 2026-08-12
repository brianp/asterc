# Plan: REPL (Read-Eval-Print Loop)

## Context

Aster currently only runs as a batch compiler: read file, lex, parse, typecheck, print success/failure. A REPL enables interactive exploration, rapid prototyping, and learning. It turns the compiler into a live environment.

**Depends on:** `diagnostics.md` (spans + structured errors), and `codegen.md` (FIR + Cranelift JIT). The REPL uses the same FIR (Flat Intermediate Representation) as the compiler — it just adds definitions incrementally instead of in batch. See `codegen.md` for the full FIR design.

## Community Research

**What makes a great REPL (Clojure, Python, IPython, Elixir):**
- Persistent state across inputs -- variables, functions, classes survive between lines
- Multiline editing with block-aware history (up-arrow recalls full blocks, not individual lines)
- Syntax highlighting and smart tab completion
- Special variables for last result (`_` or `it`)
- Graceful error handling -- errors don't kill the session
- Ability to redefine functions/classes without restarting
- Reverse-i-search through history (Ctrl+R)

**What developers hate:**
- REPLs that crash or lose state on errors
- No multiline support (have to write everything on one line)
- No way to recall/edit previous multi-line inputs
- Missing completion -- having to remember exact names
- Inconsistent behavior between REPL and file execution

**Typed language REPL challenges:**
- Redefining a variable with a different type is natural in a REPL but conflicts with static typing
- Top-level expressions need to be allowed (not just statements)
- Forward references don't work in a line-by-line model
- The semantic gap between "compile a whole module" and "evaluate one line" is real

**Solutions from other languages:**
- Swift/Scala REPL: allow type-changing redefinitions at the REPL, shadow previous bindings
- Haskell GHCi: `:type`, `:info`, `:load` meta-commands for introspection
- Python 3.14: multiline editing, syntax highlighting, autocompletion built into the stdlib REPL
- IPython: magic commands (`%timeit`, `%who`), rich display, shell integration

**Library for line editing:** `rustyline` (Rust) -- readline-compatible, history, completion, hints, multiline. The standard choice for Rust REPLs.

## Design

### Architecture

```
                    +------------------+
  User Input  --->  | REPL Driver      |
                    |  - line editing   |
                    |  - meta commands  |
                    +--------+---------+
                             |
                    +--------v---------+
                    | ReplSession       |
                    |  - TypeEnv        |  <-- persists across inputs
                    |  - Lowerer        |  <-- persists, appends to FirModule
                    |  - FirModule      |  <-- accumulates all definitions
                    |  - CraneliftJIT   |  <-- compiles new FIR incrementally
                    |  - Source Map     |
                    +--------+---------+
                             |
              +--------------+--------------+
              |              |              |
          lex(input)    parse(input)    typecheck
                                            |
                                     lower to FIR (delta)
                                            |
                                     JIT compile (delta only)
                                            |
                                     execute + display
```

### Phase 1: Core REPL Loop

**1A. `CompilerSession` (new: `src/session.rs`)**

A stateful compilation context that persists across REPL iterations:

```rust
pub struct ReplSession {
    pub type_env: TypeEnv,            // accumulated type bindings
    pub lowerer: Lowerer,             // FIR lowerer (persists across inputs)
    pub fir_module: FirModule,        // accumulated FIR definitions
    pub jit: CraneliftJIT,            // JIT compiler (persists, compiles deltas)
    pub source_map: SourceMap,        // all source snippets for diagnostics
    pub line_number: usize,
}

impl ReplSession {
    pub fn new() -> Self { ... }

    /// Process a single REPL input. Returns the result value (if expression)
    /// or Ok(None) for statements.
    pub fn eval_input(&mut self, input: &str) -> Result<Option<Value>, Vec<Diagnostic>> {
        let tokens = lex(input)?;
        let stmts = Parser::new(tokens).parse_repl_input()?;
        for stmt in &stmts {
            self.type_env.check_stmt(stmt)?;
        }

        // Incremental FIR lowering — only new definitions
        let mark = self.fir_module.mark();
        for stmt in &stmts {
            self.lowerer.lower_stmt(stmt)?;
        }

        // JIT compile only the delta
        for func in self.fir_module.functions_since(mark) {
            self.jit.compile_function(func)?;
        }

        // If the input was an expression, execute and return result
        if let Some(entry) = self.last_expr_function() {
            Ok(Some(self.jit.call_entry(entry)))
        } else {
            Ok(None)
        }
    }
}
```

**1B. Parser: `parse_repl_input` method**

The REPL parser must handle inputs that a module parser wouldn't:

```rust
/// Parse REPL input: either a statement, an expression (auto-print),
/// or a series of statements in an indented block.
pub fn parse_repl_input(&mut self) -> Result<Vec<Stmt>, Diagnostic> {
    // If it looks like an expression (starts with literal, ident, `(`, etc.),
    // try parsing as expression first. Wrap in Stmt::Expr for evaluation.
    // If it looks like a statement (starts with `let`, `def`, `class`, etc.),
    // parse as statement.
    // Support multi-statement blocks separated by newlines.
}
```

**1C. REPL Driver (`src/repl.rs`)**

```rust
use rustyline::{Editor, Config};

pub fn run_repl() {
    let config = Config::builder()
        .auto_add_history(true)
        .build();
    let mut rl = Editor::new(config);
    let mut session = CompilerSession::new();

    println!("Aster REPL v0.1.0");
    println!("Type :help for commands, :quit to exit");

    loop {
        let prompt = if session.is_continuation() { "... " } else { ">>> " };
        match rl.readline(prompt) {
            Ok(line) => {
                if line.starts_with(':') {
                    handle_meta_command(&line, &mut session);
                    continue;
                }
                session.push_line(&line);
                if session.input_is_complete() {
                    match session.eval_input() {
                        Ok(Some(value)) => println!("{}", value),
                        Ok(None) => {},
                        Err(diagnostics) => render_diagnostics(&diagnostics),
                    }
                    session.clear_input();
                }
            }
            Err(_) => break,
        }
    }
}
```

### Phase 2: Multiline Input Detection

Aster is indent-based, so multiline detection is critical:

**Incomplete input signals:**
- Line ends with `:` (block opener: `def`, `class`, `if`, `while`, `for`, `match`)
- Line ends with trailing comma (line continuation)
- Unmatched `(`, `[`, `{`
- Line ends with an operator (`+`, `-`, `and`, `or`, etc.)

```rust
impl CompilerSession {
    pub fn input_is_complete(&self) -> bool {
        let input = self.pending_input();
        // Quick heuristic check before attempting a full parse
        if input.ends_with(':') || input.ends_with(',') { return false; }
        if has_unmatched_brackets(input) { return false; }
        // Try parsing -- if it fails with "unexpected EOF", input is incomplete
        match try_parse(input) {
            Ok(_) => true,
            Err(e) if e.is_incomplete() => false,
            Err(_) => true,  // real error, not just incomplete
        }
    }
}
```

### Phase 3: Meta Commands

| Command | Action |
|---------|--------|
| `:quit` / `:q` | Exit REPL |
| `:help` / `:h` | Show help |
| `:type <expr>` / `:t` | Show inferred type without evaluating |
| `:info <name>` / `:i` | Show type, definition location, documentation |
| `:reset` | Clear all state, start fresh |
| `:load <file>` | Load and execute a .aster file |
| `:clear` | Clear screen |
| `:env` | Show all bindings in current scope |

### Phase 4: Polish

**4A. Syntax highlighting**

Use `rustyline`'s `Highlighter` trait to colorize input as the user types. Re-use the lexer to tokenize the current line and apply ANSI colors:
- Keywords: bold/blue
- Strings: green
- Numbers: cyan
- Comments: gray
- Errors: red underline

**4B. Tab completion**

Use `rustyline`'s `Completer` trait. Source completions from:
- `TypeEnv` -- all bound names (variables, functions, classes, traits)
- Keywords
- Meta commands (`:` prefix)
- Class fields/methods (after `.`)

**4C. Special variables**

- `_` or `it` -- bound to the result of the last expression
- `_1`, `_2`, ... -- history of previous results

**4D. REPL-specific type rules**

In REPL mode, allow:
- Redefinition of `let` bindings with a different type (shadow, don't error)
- Top-level expressions (implicitly print their value)
- Bare `class` and `def` definitions (add to persistent env)

## Files Modified/Created

| File | Changes |
|------|---------|
| `src/repl.rs` | NEW -- REPL driver |
| `src/session.rs` | NEW -- CompilerSession |
| `src/main.rs` | Add `--repl` flag or default to REPL when no file given |
| `parser/src/lib.rs` | Add `parse_repl_input` method |
| `Cargo.toml` | Add `rustyline` dependency |

## Integration with CLI

```
asterc                    # no args -> launch REPL
asterc file.aster         # compile/run file
asterc --repl             # explicit REPL flag
asterc -e "1 + 2"         # eval single expression
```

## Verification

- REPL launches, accepts input, shows results
- Multiline blocks (def, class, if) work with `...` continuation prompt
- Variables persist across inputs
- Functions defined in one input can be called in the next
- Errors are displayed inline without crashing the session
- `:type`, `:info`, `:reset`, `:load` all work
- Tab completion suggests bound names
- Up-arrow recalls full multiline blocks
- Syntax highlighting works in real-time

### Phase 5: Agent-Readable REPL Mode (MCP / LLM Integration)

The REPL must support a **non-interactive JSON mode** so an Aster MCP server can drive it programmatically and relay results to AI agents.

**5A. `--repl --output-format json` mode**

When launched with JSON output, the REPL reads NDJSON commands from stdin and writes NDJSON results to stdout:

```
asterc --repl --output-format json
```

**Input (one JSON object per line):**

```json
{"action": "eval", "input": "let x = 1 + 2"}
{"action": "eval", "input": "x * 10"}
{"action": "type", "input": "x"}
{"action": "env"}
{"action": "reset"}
{"action": "load", "file": "examples/01_hello.aster"}
{"action": "ast", "input": "1 + 2"}
```

**Output (one JSON object per line):**

```json
{"ok": true, "action": "eval", "type": "Int", "value": null, "diagnostics": []}
{"ok": true, "action": "eval", "type": "Int", "value": 30, "diagnostics": []}
{"ok": true, "action": "type", "name": "x", "type": "Int"}
{"ok": true, "action": "env", "bindings": [{"name": "x", "type": "Int"}]}
{"ok": true, "action": "reset"}
{"ok": false, "action": "eval", "diagnostics": [{"severity": "error", ...}]}
{"ok": true, "action": "ast", "ast": {"node": "BinaryOp", ...}}
```

**5B. `:ast` and `:tokens` meta commands (interactive mode)**

For human debugging and agent introspection from the interactive REPL:

| Command | Action |
|---------|--------|
| `:ast <expr>` | Pretty-print AST of expression |
| `:tokens <expr>` | Show token stream |
| `:ast --json <expr>` | Dump AST as JSON |
| `:hir <expr>` | Show HIR after lowering (once codegen exists) |
| `:clif <expr>` | Show Cranelift IR (once codegen exists) |

Example:
```
>>> :ast 1 + 2 * 3
BinaryOp
  left: Int(1)
  op: Add
  right: BinaryOp
    left: Int(2)
    op: Mul
    right: Int(3)
```

**5C. Session state serialization**

The REPL session state (all bindings and their types) is queryable as JSON for agent context:

```json
{"action": "env"}
// Response:
{
  "ok": true,
  "action": "env",
  "bindings": [
    {"name": "x", "type": "Int", "line": 1},
    {"name": "greet", "type": "(String) -> Void", "line": 3},
    {"name": "User", "type": "class User { name: String, age: Int }", "line": 5}
  ]
}
```

## Dependency Chain

```
diagnostics.md (spans, structured errors, JSON output)  ← done
    |
fir/ crate (FIR types, lowering)                       ← from codegen.md
    |
codegen/ JIT backend (FIR → CLIF → machine code)       ← from codegen.md
    |
repl.md Phase 1 (core loop with FIR + JIT)
    |
repl.md Phase 2-4 (multiline, meta commands, polish)
    |
repl.md Phase 5 (agent-readable JSON REPL mode)
```

The REPL requires FIR + JIT. There is no typecheck-only mode — the REPL executes code or it doesn't ship.
