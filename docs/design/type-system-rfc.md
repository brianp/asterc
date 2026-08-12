# RFC: Type System — Inheritance, Traits, Generics

Status: DECIDED — Partially implemented.

### Implementation Status

| Feature | Status |
|---------|--------|
| `extends` (single inheritance) | Implemented |
| `includes` (trait composition) | Implemented |
| Instance methods with implicit `self` | Implemented |
| No class methods | Implemented |
| Named arguments everywhere | Implemented (BC-1/BC-2) |
| Inline generic syntax for functions | Implemented (BC-5) |
| Bracket syntax `[T]` for type definitions | Implemented |
| Foreign types sealed | Implemented |
| `let` (mutable binding) | Implemented |
| Function overloading | Not implemented |
| `const` (compile-time constant) | Not implemented |
| `lazy_const` (runtime-initialized) | Not implemented |
| 3-arg limit enforcement | Not implemented |
| `super()` calls | Not implemented |
| Default parameter values | Not implemented |
| Introspection intrinsics | Not implemented (see introspection-rfc.md) |

---

## 1. Design Principles

- Classical OOP where it helps — single inheritance, polymorphism
- Trait composition for interfaces and shared behavior
- Generics only when genuinely needed (containers, type preservation)
- Syntax should read naturally — minimize punctuation and ceremony
- One way to do things — constraints defined in one place, not many
- LLM-friendly: unambiguous, low entropy, predictable patterns
- Foreign types are sealed — extend via `extends`, don't reopen
- Introspection is built in (see introspection-rfc.md)

---

## 2. extends — Single Inheritance

`extends` creates a subtype of a concrete class. Single parent only.
The child inherits all fields and methods. Can override methods.

```
class Vehicle
  speed: Int

  def drive() -> Void
    log("moving at " + to_string(speed))

class Car extends Vehicle
  brand: String

  def drive() -> Void
    log(brand + " moving at " + to_string(speed))

class Truck extends Vehicle
  payload: Int
```

Rules:
- Single parent only — `class X extends A, B` is a compile error
- Child inherits all fields and methods from the parent chain
- Child can override methods
- Child can add new fields and methods
- A `Car` is accepted anywhere a `Vehicle` is expected (polymorphism)
- The compiler only allows access to fields/methods of the declared type

```
def park(v: Vehicle) -> Void
  v.drive()      # OK — Vehicle has drive()
  # v.brand      # COMPILE ERROR — Vehicle doesn't have brand

park(Car(speed: 60, brand: "Toyota"))   # OK — Car extends Vehicle
```

### 2.1 Subtype Variance Rules

Inheritance creates a subtype relationship (`Car` is-a `Vehicle`), but this relationship
does **not** propagate into parameterized type positions. Containers and function types
are **invariant** — they require exact type matches.

**Direct arguments — subtype accepted:**
```
def greet(v: Vehicle) -> String    # accepts Car, Truck, any Vehicle subtype
  v.speed
```

**Container type parameters — invariant (exact match only):**
```
def process(cars: List[Vehicle]) -> Void   # only accepts List[Vehicle]
  cars.push(item: Vehicle(speed: 0))       # this is why — mutation is safe

let cars: List[Car] = [Car(speed: 60, brand: "Toyota")]
process(cars: cars)                        # COMPILE ERROR — List[Car] ≠ List[Vehicle]
```

Use generics with constraints for polymorphic containers:
```
def count(items: List[T extends Vehicle]) -> Int
  items.count()

count(items: cars)                         # OK — T binds to Car, Car extends Vehicle
```

**Function types — invariant (exact match only):**
```
def apply(f: (Car) -> String) -> String
  f(_0: Car(speed: 60, brand: "Toyota"))

def describe_vehicle(v: Vehicle) -> String    # (Vehicle) -> String
  "fast"

apply(f: describe_vehicle)                    # COMPILE ERROR — must be (Car) -> String exactly
```

**Rationale:** Inheritance is for class definition (is-a relationships). Inside containers and
function types, covariance would allow type-unsafe mutations (pushing a Vehicle into a List[Car])
or violate substitution guarantees. Generics with `extends`/`includes` constraints are the
correct tool for polymorphic containers and callbacks.

---

## 3. includes — Trait Composition

`includes` adds trait interfaces to a class. Multiple traits allowed.
Traits define method signatures and optional default implementations.
Traits can't have fields.

```
trait Serializable
  def to_json() -> String

trait Validatable
  def validate() throws ValidationError -> Void

class User extends BaseModel includes Serializable, Validatable
  name: String
  email: String

  def to_json() -> String
    "{\"name\": \"" + name + "\"}"

  def validate() throws ValidationError -> Void
    match email.contains("@")
      false => throw ValidationError(field: "email")
      true => log("valid")
```

Rules:
- Multiple traits allowed
- Traits can have abstract methods (no body) and default methods (with body)
- Classes must implement all abstract methods from included traits
- `extends` and `includes` can be combined on the same class

---

## 4. Methods, Functions, and Self

### Instance methods

Defined inside a class. `self` is implicit — fields and other instance
methods are accessed by name. `self` is available as an explicit keyword
when needed for disambiguation or passing the instance.

```
class User
  name: String
  email: String

  def display() -> String
    name + " <" + email + ">"       # implicit self

  def update(name: String) -> Void
    self.name = name                 # explicit self to disambiguate

  def register(registry: Registry) -> Void
    registry.add(self)               # passing self to another function
```

### No class methods

All `def` inside a class body is an instance method. No exceptions, no
`static` keyword, no detection heuristics. No ambiguity.

Factories, utilities, queries, and other class-associated functions live
in namespace files. The project structure teaches the pattern:

```
src/
  models/
    user.aster          # class User — fields + instance methods
  factories/
    user.aster          # from_json, from_csv — construct Users
  queries/
    user.aster          # find, search — DB operations on Users
  validators/
    user.aster          # validate_email, validate_name
```

```
# models/user.aster
pub class User
  name: String
  email: String

  def display() -> String
    name + " <" + email + ">"

# factories/user.aster
use models.user.User

pub def from_json(json: Json) -> User
  User(name: json.get("name").or(""), email: json.get("email").or(""))

pub def default() -> User
  User(name: "Guest", email: "none")
```

Usage:

```
use factories.user as user_factory

let user = user_factory.from_json(json)
```

### Introspection is compiler intrinsics

Class-level introspection (`.ancestors`, `.fields`, `.children`, etc.) is
built-in syntax the compiler provides on every type — not methods. Users
can't define their own class-level intrinsics.

```
User.ancestors           # compiler intrinsic
User.fields              # compiler intrinsic
user.is_a(Serializable)  # instance intrinsic
```

---

## 5. Function Overloading

A function's identity is its name + parameter types + arity. Different
signatures with the same name are different functions and don't conflict.

```
class Formatter
  def format(value: String) -> String
    value

  def format(value: Int) -> String
    to_string(value)

  def format(value: String, width: Int) -> String
    pad(value, width)
```

Rules:
- Same name, different parameter types or arity = different functions
- Same name, same parameter types and arity = conflict (override or
  resolution rules apply)
- Return type alone does NOT distinguish overloads — `def get() -> Int`
  and `def get() -> String` with the same params is a compile error
- The call site must be unambiguous from the arguments alone — no
  inference from what the return value is assigned to
- Overloading applies equally to standalone functions, class methods,
  and trait methods

Trait conflict resolution respects overloading:

```
trait Printable
  def format(value: String) -> String

trait Loggable
  def format(value: String, level: Int) -> String

class Report includes Printable, Loggable
  # No conflict — different arities
  # report.format("x")     calls Printable.format
  # report.format("x", 3)  calls Loggable.format
```

---

## 5. Function Arguments — Named Args & The 3-Arg Rule

**UPDATED by protocols-rfc.md:** All function calls and class construction
use named arguments. Names are always required. This replaces the earlier
positional-only design to ensure one consistent calling convention
everywhere.

### Named arguments everywhere

```
greet(name: "Alice")
add(a: 1, b: 2)
rgb(r: 255, g: 128, b: 0)
User(name: "Alice", email: "a@b.com")
```

Arguments are matched by name, not position. Order at the call site
does not matter:

```
rgb(b: 0, r: 255, g: 128)    # same as rgb(r: 255, g: 128, b: 0)
```

### 3-arg limit (configurable)

Functions accept a maximum of 3 parameters. If a function needs 4+
inputs, use a parameter struct.

```
# COMPILE ERROR: more than 3 parameters
def create_user(name: String, email: String, role: String, active: Bool) -> User

# Correct: use a parameter struct
class CreateUserParams
  name: String
  email: String
  role: String = "member"
  active: Bool = true

def create_user(params: CreateUserParams) -> User
  User(name: params.name, email: params.email)

create_user(params: CreateUserParams(
  name: "Alice",
  email: "alice@example.com",
  role: "admin"
))
```

### Why named args

- One calling convention for functions and construction — no ambiguity
- Every call site is self-documenting
- Argument order doesn't matter — matched by name
- LLMs never guess what a positional value means
- Consistent with "one way to do things" philosophy

### Rules

- All arguments must be named at every call site — no exceptions
- Maximum 3 parameters per function/method definition — compiler enforced
- Constructors follow the same rule: classes with 4+ fields require
  a parameter struct
- Default values on function params work within the 3-arg limit
- Default value expansion must not conflict with overloaded signatures
  (checked at definition time)
- Argument order at call site is irrelevant — compiler matches by name

### Threshold is a compiler option

The limit defaults to 3 but is configurable via compiler options. Teams
that find 3 too tight can set it to 4 or 5. The principle — "model your
inputs, don't sprawl your signatures" — is the invariant, not the number.

---

## 6. Polymorphism — Use Types Directly

Most functions don't need generics. Accept a trait or class as the
parameter type. The compiler enforces the contract of the declared type.

```
# Accept any Serializable — no generic needed
def save(item: Serializable) -> Void
  let json = item.to_json()
  db.write(json)

# Accept any Vehicle — subclasses work via polymorphism
def park(v: Vehicle) -> Void
  v.drive()

# Accept any Queryable — implementations interchangeable
def run_report(db: Queryable) -> Report
  let rows = db.query("SELECT * FROM sales")!
  build_report(rows)
```

This covers ~99% of cases. Generics are for the rest.

---

## 7. Generics — When You Need Them

Generics are needed when:
- The return type depends on the input type (type preservation)
- Container types parameterize over their element type
- A type variable appears in multiple positions and must be consistent

### Syntax: inline declaration at first use

Generic type parameters are introduced inline in the parameter list at
their first occurrence. No bracket section `[T]` or `<T>` before the
params. Constraints use `extends` or `includes` at the declaration site.

**Constraints are always in the parameter list, never in the return type.**

```
# T introduced with constraint in params, return just references T
def clone(item: T extends Vehicle) -> T

# Multiple generics — each declared at first mention
def convert(from: T extends Serializable, to: U extends Serializable) -> U

# Unconstrained generic
def identity(item: T) -> T

# Mixed constraints
def wrap(item: T, db: U includes Queryable) throws DbError -> T

# Trait + inheritance constraint
def process(item: T extends Animal includes Trackable) -> T
```

Rules:
- First occurrence of an unknown uppercase name in params introduces it
- Constraint (`extends`/`includes`) must appear at the declaration site
- Return types only reference previously declared type parameters
- Constraints appear in one place only — the parameter list

### Container types still use bracket syntax

Generic type definitions use brackets — this is for type constructors,
not function signatures:

```
class List[T]
  def first() -> T?
  def map(f: (T) -> U) -> List[U]
  def filter(f: (T) -> Bool) -> List[T]

class Map[K, V]
  def get(key: K) -> V?
  def set(key: K, value: V) -> Void

class Pair[A, B]
  first: A
  second: B
```

---

## 8. Foreign Types Are Sealed

You can't reopen, extend methods on, or modify types from other packages
inline. You can:

- `extends` a foreign class into a new class
- `includes` foreign traits on your own classes
- Write standalone functions that accept foreign types
- Use middleware/pipeline patterns for framework types

```
# Package: aster-postgres
class PgConnection includes Queryable
  # ...

# Your code — extend it, don't reopen it
class GisConnection extends PgConnection
  def st_within(geom: Geometry, bounds: Geometry) throws DbError -> List[Row]
    query("SELECT * WHERE ST_Within(...)")!
```

---

## 9. Variable Bindings & Mutability

### `let` — mutable binding

All `let` bindings are mutable. No `mut` keyword. Variables vary.

```
let x = 5
x = 6              # fine
let name = "Alice"
name = "Bob"       # fine
```

### `const` — compile-time constant

Known at compile time, inlined everywhere. No memory address, no runtime
cost. The compiler replaces every use with the literal value.

```
const MAX_RETRIES = 3
const PI = 3.14159
const APP_NAME = "MyApp"

MAX_RETRIES = 4    # COMPILE ERROR — const cannot be reassigned
```

### `lazy_const` — runtime-initialized, frozen after first set

Computed once at runtime (first access or startup), then immutable forever.
Thread-safe by nature — one write, infinite reads, no races. Internally
uses a `Once` cell.

```
lazy_const DB_URL = env("DATABASE_URL")
lazy_const CONFIG = load_config("app.toml")

# First access triggers computation, subsequent accesses are cached
log(DB_URL)        # computed here
log(DB_URL)        # cached, instant

DB_URL = "other"   # COMPILE ERROR — const cannot be reassigned
```

### No `static`

There's no `static` keyword. `static` enables global mutable state,
which contradicts "force good decisions." The use cases are covered:

- Compile-time constants → `const`
- Runtime-initialized globals → `lazy_const`
- Global mutable state → pass through arguments, or use `Mutex` explicitly

---

## 10. Summary of Decided Design

1. `extends` — single class inheritance, child IS the parent type
2. `includes` — multiple trait composition, class SATISFIES the trait
3. Polymorphism via declared types — use traits/classes as param types directly
4. Generics — inline declaration at first use, constraints in params only
5. Bracket syntax `[T]` only for type definitions (List, Map, etc.)
6. Foreign types sealed — extend or wrap, don't reopen
7. Introspection navigates the full extends/includes hierarchy at runtime
8. Flat constructors — all fields (own + inherited) as named params
9. Method override — same signature replaces, `super()` calls the original
10. `super()` follows wherever the method was inherited from:
    - From `extends` chain if method came from parent class
    - From `includes` trait if method came from a trait (no extends conflict)
    - Never reaches a namespaced/lifted conflict method
11. Conflict resolution — extends chain wins, conflicting trait methods
    lifted to `instance.trait_name.method()` namespace
12. No abstract classes — use traits for that pattern
13. No `via` delegation — use `extends` instead
14. Function overloading — name + param types + arity defines identity
15. Return type alone never distinguishes overloads
16. Call site must be unambiguous from arguments alone
17. 3-arg max per function — 4+ requires a parameter struct
18. **Named arguments everywhere** — required on all calls (functions +
    constructors), order independent, matched by name (updated by protocols-rfc.md)
19. Default values allowed within the 3-arg limit
20. Default expansion checked against overloads at definition time
21. Implicit `self` in instance methods — explicit when needed for
    disambiguation or passing the instance
22. No class methods — all `def` in a class is an instance method
23. No `static` keyword — not needed, no class methods
24. Factories/utilities/queries live in namespace files, not on classes
25. Introspection is compiler intrinsics, not methods
26. Project structure and tooling teach the patterns

## 10. Decided — Former Open Questions

- **Sealed/final classes**: No. Any class can be extended.
- **`is_a` semantics**: Both extends and includes chains. `car.is_a(Vehicle)`
  is true (extends), `car.is_a(Drivable)` is true (includes). A single
  common introspection lists the full hierarchy.
- **Trait field access in conflict**: Lifted trait methods retain full access
  to class fields and methods. They are semantically lifted (namespaced for
  disambiguation) but share the same scope.
- **Multiple trait conflicts**: Any conflict lifts all conflicting methods
  to their trait namespace. Three traits with `serialize()` = three
  namespaced versions.
- **super() in multi-include, no extends**: Both trait methods were lifted
  due to conflict. `super()` has no unambiguous resolution — compile error.
  The class must define its own implementation and call the lifted versions
  explicitly if needed (`self.trait_name.method()`).

## 11. Open Questions

- **Project scaffolding tool**: Aster needs an opinionated project generator.
  Tooling concern, not language design — defer to implementation phase.
- **`use` syntax details**: Selective imports, aliasing, depth — documented
  separately in the module system design. Not this doc's scope.
