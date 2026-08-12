# RFC: Closure Capture Semantics in Aster

Status: DECIDED

---

## 1. Problem

Lambdas exist in Aster but don't capture variables from enclosing scopes.
This blocks the Iterable protocol (map, filter, reduce all need closures)
and any higher-order pattern where a callback uses outer state.

```
let scale = 2
items.map(f: -> x: x * scale)   # ERROR today — scale not found
```

This RFC defines how closures capture outer variables and how capture
interacts with async boundaries.

---

## 2. Design Principles

From the language philosophy:

- **One way to do things** — one capture mode, not many
- **Implicit capture** — closures capture by reference automatically,
  like Ruby and JavaScript. No capture lists, no annotations
- **Low entropy** — no `move`, `ref`, `mut`, `nonlocal`, or any other
  capture modifier. The compiler knows what's happening
- **Sync is free, async has rules** — synchronous closures have no
  restrictions. Async boundaries follow the existing async RFC rules

---

## 3. The Core Rule: Where It's Defined Determines Capture

### 3.1 Nested = closure (captures scope)

Any function defined inside another scope — whether inline lambda or
nested `def` — captures the enclosing scope by reference. Full read
and write access to captured variables in synchronous contexts.

```
def process(rows: List[Row]) -> List[Record]
  let count = 0

  # nested def — captures count
  def transform(row: Row) -> Record
    count = count + 1
    row.into()

  let records = rows.map(f: transform)
  log(message: count.to_string())   # prints number of rows processed
  records
```

Inline lambdas work the same way:

```
let count = 0
let records = rows.map(f: -> row:
  count = count + 1
  row.into()
)
log(message: count.to_string())
```

### 3.2 Top-level = pure function (no capture)

A function defined at module level has no enclosing scope to capture.
Everything must come through parameters.

```
# top-level — no capture possible
def transform(row: Row) -> Record
  row.into()

def process(rows: List[Row]) -> List[Record]
  rows.map(f: transform)   # transform can't see anything in process
```

This is the natural distinction. No special syntax or annotation needed —
the shape of the code tells you the rules.

### 3.3 Sync closures: full access

In synchronous contexts, closures can read, mutate, and reassign
captured variables with no restrictions. The closure runs immediately
while the outer scope is alive, so there's no aliasing danger.

```
let count = 0
items.each(f: -> item:
  count = count + 1
  item.process()
)
log(message: count.to_string())
```

This is the Ruby/JavaScript model. Trust the programmer in sync code.

### 3.4 Async closures: follow the async RFC

When a closure crosses an async boundary (`async`, `detached async`),
the existing async RFC rules apply — no special closure rules needed:

- Captured variables are **shallow copied** into the async context
- Compiler **warns** if the original variable is used after the copy
- Use `Mutex[T]` for shared mutable state (no copy, no warning)
- Use `.copy()` for explicit deep copy (no warning)

```
let config = load_config()

detached async
  # config is shallow-copied — closure captures are just data
  process_with_config(config: config)

# warning: 'config' has been copied to an async context.
log(message: config.name)
```

With `Mutex[T]` — no copy, no warning:

```
let state = Mutex(Counter(value: 0))

async
  state.lock(f: -> s: s.increment())   # shared reference, safe

state.lock(f: -> s: log(message: s.value.to_string()))   # no warning
```

---

## 4. Iterables: Transforms Return New Data

Iterable methods that transform (`map`, `filter`, `reduce`) return new
collections. They do not mutate the source. The closure receives items
and produces new values.

```
let scores = [85, 92, 78, 95, 88]

# map returns a new list — scores is unchanged
let curved = scores.map(f: -> s: s + 5)

# filter returns a new list
let high = scores.filter(f: -> s: s > 90)

# reduce produces a single value
let total = scores.reduce(init: 0, f: -> sum, s: sum + s)
```

For side-effect iteration, use `each`:

```
scores.each(f: -> s: log(message: s.to_string()))
```

---

## 5. Lambda Syntax

### 5.1 Current syntax

Aster has two lambda forms:

**Multi-line (def-as-let / nested def):**
```
def double(x: Int) -> Int
  x * 2
```

**Inline (arrow):**
```
-> x: x * 2
```

### 5.2 Inline lambdas in named args

```
items.map(f: -> x: x * 2)
items.filter(f: -> x: x > 0)
items.reduce(init: 0, f: -> acc, x: acc + x)
```

### 5.3 Zero-parameter closures

```
let lazy_value = -> : expensive_computation()
```

---

## 6. Type System

### 6.1 Closure type is the same as function type

A closure has the same type as a regular function:
`(ParamTypes) -> ReturnType`

```
def apply(x: Int, f: (Int) -> Int) -> Int
  f(_0: x)

let result = apply(x: 5, f: -> x: x * 2)
```

There is no separate "closure type" vs "function type." They unify.

### 6.2 Type inference for inline lambdas

When a lambda is passed to a function with a known parameter type,
the lambda's parameter types can be inferred:

```
# f expects (Int) -> Int, so x is inferred as Int
items.map(f: -> x: x * 2)
```

When no context is available, parameter types must be annotated:

```
let f = -> x: Int: x * 2    # annotation needed — no context
```

---

## 7. Implementation

### 7.1 AST changes

No AST changes needed. `Expr::Lambda` already has `params`, `body`,
and everything else. Captured variables are inferred during
typechecking, not declared in the AST.

### 7.2 Typechecker changes

1. **Track closure boundary.** Add a flag to TypeChecker indicating
   "we are inside a lambda body." When resolving a variable from a
   parent scope while this flag is set, the variable is captured.

2. **Propagate expected type into lambda.** When checking a `Call`
   where a parameter expects `(T) -> U` and the argument is a `Lambda`,
   pass the expected parameter types into `check_lambda` so the lambda
   params can be inferred.

3. **Async boundary detection.** When a closure crosses an async
   boundary, apply the existing async shallow-copy + warning rules.
   No new error codes needed — reuse the async RFC's warning system.

### 7.3 Codegen

Closures are represented as a function pointer + environment struct
(a "fat closure"). The environment struct holds references (sync) or
copies (async boundary) of captured variables. Standard implementation,
well-supported by Cranelift.

---

## 8. What This Does NOT Include

- **No `move` or `ref` annotations** — one capture mode (by reference)
- **No `mut` capture modifier** — sync closures can always mutate
- **No `nonlocal` keyword** — no Python-style declaration needed
- **No explicit capture lists** — compiler infers from body analysis
- **No closure types distinct from function types** — they unify
- **No restrictions on sync capture** — read, write, call methods, all fine

---

## 9. Examples

### Counter during map
```
let count = 0
let records = rows.map(f: -> row:
  count = count + 1
  row.into()
)
log(message: count.to_string())
```

### Filter with captured threshold
```
let threshold = 10
let high_scores = scores.filter(f: -> s: s > threshold)
```

### Nested def as closure
```
def process(rows: List[Row]) -> List[Record]
  let errors = []
  let count = 0

  def try_convert(row: Row) -> Record
    count = count + 1
    row.into()

  let records = rows.map(f: try_convert)
  log(message: "Processed " + count.to_string() + " rows")
  records
```

### Top-level function (no capture)
```
def double(x: Int) -> Int
  x * 2

let doubled = items.map(f: double)
```

### Async boundary
```
let config = Config(retries: 3)

detached async
  process_with_config(config: config)

# warning: 'config' has been copied to an async context
log(message: config.retries.to_string())
```

### Shared state across async tasks
```
let counter = Mutex(0)

async
  counter.lock(f: -> c: c + 1)   # no warning, shared safely

counter.lock(f: -> c: log(message: c.to_string()))
```

---

## 10. Resolved Questions

1. **Mutable capture?** Yes. Sync closures have full read/write access
   to captured variables. No restrictions, no annotations. Async
   boundaries follow the async RFC (shallow copy + warning).

2. **Capture syntax?** None. Capture is implicit. The compiler infers
   which variables are captured by analyzing the closure body.

3. **Nested def vs lambda?** Same semantics. Both capture the enclosing
   scope. The distinction is nested (captures) vs top-level (no capture).

4. **Deep copy vs shallow copy at async boundaries?** Shallow copy,
   per the async RFC. Use `Mutex[T]` for shared state, `.copy()` for
   explicit deep copy.

5. **Lambda type inference without context?** Require type annotations
   when no expected type is available.

6. **Visual distinction for captured variables?** No syntax difference.
   IDE tooling (LSP) can highlight them.

---

## 12. Codegen Implementation (2026-03-12)

The closure calling convention is fully implemented in the FIR lowering and
Cranelift codegen:

### Lambda lifting
All lambdas (inline `-> x: body` and nested `def`) are lifted to top-level
functions with `__env: Ptr` as a hidden first parameter. The lifted function
loads captured variables from the env struct at known offsets.

### Environment allocation
For closures with captures, the lowerer emits:
1. `aster_class_alloc(num_captures * 8)` to allocate an env struct
2. Store each captured variable's value into the env at `offset = i * 8`
3. The env pointer is passed as the first argument to the lifted function

### Static resolution
Closures are resolved statically via `closure_info` — a map from variable
name to `(lifted_func_id, env_local_id, capture_names)`. At call sites,
the lowerer checks `closure_info` before other resolution paths and emits
a direct `FirExpr::Call` with the env prepended as the first argument.

### No dynamic dispatch (yet)
`FirExpr::ClosureCall` exists in the IR but is not used by the current
lowerer. All closures are resolved statically. Dynamic dispatch (passing
closures as first-class values to unknown callees) will require an indirect
call via function pointer table — tracked as future work.
