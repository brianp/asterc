# Aster

<p align="center"><img src="aster.png" width="120" alt="aster"></p>

Writing code should feel good. Not wrestling-with-the-type-system good. Not finally-got-the-semicolons-right good. Actually good, where you think the thing, write the thing, and the thing works.

Aster is an opinionated language that gets out of your way. You get safety, strong types, and real error handling without the ceremony that usually comes with them. The syntax is small. The rules are strict but not annoying. There's one way to do most things, and that one way is the obvious one.

It's also built for a world where AI writes code alongside you. One syntax per concept means less for a model to get wrong, and named arguments mean a transposed call doesn't silently compile. The compiler emits structured diagnostics with stable codes, so tools work with facts instead of guessing from prose.

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

Errors are typed and visible at the call site. `!` means "pass it up." No try/catch pyramids, no `if err != nil` boilerplate. You see exactly where things can fail by reading the code.

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
- Named arguments everywhere, so `resize(width: 100, height: 50)` can't silently become `resize(50, 100)`. A callee that declares a single parameter takes a label-free argument ([#52](https://github.com/brianp/asterc/issues/52)); two or more always need names.

The goal is that you spend your time thinking about what the code should do, not fighting the language to express it.

## Status

The compiler runs end-to-end: lexer, parser, type checker, FIR lowering, and a Cranelift back-end with both JIT and native AOT builds sharing one Rust runtime. Most of the language executes today, including the parts that usually lag: generics, protocols with auto-derive, typed catch dispatch, closures, and the whole green-thread concurrency stack (spawn/resolve, channels, mutexes, work-stealing scheduler, I/O poller). The few constructs that type-check but don't lower yet fail with a clean "not executable yet" diagnostic instead of a crash.

The audited, code-verified breakdown lives in the docs site: [Implementation Status](docs/src/content/docs/reference/status.mdx) for what's done, [Roadmap](docs/src/content/docs/reference/roadmap.mdx) for what isn't.

```
asterc check examples/spec/12_async_errors_matching.aster   # type-check only
asterc run examples/executable/hello.aster                  # JIT compile and run
asterc build examples/executable/hello.aster -o hello       # produce a native binary
asterc fmt src/                                             # format in place
```

### What's next

The testing story (`asterc test` plus `std/test`), the package manager's resolver, the REPL (the eval-with-persistent-context core already exists; it needs a driver), std networking, and an LSP. The docs Roadmap has the full list with honest verdicts.

## Build and run

You'll need a Rust toolchain. A C compiler is only used as the linker driver for `asterc build`; the runtime itself is Rust, compiled into the workspace.

```
cargo build -p codegen   # integration tests link the runtime staticlib
cargo test
```

For the docs site:

```
cd docs && pnpm install && pnpm dev
```

## Project layout

```
lexer/       Tokenizer, indent/dedent handling
ast/         AST nodes, types, diagnostics with typed templates
parser/      Recursive descent + table-driven precedence
typecheck/   Type inference, unification, generics, traits, modules
fir/         Flat intermediate representation (FIR) lowering
codegen/     Cranelift JIT + AOT, Rust runtime, green threads, GC
aster-fmt/   Opinionated formatter
aster-pkg/   Package manager CLI, written in Aster (Seedfile DSL)
src/         Compiler driver (check/run/build/fmt/clean)
tests/       Integration tests, one module per feature
examples/    Executable contracts + front-end-only spec examples
docs/        Starlight docs site: language docs, internals, RFCs, plans
```

## Features

**Syntax and basics:**
Indent-based (no braces, no semicolons), functions with named arguments, classes with single inheritance (`extends`), traits (`includes`), closures with capture and type inference, pattern matching (`match`/`=>`) with enum destructuring, ranges (`1..10`, `1..=10`), control flow (`while`, `for`, `break`, `continue`, `elif`).

**Type system:**
Generics with constraints (`T extends Class`, `T includes Trait`), parametric traits (`trait From[T]`), auto-derivable protocols (Eq, Ord, Printable, Iterable, From/Into), dynamic dispatch (`DynamicReceiver`/`method_missing`), nullable types (`T?`) with `.or()`, `.or_else()`, `.or_throw()`.

**Error handling:**
`throws` declarations, `throw`, `!` propagation, `!.or(default)`, `!.or_else(-> expr)`, `!.catch` with typed arms that dispatch on the error's actual type at runtime.

**Concurrency:**
Call-site async (`async f()` returns `Task[T]`, `blocking` for suspendable calls, `resolve task!` to consume), M:N green threads with work stealing and safepoint preemption, `Mutex[T]`, `Channel[T]`, must-consume task tracking, `Drop`/`Close` cleanup on every scope exit (cancellation-path cleanup is [#51](https://github.com/brianp/asterc/issues/51)).

**Standard library:**
Virtual stdlib with prelude plus `std/cmp`, `std/fmt`, `std/collections`, `std/convert`, `std/random`, `std/sys`, `std/fs`, `std/process`, `std/crypto`, and gated `std/runtime` (JIT eval) and `std/unstable`.

**Diagnostics:**
Structured diagnostics with typed templates and stable codes (L/P/E/M/W series), Ariadne rendering, did-you-mean suggestions, parser recovery, multi-error accumulation.

## Codegen

The back-end compiles through Cranelift in two modes:

- **JIT** (`asterc run`): compiles in-memory and executes immediately.
- **AOT** (`asterc build`): emits an object file and links it against the same Rust runtime, built as a static library. Incremental builds are cached under `.aster/build/` with content hashing.

Both modes share one FIR lowering, one translation layer, and one runtime, so they can't drift apart. Memory management is a non-moving mark-and-sweep GC with shadow-stack roots and precise tracing of pointer fields.

## Design docs

The "why" lives in the docs site. Summaries under RFCs, complete texts under Full RFCs:

- [Language Philosophy](docs/src/content/docs/rfcs/full/language-philosophy.mdx)
- [Error Handling](docs/src/content/docs/rfcs/full/error-handling.mdx) covering `throws`, `!`, and `T?`
- [Concurrency and Async](docs/src/content/docs/rfcs/full/async.mdx) covering green threads, channels, mutexes
- [Type System](docs/src/content/docs/rfcs/full/type-system.mdx) covering inheritance, traits, generics
- [Standard Protocols](docs/src/content/docs/rfcs/full/protocols.mdx) covering Eq, Ord, Printable, Iterable, From/Into, and the named-arguments rule
- [Closures](docs/src/content/docs/rfcs/full/closures.mdx) covering capture and lambda lifting
- [Modules and Imports](docs/src/content/docs/rfcs/full/modules.mdx)
- [Introspection](docs/src/content/docs/rfcs/full/introspection.mdx) covering runtime type info

The compiler's own internals (lexer through codegen, the runtime, the GC) are documented under Compiler Internals on the docs site, verified against the code they describe.

## Contributing

File issues using the templates. Pick Bug, Feature Request, or Gap depending on what you're reporting.

### Issue labels

| Label            | What it's for                              |
| ---------------- | ------------------------------------------ |
| `bug`            | Something that's broken                    |
| `feature`        | New language feature or capability         |
| `gap`            | Specced or planned but not yet implemented |
| `soundness`      | Type system or runtime correctness         |
| `security`       | Security concern or hardening              |
| `critical`       | Must fix before next milestone             |
| `high`           | Important, address soon                    |
| `medium`         | Should get done, not urgent                |
| `low`            | Nice to have, no rush                      |
| `type-system`    | Type checker, inference, generics, traits  |
| `codegen`        | JIT, AOT, FIR lowering, runtime            |
| `tooling`        | CLI, formatter, LSP, REPL                  |
| `async`          | Concurrency, channels, tasks               |
| `error-handling` | throws, catch, propagation, Error types    |
| `stdlib`         | Standard library modules and builtins      |
| `parser`         | Parsing, syntax, lexer                     |
| `rfc`            | Tied to a specific RFC or design doc       |

## License

[MIT](LICENSE)
