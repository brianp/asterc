# Plan: Stack Traces

status: landed
issue: https://github.com/brianp/asterc/issues/15 (resolved in-repo; close the
GitHub issue out-of-band)

Landed: frame pointers forced on in both backends + staticlib; stack bounds
recorded for main / blocking pool / green threads; capture-once frame-pointer
walk at `throw`; the `Frame` builtin class and `error.trace() -> List[Frame]`;
`[runtime]` collapse of unresolved frames; JIT address-range registration and
AOT self-contained symbol data section resolving a PC to `function` + `file:line`
with AOT/JIT parity; `throw` and `!` now terminate/propagate by return with the
entry point rendering uncaught message + trace; per-statement line granularity
(FIR statement spans threaded to Cranelift srclocs, PC -> statement line); and a
typed error *value* thrown on a green thread or blocking-pool worker carries its
value/tag/trace across a cross-thread task `resolve`. The frame-trimming and
assertion-pairing logic for test failures ships wired into a minimal harness
core in `codegen/src/runtime/stacktrace.rs`: `run_test` runs one `test_*` case
and, on a thrown failure, `report_test_failure` resolves the captured trace,
takes the failing test's call-site as the injected assertion span
(`#[track_caller]`-style), trims the harness frames below the `test_*`
definition (`trim_below_test_frame`), and pairs the two
(`pair_failure_with_assertion`) into a `TestFailure`. This runs end-to-end
against a real JIT-compiled failing `test_*` program
(`test_harness_trims_below_test_frame_and_pairs_assertion_site`). The FULL
`asterc test` harness — file/`test_*` discovery, the `std/test` assertion
library and `AssertionError` type, the seeded runner, and the formatter seam —
builds on this core and remains issue #2.

Errors carry a captured stack trace. Native frame-pointer walk, symbolized
through compiler-emitted tables, captured once at throw. Zero steady-state
cost: no per-call bookkeeping, the price is paid only when an error is thrown.
This is the final mechanism, not a stepping stone.

## Why the design space is unusual

Aster has no unwinding. Errors propagate by return path: `!` checks the error
slot after a call and returns early. Two consequences:

1. DWARF-style unwind machinery is useless here — nothing unwinds.
2. A `throw` executes at the deepest live frame with the native stack fully
   intact, so the complete trace is walkable at that moment. Capture happens
   exactly once, on the error object; propagation upward is ordinary returns
   and appends nothing.

## Design

### Capture: frame-pointer walk at throw

- Frame pointers on everywhere: Cranelift `preserve_frame_pointers` in both
  JIT and AOT configs, and `force-frame-pointers` for the codegen staticlib
  build so runtime (Rust) frames walk cleanly too.
- At `aster_error_set_typed`, walk the FP chain from the current frame to the
  stack bounds. Green threads know their exact bounds (mmap'd stacks); the
  runtime records main-thread and blocking-pool stack bounds at init so
  throws from those contexts walk safely too.
- Frames that resolve in the Aster function table become real frames; frames
  that don't (runtime internals) collapse into a single `[runtime]` marker
  rather than noise.
- Recapture semantics: capture only if the error doesn't already carry a
  trace. A `catch` arm rethrowing the same error preserves the original
  trace; constructing and throwing a new error captures fresh.

### Symbolization: compiler-emitted tables

- **JIT**: the compiler knows every function's address range at compile time;
  register a sorted range table (address → function id) as functions are
  finalized.
- **AOT**: codegen emits a data section of (function address via relocation,
  size, name index) entries plus a string table; runtime init walks it into
  the same sorted structure. No dladdr, no platform symbol dependence —
  binaries stay self-contained.
- **Line info**: FIR lowering threads AST spans through to Cranelift srclocs
  (verify FIR span coverage; extend where lowering drops spans today). Each
  compiled function keeps its PC-offset → span table from the finalized
  machine buffer. Frame resolution = range lookup + offset lookup → file:line.

### Surface

- `Frame` builtin class: `function: String`, `file: String`, `line: Int`,
  including `Printable`.
- `error.trace() -> List[Frame]` — structured from day one; rendering is just
  `Printable` over the list. No string-only interim API.
- Uncaught error reaching the entry point: runtime renders the trace after
  the error message, every frame, worst frame first.
- Test harness (per docs/plans/testing-story.md): trims frames below the
  failing `test_*` def and pairs the trace with the injected assertion
  call-site span.

## Tasks

1. **Frame pointers + stack bounds** — Cranelift flags both modes; staticlib
   RUSTFLAGS; record main/blocking-pool stack bounds at runtime init.
2. **Span threading** — audit FIR for span coverage, attach srclocs in
   translation, keep PC→span tables per finalized function.
3. **Function tables** — JIT-side registration; AOT data-section emission +
   init-time registration; shared sorted-range lookup in the runtime.
4. **The walker** — bounded FP walk in `aster_error_set_typed`, resolution,
   `[runtime]` collapsing, capture-once semantics, trace storage on the error
   object (GC-allocated at throw; verify allocation is safe on the error
   path).
5. **Surface** — `Frame` class registration, `trace()` method, uncaught-error
   rendering at entry.
6. **Tests** — JIT + AOT parity: deep recursion, throw through nested calls,
   rethrow-preserves-trace, new-throw-in-catch captures fresh, traces from
   green threads and from the blocking pool, line accuracy against known
   spans.
7. **Docs** — advanced/errors page (trace surface), internals/codegen
   (tables + walker), status/roadmap updates, close #15.

## Explicitly rejected

- Side stack (per-call push/pop of frame metadata): steady-state cost on
  every call to subsidize the rare throw path, and a second source of truth
  for what the native stack already knows. Rejected outright, not deferred.
- String-only trace API as a first step. Structured now.
