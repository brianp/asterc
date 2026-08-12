---
status: deprecated
deprecated: 2026-03-15
note: "Async call syntax section superseded by green-threads.md. Protocol implementations themselves are complete — retained as historical reference."
---

# Plan: Standard Protocols Implementation

## Context

The protocols RFC (`docs/design/protocols-rfc.md`) defines seven standard
protocols that make user types first-class: From/Into, Eq, Ord, Printable,
Iterable, and implicit Hash. These require foundational language features
that don't exist yet: parametric traits, Self type, enums, named arguments,
and operator desugaring.

This plan is ordered by dependencies. Each phase is testable in isolation.
TDD throughout — tests are written before implementation.

See `STATUS.md` for the full picture of what's implemented vs decided.

## Breaking Changes First

Before building protocols, resolve the breaking changes from the RFC
that affect existing code. See `STATUS.md` Breaking Changes Queue.

### Phase 0: Named Arguments Everywhere

**Why first:** Every subsequent phase writes tests using named arg syntax.
If we build protocols on positional args and then switch, we rewrite
every test twice.

**0A. AST changes**

Update `Expr::Call` to carry named arguments:

```rust
// Before
Call { func: Box<Expr>, args: Vec<Expr>, span: Span }

// After
Call { func: Box<Expr>, args: Vec<NamedArg>, span: Span }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamedArg {
    pub name: String,
    pub value: Expr,
    pub span: Span,
}
```

Same change for `AsyncCall`, `ResolveCall`, `DetachedCall`.

Tests (write first):
- [ ] Parse `func(a: 1, b: 2)` into `Call` with named args
- [ ] Parse `func(b: 2, a: 1)` — order preserved in AST
- [ ] Parse error on `func(1, 2)` — positional args rejected
- [ ] Parse error on `func(a: 1, 2)` — mixing named/positional rejected
- [ ] Parse `MyClass(field: value)` — construction uses same syntax
- [ ] Parse `async func(a: 1)` — async calls with named args
- [ ] Parse `resolve func(a: 1)` — resolve calls with named args
- [ ] Parse `func(a: 1)!` — error propagation with named args
- [ ] Parse `func(a: 1)!.or(default: 0)` — error recovery with named args

**0B. Parser updates**

Modify `parse_call_args()` to require `name: value` pairs.

**0C. Typechecker updates**

Modify call checking to match args by name, not position:
- Look up parameter name, match to argument name
- Report error for unknown argument names
- Report error for missing required arguments
- Report error for duplicate argument names
- Argument order doesn't matter — resolved by name

Tests (write first):
- [ ] `def f(a: Int, b: String)` called as `f(b: "x", a: 1)` — OK
- [ ] `f(a: 1, c: "x")` — error: unknown argument `c`
- [ ] `f(a: 1)` — error: missing argument `b`
- [ ] `f(a: 1, a: 2)` — error: duplicate argument `a`
- [ ] Constructor `User(name: "Alice", email: "a@b")` — OK
- [ ] Constructor `User(email: "a@b", name: "Alice")` — OK (order independent)
- [ ] Method calls: `obj.method(arg: value)` — OK

**0D. Update all existing tests and examples**

Every `check_ok` / `check_err` test and every `.aster` example file
must be updated to use named arg syntax.

Tests:
- [ ] All 212+ existing tests pass with named arg syntax
- [ ] All example files parse and typecheck

---

## Foundation Phases

### Phase 1: Enums

**Why:** Ordering requires the `Ordering` enum. Enums also interact with
match, Eq, Ord, Printable, and Iterable. They're a prerequisite.

**1A. AST**

```rust
// New Stmt variant
Enum {
    name: String,
    variants: Vec<EnumVariant>,
    methods: Vec<Stmt>,  // for includes trait methods
    includes: Vec<String>,
    is_public: bool,
    span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnumVariant {
    pub name: String,
    pub fields: Vec<(String, Type)>,  // empty for unit variants
    pub span: Span,
}
```

```rust
// New Type variant
Type::Enum(String)  // enum name
```

Tests (write first):
- [ ] Parse unit enum: `enum Color` with `Red`, `Green`, `Blue`
- [ ] Parse enum with fields: `enum Shape` with `Circle(radius: Float)`
- [ ] Parse enum method: `enum Color` with `def to_hex() -> String`
- [ ] Type check: `let c = Color.Red` has type `Color`
- [ ] Type check: `let s = Shape.Circle(radius: 1.0)` has type `Shape`
- [ ] Match on enum: exhaustiveness checking
- [ ] Match error: missing variant
- [ ] Enum variant accessed as `EnumName.VariantName`

**1B. Parser**

Parse `enum Name:` followed by indented variants. Variants are
either unit (`Red`) or carry fields (`Circle(radius: Float)`).

**1C. Typechecker**

Register enum in TypeEnv. Check variant construction, field types,
match exhaustiveness for enum variants.

**1D. Built-in Ordering enum**

Register `Ordering` with variants `Less`, `Equal`, `Greater` as a
built-in type (like `Exception` and `Error`).

Tests:
- [ ] `Ordering.Less` has type `Ordering`
- [ ] Match on Ordering is exhaustive with all three variants
- [ ] `Ordering.Less == Ordering.Less` is `true` (once Eq exists)

---

### Phase 2: Self Type in Traits

**Why:** Eq, Ord, and From all use `Self` in trait method signatures.

**2A. Type variant**

```rust
Type::SelfType  // resolves to the implementing class during type checking
```

**2B. Trait parsing**

Allow `Self` as a type in trait method signatures. Parser treats it
as a type reference. No changes to trait syntax itself.

**2C. Typechecker**

When checking that a class satisfies a trait's required methods,
substitute `Self` with the class type. When type-checking a method
body inside a class that includes a trait, `Self` resolves to the
class type.

Tests (write first):
- [ ] Trait with `def eq(other: Self) -> Bool` parses
- [ ] Class includes trait — `Self` becomes `ClassName` in method signature check
- [ ] Error: class method signature uses `Self` where trait doesn't — mismatch
- [ ] `Self` in return position: `def clone() -> Self`
- [ ] `Self` not valid outside trait/class context — error

---

### Phase 3: Parametric Traits

**Why:** `Into[T]`, `From[T]`, `Iterable[T]`, `Iterator[T]` all need this.

**3A. AST update**

Add generic parameters to `Stmt::Trait`:

```rust
Trait {
    name: String,
    methods: Vec<Stmt>,
    generic_params: Vec<String>,  // NEW
    is_public: bool,
    span: Span,
}
```

Update `TraitInfo` in type_env:

```rust
pub struct TraitInfo {
    pub methods: HashMap<String, Type>,
    pub required_methods: Vec<String>,
    pub generic_params: Vec<String>,  // NEW
}
```

**3B. Parser**

Parse `trait Name[T, U]:` — same bracket syntax as generic classes.

**3C. Typechecker**

When a class `includes Trait[ConcreteType]`:
- Bind trait type params to concrete types
- Substitute in all method signatures before checking satisfaction
- A class can include the same trait multiple times with different params

Tests (write first):
- [ ] Parse `trait Into[T]` with `def into() -> T`
- [ ] `class Celsius includes Into[Fahrenheit]` — `T` bound to `Fahrenheit`
- [ ] Method signature check: `def into() -> Fahrenheit` matches `Into[Fahrenheit]`
- [ ] Multiple includes: `class X includes Into[A], Into[B]` — both satisfied
- [ ] Error: `includes Into[Fahrenheit]` but `def into() -> Kelvin` — mismatch
- [ ] Parametric trait + Self: `trait Eq` with `def eq(other: Self) -> Bool`
- [ ] Combined: `trait From[T]` with `def from(value: T) -> Self`

---

## Protocol Phases

### Phase 4: Eq Protocol

**Why:** Simplest protocol. Tests the Self + auto-derive machinery.
No parametric traits needed (Eq isn't parametric).

**4A. Built-in Eq trait**

Register `Eq` as built-in trait with method `def eq(other: Self) -> Bool`.

**4B. Operator desugaring**

In the typechecker, when checking `BinaryOp { op: Eq, .. }`:
- If both sides are primitive types (Int, Float, String, Bool) — existing behavior
- If either side is a user type that includes `Eq` — desugar to `.eq()` call
- If a user type doesn't include `Eq` — error with guidance

Tests (write first):
- [ ] Primitives: `1 == 1` still works (no regression)
- [ ] User type without Eq: `point1 == point2` — error "Point does not include Eq"
- [ ] User type with manual Eq: `point1 == point2` — type checks, returns Bool
- [ ] `!=` desugars to `not .eq()`: `point1 != point2` — Bool
- [ ] `==` between different types: `point == 5` — error

**4C. Auto-derive Eq**

When a class `includes Eq` but doesn't define `eq()`:
- All fields must include Eq
- Compiler generates field-by-field comparison

Tests (write first):
- [ ] Auto-derive: `class Point includes Eq` with `x: Int, y: Int` — works
- [ ] Auto-derive field check: class has field of type without Eq — error
- [ ] Auto-derive foreign type: error with From[T] guidance diagnostic
- [ ] Manual override: class defines `eq()` — uses manual, not auto
- [ ] Nested: `class Line includes Eq` with `start: Point, end: Point` where Point includes Eq

**4D. Stdlib**

Register `Int`, `Float`, `String`, `Bool` as including `Eq`.
Register `List[T]` as including `Eq` when `T includes Eq`.

Tests:
- [ ] `[1, 2] == [1, 2]` — true (List Eq)
- [ ] `"hello" == "hello"` — true (String Eq, already works)

---

### Phase 5: Ord Protocol

**Why:** Depends on Eq (Ord includes Eq) and enums (Ordering type).

**5A. Built-in Ord trait**

Register `Ord` as built-in trait with method `def cmp(other: Self) -> Ordering`.
`Ord` includes `Eq` — including Ord auto-includes Eq.

**5B. Auto-derive Eq from Ord**

When a class `includes Ord` but doesn't define `eq()`, derive it from `cmp`:
`eq(other) = cmp(other) == Ordering.Equal`

**5C. Operator desugaring**

`<`, `>`, `<=`, `>=` desugar to `.cmp()` for user types that include Ord.

Tests (write first):
- [ ] Primitives: `1 < 2` still works
- [ ] User type without Ord: `task1 < task2` — error
- [ ] User type with Ord: `task1 < task2` — Bool
- [ ] All four operators: `<`, `>`, `<=`, `>=`
- [ ] `includes Ord` without `eq()` — auto-derived from cmp
- [ ] Auto-derive cmp: field-by-field in declaration order
- [ ] Manual cmp: custom ordering works
- [ ] Ordering enum values: `Ordering.Less == Ordering.Less` is true

---

### Phase 6: Printable Protocol

**Why:** No dependency on parametric traits. Uses auto-derive machinery
from Eq/Ord phases.

**6A. Built-in Printable trait**

Register `Printable` with methods:
- `def to_string() -> String` (required or auto-derived)
- `def debug() -> String` (default: calls `to_string()`)

**6B. Auto-derive**

Generate structural form: `"ClassName(field1: value1, field2: value2)"`.
All fields must include Printable.

**6C. Stdlib integration**

`log()` and `print()` accept any Printable, not just String.
(Or: they still take String, and users call `.to_string()` explicitly.
Decision during implementation.)

Tests (write first):
- [ ] Manual to_string: class defines it, works
- [ ] Auto-derive: `class Point includes Printable` → `"Point(x: 3, y: 4)"`
- [ ] debug() defaults to to_string()
- [ ] Override debug() separately
- [ ] Primitives include Printable: `(5).to_string()` → `"5"`
- [ ] Nested: class with Printable fields auto-derives correctly

---

### Phase 7: From / Into Protocol

**Why:** Requires parametric traits (Phase 3) + Self type (Phase 2).

**7A. Built-in traits**

Register `From[T]` and `Into[T]` as built-in parametric traits.

**7B. `Type.from()` intrinsic**

Add `Type.from(value: x)` as a compiler intrinsic. When the parser
sees `Ident.from(...)`, the typechecker checks if the type includes
`From[ArgType]` and resolves accordingly.

**7C. `.into()` expected-type resolution**

When `.into()` is called on a value:
- Check what type is expected at the call site
- Find matching `Into[T]` or `From[SourceType]` on the target
- Resolve to the correct implementation

**7D. Auto-derived reverse**

When `User includes From[PgRow]`, enable `pg_row.into()` in contexts
expecting `User`.

**7E. Fallible conversion**

`from()` and `into()` can declare `throws`. Standard `!`/`!.catch`/`!.or()`
error handling applies.

Tests (write first):
- [ ] `class B includes From[A]` with `def from(value: A) -> Self` — type checks
- [ ] `B.from(value: a_instance)` — resolves to From implementation
- [ ] `let b: B = a_instance.into()` — expected-type resolution
- [ ] `take_b(value: a_instance.into())` — argument-type resolution
- [ ] Multiple From: `class C includes From[A], From[B]` — both work
- [ ] Fallible: `def from(value: A) throws ConversionError -> Self`
- [ ] `B.from(value: a)!` — error propagation on From
- [ ] Auto-reverse: `From[A]` on B enables `a.into()` for B context
- [ ] Error: `.into()` with ambiguous context — compiler asks for annotation
- [ ] Error: `B.from(value: wrong_type)` — type mismatch
- [ ] Stdlib: `Int` includes `Into[String]`, etc.

---

### Phase 8: Iterable / Iterator Protocol

**Why:** Requires parametric traits. Most complex protocol — builds the
Ruby-style method vocabulary.

**8A. Built-in traits**

Register `Iterator[T]` with `def next() -> T?`.
Register `Iterable[T]` with `def each() -> Iterator[T]` plus all
default methods.

**8B. For loop desugaring**

Update `for x in thing` to desugar to `thing.each()` + `next()` loop.
Currently `for` works on built-in lists only — generalize to anything
Iterable.

**8C. Default methods (incremental)**

Implement default methods on Iterable one at a time. Each is built
on `each()`. Start with the most commonly used:

1. `to_list()`, `count()`, `first()`, `last()`
2. `map()`, `filter()`, `reject()`
3. `find()`, `any()`, `all()`, `none()`
4. `reduce()`
5. `take()`, `skip()`
6. `sort()`, `min()`, `max()` (require T includes Ord)
7. `sort_by()`
8. `group_by()`, `zip()`
9. `flat_map()`, `to_map()`

Tests (write first, per sub-phase):
- [ ] Custom class with `each()` — `for` loop works
- [ ] `Iterator[T].next()` returns `T?`, nil means done
- [ ] `.to_list()` collects all elements
- [ ] `.count()` counts elements
- [ ] `.first()` returns first or nil
- [ ] `.map(f: ...)` transforms elements
- [ ] `.filter(f: ...)` selects elements
- [ ] `.find(f: ...)` returns first match or nil
- [ ] `.any(f: ...)` / `.all(f: ...)` / `.none(f: ...)`
- [ ] `.reduce(init: ..., f: ...)` accumulates
- [ ] `.sort()` requires T includes Ord
- [ ] `.sort()` on non-Ord type — error
- [ ] `.min()` / `.max()` return T?
- [ ] Chaining: `.filter(...).map(...).take(n: 5)` works
- [ ] `List[T]` includes Iterable[T] — existing list operations work
- [ ] `String` includes Iterable[String] (character iteration)
- [ ] `Map[K, V]` includes Iterable[Pair[K, V]]

---

### Phase 9: Hash (Implicit)

**Why:** Depends on Eq. Should be last because it's invisible — no
syntax, no trait, no tests from the user's perspective.

**9A. Internal hash generation**

When a type includes Eq, the compiler internally generates a hash
function based on the same fields eq() uses. This is not exposed
as a trait or method.

**9B. Map key constraint**

`Map[K, V]` requires `K includes Eq`. The compiler internally uses
the generated hash for map operations.

Tests (write first):
- [ ] `Map[Point, String]` where Point includes Eq — compiles
- [ ] `Map[Point, String]` where Point lacks Eq — error
- [ ] Map operations: `.get()`, `.set()` work with user-type keys
- [ ] Hash consistency: equal values hash the same (runtime test, post-codegen)

---

## Dependency Graph

```
Phase 0: Named Args
    |
    +---> Phase 1: Enums
    |         |
    +---> Phase 2: Self Type
    |         |
    +---> Phase 3: Parametric Traits
              |
    +---------+---------+
    |         |         |
Phase 4:  Phase 6:  Phase 7:
  Eq      Printable  From/Into
    |                   |
Phase 5:            Phase 8:
  Ord               Iterable
    |
Phase 9:
  Hash (implicit)
```

Phases 4, 6, and 7 can run in parallel after Phase 3.
Phase 5 depends on Phase 4 (Ord includes Eq) and Phase 1 (Ordering enum).
Phase 8 depends on Phase 3 (parametric traits) and Phase 7 (From/Into for to_map, etc.).
Phase 9 depends on Phase 4 (Eq).

## Files Modified Per Phase

### Phase 0 (Named Args)
| File | Changes |
|------|---------|
| `ast/src/expr.rs` | Add `NamedArg` struct, update Call/AsyncCall/ResolveCall/DetachedCall |
| `parser/src/expr.rs` | Update `parse_call_args()` to require named args |
| `parser/src/lib.rs` | Update any direct call parsing |
| `typecheck/src/check_call.rs` | Match args by name, not position |
| `typecheck/src/check_expr.rs` | Update call expression checking |
| `tests/**` | Update ALL existing tests to named arg syntax |
| `examples/**` | Update ALL example files |

### Phase 1 (Enums)
| File | Changes |
|------|---------|
| `ast/src/expr.rs` | Add `Stmt::Enum`, `EnumVariant` |
| `ast/src/types.rs` | Add `Type::Enum(String)` |
| `ast/src/type_env.rs` | Add `EnumInfo`, registration |
| `lexer/src/token.rs` | Add `Enum` keyword |
| `lexer/src/lib.rs` | Lex `enum` keyword |
| `parser/src/lib.rs` | Parse enum declarations |
| `typecheck/src/typechecker.rs` | Check enum definitions, variant construction |
| `typecheck/src/check_expr.rs` | Check enum member access, match exhaustiveness |

### Phase 2 (Self Type)
| File | Changes |
|------|---------|
| `ast/src/types.rs` | Add `Type::SelfType` |
| `parser/src/type_parser.rs` | Parse `Self` as type |
| `typecheck/src/check_class.rs` | Substitute Self when checking trait satisfaction |

### Phase 3 (Parametric Traits)
| File | Changes |
|------|---------|
| `ast/src/expr.rs` | Add `generic_params` to `Stmt::Trait` |
| `ast/src/type_env.rs` | Add `generic_params` to `TraitInfo`, update `includes` to carry type args |
| `parser/src/class_trait.rs` | Parse `trait Name[T]` |
| `parser/src/lib.rs` | Parse `includes Trait[Type]` |
| `typecheck/src/check_class.rs` | Bind trait type params, substitute in method signatures |

### Phases 4-9 (Protocols)
| File | Changes |
|------|---------|
| `ast/src/type_env.rs` | Register built-in traits (Eq, Ord, Printable, From, Into, Iterable, Iterator) |
| `typecheck/src/check_expr.rs` | Operator desugaring (==, !=, <, >, <=, >=), .into() resolution |
| `typecheck/src/check_class.rs` | Auto-derive logic (Eq, Ord, Printable) |
| `typecheck/src/check_call.rs` | `Type.from()` intrinsic resolution |
| `typecheck/src/typechecker.rs` | For loop desugaring to Iterable, built-in registrations |

## Verification

At every phase:
- `cargo test` passes — zero regressions
- New tests written BEFORE implementation (TDD)
- Tests cover: happy path, error cases, edge cases, diagnostic messages
- Foreign type diagnostic pattern tested where applicable
- No warnings from `cargo build`

## Estimated Test Count per Phase

| Phase | New Tests | Cumulative |
|-------|-----------|------------|
| 0: Named Args | ~30 + update 212 existing | ~242 |
| 1: Enums | ~20 | ~262 |
| 2: Self Type | ~10 | ~272 |
| 3: Parametric Traits | ~15 | ~287 |
| 4: Eq | ~20 | ~307 |
| 5: Ord | ~15 | ~322 |
| 6: Printable | ~12 | ~334 |
| 7: From/Into | ~20 | ~354 |
| 8: Iterable | ~30 | ~384 |
| 9: Hash | ~8 | ~392 |
