---
status: done
created: 2026-03-28 17:00
executed: 2026-03-28
---

# Implementation Plan: OS Primitives for Package Manager

Five stdlib modules that expose operating system primitives to Aster code. These are the last compiler-side blockers before the package manager can be written in Aster.

All five follow the same pattern: Rust `extern "C"` functions in `codegen/src/runtime/`, registered via the `runtime_functions!` macro, exposed as importable functions through virtual stdlib modules (`use std/sys`, `use std/fs`, etc.), and lowered via `FirExpr::RuntimeCall` in the FIR layer.

## Design Decision: Functions, Not Static Methods

The existing `File.read(path:)` pattern uses static methods on a class name. That was fine as a one-off but doesn't scale. These new primitives are standalone functions imported from stdlib modules, matching how traits are imported today:

```
use std/sys { args, env, set_env, exit }
use std/fs { read_file, write_file, exists, mkdir, list_dir, remove, copy, rename }
use std/process { run }
use std/crypto { sha256 }
```

This is consistent with how `use std/cmp { Eq }` and `use std/fmt { Printable }` work. No classes, no static methods. Just functions you import and call.

The existing `File.read`/`File.write`/`File.append` static methods stay as-is for backwards compat within this release cycle. The new `std/fs` functions are the canonical path going forward.

## Codebase Analysis

The pattern for adding runtime functions is well-established:

1. **Runtime function** (`codegen/src/runtime/<module>.rs`): `#[unsafe(no_mangle)] pub extern "C" fn aster_*(...)`
2. **Signature registration** (`codegen/src/runtime_sigs.rs`): Add to `runtime_functions!` macro
3. **Re-export** (`codegen/src/runtime/mod.rs`): `pub use <module>::*;`
4. **Typechecker** (`typecheck/src/typechecker.rs`): Register functions in a new virtual stdlib module via `builtin_std_submodule_exports`
5. **FIR lowering** (`fir/src/lower/method.rs` or `expr.rs`): Map function calls to `FirExpr::RuntimeCall`

Key files already doing this:
- `codegen/src/runtime/io.rs` has `aster_file_read`, `aster_file_write`, `aster_file_append`
- `codegen/src/runtime/string.rs` has string operations
- `codegen/src/runtime_sigs.rs` has 70+ functions in `runtime_functions!`
- `typecheck/src/typechecker.rs` has `builtin_std_submodule_exports` mapping submodule names to trait/enum exports
- `typecheck/src/check_expr.rs` has `check_file_static_member` for the existing File.read pattern

The new wrinkle: existing stdlib modules export **traits and enums**. These new modules export **functions**. The `ModuleExports` struct already has a `variables` field (`HashMap<String, Type>`) which can hold function types. The `builtin_exports_from` helper only populates traits and enums today, so we need a new helper or an extension.

## Task Breakdown

### 1. Extend stdlib module exports to support functions

- **Files to modify:** `typecheck/src/typechecker.rs`
- **Approach:** Add a `builtin_function_exports` helper (or extend `builtin_exports_from`) that populates `ModuleExports.variables` with function types. Each OS module will call this to register its exported functions.
- **Integration points:** `builtin_std_submodule_exports` match arms, `resolve_use` for injecting function bindings
- **Key decision:** Functions are registered as variables with `Type::Function` values, same as how `def` bindings work. When the user writes `use std/sys { args }`, the typechecker injects `args` as a variable of type `() -> List[String]` into the caller's scope.

### 2. std/sys module (argv, env vars, exit)

- **Files to create:** `codegen/src/runtime/sys.rs`
- **Files to modify:** `codegen/src/runtime/mod.rs`, `codegen/src/runtime_sigs.rs`, `typecheck/src/typechecker.rs`
- **Dependencies:** Task 1
- **Runtime functions:**
  - `aster_sys_args() -> *mut u8` (returns a List[String] pointer)
  - `aster_sys_env_get(key: *mut u8) -> *mut u8` (returns String, nil-tagged if not set)
  - `aster_sys_env_set(key: *mut u8, value: *mut u8)` (sets env var)
  - `aster_sys_exit(code: i64)` (terminates process)
- **Stdlib surface:**
  - `args() -> List[String]`
  - `env(key: String) -> String?`
  - `set_env(key: String, value: String) -> Void`
  - `exit(code: Int) -> Void`
- **FIR lowering:** When the typechecker sees a call to an imported `args` function, it resolves to a known qualified name (e.g., `std.sys.args`). The lowerer maps this to `FirExpr::RuntimeCall { name: "aster_sys_args", ... }`.
- **Implementation notes:** `aster_sys_args` uses `std::env::args()`, allocates a List via `aster_list_new`/`aster_list_push`, and returns the handle. `aster_sys_env_get` uses `std::env::var()`, returns nil-tagged pointer on `Err(NotPresent)`. `aster_sys_exit` calls `std::process::exit()`.

### 3. std/fs module (filesystem operations)

- **Files to create:** `codegen/src/runtime/fs.rs`
- **Files to modify:** `codegen/src/runtime/mod.rs`, `codegen/src/runtime_sigs.rs`, `typecheck/src/typechecker.rs`
- **Dependencies:** Task 1
- **Runtime functions:**
  - `aster_fs_read_file(path: *mut u8) -> *mut u8` (String, sets error on failure)
  - `aster_fs_write_file(path: *mut u8, content: *mut u8)` (sets error on failure)
  - `aster_fs_append_file(path: *mut u8, content: *mut u8)` (sets error on failure)
  - `aster_fs_exists(path: *mut u8) -> i8` (Bool)
  - `aster_fs_is_dir(path: *mut u8) -> i8` (Bool)
  - `aster_fs_mkdir(path: *mut u8)` (recursive, sets error on failure)
  - `aster_fs_remove(path: *mut u8)` (recursive for dirs, sets error on failure)
  - `aster_fs_list_dir(path: *mut u8) -> *mut u8` (List[String], sets error on failure)
  - `aster_fs_copy(src: *mut u8, dst: *mut u8)` (sets error on failure)
  - `aster_fs_rename(src: *mut u8, dst: *mut u8)` (sets error on failure)
- **Stdlib surface:**
  - `read_file(path: String) -> String throws IOError`
  - `write_file(path: String, content: String) -> Void throws IOError`
  - `append_file(path: String, content: String) -> Void throws IOError`
  - `exists(path: String) -> Bool`
  - `is_dir(path: String) -> Bool`
  - `mkdir(path: String) -> Void throws IOError`
  - `remove(path: String) -> Void throws IOError`
  - `list_dir(path: String) -> List[String] throws IOError`
  - `copy(src: String, dst: String) -> Void throws IOError`
  - `rename(src: String, dst: String) -> Void throws IOError`
- **Implementation notes:** `read_file` and `write_file` delegate to the existing `aster_file_read`/`aster_file_write` runtime functions (or share the same Rust implementation). `mkdir` uses `std::fs::create_dir_all`. `remove` uses `std::fs::remove_file` for files and `std::fs::remove_dir_all` for directories. `list_dir` uses `std::fs::read_dir`, collects entry names into a List.

### 4. std/process module (process spawning)

- **Files to create:** `codegen/src/runtime/process.rs`
- **Files to modify:** `codegen/src/runtime/mod.rs`, `codegen/src/runtime_sigs.rs`, `typecheck/src/typechecker.rs`
- **Dependencies:** Task 1
- **Runtime functions:**
  - `aster_process_run(cmd: *mut u8, args_list: *mut u8) -> *mut u8` (returns a ProcessResult class pointer)
- **Stdlib surface:**
  - `run(cmd: String, args: List[String]) -> ProcessResult throws ProcessError`
- **ProcessResult class** (registered as a built-in class):
  - `exit_code: Int`
  - `stdout: String`
  - `stderr: String`
- **ProcessError class** (registered as a built-in error class extending Error):
  - `message: String` (inherited)
  - `command: String`
- **Implementation notes:** `aster_process_run` uses `std::process::Command::new(cmd).args(args).output()`. It allocates a ProcessResult object (3 fields: exit_code as i64, stdout and stderr as string pointers). On spawn failure, sets error flag with ProcessError. Synchronous, no async.
- **Key decision:** The runtime function receives the args list handle, iterates it using `aster_list_len`/`aster_list_get` to build the Rust `Command`, then packs the output into a ProcessResult. This avoids inventing a new argument-passing convention.

### 5. std/crypto module (hashing)

- **Files to create:** `codegen/src/runtime/crypto.rs`
- **Files to modify:** `codegen/src/runtime/mod.rs`, `codegen/src/runtime_sigs.rs`, `typecheck/src/typechecker.rs`
- **Dependencies:** Task 1
- **Runtime functions:**
  - `aster_crypto_sha256(data: *mut u8) -> *mut u8` (returns hex digest as String)
- **Stdlib surface:**
  - `sha256(data: String) -> String`
- **Implementation notes:** Uses the `sha2` crate already in `Cargo.toml`. `Sha256::digest(data.as_bytes())` then hex-encode. One function, no error cases (hashing can't fail).

### 6. FIR lowering for stdlib function calls

- **Files to modify:** `fir/src/lower/expr.rs` (or a new `fir/src/lower/stdlib.rs`)
- **Files to modify:** `fir/src/builtins.rs` (add module/function constants)
- **Dependencies:** Tasks 2-5
- **Approach:** When lowering a function call, check if the callee is a known stdlib function (by its qualified name or a marker in the type environment). If so, emit the corresponding `FirExpr::RuntimeCall` with the correct runtime function name.
- **Key decision:** The typechecker registers each stdlib function with a qualified name (e.g., the function `args` imported from `std/sys` is tracked as originating from that module). The lowerer uses this to map to the correct `aster_*` runtime function. The simplest approach: the typechecker stores a mapping of `variable_name -> runtime_call_name` in a side table, and the lowerer consults it.
- **Integration points:** `lower_call` in `fir/src/lower/expr.rs` already handles function calls. Add a check before the general-purpose call lowering path.

### 7. Tests

- **Files to create:** `tests/integration/os_primitives.rs` (single file for all five modules)
- **Files to modify:** `tests/integration/main.rs` (register module)
- **Approach:** One test file with sections for each module. Typechecker tests (import works, types are correct, missing import errors). Where possible, end-to-end tests using `asterc run` to verify runtime behavior (e.g., `args()` returns a list, `exists()` checks a real file, `sha256()` returns expected digest).
- **Test categories per module:**
  - Contract: import syntax works, function signatures are correct
  - Happy path: functions return expected values
  - Error: IOError thrown on invalid paths, missing env vars return nil
  - Rejection: importing nonexistent functions from these modules fails
  - Composition: functions work inside other functions, with error handling

## Unwired Code Audit

- [ ] Each `aster_*` runtime function (producer) is registered in `runtime_functions!` macro AND re-exported in `mod.rs` (consumed by JIT + AOT)
- [ ] Each stdlib module (producer: `builtin_std_submodule_exports`) exports function types consumed by `resolve_use`
- [ ] Each exported function type (producer: typechecker) is consumed by FIR lowering to emit the correct `RuntimeCall`
- [ ] ProcessResult and ProcessError classes (producer: `register_builtins`) are consumed by user code via field access and catch blocks
- [ ] Error-setting runtime functions (producer: `aster_error_set()`) are consumed by the `throws` mechanism in the typechecker and the error-check codegen
- [ ] `with_loader` removes these modules from prelude scope (producer) so that module-mode code must import them explicitly (consumer: `resolve_use`)

## Potential Challenges & Mitigations

1. **Challenge:** Stdlib modules currently only export traits and enums, not functions.
   **Mitigation:** `ModuleExports` already has a `variables` field. We populate it with `Type::Function` values. The `resolve_use` path already injects variables into scope from module exports.

2. **Challenge:** FIR lowering doesn't know which function calls map to runtime functions.
   **Mitigation:** Use a side table or naming convention. The typechecker can tag stdlib functions with metadata, or the lowerer can check a hardcoded mapping of known stdlib function names to runtime call names.

3. **Challenge:** `aster_process_run` needs to read the args List from Aster's runtime representation.
   **Mitigation:** Call existing `aster_list_len`/`aster_list_get` from within the runtime function. Runtime functions can call each other, this is already done (e.g., `aster_string_to_rust` is called from `aster_file_read`).

4. **Challenge:** AOT parity. All runtime functions must work in both JIT and AOT.
   **Mitigation:** The `runtime_functions!` macro handles both paths. JIT gets function pointers via `runtime_builtin_symbols()`. AOT gets them via the staticlib linked at compile time. No special work needed per function.

5. **Challenge:** Process spawning on different platforms.
   **Mitigation:** `std::process::Command` is cross-platform. No platform-specific code needed.

## Validation Steps

- [ ] `use std/sys { args }` typechecks and `args()` returns `List[String]`
- [ ] `use std/sys { env }` typechecks and `env(key: "PATH")` returns `String?`
- [ ] `use std/sys { exit }` typechecks and `exit(code: 0)` compiles
- [ ] `use std/fs { read_file, write_file, exists, mkdir, list_dir, remove }` all typecheck
- [ ] `exists(path: "/tmp")` returns `true` at runtime
- [ ] `write_file` then `read_file` round-trips content
- [ ] `mkdir` creates a directory, `list_dir` lists it, `remove` deletes it
- [ ] `use std/process { run }` typechecks, `run(cmd: "echo", args: ["hello"])` returns ProcessResult with stdout "hello\n"
- [ ] `run` with invalid command throws ProcessError
- [ ] `use std/crypto { sha256 }` typechecks, `sha256(data: "hello")` returns known hex digest
- [ ] All five modules require explicit import (not available in prelude with module loader)
- [ ] All five modules work in both JIT (`asterc run`) and AOT (`asterc build`)
- [ ] Importing a nonexistent function from any module produces a clear error
- [ ] Error-throwing functions (`read_file`, `run`, etc.) work with `catch` blocks
