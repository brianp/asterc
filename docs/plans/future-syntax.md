# Future Syntax Additions

Small additions that are intentionally deferred but should be easy to add later.

## String * Int (repetition)

Like Ruby: `"ha" * 3` → `"hahaha"`

- Add `(String, Int) → String` rule for `Mul` in the type checker
- One-line addition once Phase 1 BinaryOp infrastructure is in place
- No architectural decisions needed now; nothing in Phase 1 blocks this

## BigInt auto-promotion

- MVP uses i64 for Int
- Design goal: integers scale infinitely (like Ruby Integer)
- Type system doesn't change — it's still `Int`, just a wider runtime representation
- Requires runtime work, not type system work

## Match guards

Pattern matching with `if` guards on bound variables:

```
match value
  n if n <= 1 => false
  n if n <= 3 => true
  n if n % 2 == 0 => false
```

- Bind a variable in the pattern, then filter with an arbitrary boolean expression
- Guard expression has access to the bound variable
- Works with all pattern types (literal, enum variant, wildcard binding)
- Without this, conditional logic on a single value requires if/elif chains instead of match

## Match arms as full blocks

Match arms currently only accept a single expression after `=>`. They should accept any statement or block — if/elif/else, while, nested match, etc. — as long as every arm either:

1. Returns a value of the same type as the other arms, or
2. Returns / throws (diverges)

```
match value
  1 => false
  _ =>
    if is_even(value: value)
      false
    else
      check_odd(value: value)
```

This also enables multi-statement arms with indented blocks after `=>`.

## If/else as expression

`if/else` should be usable as an expression that produces a value:

```
let x = if condition then a else b
```

Both branches must produce the same type. Without `else`, it's a statement (returns Void). With `else`, it's an expression.

## Ranges

`Range` type in `std/collections`, `..` syntax in the prelude.

```
for n in 1..10
  say(message: to_string(value: n))

for n in a..b
  say(message: to_string(value: n))
```

- `a..b` is exclusive (does not include `b`), `a..=b` is inclusive
- `..` desugars to `Range(start: a, end: b)` — a stdlib class that includes Iterable
- `Range` lives in `std/collections { Range }` but `..` syntax is prelude (no import needed)
- Works with `for`, and anywhere an Iterable is expected (map, filter, reduce, etc.)
- `Range` fields are readable: `let r = 1..10` then `r.start`, `r.end`

```
let evens = (0..100).filter(f: -> n: n % 2 == 0)
let sum = (1..=100).reduce(init: 0, f: -> acc, n: acc + n)
```

## Design principle

The type checker is a **whitelist**: defined rule exists → allowed, no rule → compile error. No implicit coercions, no silent nil propagation, no NaN. The opposite of JS. New operator/type combinations are added as explicit rules.
