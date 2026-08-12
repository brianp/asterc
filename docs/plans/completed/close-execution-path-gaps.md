---
status: in-progress
created: 2026-03-13 01:56
executed: null
---

# Implementation Plan: Close Missing Execution Paths

## Prerequisites
- The current goal is not "more features on paper". It is parity between `check`, `run`, and `build` for the language surface already presented in the README, examples, and tests.
- Keep the existing pipeline shape: `lexer -> parser -> typecheck -> FIR lowering -> codegen -> runtime`.
- Prefer shared runtime and lowering helpers over adding more one-off paths in `src/main<dot>rs`.

## Codebase Analysis
- The compiler is split cleanly across workspace crates, with execution concentrated in `fir/src/lower<dot>rs`, `codegen/src/translate<dot>rs`, `codegen/src/runtime<dot>rs`, and `src/main<dot>rs`.
- `check` is materially ahead of `run` and `build`. The typechecker accepts language constructs that FIR lowering still rejects or only partially lowers.
- Root integration coverage is front-end heavy. `tests/examples<dot>rs` goes through `tests/common/mod<dot>rs::compile_file`, which stops at lex/parse/typecheck and never exercises the executable path.
- The codegen crate has stronger backend coverage than the root binary, but the root driver still carries its own AOT runtime source in `src/main<dot>rs`, which creates drift risk.
- The clearest current gaps are:
  - top-level statement support is narrower in FIR than the example surface suggests
  - some nullable and error-handling operations typecheck but are only partially executable
  - async, match, trait-heavy, and module-heavy examples pass `check` but are not treated as executable contracts
  - JIT and AOT can diverge when runtime support is duplicated

## Research Findings
- No external provider or MCP research tools were available in this environment, so this plan is based on direct repository inspection and verified CLI behavior.
- Best practice to adopt here: define executable support as an explicit contract matrix and drive it through end-to-end tests, not README claims or typechecker-only examples.
- Anti-pattern to avoid: adding isolated lowering branches without also wiring the runtime, CLI behavior, and integration tests that prove the feature works in both `run` and `build`.
- Anti-pattern to avoid: duplicated runtime definitions. The AOT runtime and JIT runtime should have one source of truth for exported behavior and symbol coverage.

## Task Breakdown

### 1. Define the executable support contract
- **Files to modify:**
  - README<dot>md
  - docs/architecture/compiler-pipeline<dot>md
  - tests/examples<dot>rs
- **Files to create:** none
- **Dependencies:** none
- **Approach:** Split the language surface into three explicit support levels: typecheck-only, JIT-executable, and AOT-executable. Use that contract to decide whether a feature needs implementation work or simply a narrowed claim.
- **Integration points:** Aligns the public language story with the root CLI and example suite.
- **Key decisions:**
  - Contract-first: execution support is a tested capability matrix, not an implicit claim
  - Example ownership: every example file is either executable by contract or clearly marked as front-end only
- **Implementation notes:** Add a small support matrix section to the docs and map each example to its expected execution level.
- **Potential issues:** This may force a temporary reduction in claims while backend support catches up.

### 2. Unify runtime ownership between JIT and AOT
- **Files to modify:**
  - codegen/src/runtime<dot>rs
  - codegen/src/runtime_sigs<dot>rs
  - src/main<dot>rs
  - codegen/src/tests<dot>rs
- **Files to create:**
  - codegen/src/runtime_c<dot>rs or codegen/src/runtime_source<dot>rs
- **Dependencies:** Task 1
- **Approach:** Move the embedded C runtime source out of `src/main<dot>rs` and into the `codegen` crate so both the runtime symbol table and the AOT runtime implementation are maintained together. Keep a single exported source string and a single exported symbol list.
- **Integration points:** `src/main<dot>rs::cmd_build` consumes the shared runtime source; `codegen` tests assert symbol parity and selected behavior parity.
- **Key decisions:**
  - Single source of truth: runtime symbol declarations and runtime implementation live together
  - Parity tests: every runtime helper added for JIT must have an AOT parity assertion
- **Data structures:**
  ```text
  RuntimeSupport: { symbol_name, jit_impl, aot_impl, covered_by_test }
  ```
- **Potential issues:** Some helpers may be easier in Rust than C. If so, the plan must either implement them in C or mark the feature as JIT-only until parity exists.

### 3. Finish nullable and error-path lowering as a coherent subsystem
- **Files to modify:**
  - fir/src/lower<dot>rs
  - fir/src/tests<dot>rs
  - codegen/src/tests<dot>rs
- **Files to create:** none
- **Dependencies:** Task 1
- **Approach:** Treat nullable execution as its own lowering layer instead of a collection of scattered special cases. Standardize representation for nullable locals, parameters, returns, field values, collection elements where applicable, and method helpers such as `.or()`, `.or_else()`, `.or_throw()`, and `match`.
- **Integration points:** FIR lowering feeds both JIT and AOT; error propagation relies on runtime flag helpers and caller checks already present in the codegen path.
- **Key decisions:**
  - Representation consistency: nullable values use one executable representation across lets, returns, calls, and matches
  - Helper completeness: support all four nullable operations claimed in the README, not just the ones needed by interpolation or returns
- **Implementation notes:** Add red tests first for `let`, `const`, param passing, field assignment, list access, and nullable `match` execution.
- **Potential issues:** The current boxed-pointer model may become brittle once nullable enums and richer payloads are exercised. If that happens, introduce a first-class FIR nullable abstraction before adding more one-off fixes.

### 4. Close the top-level statement gap
- **Files to modify:**
  - fir/src/lower<dot>rs
  - fir/src/tests<dot>rs
  - tests/examples<dot>rs
- **Files to create:** none
- **Dependencies:** Tasks 1 and 3
- **Approach:** Decide which top-level statements should execute directly, which should desugar into an init thunk, and which should remain illegal outside functions. Then implement that policy explicitly.
- **Integration points:** Affects example files, REPL behavior, and any future module initialization semantics.
- **Key decisions:**
  - Top-level `let` and expression statements stay executable
  - Top-level `for`, `if`, and similar side-effecting statements either lower into an init function or are rejected consistently at parse/typecheck time
  - Top-level `trait` and `use` are compile-time declarations only and should not block execution when present in otherwise executable modules
- **Implementation notes:** The unwired bug to avoid here is lowering declaration statements while forgetting to wire their initialization or visibility effects into the generated entry path.
- **Potential issues:** Module initialization order gets harder once imports and top-level executable statements coexist.

### 5. Make `match` executable for the supported value categories
- **Files to modify:**
  - fir/src/lower<dot>rs
  - codegen/src/translate<dot>rs
  - fir/src/tests<dot>rs
  - codegen/src/tests<dot>rs
- **Files to create:** none
- **Dependencies:** Task 3
- **Approach:** Finish lowering for `match` on integers, booleans, strings, nullable values, and enums that already typecheck. Use a staged rollout: literals and wildcard first, enum variants second, nullable-arm semantics third.
- **Integration points:** Examples 12 and many error-handling and pattern-matching tests stop being check-only once this lands.
- **Key decisions:**
  - Lower simple matches to nested `if` chains first for correctness
  - Add specialized lowering later only if performance becomes relevant
- **Implementation notes:** Reuse the current enum variant metadata gathered in FIR instead of introducing a second pattern-dispatch table.
- **Potential issues:** String matching will need runtime equality support if it is to execute natively and not just typecheck.

### 6. Decide the first shippable async execution slice
- **Files to modify:**
  - README<dot>md
  - fir/src/lower<dot>rs
  - codegen/src/tests<dot>rs
  - tests/examples<dot>rs
- **Files to create:** none initially
- **Dependencies:** Task 1
- **Approach:** Pick a deliberately narrow async contract for the next milestone. The most pragmatic slice is "eager lowering for `async f()` and `resolve` with no scheduler", because parts of that behavior already exist in crate-local tests.
- **Integration points:** Keeps async examples from being permanently check-only while avoiding premature runtime architecture.
- **Key decisions:**
  - No scheduler in this milestone
  - `async`, `resolve`, and `detached async` only ship if they have executable semantics in both tests and CLI behavior
- **Potential issues:** Pretend-async semantics can confuse users if the docs do not clearly say what is and is not concurrent yet.

### 7. Separate front-end examples from executable examples
- **Files to modify:**
  - examples/09_collections<dot>aster
  - examples/11_generics_and_traits<dot>aster
  - examples/12_async_errors_matching<dot>aster
  - examples/13_throws_and_extends<dot>aster
  - tests/examples<dot>rs
  - tests/common/mod<dot>rs
- **Files to create:**
  - examples/executable/
  - examples/spec/
- **Dependencies:** Task 1
- **Approach:** Stop using one examples directory for two different jobs. Executable examples should run under `asterc run` or `asterc build`; spec examples can remain front-end-only while a feature is in progress.
- **Integration points:** Makes CI intent obvious and prevents false confidence from typecheck-only examples passing.
- **Key decisions:**
  - Directory split over naming convention, because it is harder to misuse in CI
  - Each executable example gets an expected outcome or output assertion
- **Potential issues:** Some existing examples may need to be trimmed into smaller executable slices rather than kept as full feature showcases.

### 8. Add root-level end-to-end execution tests
- **Files to modify:**
  - tests/common/mod<dot>rs
  - tests/examples<dot>rs
  - tests/error_handling<dot>rs
  - tests/pattern_matching_async<dot>rs
- **Files to create:**
  - tests/execution_cli<dot>rs
  - tests/aot_cli<dot>rs
- **Dependencies:** Tasks 2 through 7
- **Approach:** Introduce helpers that invoke the built `asterc` binary for `run` and `build`, assert on exit status, and capture stdout/stderr. Use them to promote a subset of language tests from "typechecks" to "executes correctly".
- **Integration points:** This closes the current blind spot where root integration tests stop before FIR/codegen/runtime.
- **Key decisions:**
  - Keep crate-local backend tests, but add CLI tests as the release gate
  - Assert on user-visible behavior, not just on successful lowering
- **Implementation notes:** Start with a stable contract set: hello world, list iteration, constructor calls, map literals, `to_string`, nullable fallbacks, and selected enum matches.
- **Potential issues:** CLI tests can be slower. Split them into a targeted execution suite instead of running every front-end test twice.

### 9. Add a feature gate for "declared but not executable yet"
- **Files to modify:**
  - src/main<dot>rs
  - fir/src/lower<dot>rs
  - ast/src/diagnostic<dot>rs
- **Files to create:** none
- **Dependencies:** Task 1
- **Approach:** When a user invokes `run` or `build` on a feature that still typechecks but is outside the current executable contract, emit a structured "feature not executable yet" diagnostic instead of a raw lowering discriminant.
- **Integration points:** Improves CLI trust while backend support is being filled in.
- **Key decisions:**
  - Replace opaque lowering fallback errors with intentional diagnostics for unsupported execution paths
  - Keep those diagnostics structured so future MCP tooling can act on them
- **Potential issues:** This can become a crutch. Do this only for features explicitly deferred by the contract matrix, not for regressions.

## Potential Challenges & Mitigations
1. **Challenge:** Feature interactions will surface representation bugs, especially around nullable values, enums, and match lowering.
   **Mitigation:** Add red tests at the FIR and CLI layers for each new execution slice before expanding the supported matrix.
2. **Challenge:** JIT and AOT parity can drift again if runtime helpers are added ad hoc.
   **Mitigation:** Centralize runtime ownership and gate merges on a symbol-parity test plus at least one AOT execution test.
3. **Challenge:** Example files are currently overloaded as both language tours and regression tests.
   **Mitigation:** Split executable examples from spec examples and make CI intent explicit.
4. **Challenge:** Async can sprawl into runtime architecture work too early.
   **Mitigation:** Lock the first milestone to an eager execution model and defer real concurrency until the contract is proven end to end.

## File Description Updates
- codegen/src/runtime<dot>rs or codegen/src/runtime_source<dot>rs
- fir/src/lower<dot>rs
- tests/common/mod<dot>rs
- tests/execution_cli<dot>rs
- tests/aot_cli<dot>rs
- tests/examples<dot>rs

## Codebase Overview Updates
- Compiler pipeline section: clarify current executable contract vs front-end support
- Root CLI section: note `check`, `run`, and `build` parity expectations
- Testing section: add CLI execution suites and executable example coverage

## Unwired Code Audit
For each feature, verify that both sides of every data flow are accounted for in the plan:
- [x] Every flag/status checked has a code path that sets it
- [x] Every endpoint/function created has an identified caller
- [x] Every state transition has a trigger AND its accompanying side-effects

## Validation Steps
- Add an execution support matrix and verify each claimed executable feature has at least one root-level `run` or `build` test.
- Run `cargo test --workspace` and a separate CLI execution suite after each milestone.
- Verify that `examples/executable/` succeeds under `asterc run` or `asterc build`, depending on its contract.
- Verify that unsupported-yet-checked features produce explicit diagnostics, not lowering discriminant errors.
- Verify JIT/AOT runtime parity by checking runtime symbols and at least one executable test per newly added runtime helper.
