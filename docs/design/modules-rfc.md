# Modules & Imports RFC

**Status**: Decided
**Resolves**: BC-8
**Prerequisite for**: Protocols (cross-module type resolution), inline generics with imports, From/Into

---

## Summary

Every `.aster` file is a module. The `use` keyword imports public names from other modules. Module paths use `/` as separator and resolve to filesystem paths relative to the project root.

## Import Forms

### Selective import (Phase M1)

```
use models/user { User, AdminUser }
```

Imports specific named items directly into the current scope. Only items marked `pub` in the target module are visible.

### Wildcard import (Phase M1)

```
use models/user
```

Without `{ }` or `as`, imports **all public names** from the module directly into the current scope. Equivalent to `use models/user { * }`.

### Namespace import (Phase M2 — implemented)

```
use models/user as u
```

Imports the module as a namespace. Access via `u.User(...)`, `u.func()`, `u.VARIABLE`.

Namespace members are resolved in `check_member` — when the object is a namespace identifier, exports are looked up and classes/enums are injected into the environment on first access so constructor calls and field access work downstream.

Selective imports + alias is an error: `use foo { Bar } as s` → M004.

## Visibility

Everything is **private by default**. Only items marked `pub` are importable:

```
pub class User          # importable
  name: String          # field visibility is separate concern (deferred)

pub def greet() -> String    # importable
  "hello"

class Internal              # NOT importable
  x: Int

def helper() -> Int         # NOT importable
  42
```

`pub` applies to: `let` bindings, `def` functions, `class` definitions, `trait` definitions, `enum` definitions.

## Module Resolution

A module path maps to a filesystem path relative to the **project root** (the directory containing the entry file):

```
use models/user        →  <root>/models/user.aster
use std/http           →  <root>/std/http.aster
use utils              →  <root>/utils.aster
```

If the file doesn't exist, emit error M001 "Module not found".

## What Gets Exported

After typechecking a module, its exports are the `pub`-marked items:

- **`pub class`** → ClassInfo (type, fields, methods, generics, extends, includes)
- **`pub trait`** → TraitInfo (name, methods, required_methods)
- **`pub enum`** → EnumInfo (name, variants, includes)
- **`pub def` / `pub let`** → variable name + Type

Protocol metadata (`includes: ["Eq", "Ord"]`) transfers with the ClassInfo, so `==`/`<` work on imported types automatically.

## Circular Import Detection

The module loader tracks which modules are currently being compiled. If module A imports B and B imports A, emit error M003 "Circular import detected". This is a hard error — no lazy resolution.

## Re-exports (Phase M3 — implemented)

`pub use` re-exports imported items as part of this module's public API:

```
# facade.aster
pub use internal/user { User }        # selective re-export
pub use internal/utils                 # wildcard re-export (all pub items)
pub def helper() -> Int                # own definition alongside re-exports
  42
```

Consumers import from the facade module — they don't need to know about `internal/user`:

```
use facade { User, helper }
```

Re-exports are chained: if A does `pub use B { X }` and B does `pub use C { X }`, consumers can import X from A.

Only `pub` items from the source module are re-exportable. Non-`pub use` (regular import) does NOT re-export — it's for local use only.

## Transitive Imports

Without `pub use`, each module's exports are its own `pub` items only. If A imports B, and B imports C, A cannot access C's items through B unless B uses `pub use` to explicitly re-export them.

## Inline Generics Interaction

The inline generics heuristic (`is_known_type_name()`) checks `self.env` for known classes, traits, and enums. When `use` injects imported types into `self.env`, they become visible to the heuristic automatically. No special handling needed.

## Error Codes

| Code | Meaning |
|------|---------|
| M001 | Module not found |
| M002 | Name not exported by module (not `pub` or doesn't exist) |
| M003 | Circular import detected |
| M004 | Name not found in namespace / selective+alias not allowed |

## Architecture

### FileResolver trait

Abstracts filesystem access for testability:

```rust
pub trait FileResolver {
    fn resolve(&self, module_path: &[String]) -> Option<(String, String)>;
    // Returns (source_code, canonical_filename) or None
}
```

Two implementations:
- `FsResolver` — real filesystem for production
- `VirtualResolver` — HashMap-based for tests

### ModuleLoader

Owns the resolver and a cache of compiled module exports. Each module is compiled exactly once (cache on canonical path string).

### TypeChecker integration

`TypeChecker` gains an optional `module_loader: Option<Rc<RefCell<ModuleLoader>>>`. When present, `Stmt::Use` resolves imports. When absent (all existing tests), `Stmt::Use` remains a no-op. This preserves full backward compatibility.

### Export extraction

After typechecking a module, iterate its top-level statements to find `pub` items. Look up their types from the child TypeChecker's environment.

---

## Non-Goals (current)

- Standard library modules (`std/http` etc.) — needs stdlib to exist first
- Package management / external dependencies — deferred
- Field-level visibility enforcement — deferred
- Enum variant access through namespaces (`ns.Color.Red`) — import enums selectively instead
- Trait access through namespaces for `includes` — import traits selectively instead

## Design Note: Namespace-based, not Filesystem-based

Imports are **namespace-based**. There are no relative imports (`use ./sibling`) because module paths resolve from the project root. The `/` separator maps to directory structure but the mental model is namespace navigation, not filesystem traversal.
