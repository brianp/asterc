---
status: pending
created: 2026-04-02
executed: null
---

# Implementation Plan: Runtime JIT Evaluation

## Prerequisites

- Closures with capture-by-reference are implemented and passing all 36 integration tests
- `use` statements work inside class method bodies (verified)
- DynamicReceiver bare-call fallback routes through `method_missing` when `current_class` is set (typecheck/src/check_call.rs:286-289)
- CraneliftJIT compiles self-contained FIR modules and exposes `call_i64()` entry points (codegen/src/jit.rs)
- Closure lowering heap-lifts captured variables into env structs (fir/src/lower/closure.rs)

## Phases

### Phase 1: Extract reusable JIT pipeline

**Goal:** Pull the compile-and-execute pipeline out of `cmd_run` in `src/main.rs` into a standalone function in the codegen crate that can be called from anywhere.

**Files to modify:**
- src/main.rs (extract `frontend_and_lower` + JIT execution into codegen-callable function)
- codegen/src/lib.rs (expose new public API)

**Files to create:**
- codegen/src/eval_pipeline.rs (reusable JIT pipeline entry point)

**Approach:** The current `cmd_run` does: read source, lex, parse, typecheck, lower FIR, JIT compile, call entry point. Extract everything after "read source" into a function:

```
pub fn jit_compile_and_run(source: &str, filename: &str) -> Result<i64, RuntimeEvalError>
```

This function takes a source string and returns a result. No file I/O, no `process::exit`. Errors are returned, not printed.

**Testable:** Write a Rust unit test in `codegen/src/eval_pipeline.rs` that calls `jit_compile_and_run` with a simple Aster program string (e.g., `"def main() -> Int\n  42"`) and asserts the return value is 42. Write a second test with invalid syntax and assert it returns a RuntimeEvalError with kind `"syntax"`.

---

### Phase 2: JIT-from-JIT (nested JIT invocation)

**Goal:** Prove that JIT-compiled code can invoke the JIT pipeline. This is the critical path validation. If Cranelift can't be called from within Cranelift-compiled code, the entire design fails.

**Files to modify:**
- codegen/src/eval_pipeline.rs (expose as a runtime-callable function)
- codegen/src/runtime_sigs.rs (register `aster_runtime_jit_eval` signature)
- codegen/src/runtime/mod.rs (add runtime_eval module)

**Files to create:**
- codegen/src/runtime/runtime_eval.rs (runtime entry point: `aster_runtime_jit_eval(code_ptr) -> i64`)

**Approach:** Create a runtime function `aster_runtime_jit_eval` that takes a string pointer, converts it to a Rust string, and calls the `jit_compile_and_run` function from Phase 1. Register it in the runtime signatures so JIT-compiled code can call it. Write an Aster test program that calls this function with a hardcoded string.

**Testable:** An Aster program that does:
```
use std/runtime { jit_run }

def main() -> Int
  jit_run(code: "def main() -> Int\n  0")
  0
```
Run via `asterc run`. Verify it completes without crashing. This proves JIT-from-JIT works. If this phase fails, the entire RFC is infeasible and must be revisited.

---

### Phase 3: Context capture serialization

**Goal:** When the compiler encounters an `evaluate()` call, snapshot the calling scope's type metadata and serialize it into the binary.

**Files to modify:**
- typecheck/src/typechecker.rs (add method to serialize current scope state)
- fir/src/lower/stmt.rs (detect `evaluate()` calls and trigger context capture)
- codegen/src/translate.rs (emit serialized context as data section alongside the call)

**Files to create:**
- typecheck/src/context_snapshot.rs (serialization of TypeChecker state: current_class, imports, variable types, method signatures)
- fir/src/eval_context.rs (FIR representation of captured context)

**Approach:** Define a `ContextSnapshot` struct that contains: optional class name + class definition (fields, methods, traits), import map (module path to available symbols), local variable map (name to type). When the FIR lowerer encounters an `evaluate()` call, it asks the typechecker to produce a `ContextSnapshot` for the current scope. This snapshot is serialized (serde) and embedded in the FIR module as a data blob associated with the call site.

**Testable:** Write a test that compiles an Aster program containing `evaluate()` inside a class method, then inspects the FIR module to verify the context snapshot is present and contains the expected class definition and import tree. Write a second test for `evaluate()` inside a bare function and verify the snapshot contains the expected imports and locals but no class context.

---

### Phase 4: JIT loads context and typechecks against it

**Goal:** The JIT pipeline accepts a `ContextSnapshot` and uses it to set up the typechecker before compiling the evaluated code string.

**Files to modify:**
- codegen/src/eval_pipeline.rs (accept `ContextSnapshot` parameter)
- typecheck/src/typechecker.rs (add constructor that initializes from a `ContextSnapshot`)

**Approach:** Extend `jit_compile_and_run` to accept an optional `ContextSnapshot`. When present, the typechecker is initialized with `current_class` set, imports loaded, and variable types pre-populated. The code string is then parsed and typechecked in this pre-populated context. For class context, `current_class` is set so DynamicReceiver bare-call fallback activates.

**Testable:** Write a test that creates a `ContextSnapshot` with a simple class definition (name: String, a `set_name` method), passes it to the JIT along with the code string `"set_name(value: \"hello\")"`, and verifies typechecking passes. Write a second test with `"nonexistent_method(x: 1)"` against a class without DynamicReceiver and verify it returns a type error.

---

### Phase 5: Runtime value passing (closure env for `self` and locals)

**Goal:** The evaluated code can access and mutate `self` and local variables from the calling scope at runtime.

**Files to modify:**
- fir/src/lower/stmt.rs (treat `evaluate()` as a closure boundary for captured variables)
- fir/src/lower/closure.rs (reuse env boxing for call sites that use runtime evaluation)
- codegen/src/eval_pipeline.rs (accept env pointer and wire it into JIT-compiled function)
- codegen/src/runtime/runtime_eval.rs (pass env pointer from runtime call)

**Approach:** When the FIR lowerer encounters `evaluate()` inside a method, it treats the call like a closure boundary. `self` and any referenced locals are heap-lifted into a closure env struct. The env pointer is passed to the runtime function alongside the code string and context snapshot. The JIT-compiled code receives the env pointer and accesses `self`/locals through it, exactly as closures do.

**Testable:** Write an Aster program:
```
class Counter
  pub count: Int

  pub def run_code(code: String) -> Void
    evaluate(code: code)

def main() -> Int
  let c = Counter(count: 0)
  c.run_code(code: "self.count = 42")
  c.count
```
Run via `asterc run`. Verify exit code is 42. This proves `self` mutation through the runtime evaluation mechanism works.

---

### Phase 6: Function pointer capture

**Goal:** The evaluated code can call methods that were compiled in the host binary.

**Files to modify:**
- typecheck/src/context_snapshot.rs (include function addresses in snapshot)
- codegen/src/eval_pipeline.rs (register host function pointers with JIT)
- codegen/src/translate.rs (emit function addresses into context data)

**Approach:** When the context snapshot is created at compile time, the compiler records which methods exist on the class and which imported functions are available. At JIT setup time, the actual function addresses are resolved and included. When the JIT compiles the evaluated code, it uses these addresses for direct calls rather than trying to compile the called functions from source.

For methods already compiled into the binary, the runtime populates the function pointer table from the binary's symbol table. For runtime functions (GC, allocator, etc.), the existing runtime_sigs mechanism already handles this.

**Testable:** Write an Aster program where a class has a method that does non-trivial work (e.g., string manipulation), and the evaluated code calls that method:

```
class Greeter
  pub greeting: String

  pub def greet(name: String) -> Void
    greeting = "Hello, " + name

  pub def run_code(code: String) -> Void
    evaluate(code: code)

def main() -> Int
  let g = Greeter(greeting: "")
  g.run_code(code: "greet(name: \"world\")")
  say(message: g.greeting)
  0
```
Verify "Hello, world" is printed.

---

### Phase 7: RuntimeEvalError and error boundary

**Goal:** Compile errors and runtime panics in evaluated code surface as catchable errors, not process crashes.

**Files to modify:**
- codegen/src/runtime/runtime_eval.rs (wrap JIT call in panic catch)
- codegen/src/eval_pipeline.rs (return structured errors from all pipeline stages)

**Files to create:**
- ast/src/eval_error.rs (RuntimeEvalError type definition: kind + message)

**Approach:** The `jit_compile_and_run` function already returns `Result<_, RuntimeEvalError>` from Phase 1. Now add panic trapping: wrap the JIT function call in `std::panic::catch_unwind`. If the evaluated code panics, convert the panic message into a RuntimeEvalError with kind `"runtime"`. Syntax errors, type errors, and lowering errors already produce errors from earlier phases.

On the Aster side, `EvalError` is a class with `kind: String` and `message: String` fields. The `evaluate()` function is declared `throws EvalError`.

**Testable:** Four tests:
1. Evaluated code with a syntax error -> EvalError(kind: "syntax", ...)
2. Evaluated code with a type error -> EvalError(kind: "type", ...)
3. Evaluated code that divides by zero -> EvalError(kind: "runtime", ...)
4. Evaluated code that succeeds -> no error, mutations visible

All four should be runnable via `asterc run` without crashing the host process.

---

### Phase 8: `--jit` flag for `asterc build`

**Goal:** AOT-compiled binaries can optionally include the JIT for runtime evaluation.

**Files to modify:**
- src/main.rs (add `--jit` flag to build command argument parsing)
- src/build_dir.rs (potentially adjust build paths for JIT-enabled builds)
- Cargo.toml (conditional feature flag for bundling JIT dependencies)

**Approach:** When `--jit` is passed to `asterc build`, the linker includes the full compiler frontend (lexer, parser, typechecker, FIR lowerer) and Cranelift JIT as additional static libraries alongside `libcodegen.a`. Without `--jit`, `evaluate()` calls compile to a function that immediately returns an EvalError with kind `"runtime"` and message `"JIT not available: rebuild with --jit"`.

If the source imports `std/runtime` and `--jit` is not set, the compiler emits a warning: "std/runtime imported but --jit not enabled; evaluate() will fail at runtime."

**Testable:** Build a program that uses `evaluate()` with `--jit`. Run the binary. Verify the runtime evaluation mechanism works. Build the same program without `--jit`. Run the binary. Verify it produces the "JIT not available" error, not a crash.

---

### Phase 9: `std/runtime` module and `evaluate()` stdlib function

**Goal:** Expose `evaluate()` as a proper stdlib function that Aster code can import.

**Files to modify:**
- typecheck/src/typechecker.rs (register `std/runtime` module with `evaluate` and `EvalError`)
- fir/src/lower/stmt.rs (lower `evaluate()` calls to the runtime function with context capture)
- codegen/src/runtime_sigs.rs (register the runtime function signature)

**Approach:** `std/runtime` exports two symbols: `evaluate(code: String) throws EvalError -> Void` and the `EvalError` class. The typechecker recognizes `evaluate` as a special builtin (like `say` or `args`) that triggers context capture during FIR lowering. The FIR lowerer emits: serialize context snapshot, box captured locals into env, call `aster_runtime_jit_eval(code_ptr, context_ptr, env_ptr)`.

**Testable:** An Aster program that does:
```
use std/runtime { evaluate }

def main() throws EvalError -> Int
  let x = 10
  evaluate(code: "say(message: to_string(value: x))")
  0
```
Run via `asterc run`. Verify "10" is printed.

---

### Phase 10: DSL mode Seedfile evaluation

**Goal:** The Seedfile class has an `execute()` method that uses `evaluate()` to run DSL-mode Seedfiles.

**Files to modify:**
- aster-pkg/src/seedfile.aster (add `execute()` method)
- aster-pkg/src/main.aster (replace the current wrapper/subprocess pipeline with `seed.execute()`)

**Approach:** Add `pub def execute(code: String) throws EvalError -> Void` to the Seedfile class. Inside it, call `evaluate(code: code)`. The class context (DynamicReceiver, all DSL methods) is captured automatically. Remove the `wrap_seedfile`, `seedfile_class_source`, `seedfile_constructor`, `seedfile_footer`, `transform_line` functions. Remove the temp file creation, subprocess call, and output parsing in `cmd_info`.

For DSL mode, `aster` scans for `allow_eval()` on the first line. If absent, the Seedfile is evaluated via `seed.execute()`. The context capture ensures only DSL calls (resolving through DynamicReceiver) are available.

**Testable:** Run `aster info` on the existing `aster-pkg/Seedfile`. Verify the same output as the current implementation (package name, version, dependencies, tasks, overrides). Run the existing `test_eval.aster` and `test_seedfile.aster` tests and verify they still pass.

---

### Phase 11: Code mode (`allow_eval()`) Seedfile evaluation

**Goal:** Seedfiles with `allow_eval()` can use imports, conditionals, and arbitrary code.

**Files to modify:**
- aster-pkg/src/main.aster (add `allow_eval()` scanning and code-mode evaluation path)
- aster-pkg/src/seedfile.aster (add `execute_unrestricted()` or similar method)

**Approach:** When `aster` detects `allow_eval()` on the first line, it strips that line and uses an unrestricted evaluation path that permits `use` statements in the evaluated code. The code still runs as a method body on Seedfile (so bare DSL calls work), but the JIT processes `use` statements at the top of the code string before typechecking the rest.

**Testable:** Create a test Seedfile with `allow_eval()`:
```
allow_eval()
use std/sys { env_get }

package(pkg_name: "test-app", version: "1.0.0")
compiler(ver: "0.1.0")

let env = env_get(key: "ASTER_ENV").or(default: "development")

if env == "production"
  sentry(version: "2.0.0")

http(version: "1.2.0")
```
Run `ASTER_ENV=production aster info` and verify sentry appears in the output. Run `aster info` without the env var and verify sentry does not appear.

---

### Phase 12: `seedfile(version:)` and `Seedfile.lock` caching

**Goal:** Version declaration for forward compatibility, and lockfile caching to avoid re-evaluation.

**Files to modify:**
- aster-pkg/src/seedfile.aster (add `seedfile()` method, version field)
- aster-pkg/src/main.aster (add version checking, lockfile read/write)

**Files to create:**
- aster-pkg/src/lock.aster (Seedfile.lock read/write logic)

**Approach:** `seedfile(version: 1)` is a DSL call that sets a version field on the Seedfile object. After evaluation, `aster` checks the version against its supported range. If unsupported, it prints the upgrade message and exits.

`Seedfile.lock` is a serialized representation of the evaluated Seedfile (package name, version, resolved deps, tasks, overrides). On subsequent runs, `aster` compares the Seedfile's hash against the lockfile. If unchanged, it loads from the lockfile without re-evaluation.

**Testable:** Run `aster info` twice. Verify the second run is faster (skips evaluation). Modify the Seedfile and run again. Verify re-evaluation occurs. Add `seedfile(version: 999)` and verify the version error message appears.

---

## Potential Challenges & Mitigations

1. **Challenge:** Reconstructing TypeChecker state from a serialized snapshot may be fragile as the typechecker evolves.
   **Mitigation:** Define `ContextSnapshot` as a stable, versioned format separate from TypeChecker internals. The snapshot stores the logical information (types, methods, imports), not TypeChecker implementation details.

2. **Challenge:** Function pointer addresses are not known until link time for AOT builds.
   **Mitigation:** For JIT-from-JIT (asterc run), addresses are known at JIT time. For AOT builds with `--jit`, use relocation-style fixups: embed placeholder addresses in the snapshot, patch them at binary load time using the symbol table.

3. **Challenge:** The FIR lowerer has existing limitations (cross-module calls, List[ClassName] iteration) that may interact with the context of runtime evaluation.
   **Mitigation:** The runtime evaluation mechanism in the Seedfile class is a single-module operation. It doesn't need cross-module FIR calls. The evaluated code is compiled as a fresh FIR module, not merged into the host's FIR.

4. **Challenge:** GC must see roots in JIT-compiled stack frames during collection.
   **Mitigation:** The existing JIT already handles GC roots (asterc run works today). The runtime evaluation path uses the same JIT, so GC root handling is inherited.

## Validation Steps

- [x] Phase 1: `jit_compile_and_run("def main() -> Int\n  42")` returns 42 from Rust
- [x] Phase 2: An Aster program invoking the JIT from JIT-compiled code completes without crashing
- [x] Phase 3: FIR module contains serialized context snapshot at call sites using runtime evaluation
- [x] Phase 4: JIT typechecks evaluated code against a pre-populated context
- [x] Phase 5: Evaluated code mutates `self.field` and the caller sees the mutation
- [x] Phase 6: Evaluated code calls a host-compiled method and it executes correctly
- [x] Phase 7: Syntax errors, type errors, and panics in evaluated code produce EvalError, not crashes
- [x] Phase 8: `--jit` flag produces a working binary; no `--jit` produces a clear error
- [x] Phase 9: `use std/runtime { evaluate }` works in normal Aster code
- [x] Phase 10: `aster info` works without subprocess/serialization machinery
- [x] Phase 11: `allow_eval()` Seedfiles with conditionals and imports work
- [ ] Phase 12: `Seedfile.lock` caching skips re-evaluation; version checking works
