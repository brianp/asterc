---
status: executed
created: 2026-03-20 22:00
executed: 2026-03-20
---

# Implementation Plan: Random Numbers and Range Type

## Prerequisites

- Existing polymorphic builtin infrastructure (`say`, `log`, `len`, `to_string`)
- Expected-type threading in the typechecker (used by `From[T]` resolution)
- Iterable trait and vocabulary methods (map, filter, reduce, etc.)
- `for` loop iterates over anything Iterable
- `..` is not currently a lexer token

## Codebase Analysis

- **Polymorphic builtins** are registered in `TypeChecker::new()` and type-checked in `check_call_inner()` in `typecheck/src/check_call.rs`. The typechecker matches on the function name and applies custom type logic. This is the pattern `random()` should follow.
- **Expected-type threading** already exists: `self.expected_type` is set from `let` type annotations and function parameter types. `From[T]` uses this to disambiguate which implementation to call. `random()` can use the same mechanism to determine Int vs Float vs Bool return type.
- **Iterable** vocabulary methods are injected by the typechecker when a class includes Iterable. The `each()` method is the only requirement; map/filter/reduce/etc. are auto-provided. Range needs to include Iterable and implement `each()`.
- **Runtime functions** are declared in `codegen/src/runtime_sigs.rs`, implemented in `codegen/src/runtime.rs` (Rust, for JIT) and `codegen/src/runtime_source.rs` (C, for AOT), and dispatched in `codegen/src/translate.rs`.
- **For loops** lower through FIR as `each()` calls on the iterable expression. Range just needs to be Iterable for `for` to work.

## Task Breakdown

### 1. Add `..` and `..=` tokens to the lexer

- **Files to modify:** `lexer/src/lib.rs`
- **Dependencies:** None
- **Approach:** Add `DotDot` and `DotDotEq` token kinds. Lex `..=` first (longer match wins), then `..`. Currently `.` is lexed as `Dot` — extend the `.` branch to check for a second `.`.
- **Key decisions:**
  - `..` is exclusive (does not include end), `..=` is inclusive — matches Rust convention, intuitive
  - These are infix operators, not standalone tokens

### 2. Add `Range` type and `..` expression to the AST

- **Files to modify:** `ast/src/expr.rs`, `ast/src/types.rs`
- **Dependencies:** Task 1
- **Approach:** Add `Expr::Range { start, end, inclusive, span }` to the expression enum. No new AST type needed — Range is a `Custom("Range", [])` at the type level, registered as a builtin class.
- **Key decisions:**
  - Range is an expression node, not a binary operator — it has different semantics (constructs a value, not arithmetic)
  - `inclusive: bool` distinguishes `..` from `..=`

### 3. Parse `..` and `..=` as range expressions

- **Files to modify:** `parser/src/expr.rs`
- **Dependencies:** Tasks 1, 2
- **Approach:** Parse range as a low-precedence infix operator, below `or` but above assignment. After parsing the left side as a normal expression, check for `DotDot` or `DotDotEq` and parse the right side. Handle precedence so `1..10` doesn't greedily consume `10` as part of a larger expression.
- **Potential issues:** `1..10.map(...)` — the `.map` could be parsed as part of the range end. Requiring parens `(1..10).map(...)` for chaining is the safe approach and matches Rust.

### 4. Register `Range` as a builtin class in the typechecker

- **Files to modify:** `typecheck/src/typechecker.rs`
- **Dependencies:** Task 2
- **Approach:** Register `Range` in `TypeChecker::new()` with fields `start: Int`, `end: Int`, `inclusive: Bool`. Mark it as including `Iterable`. This makes `Range` available in prelude mode without imports, and the Iterable vocabulary methods (map, filter, reduce, etc.) are auto-injected.
- **Key decisions:**
  - Range is Int-only for now. Float ranges are a future extension.
  - Range lives in prelude — `..` syntax would be useless if you had to import the type

### 5. Type-check range expressions

- **Files to modify:** `typecheck/src/check_expr.rs`
- **Dependencies:** Tasks 2, 4
- **Approach:** Add a `check_range()` handler. Both start and end must be `Int`. Returns `Type::Custom("Range", [])`. The Iterable inclusion means `.map()`, `.filter()`, etc. are available on the result.
- **Integration points:** `for n in 1..10` works automatically because `for` calls `each()` on the iterable, and Range includes Iterable.

### 6. Lower Range to FIR

- **Files to modify:** `fir/src/lower.rs`, `fir/src/exprs.rs`
- **Dependencies:** Task 2
- **Approach:** Lower `Expr::Range` to a `FirExpr::RuntimeCall` that constructs a Range object, or to a dedicated `FirExpr::Range` node. The `each()` implementation needs to emit a counting loop from start to end (exclusive or inclusive based on the flag).
- **Key decisions:**
  - Range `each()` is a compiler intrinsic, not a user-defined method — the compiler emits the loop directly rather than calling a method. This is the same pattern used for List's `each()`.

### 7. Codegen for Range

- **Files to modify:** `codegen/src/translate.rs`, `codegen/src/runtime.rs`, `codegen/src/runtime_source.rs`, `codegen/src/runtime_sigs.rs`
- **Dependencies:** Task 6
- **Approach:** Two options for Range representation at runtime:
  1. **Struct on stack** — Range is just `(start: i64, end: i64, inclusive: i8)`. No heap allocation. The `for` loop emits a simple counting loop from start to end.
  2. **Heap object** — like classes. More uniform but wasteful for a pair of ints.

  Go with option 1. Range `each()` in codegen emits: `let i = start; while (inclusive ? i <= end : i < end) { yield i; i += 1 }`.
- **Key decisions:**
  - Stack-allocated — ranges are small and short-lived
  - `each()` is inlined by codegen, not a runtime function call

### 8. Add `random()` as a polymorphic builtin

- **Files to modify:** `typecheck/src/typechecker.rs`, `typecheck/src/check_call.rs`
- **Dependencies:** None (independent of Range)
- **Approach:** Register `random` in `TypeChecker::new()`. In `check_call_inner()`, add a `"random"` match arm that:
  1. Checks `self.expected_type` to determine return type
  2. `Int` context + `max:` arg → random int in `0..max`
  3. `Float` context + `max:` arg → random float in `0.0..max`
  4. `Bool` context + no args → coin flip
  5. No context → error: "Cannot infer type for random(). Add a type annotation."
- **Key decisions:**
  - Uses expected-type threading, same as `From[T]` resolution
  - `max:` is the named parameter (optional for Bool)
  - Returns values in `[0, max)` exclusive — consistent with range semantics

### 9. Runtime functions for random

- **Files to modify:** `codegen/src/translate.rs`, `codegen/src/runtime.rs`, `codegen/src/runtime_source.rs`, `codegen/src/runtime_sigs.rs`
- **Dependencies:** Task 8
- **Approach:** Add three runtime functions:
  - `aster_random_int(max: i64) -> i64` — uses `arc4random_uniform` (macOS) or `getrandom` (Linux)
  - `aster_random_float(max: f64) -> f64` — generate random bytes, convert to `[0.0, 1.0)`, scale by max
  - `aster_random_bool() -> i8` — single random bit

  In `translate_runtime_call()`, dispatch `"random"` to the appropriate function based on the FIR return type.
- **Key decisions:**
  - Use OS entropy, not a PRNG — no seed state to manage, secure by default
  - C runtime uses `arc4random_uniform` (available on both macOS and Linux glibc 2.36+). Fallback: `getrandom()` syscall + modulo for older Linux.

### 10. Add `.random()` method to Range

- **Files to modify:** `typecheck/src/check_call.rs`
- **Dependencies:** Tasks 4, 8
- **Approach:** When a method call `.random()` is made on a Range type, type-check it as returning `Int`. At codegen, emit: get start and end from the range, call `aster_random_int(end - start) + start`.
- **Integration points:** This is a vocabulary method on Range, similar to how Iterable methods are injected on classes that include Iterable.

### 11. Add `Random` trait to stdlib

- **Files to modify:** `typecheck/src/typechecker.rs` (builtin traits registration)
- **Dependencies:** Task 8
- **Approach:** Define a `Random` trait with method `random() -> Self`. User classes can include it:
  ```
  class Dice includes Random
    face: Int
    def random() -> Dice
      Dice(face: random_int(max: 6) + 1)
  ```
  This is a stdlib trait under `std/random { Random }`. Available in prelude mode, must be imported in module mode.
- **Key decisions:**
  - The trait is simple: one method, `random() -> Self`
  - The `random()` builtin function is separate from the trait — the function is polymorphic on primitives, the trait is for user types

### 12. Tests

- **Files to create:** `tests/ranges.rs`, `tests/random.rs`
- **Dependencies:** All above
- **Approach:**
  - **Range parsing:** `1..10`, `a..b`, `1..=10`, precedence with arithmetic
  - **Range type checking:** both sides must be Int, result is Iterable
  - **Range in for loops:** `for n in 1..10` iterates correctly
  - **Range vocabulary:** `(1..10).map(...)`, `(1..10).filter(...)`, `(1..10).reduce(...)`
  - **Range codegen:** JIT execution produces correct sequences
  - **random() type inference:** Int/Float/Bool from context, error without context
  - **random() bounds:** `random(max: 100)` returns Int, `random(max: 1.0)` returns Float
  - **Range.random():** `(1..100).random()` returns Int in range

## Potential Challenges & Mitigations

1. **Challenge:** Range precedence conflicts with method chaining (`1..10.map(...)`)
   **Mitigation:** Require parens for chaining: `(1..10).map(...)`. The `..` binds tighter than method calls only when both operands are simple expressions.

2. **Challenge:** `arc4random_uniform` availability on older Linux
   **Mitigation:** Use `getrandom()` syscall as fallback in the C runtime. Feature-detect at compile time with `#ifdef`.

3. **Challenge:** `random()` without type context is ambiguous
   **Mitigation:** Emit a clear error: "Cannot infer type for random(). Use `let n: Int = random(max: 100)` or `let f: Float = random(max: 1.0)`."

4. **Challenge:** Range `each()` codegen must handle both exclusive and inclusive
   **Mitigation:** Emit different comparison ops (`i < end` vs `i <= end`) based on the `inclusive` flag stored in the FIR node.

## Unwired Code Audit

- [x] `..` token is lexed AND consumed by the parser
- [x] `Expr::Range` is created by parser AND handled by typechecker, FIR lowerer, and codegen
- [x] Range class is registered AND Iterable methods are injected on it
- [x] `random()` is registered in typechecker AND dispatched in codegen AND backed by runtime functions
- [x] Runtime functions are declared in sigs AND implemented in Rust runtime AND implemented in C runtime
- [x] `.random()` on Range is type-checked AND lowered AND translated to codegen calls
- [x] `Random` trait is registered AND can be included by user classes

## Validation Steps

- `for n in 1..10` compiles and iterates 1 through 9
- `for n in 1..=10` compiles and iterates 1 through 10
- `(1..100).filter(f: -> n: n % 2 == 0)` produces even numbers
- `let n: Int = random(max: 100)` compiles and returns a value in 0..100
- `let f: Float = random(max: 1.0)` compiles and returns a value in 0.0..1.0
- `let b: Bool = random()` compiles and returns true or false
- `(1..100).random()` compiles and returns an Int in range
- `random(max: 100)` without type annotation produces a clear error
- All existing tests continue to pass
