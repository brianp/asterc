# Low-Entropy Syntax Benchmark

We claim "low-entropy syntax" on the website. Here's what we measured and what we need to maintain.

## Token Vocabulary Size

Counted as distinct TokenKind variants in each language's lexer/spec (keywords + operators/punctuation/delimiters + literal types).

| Language       | Keywords | Operators/Punct | Literal Types | Total |
|----------------|----------|-----------------|---------------|-------|
| **Aster**      | **31**   | **31**          | **4**         | **70** |
| Go             | 25       | 47              | 5             | 77    |
| Swift          | ~72      | 17              | 5             | ~94   |
| Ruby           | 41       | ~46             | 8             | ~95   |
| Python         | 39       | 52              | 5             | 96    |
| Rust           | 55       | 47              | 10            | 112   |
| TypeScript     | ~63      | 48              | 7             | ~118  |

**Aster's current count: 70.** That's 9% fewer than Go, which is famously minimal.

Sources for comparison numbers:
- Go: [Go Language Specification](https://go.dev/ref/spec)
- Python: [Python 3.12 Lexical Analysis](https://docs.python.org/3.12/reference/lexical_analysis.html)
- Rust: [Rust Reference - Tokens](https://doc.rust-lang.org/reference/tokens.html)
- TypeScript: [ECMAScript 2024 Spec](https://tc39.es/ecma262/multipage/ecmascript-language-lexical-grammar.html)
- Ruby: [Ruby 3.3 Keywords](https://docs.ruby-lang.org/en/3.3/keywords_rdoc.html)
- Swift: [Swift Lexical Structure](https://docs.swift.org/swift-book/documentation/the-swift-programming-language/lexicalstructure/)

## Hard Rule: Stay Under Go

Go has 77 token kinds. We must stay below that. Every new keyword or operator needs to justify itself against this budget. We currently have 7 tokens of headroom.

When adding a new token kind, ask:
1. Can an existing keyword or operator cover this?
2. Can this be a contextual keyword (parsed as Ident in most positions) instead of a reserved keyword?
3. Is the feature worth spending one of our 7 remaining slots?

## Syntactic Variance

Token count is one dimension. The other is how many ways you can express the same thing. Aster scores well here:

- **One function syntax.** No arrow functions, no shorthand, no anonymous-function-with-different-syntax.
- **One block syntax.** Indentation. No braces-vs-indent choice.
- **No optional terminators.** No semicolons to insert or omit.
- **Canonical formatting.** One way to format any program. The formatter is prescriptive.
- **No operator overloading.** Operators mean one thing.
- **3-arg limit.** Functions can't sprawl. Complex signatures become structs.

Go allows both `:=` and `var`, has optional semicolons (inserted by the lexer), and has multiple ways to declare variables. Python has lambda vs def, list comprehensions vs loops, and f-strings vs format() vs %. Aster avoids all of this.

## Future Measurement Ideas

These aren't done yet but would strengthen the claim:

- **LLM predictability:** Give Claude/GPT equivalent programs in each language, measure next-token prediction accuracy. Higher accuracy = lower entropy in practice.
- **Token-per-concept ratio:** Count tokens needed to express common patterns (define a function, handle an error, iterate a list) across languages. Fewer tokens per concept = more efficient encoding.
- **AST depth per LOC:** Shallower ASTs for the same functionality suggest simpler syntax.

## Current Token Budget (70/76)

Keywords (31): def, class, async, blocking, return, if, else, elif, while, for, in, break, continue, true, false, nil, let, use, as, pub, trait, enum, includes, extends, throw, throws, match, catch, resolve, detached, const

Operators/Punctuation (31): ( ) , : -> . .. ..= = + - * / % ** == != < > <= >= and or not [ ] { } ? ! =>

Literals (4): Int, Float, Str, Ident

Structure (4): Indent, Dedent, Newline, EOF

String interpolation (3): StringStart, StringMid, StringEnd (not counted in budget — implementation detail)
