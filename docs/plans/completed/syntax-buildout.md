# Plan: Build Out Aster Language Syntax

## Context

The Aster compiler has a working lex → parse → typecheck pipeline, but the expression parser is essentially primary-only. It handles literals and bare identifiers but **cannot parse binary operators, function calls with arguments, member access, or return statements**. The existing example files (02-04) likely fail because they use `+` and call syntax. The compiler needs a real expression parser before any other language features can be built.

## Current State Summary

**What works:** class defs, function defs (sync/async), let bindings, if/else, literals, identifiers
**What's broken/missing:** operators (+,-,*,/,==,etc.), function calls with args, member access, return statements, loops, collections, modules, generics

## Plan: 6 Phases (Start with Phase 1)

### Phase 1: Operators, Calls, Member Access ← **BUILD THIS NOW**

This is the critical path. Everything else depends on a working expression parser.

**1A. Lexer** (`lexer/src/lib.rs`) — Add 14 token kinds:
- Arithmetic: `Plus`, `Minus`, `Star`, `Slash`, `Percent`, `StarStar` (`**` exponent)
- Comparison: `EqualEqual`, `BangEqual`, `Less`, `Greater`, `LessEqual`, `GreaterEqual`
- Logical keywords: `And`, `Or`, `Not`
- Fix bare `-` bug (currently silently dropped; should emit `Minus` unless followed by `>`)

**1B. AST** (`ast/src/expr.rs`) — Add nodes:
- `Expr::BinaryOp { left, op, right }` with `BinOp` enum (Add, Sub, Mul, Div, Mod, Pow, Eq, Neq, Lt, Gt, Lte, Gte, And, Or)
- `Expr::UnaryOp { op, operand }` with `UnaryOp` enum (Neg, Not)
- Re-export from `ast/src/lib.rs`

**1C. Parser** (`parser/src/lib.rs`) — Replace `parse_expr` with precedence-climbing:

| Precedence | Operators | Associativity |
|-----------|-----------|---------------|
| 1 (lowest) | `or` | Left |
| 2 | `and` | Left |
| 3 | `== !=` | Left |
| 4 | `< > <= >=` | Left |
| 5 | `+ -` | Left |
| 6 | `* / %` | Left |
| 7 | `**` (exponent) | Right |
| 8 | Unary `- not` | Prefix |
| 9 (highest) | Call `()`, Member `.` | Postfix |

Follows PEMDAS: Parentheses (in parse_primary) > Exponents > Multiply/Divide > Add/Subtract > Comparisons > Logic.
Note: `**` is **right-associative** — `2 ** 3 ** 2` = `2 ** (3 ** 2)` = 512, matching math convention and Ruby/Python.

Methods: `parse_expr` → `parse_or` → `parse_and` → `parse_equality` → `parse_comparison` → `parse_additive` → `parse_multiplicative` → `parse_exponent` → `parse_unary` → `parse_postfix` → `parse_primary`

Also: add `Return` statement parsing in `parse_stmt`, add grouped expressions `(expr)` in `parse_primary`.

**1D. Type Checker** (`typecheck/src/typechecker.rs`):
- BinaryOp: arithmetic including `**` (Int,Int→Int; Float,Float→Float; Int↔Float→Float; String+String→String), comparison (same types→Bool), logical (Bool,Bool→Bool)
- Type checker is a **whitelist** — no rule = compile error. No implicit coercions, no NaN, no silent nil. The opposite of JS.
- UnaryOp: Neg (Int→Int, Float→Float), Not (Bool→Bool)

**1E. Verify** all 5 example files parse and typecheck. Fix `hello.aster` if `log` needs parens.

### Phase 2: Control Flow (while, for, elif, assignment)

- Lexer: `While`, `For`, `In`, `Elif`, `Break`, `Continue`, `LBracket`, `RBracket`
- AST: `Stmt::While`, `Stmt::For`, `Stmt::Assignment`, `Stmt::Break`, `Stmt::Continue`, add `elif_branches` to `Stmt::If`
- Parser: `parse_while`, `parse_for`, modify `parse_if` for elif chains, assignment detection after expression
- Type checker: While cond must be Bool, assignment target type must match value

### Phase 3: Collections and Type Annotations on Let

- Types: `List(Box<Type>)`, `Map(Box<Type>, Box<Type>)`
- AST: `Expr::ListLiteral`, `Expr::Index`
- Parser: `[1, 2, 3]` syntax, `xs[0]` indexing, `let x: Int = 5` annotations, type syntax parser for `List[Int]`
- Type checker: list element consistency, index types, for-in over List[T]

### Phase 4: Modules, Imports, Builtins

- Lexer: `Use`, `Pub` keywords
- AST: `Stmt::Use`, `is_public` on defs
- Parser: `use std/http` (imports entire namespace), `use std/http { Request, Response }` (selective imports), `pub` modifier
- Visibility: **private by default**, `pub` to export. Applied consistently to classes, fields, methods, top-level functions.
- Builtins: pre-populate TypeEnv with `log`, `print`, `len`, etc.
- Multi-file compilation in `main.rs`

### Phase 5: Generics and Traits

- Types: `Generic`, `TypeVar`
- AST: generic params on Class/functions, `Stmt::Trait`, `Includes` on class definitions
- Parser:
  - Generics: `class Stack[T]`, `def map[T, U](...)`
  - Traits with default implementations:
    ```aster
    pub trait Printable
      def to_string() -> String        # required
      def print()                       # default impl
        log(to_string())
    ```
  - Class includes (in definition line): `pub class User includes Printable, Serializable`
  - Multiple includes with line continuation (trailing comma):
    ```aster
    pub class User includes Printable, Serializable,
                            Validatable, Cacheable
    ```
- Conflict resolution: if two included traits define the same method, only the conflicting methods get namespaced (`user.validatable.validate()`). Non-conflicting methods stay flat.
- Type checker: generic instantiation, unification, trait constraints
- Line continuation rule: **trailing comma suppresses newline** — applies everywhere (includes, params, lists)

### Phase 6: Call-Site Async, Error Handling, Pattern Matching

**Async (call-site model — no `await` keyword):**
- Lexer: `Blocking`, `Detached` keywords (lexer already has `Async`)
- Three invocation modes:
  - `f()` — sync, compiler enforces no suspension inside
  - `async f()` — spawns task, returns `Task[T]`
  - `blocking f()` — runs and waits, thread parks until done. **Resolves suspension** — parent function stays sync.
  - `detached async f()` — fire-and-forget, explicit opt-out of structured concurrency
- `async scope` blocks for structured concurrency (tasks must complete or cancel before scope exits)
- Compiler infers which functions may suspend based on their children's call styles
- `blocking` is a firewall — it stops async propagation. Parent function is sync.
- Must-consume: dropping a `Task[T]` without consuming or detaching is a compile error
- Callbacks: typed as sync `(A) -> B`. If caller needs async internally, they wrap with `blocking`. Library authors don't need effect polymorphism.

**Error Handling:**
- `try`/`catch` or equivalent
- `Result[T, E]` type
- `?` operator equivalent for error propagation

**Pattern Matching:**
- `match` keyword
- `Stmt::Match` with pattern arms
- Patterns: literal, identifier binding, wildcard `_`, destructuring
- Exhaustiveness checking in type checker

## Dependency Graph

```
Phase 1 (operators, calls) ← CRITICAL PATH
    ↓
Phase 2 (loops, control flow)
    ↓
Phase 3 (collections, type annotations)
    ↓
Phase 4 (modules)  ←→  Phase 5 (generics)  [parallel]
                  ↘    ↙
              Phase 6 (async, match, errors)
```

## Files Modified Per Phase

| File | P1 | P2 | P3 | P4 | P5 | P6 |
|------|----|----|----|----|----|----|
| `lexer/src/lib.rs` | +14 tokens, fix `-` | +8 tokens | — | +2 tokens (Use, Pub) | — | +2 tokens |
| `ast/src/expr.rs` | +BinaryOp, +UnaryOp | +While, +For, +Assignment, +Break, +Continue | +ListLiteral, +Index | +Import, +pub | +Trait, generics | +Await, +Match |
| `ast/src/types.rs` | — | — | +List, +Map | — | +Generic, +TypeVar | +Result |
| `parser/src/lib.rs` | New precedence parser (~150 LOC) | +loops, +elif, +assignment | +list/index, +type parser | +import, +pub | +generics syntax | +await, +match |
| `typecheck/src/typechecker.rs` | +BinaryOp, +UnaryOp rules | +While, +For, +Assignment | +List/Index checks | +module resolution | +unification | +async, +match |
| `src/main.rs` | — | — | — | multi-file | — | — |

## Verification

After Phase 1:
- `cargo test` passes across all crates
- All 5 existing example files parse and typecheck successfully
- New test cases cover: operator precedence, function calls, member access, unary ops, grouped expressions, return statements
- Run `cargo run -- examples/03_simple_function.aster` and get "Type checking passed"
