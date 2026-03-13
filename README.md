# Aster

![aster](aster.png)

Writing code should feel good. Not wrestling-with-the-type-system good. Not finally-got-the-semicolons-right good. Actually good, where you think the thing, write the thing, and the thing works.

Aster is an opinionated language that gets out of your way. You get safety, strong types, and real error handling without the ceremony that usually comes with them. The syntax is small. The rules are strict but not annoying. There's one way to do most things, and that one way is the obvious one.

It's also built for a world where AI writes code alongside you. The compiler emits structured data, not prose error messages, so when your AI tools try to fix something they're working with facts instead of guessing from text.

## What it looks like

```
def main() -> Void
  log("Hello")
  if true
    log("Yes")
  else
    log("No")
```

No braces, no semicolons. Indentation does the work. If you've written Python or Ruby, this already makes sense.

```
class NetworkError extends Error
  url: String

def fetch(url: String) throws NetworkError -> String
  throw NetworkError("timeout", url)

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

Pattern matching, async that isn't viral (`async f()` at the call site, not in the function signature), traits, generics, nullable types that force you to deal with them. Some of that is still front-end-only in the executable pipeline today. Strong opinions, fewer decisions for you to make.

## Why

Most languages make you choose: you get safety or you get a short learning curve. You get powerful types or you get readable code. Aster doesn't think those are real tradeoffs.

- One syntax for each concept. You don't learn three ways to write a function and then pick a favorite.
- Errors are part of the type system but they're not heavy. `throws`/`!` reads like English.
- Async isn't a color that infects your whole codebase. The caller decides, not the function.
- Nullable types (`T?`) have exactly four operations. You can't ignore them and you can't get clever with them.
- Max 3 function arguments. More than that and you use a struct. The language pushes you toward clean APIs by default.

The goal is that you spend your time thinking about what the code should do, not fighting the language to express it.

## Status

The compiler works end-to-end for a small executable slice today. `check` is ahead of `run` and `build`, and the repo now treats that as an explicit contract instead of pretending the whole surface executes.

```
asterc check examples/spec/12_async_errors_matching.aster   # front-end only
asterc run examples/executable/hello.aster                  # JIT compile and run
asterc build examples/executable/hello.aster                # produce a native binary
```

### Execution support matrix

| Surface | `check` | `run` | `build` |
|---------|---------|-------|---------|
| Basic functions, arithmetic, conditionals, recursion | Yes | Yes | Yes |
| `examples/executable/*` contract programs | Yes | Yes | Yes |
| Collections, modules, traits, async/error tours in `examples/spec/*` | Yes | Not yet | Not yet |
| Unsupported executable paths | N/A | Explicit `E028` diagnostic | Explicit `E028` diagnostic |

## Build and run

You'll need a Rust toolchain and a C compiler (for the runtime).

```
cargo build
cargo test
```

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
tests/       Integration tests
examples/    executable contracts plus front-end-only spec examples
docs/design/ Design RFCs
```

## Features

- Indent-based syntax (no braces, no semicolons)
- Functions, classes, single inheritance (`extends`), traits (`includes`)
- Generics with constraints (`T extends Class`, `T includes Trait`)
- Pattern matching (`match`/`=>`) in the front-end, executable support in progress
- Error handling: `throws`/`throw`/`!`, `!.or()`, `!.or_else()`, `!.catch` in the front-end, executable support in progress
- Nullable types (`T?`) with `.or()`, `.or_else()`, `.or_throw()`, `match`
- Call-site async: `async f()` returns `Task[T]`, `resolve` to wait, executable support in progress
- Closures with capture and type inference
- Protocols: Eq, Ord, Printable, Iterable, From/Into (auto-derivable)
- Lists, maps, indexing
- Modules (`use`/`pub`), selective and wildcard imports, re-exports
- Virtual stdlib with prelude (`use std/cmp { Eq, Ord }`, etc.)
- Named arguments everywhere, order independent
- Control flow: `while`, `for`, `break`, `continue`, `elif`
- Structured diagnostics with span-based error reporting

## Codegen

The backend compiles FIR (a flat intermediate representation) through Cranelift. Two modes:

- **JIT** (`asterc run`): Compiles in-memory and executes immediately. Good for development.
- **AOT** (`asterc build`): Emits an object file, links against a C runtime, produces a native binary.

The runtime contract for those backends now lives in `codegen`, including the embedded C runtime source used by `build`, so the JIT and AOT paths stop drifting apart.

Memory management uses a non-moving mark-and-sweep garbage collector with a shadow stack for root tracking. Lists and maps use handle-based indirection so reallocation doesn't invalidate references.

## Design decisions

All the "why" lives in the design RFCs:

- [Language Philosophy](docs/design/language-philosophy.md)
- [Error Handling](docs/design/error-handling-rfc.md) - `throws`/`!`/`T?`
- [Async](docs/design/async-rfc.md) - green threads, channels, mutexes
- [Type System](docs/design/type-system-rfc.md) - inheritance, traits, generics, the 3-arg rule
- [Protocols](docs/design/protocols-rfc.md) - Eq, Ord, Printable, Iterable, From/Into
- [Closures](docs/design/closures-rfc.md) - capture, lambda lifting
- [Modules](docs/design/modules-rfc.md) - imports, namespacing, re-exports
- [Introspection](docs/design/introspection-rfc.md) - runtime type info, Ruby-inspired

## What's next

A REPL, LSP support, an opinionated formatter, and an MCP server that gives AI agents direct access to compiler artifacts.

## License

[MIT](LICENSE)
