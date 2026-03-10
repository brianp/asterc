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

Pattern matching, async that isn't viral (`async f()` at the call site, not in the function signature), traits, generics, nullable types that force you to deal with them. Strong opinions, fewer decisions for you to make.

## Why

Most languages make you choose: you get safety or you get a short learning curve. You get powerful types or you get readable code. Aster doesn't think those are real tradeoffs.

- One syntax for each concept. You don't learn three ways to write a function and then pick a favorite.
- Errors are part of the type system but they're not heavy. `throws`/`!` reads like English.
- Async isn't a color that infects your whole codebase. The caller decides, not the function.
- Nullable types (`T?`) have exactly four operations. You can't ignore them and you can't get clever with them.
- Max 3 function arguments. More than that and you use a struct. The language pushes you toward clean APIs by default.

The goal is that you spend your time thinking about what the code should do, not fighting the language to express it.

## Status

Early. The compiler front-end works (lexer, parser, type checker) but there's no codegen yet. It validates your program and tells you if it's correct. That's it for now.

212 tests passing, zero warnings.

```
cargo run -- examples/hello.aster
# => Type checking passed for examples/hello.aster
```

## Build and run

You'll need a Rust toolchain.

```
cargo build
cargo test
```

## Project layout

```
lexer/       Tokenizer, indent/dedent handling
ast/         AST nodes, types, type environment
parser/      Recursive descent + Pratt precedence
typecheck/   Type inference, unification, generics, traits
src/         Compiler driver (lex -> parse -> typecheck)
tests/       Integration tests
examples/    .aster files covering each language feature
docs/design/ Design RFCs
```

## Features (what's working)

- Indent-based syntax
- Functions, classes, single inheritance (`extends`), traits (`includes`)
- Generics with constraints
- Pattern matching (`match`/`=>`)
- Error handling: `throws`/`throw`/`!`, `!.or()`, `!.or_else()`, `!.catch`
- Nullable types (`T?`) with `.or()`, `.or_else()`, `.or_throw()`, `match`
- Call-site async: `async f()` returns `Task[T]`, `resolve` to wait, `detached async` for fire-and-forget
- Structured concurrency with `async scope`
- Lists, indexing, `List[T]`
- Modules (`use`/`pub`), builtins (`log`, `print`, `len`, `to_string`)
- Control flow: `while`, `for`, `break`, `continue`, `elif`
- Structured diagnostics with span-based error reporting

## Design decisions

All the "why" lives in the design RFCs:

- [Language Philosophy](docs/design/language-philosophy.md)
- [Error Handling](docs/design/error-handling-rfc.md) - `throws`/`!`/`T?`
- [Async](docs/design/async-rfc.md) - green threads, channels, mutexes
- [Type System](docs/design/type-system-rfc.md) - inheritance, traits, generics, the 3-arg rule
- [Introspection](docs/design/introspection-rfc.md) - runtime type info, Ruby-inspired

## What's next

Codegen with Cranelift, a REPL, LSP support, an opinionated formatter, and an MCP server that gives AI agents direct access to compiler artifacts.

## License

[MIT](LICENSE)
