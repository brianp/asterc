# RFC: Introspection & Dynamic Dispatch in Aster

Status: DECIDED

---

## 1. Design Principles

- Introspection is built in from the start, on every object, no imports
- Every instance knows its class, ancestry, traits, fields, and methods at runtime
- Read-only. You can ask "what are you?" but you can't dynamically construct or modify classes
- `Type` is a comparable, stringifiable value, but not passable or invocable
- Dynamic dispatch via `DynamicReceiver` is opt-in per class, with compiler-assisted optimization
- Metaprogramming scope is limited: no `define_method`, no runtime class modification, no annotations

---

## 2. Motivation

Ruby's introspection is immediate and ergonomic. You can ask any object what it
is, what it can do, where it came from. Aster should have this.

Dart removed most runtime reflection and it forced code generation for everything:
serialization, dependency injection, plugin systems. Aster won't repeat that.

The `DynamicReceiver` trait enables DSL patterns (package managers, ORMs, test
frameworks) without compromising the type system for normal code. The three-mode
compiler behavior (strict, hybrid, open) is novel: no other language inspects the
`method_missing` implementation to determine how much static checking to apply.

---

## 3. Type as a Value

`Type` is a runtime value representing a class. It is returned by `.class_name`
and accepted by `.is_a()`. It is not a general-purpose first-class value.

What you can do with `Type`:
- Compare: `user.class_name == other.class_name`
- Stringify: `user.class_name.to_string()` returns `"User"`
- Pass to `is_a`: `user.is_a(NetworkError)`

What you cannot do with `Type`:
- Store in a variable: `let t = User` is not valid
- Invoke as a constructor: no dynamic construction
- Pass as a function argument (except to `is_a`)

If someone wants dynamic construction, they match on the string:

```
match obj.class_name.to_string()
    "User"
        User(name: "default")
    "Admin"
        Admin(name: "default", role: "admin")
```

The compiler checks every arm. No magic.

---

## 4. Introspection API

Seven methods, available on every instance, no imports needed. The compiler
emits a static metadata table per class (not per instance). Each instance
holds a pointer to its class table.

### Methods

```
instance.class_name       # -> Type
instance.fields           # -> List[FieldInfo]
instance.methods          # -> List[MethodInfo]
instance.ancestors        # -> List[Type]
instance.children         # -> List[Type]
instance.is_a(SomeClass)  # -> Bool
instance.responds_to("method_name")  # -> Bool
```

All introspection is instance-only. No static introspection methods on classes.

### Built-in Types

Three compiler-generated classes, same category as `Ordering`:

```
class FieldInfo
    name: String
    type_name: Type
    is_public: Bool

class MethodInfo
    name: String
    params: List[ParamInfo]
    return_type: Type
    is_public: Bool

class ParamInfo
    name: String
    param_type: Type
    has_default: Bool
```

### Class Metadata Table

The compiler generates a static metadata table for every class during
compilation. This table contains:

- Class name (as a `Type` value)
- Field descriptors (name, type, visibility)
- Method descriptors (name, params, return type, visibility)
- Parent class pointer (from `extends`)
- Included traits list
- Children list (populated during compilation via a reverse registry:
  when the compiler sees `class B extends A`, it adds `B` to `A`'s
  children list)

Cost: one pointer per instance to the shared class table. The table
itself is static data in the binary, roughly 200-500 bytes per class.

### Examples

```
let user = User(name: "Alice", email: "alice@example.com")

user.class_name                    # Type (prints as "User")
user.class_name.to_string()        # "User"
user.is_a(User)                    # true
user.is_a(Error)                   # false
user.responds_to("name")           # true
user.responds_to("nonexistent")    # false
user.fields                        # [FieldInfo(name: "name", ...), FieldInfo(name: "email", ...)]
user.methods                       # [MethodInfo(name: "to_string", ...), ...]
user.ancestors                     # [Type(User)]
user.children                      # []
```

### Navigating the class hierarchy

```
class AppError extends Error
class NetworkError extends AppError
class TimeoutError extends NetworkError
class ParseError extends AppError

let err = TimeoutError(message: "timed out")

err.is_a(TimeoutError)     # true
err.is_a(NetworkError)     # true (transitive via extends)
err.is_a(AppError)         # true (transitive)
err.is_a(ParseError)       # false (different branch)

err.ancestors              # [TimeoutError, NetworkError, AppError, Error]
err.children               # [] (TimeoutError has no subclasses)

let app_err = AppError(message: "base")
app_err.children           # [NetworkError, ParseError]
```

---

## 5. DynamicReceiver Trait

A class that includes `DynamicReceiver` can receive calls to methods that
are not defined on the class. The compiler routes unknown calls through
`method_missing` instead of emitting a type error.

### Mechanism

The user defines `method_missing` with whatever arg types they want.
The compiler doesn't prescribe an inner enum or a specific map type.
The signature is the contract.

```
class SeedfileDSL includes DynamicReceiver
    deps: List[Dependency]

    def method_missing(fn_name: String, args: Map[String, String]) -> Void
        deps.push(Dependency(name: fn_name, version: args.get(key: "version").or(default: "*")))
```

When someone writes `http(version: "1.2.0")` on a `SeedfileDSL` instance:

1. The compiler checks if `http` is a real method. It's not.
2. The compiler checks if the class includes `DynamicReceiver`. It does.
3. The compiler rewrites the call to `self.method_missing(fn_name: "http", args: {"version": "1.2.0"})`
4. The compiler checks that the call site args pack into the declared map value type (`String`). If not, compile error.

### Three Compiler Modes

The compiler inspects the `method_missing` implementation to determine
how much static checking to apply. No flags, no annotations. The behavior
emerges from what you wrote.

**Strict mode**: match with catch-all that throws `FunctionNotFound`

```
def method_missing(fn_name: String, args: Map[String, QueryArg]) -> Void
    match fn_name
        "find"
            // handle find
        "find_by"
            // handle find_by
        _
            throw FunctionNotFound(name: fn_name)
```

The compiler sees string literal match arms and a catch-all that throws. It
treats `"find"` and `"find_by"` as the complete set of virtual methods. It
can pre-compile direct dispatch for known arms, provide autocomplete, and
error at compile time if you call something not in the list.

**Open mode**: no match or catch-all that doesn't throw

```
def method_missing(fn_name: String, args: Map[String, String]) -> Void
    deps.push(Dependency(name: fn_name, ...))
```

Anything goes. The compiler can't check call validity at compile time,
which is the point. This is the Seedfile DSL pattern.

**Hybrid mode**: known arms plus an accepting catch-all

```
def method_missing(fn_name: String, args: Map[String, SeedArg]) -> Void
    match fn_name
        "http"
            // special handling, maybe pin a mirror
        _
            deps.push(Dependency(name: fn_name, ...))
```

Known names get optimized direct dispatch. Everything else still works
through the dynamic path.

### FunctionNotFound

`FunctionNotFound` is a built-in error type:

```
class FunctionNotFound extends Error
    name: String
```

When the catch-all arm throws `FunctionNotFound`, the compiler knows
the class has a closed set of dynamic methods. This is the signal
that enables strict-mode checking.

### Parser Impact

None. `http(version: "1.2.0")` already parses as a function call. The
rewrite happens in the typechecker.

### Codegen Impact

None beyond the rewrite. `method_missing` is a regular method call after
the typechecker rewrites it. Pre-compiled arms in strict/hybrid mode
are an optimization the codegen can apply by matching the fn_name
string against known literals and jumping directly.

---

## 6. FieldAccessible Trait (Unstable)

An opt-in trait for dynamic field access by name. This lives in
`std/unstable` and requires the `--unstable` compiler flag.

### Why Unstable

Dynamic field access by name returns a value whose type isn't known
at compile time. The user must define an enum that covers their field
types. This API may change as the language evolves.

### Mechanism

A class that includes `FieldAccessible` must define an inner enum
called `FieldValue` that covers the types of all its fields. The
compiler checks coverage and auto-generates the `field_value`
implementation.

```
use std/unstable { FieldAccessible }

class User includes FieldAccessible
    name: String
    age: Int

    enum FieldValue
        StringVal(value: String)
        IntVal(value: Int)
```

The compiler verifies that `FieldValue` has variants covering
`String` and `Int`. If a field type isn't covered, compile error.

### API

```
user.field_value("name")    # -> User.FieldValue.StringVal(value: "Alice")
user.field_value("age")     # -> User.FieldValue.IntVal(value: 30)
user.field_value("nope")    # -> nil (returns FieldValue?)
```

### Why Not Built-in

If we shipped a built-in `DynamicValue` enum that holds any type, people
would use it as `Any` everywhere and the type system becomes optional.
By requiring the user to define their own enum, the dynamic access is
scoped to exactly the types they declare.

---

## 7. Unstable Features

### The `std/unstable` Module

Unstable traits live in `std/unstable`. Importing from this module
requires the `--unstable` compiler flag. Without it, the import
fails with a compile error.

```
# Only compiles with: asterc --unstable main.aster
use std/unstable { FieldAccessible }
```

### Transitivity

The `--unstable` flag is required for the entire compilation unit,
including dependencies. If package A uses `FieldAccessible` internally,
any project depending on package A must also compile with `--unstable`.

This is intentional. Unstable features are a health signal. If your
dependency tree uses unstable features, you should know, because a
toolchain update might break things.

### Stabilization Path

When a feature stabilizes, it moves from `std/unstable` to `std`.
The import path changes, `--unstable` is no longer required. This is
a breaking change for the import, but a one-line fix.

### Seedfile Declaration

Projects using unstable features declare it in their Seedfile:

```
package(name: "my-orm", version: "0.1.0")
unstable(enabled: true)
```

This makes the unstable dependency visible in the manifest, not
hidden in build scripts.

---

## 8. What's In Scope

| Feature | Status |
|---------|--------|
| Instance introspection (7 methods) | In scope, always present |
| `Type` as comparable/stringifiable value | In scope |
| `FieldInfo`, `MethodInfo`, `ParamInfo` | In scope, built-in |
| Class metadata tables | In scope, compiler-generated |
| `DynamicReceiver` trait | In scope, stable |
| Three-mode compiler behavior | In scope |
| `FunctionNotFound` error | In scope, built-in |
| `FieldAccessible` trait | In scope, unstable |
| `std/unstable` module + `--unstable` flag | In scope |

## 9. What's Out of Scope

| Feature | Reason |
|---------|--------|
| `Type` as a passable/invocable value | Not needed. Match on string for dynamic construction. |
| Static introspection methods on classes | Instance-only. No `User.fields`, only `user.fields`. |
| `define_method` / runtime method creation | Conflicts with static typing. Not worth the complexity. |
| Runtime class modification | Same. |
| Annotations / decorators | No concrete use case yet. Separate RFC if needed. |
| Macros | Separate concern entirely. |

## 10. What Was Rejected

- **`Type` as first-class passable value**: In a strongly typed language,
  passing types around and constructing from them dynamically is nearly
  impossible to check at compile time. If you need it, match on the string.
- **Built-in `DynamicValue` / `Any` type**: Would become an escape hatch
  from the type system. Users define their own enums for dynamic contexts.
- **Prescribed `ArgValue` enum for `DynamicReceiver`**: The user's
  method_missing signature is the contract. The compiler doesn't need to
  prescribe the map value type.
- **Opt-in introspection via trait**: The RFC principle is "built in from
  the start." The cost (one pointer per instance, static table per class)
  is negligible.
- **Static introspection on class names**: Aster has limited static methods.
  Introspection goes through instances.
- **Annotations**: No concrete use case. Can be added later via separate RFC.
