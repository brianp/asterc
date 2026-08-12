# Introspection API Implementation Plan

## Context
Converting `workingfiles/docs/design/introspection-rfc.md` into a completed feature. User chose to implement Foundation + Core introspection (Decision 1, option 2), deferring DynamicReceiver/FieldAccessible/std-unstable to a separate effort.

## What's Done

### Typechecker (complete)
- `Type`, `FieldInfo`, `MethodInfo`, `ParamInfo` registered as built-in classes in `register_builtins`
- 7 introspection methods resolve on every type via fallback in `check_member` -> `check_introspection_member`
- `is_a(ClassName)` validates bare type name identifiers (classes + primitives) in `check_is_a_call`
- `responds_to("name")` validates String argument in `check_responds_to_call`
- `==`/`!=` on Type values (exempted from Eq requirement)
- `.to_string()` on Type values via `check_type_member`
- User-defined methods shadow introspection (tested)
- 62 integration tests, all passing

### FIR Lowering (routing only)
- `fir/src/lower/introspection.rs` intercepts introspection in `lower_expr` (Member) and `lower_method_call`
- Passes static type name as a string to RuntimeCall functions
- `resolve_static_type_name` resolves the compile-time type from type_table/local_ast_types/literal

### Codegen Runtime (stubs)
- 7 `aster_introspect_*` functions registered in `runtime_sigs.rs`
- `codegen/src/runtime/introspection.rs` exists with stub implementations
- Only `class_name` actually works (returns the type name string)

## What's Not Done (must finish now)

### 1. `fields` returns actual FieldInfo objects
- **Current**: returns empty list
- **Fix**: Lowerer has access to `self.type_env.get_class(name)` which contains `fields: IndexMap<String, Type>` and `pub_fields: HashSet<String>`. Serialize field metadata (name, type name, is_public) into the RuntimeCall args. Runtime function constructs FieldInfo instances and returns a list.

### 2. `methods` returns actual MethodInfo objects
- **Current**: returns empty list
- **Fix**: ClassInfo has `methods: HashMap<String, Type>` and `pub_methods: HashSet<String>`. For each method, extract param names/types from the Function type. Serialize into RuntimeCall args. Runtime constructs MethodInfo/ParamInfo instances.

### 3. `ancestors` walks the real class hierarchy
- **Current**: returns list with just self
- **Fix**: Lowerer can walk `ClassInfo.extends` chain via `self.type_env`. Collect all ancestor names. Pass them as serialized string to runtime. Runtime constructs Type values (strings) for each.

### 4. `children` returns real child classes
- **Current**: returns empty list
- **Fix**: Lowerer must scan all classes in `self.type_env` to find which ones have `extends == this_class`. Pass child names to runtime.

### 5. `is_a` walks ancestor chain
- **Current**: exact string match only
- **Fix**: Lowerer can resolve this entirely at compile time for statically-known types. Walk the extends chain from the object's type. If target is anywhere in the chain, emit `BoolLit(true)`. Otherwise `BoolLit(false)`. No runtime call needed.

### 6. `responds_to` actually checks
- **Current**: always returns false
- **Fix**: The argument is a string literal in most cases, but can be a variable. For string literal args, the lowerer can resolve at compile time (check fields + methods + inherited + built-in methods). For variable args, pass the full method/field name list to runtime and let it do a string search.

### 7. Primitive built-in methods for `responds_to` and `methods`
- Int methods: is_even, is_odd, abs, clamp, min, max
- Float methods: abs, round, floor, ceil, clamp, min, max
- String methods: length, contains, starts_with, ends_with, trim, to_upper, to_lower, slice, replace, split
- Bool: (none beyond to_string)
- List: push, pop, length, get, set, insert, remove, contains, etc.

## Implementation Approach

The key insight: for statically-known types (which is ALL types in current Aster codegen since there's no dynamic dispatch), everything can be resolved at compile time in the lowerer. The lowerer knows the full class hierarchy, all fields, all methods.

**Strategy**: Move the logic from runtime stubs into the lowerer. The lowerer constructs FIR expressions that build the introspection objects directly, or emits compile-time constants where possible.

### For `is_a`: emit `BoolLit(true/false)` directly (compile-time resolution)
### For `responds_to` with string literal arg: emit `BoolLit(true/false)` directly
### For `responds_to` with variable arg: pass field+method name list to runtime, runtime does string search
### For `class_name`: keep current RuntimeCall (returns type name as string/Type value)
### For `fields`/`methods`/`ancestors`/`children`: lowerer serializes metadata into RuntimeCall string args, runtime deserializes and constructs objects

Serialization format for RuntimeCall args: pipe-delimited strings.
- `fields`: `"name:String:true|age:Int:false"` (name:type:is_public)
- `methods`: `"greet:0::String:true|add:2:x:Int,y:Int:Int:false"` (name:param_count:params:ret:is_public)
- `ancestors`: `"TimeoutError|NetworkError|AppError|Error|Exception"`
- `children`: `"Dog|Cat"`

Runtime functions parse these strings and allocate the FieldInfo/MethodInfo/ParamInfo/Type objects.

## Out of Scope (per Decision 1)
- DynamicReceiver trait / method_missing / FunctionNotFound
- Three-mode compiler behavior (strict/hybrid/open)
- FieldAccessible trait
- std/unstable module / --unstable flag

These are separate features from the introspection API and were explicitly deferred by user choice.

## Files to Modify
- `fir/src/lower/introspection.rs` - bulk of the work, serialize real metadata
- `codegen/src/runtime/introspection.rs` - deserialize and construct real objects
- `codegen/src/runtime_sigs.rs` - may need signature adjustments

## Verification
- All 62 introspection integration tests pass
- All 1,882+ existing tests pass
- `cargo fmt/clippy/machete/audit` clean
- Manual test: `asterc run` with a program that uses introspection and prints results
