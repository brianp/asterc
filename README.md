# Aster

![aster](aster.png)

Writing code should feel good. Not wrestling-with-the-type-system good. Not finally-got-the-semicolons-right good. Actually good, where you think the thing, write the thing, and the thing works.

Aster is an opinionated language that gets out of your way. You get safety, strong types, and real error handling without the ceremony that usually comes with them. The syntax is small. The rules are strict but not annoying. There's one way to do most things, and that one way is the obvious one.

It's also built for a world where AI writes code alongside you. The compiler emits structured data, not prose error messages, so when your AI tools try to fix something they're working with facts instead of guessing from text.

## What it looks like

```
def main()
  log(message: "Hello")
  if true
    log(message: "Yes")
  else
    log(message: "No")
```

No braces, no semicolons. Indentation does the work. If you've written Python or Ruby, this already makes sense.

```
class NetworkError extends Error
  url: String

def fetch(url: String) throws NetworkError -> String
  throw NetworkError(message: "timeout", url: url)

def load(url: String) throws AppError -> String
  fetch(url)!
```

Errors are typed and visible at the call site. `!` means "pass it up." No try/catch, no `if err != nil` boilerplate. You see exactly where things can fail by reading the code.

```
let message = match status
  200 => "OK"
  404 => "Not Found"
  _ => "Unknown"
```

Pattern matching, async that isn't viral (`async f()` at the call site, not in the function signature), traits, generics, nullable types that force you to deal with them. Strong opinions, fewer decisions for you to make.

## Why

Most languages make you choose: you get safety or you get a short learning curve. You get powerful types or you get readable code. Aster doesn't think those are real tradeoffs.

- One syntax for each concept. You don't learn three ways to write a function and then pick a favorite.
- Errors are part of the type system but they're not heavy. `throws`/`!` reads like English.
- Async isn't a color that infects your whole codebase. The caller decides, not the function.
- Nullable types (`T?`) have exactly four operations. You can't ignore them and you can't get clever with them.
- Max 3 function arguments (planned, not yet enforced). More than that and you use a struct. The language pushes you toward clean APIs by default.

The goal is that you spend your time thinking about what the code should do, not fighting the language to express it.

## Status

The compiler works end-to-end for a growing slice of the language. The front-end (lexer, parser, type checker) is well ahead of the back-end (codegen, runtime), so `check` accepts more programs than `run` or `build` can execute today. That gap is shrinking with each release.

```
asterc check examples/spec/12_async_errors_matching.aster   # type-check only
asterc run examples/executable/hello.aster                  # JIT compile and run
asterc build examples/executable/hello.aster -o hello       # produce a native binary
```

### What works today

**Full pipeline (check + run + build):**
Functions, arithmetic, conditionals, recursion, classes, single inheritance, string interpolation, closures, lists, maps, pattern matching on literals, for/while loops, basic I/O, and the `examples/executable/*` contract programs.

**Front-end only (check):**
Generics with constraints, traits and protocols (Eq, Ord, Printable, Iterable, From/Into), async/blocking/resolve, typed error handling (throws/catch), nullable types, modules with selective and wildcard imports, re-exports, and the full `examples/spec/*` tour.

**Formatter:**
`asterc fmt` ships with a Wadler-Lindig pretty printer, comment preservation, magic trailing comma, import sorting, and `--check`/`--diff`/`--stdin` modes.

### What's next

The big-ticket items right now are closing the remaining codegen gaps (so more of the front-end surface actually executes), building out the standard library, and getting a testing story in place. After that: REPL, LSP, and an MCP server that gives AI agents direct access to compiler internals.

## Build and run

You'll need a Rust toolchain and a C compiler (for the runtime).

```
cargo build
cargo test
```

The test suite has 1500+ tests across the workspace. If they all pass, you're good.

## Project layout

```
lexer/       Tokenizer, indent/dedent handling
ast/         AST nodes, types, type environment, diagnostics
parser/      Recursive descent + Pratt precedence
typecheck/   Type inference, unification, generics, traits, modules
fir/         Flat intermediate representation (FIR) lowering
codegen/     Cranelift JIT + AOT backends, C runtime, GC
aster-fmt/   Opinionated formatter
src/         Compiler driver (check/run/build)
tests/       Integration tests (organized by feature)
examples/    Executable contracts + front-end-only spec examples
docs/design/ Design RFCs
```

## Features

**Syntax and basics:**
Indent-based (no braces, no semicolons), functions with named arguments, classes with single inheritance (`extends`), traits (`includes`), closures with capture and type inference, pattern matching (`match`/`=>`), control flow (`while`, `for`, `break`, `continue`, `elif`).

**Type system:**
Generics with constraints (`T extends Class`, `T includes Trait`), parametric traits (`trait From[T]`), auto-derivable protocols (Eq, Ord, Printable, Iterable, From/Into), nullable types (`T?`) with `.or()`, `.or_else()`, `.or_throw()`.

**Error handling:**
`throws` declarations, `throw`, `!` propagation, `!.or(default)`, `!.or_else(-> expr)`, `!.catch` with typed arms. Errors are part of the type system but they read like English.

**Async:**
Call-site async (`async f()` returns `Task[T]`, `blocking f()` for suspendable calls, `resolve task!` to consume). The function doesn't decide if it's async, the caller does.

**Modules:**
`use`/`pub` imports, selective and wildcard, namespace imports (`as`), re-exports (`pub use`), virtual stdlib with prelude.

**Diagnostics:**
Structured `Diagnostic` type with spans, error codes (L/P/E series), Ariadne rendering, did-you-mean suggestions, parser recovery, multi-error accumulation.

## Codegen

The back-end compiles through Cranelift in two modes:

- **JIT** (`asterc run`): Compiles in-memory and executes immediately.
- **AOT** (`asterc build`): Emits an object file, links against a C runtime, produces a native binary.

Both share the same FIR (Flat Intermediate Representation) lowering and C runtime source, so they don't drift apart. Memory management is a non-moving mark-and-sweep GC with shadow stack root tracking.

## Design decisions

All the "why" lives in the design RFCs:

- [Language Philosophy](docs/design/language-philosophy.md)
- [Error Handling](docs/design/error-handling-rfc.md) — `throws`/`!`/`T?`
- [Async](docs/design/async-rfc.md) — green threads, channels, mutexes
- [Type System](docs/design/type-system-rfc.md) — inheritance, traits, generics, the 3-arg rule
- [Protocols](docs/design/protocols-rfc.md) — Eq, Ord, Printable, Iterable, From/Into
- [Closures](docs/design/closures-rfc.md) — capture, lambda lifting
- [Modules](docs/design/modules-rfc.md) — imports, namespacing, re-exports
- [Introspection](docs/design/introspection-rfc.md) — runtime type info, Ruby-inspired

## Contributing

File issues using the templates. Pick Bug, Feature Request, or Gap depending on what you're reporting.

### Issue labels

| Label | What it's for |
|-------|---------------|
| `bug` | Something that's broken |
| `feature` | New language feature or capability |
| `gap` | Specced or planned but not yet implemented |
| `soundness` | Type system or runtime correctness |
| `security` | Security concern or hardening |
| `critical` | Must fix before next milestone |
| `high` | Important, address soon |
| `medium` | Should get done, not urgent |
| `low` | Nice to have, no rush |
| `type-system` | Type checker, inference, generics, traits |
| `codegen` | JIT, AOT, FIR lowering, runtime |
| `tooling` | CLI, formatter, LSP, REPL |
| `async` | Concurrency, channels, tasks |
| `error-handling` | throws, catch, propagation, Error types |
| `stdlib` | Standard library modules and builtins |
| `parser` | Parsing, syntax, lexer |
| `rfc` | Tied to a specific RFC or design doc |

## License

[MIT](LICENSE)
