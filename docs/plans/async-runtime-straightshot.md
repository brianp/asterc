# Async Runtime Recovery Plan

Status: in-progress
Rewritten: 2026-03-15
Supersedes: the previous "executed" straight-shot status in this file

## Purpose

This plan replaces the previous straight-shot framing with an honest finish plan from the repository's current state.

The old version of this file correctly described the target architecture, but it was treated as "done" once syntax, FIR nodes, tests, and local runtime models existed. That bar was too low. The compiled execution path still does not run real green threads.

This rewritten plan uses a stricter rule:

- A workstream is only done when the compiled `run` and `build` paths use the real implementation.
- Tests that target models, scaffolding, or intended shapes are necessary but not sufficient.
- Surface syntax, typechecking, and FIR nodes do not count as runtime completion by themselves.

## Fixed Decisions

These architecture decisions still stand. They are not being reopened by this rewrite.

### 1. Runtime model

- Decision: stackful green threads.
- Rejected alternatives: stackless futures/state machines, OS-thread-backed tasks.

### 2. Suspension model

- Decision: any Aster function may suspend at any call site.
- Implication: the implementation must support real suspension/resumption across ordinary function calls.

### 3. Stack substrate

- Decision: segmented, non-moving stacks managed by the runtime.
- Rejected alternative: fixed native stacks per task.

### 4. Context switching

- Decision: dedicated coroutine backend in C/Rust with minimal assembly shims.
- Rejected alternative: `ucontext`-style platform APIs.

### 5. Scheduler topology

- Decision: one local run queue per worker OS thread plus work stealing and a global injector queue.
- Decision detail: stealing takes 50% of another worker's queued tasks.

### 6. `main()` execution model

- Decision: `main()` runs as a green thread under the runtime scheduler.

### 7. Preemption and cancellation

- Decision: cooperative only.
- Decision detail: safe points are compiler-inserted at function calls and loop back-edges.
- Decision detail: cancellation is observed only at safe points.

### 8. Invocation model

- Decision: preserve four distinct call forms:
  - `f()`
  - `blocking f()`
  - `async f()`
  - `resolve task`
- Decision detail: plain `f()` against a suspendable callee is a compile error.

### 9. Suspendability

- Decision: suspendability is inferred and exported through module metadata.
- Decision detail: inference is conservative across unknown boundaries.

### 10. Task model

- Decision: `Task[T]` is a GC-visible handle to runtime-owned task state.
- Decision detail: terminal states are `Ready(T)`, `Failed(E)`, and `Cancelled`.
- Decision detail: `resolve` is single-consumer.

### 11. Cleanup model

- Decision: both `Close` and `Drop` are part of v1.
- Decision detail: runtime cleanup does `Close` then `Drop`, logs cleanup errors, and remains bounded during unwind.

### 12. Heap model

- Decision: one global heap/GC domain across all workers.
- Decision detail: initial collector stays stop-the-world and non-generational.

### 13. I/O model

- Decision: network I/O uses poller-based suspension.
- Decision: disk I/O uses blocking-thread-pool offload initially.

## Current Reality

This is the starting point for the rest of the work.

### What is genuinely in place

- The language surface for `blocking`, `async`, `detached async`, and `resolve` exists.
- Suspendability inference and plain-call diagnostics exist.
- Module metadata for suspendability exists.
- FIR has explicit async nodes such as `Spawn`, `BlockOn`, `ResolveTask`, `CancelTask`, `WaitCancel`, and `Safepoint`.
- There is a substantial async runtime model in `codegen/src/async_runtime.rs`.
- There is meaningful parser, typechecker, FIR, codegen, and benchmark coverage for the intended runtime behavior.

### What is not actually done

- `Spawn` in compiled code still lowers to an immediate direct function call plus task wrapping.
- `BlockOn` in compiled code still lowers to an immediate direct function call.
- Suspendable functions do not yet use a real coroutine entry/resume ABI in the compiled execution path.
- The scheduler/coroutine runtime model is not the engine that `run` and `build` use for compiled Aster programs.
- Task cancellation, `wait_cancel`, `resolve_first`, blocking jobs, and network waits still have shim behavior in the production runtime path.
- Async scope cleanup and `Close`/`Drop` ordering are not fully wired end to end.
- GC root handling for suspended tasks is covered in model tests but not fully integrated with the compiled runtime path.

## Workstream Audit

This section replaces the implied "1-13 are done" narrative.

### Workstream 1: Restore the language model

- Status: done
- Reason: syntax, parser, formatter, docs, and diagnostics exist and are exercised.

### Workstream 2: Suspendability analysis and diagnostics

- Status: mostly done
- Remaining gap:
  - keep cross-module and unknown-boundary behavior honest as runtime intrinsics are replaced with real runtime calls

### Workstream 3: Redesign FIR for real suspension

- Status: partial
- Done:
  - explicit FIR nodes exist
  - safepoint insertion exists
- Not done:
  - FIR async nodes are still consumed by codegen as direct-call shims instead of real suspension machinery

### Workstream 4: Coroutine ABI and codegen

- Status: not done
- Reason:
  - suspendable functions do not yet use a real coroutine ABI in the compiled path
  - `Spawn` and `BlockOn` are still direct calls in codegen

### Workstream 5: Runtime coroutine substrate

- Status: partial
- Done:
  - segmented-stack and task/scheduler model exists in a crate-local runtime module
- Not done:
  - compiled programs do not execute through that substrate

### Workstream 6: Scheduler

- Status: partial
- Done:
  - scheduler behavior exists in the runtime model and tests
- Not done:
  - compiled programs are not actually scheduled as green threads

### Workstream 7: Task state machine

- Status: partial
- Done:
  - intended state model exists in tests and runtime scaffolding
- Not done:
  - production task helpers still expose simplified handle-level behavior

### Workstream 8: Async scope and cleanup

- Status: partial
- Done:
  - syntax, FIR ownership markers, and some tests exist
- Not done:
  - runtime scope teardown, cleanup ordering, and unwind integration are not complete end to end

### Workstream 9: Heap and GC rewrite

- Status: partial
- Done:
  - model tests cover multi-worker and suspended-task roots
- Not done:
  - compiled runtime path is not yet a fully task-aware green-thread runtime with integrated root management

### Workstream 10: Blocking pool and foreign/native boundaries

- Status: partial
- Done:
  - modeled in async runtime tests
- Not done:
  - not fully wired through compiled execution

### Workstream 11: Network and disk I/O

- Status: partial
- Done:
  - modeled poller/blocking behavior and coverage exist
- Not done:
  - compiled runtime path does not yet suspend and resume real network work through the poller

### Workstream 12: Task combinators

- Status: partial
- Done:
  - syntax, typechecking, lowering, model runtime behavior, and tests exist
- Not done:
  - production runtime semantics still need to ride on the real task state machine

### Workstream 13: Test and benchmark matrix

- Status: partial but useful
- Done:
  - substantial test matrix and benchmark harness now exist
- Not done:
  - tests currently cover both real behavior and model behavior; the remaining implementation gap still needs to be closed

## New Delivery Plan

The rest of the work should be treated as an implementation recovery, not as "more polishing."

### Phase 1: Wire codegen to a real suspendable calling convention

- Replace direct-call lowering for `Spawn` and `BlockOn`.
- Introduce separate codegen paths for:
  - normal ABI functions
  - suspendable coroutine ABI functions
- Add compiler-generated entry/resume shims for suspendable functions.
- Make `blocking f()` enter coroutine execution without materializing a user-visible `Task[T]`.
- Make `async f()` create runtime task state and return a task handle tied to that state.

### Phase 2: Put the coroutine runtime on the compiled execution path

- Promote the runtime substrate from test/model code into the execution path used by JIT and AOT.
- Define the concrete runtime hooks that compiled code calls for:
  - task creation
  - first resume
  - subsequent resume
  - cooperative yield
  - cancellation request
  - terminal-state publication
- Remove shim behavior that pretends task execution already happened.

### Phase 3: Make the scheduler real

- Start worker threads for the runtime.
- Run `main()` as the first green thread.
- Route spawned tasks into worker-local queues.
- Route external wakeups into the global injector queue.
- Make stealing and wakeup policy part of the live runtime, not just model tests.

### Phase 4: Finish real task semantics

- Back `Task.is_ready()`, `cancel()`, `wait_cancel()`, and `resolve` with live task records.
- Store terminal values and failures in runtime-managed task state.
- Enforce real single-consumer resolution semantics in the runtime and preserve compile-time checks where detectable.
- Reimplement `resolve_all` and `resolve_first` on top of the live task state machine.

### Phase 5: Finish async scope and cleanup

- Track parent/child ownership in runtime task records.
- Cancel unresolved children on async-scope exit.
- Keep scope teardown bounded.
- Implement runtime cleanup order:
  - explicit user `Close`
  - then `Drop`
- Distinguish user-visible `Close` propagation from runtime best-effort cleanup logging.

### Phase 6: Wire blocking and I/O suspension

- Add the real blocking executor path used by compiled code.
- Route disk work to the blocking pool.
- Add the live network poller abstraction.
- Suspend tasks on non-ready network operations and resume them through poller wakeups.
- Ensure these operations feed suspendability inference and plain-call diagnostics consistently.

### Phase 7: Finish GC/runtime integration

- Integrate suspended-task stacks, live worker stacks, task records, and task result storage into one GC root model.
- Ensure stop-the-world coordination brings all workers to valid collection points.
- Validate that segmented non-moving stacks remain root-stable during collection.

### Phase 8: Rebaseline tests and docs against the live runtime

- Reclassify tests so model-only tests are clearly separate from end-to-end runtime tests.
- Add CLI-level tests that prove `run` and `build` use the real runtime path.
- Remove any docs or comments that imply eager semantics are acceptable.
- Only mark phases complete once the live runtime path passes the matching end-to-end tests.

## Definition of Done

This section is the key correction.

A phase or workstream is not done because syntax exists, FIR exists, or model tests pass. It is only done when all of the following are true for that slice:

- `cargo test --workspace` passes.
- The live `run` path exercises the real implementation.
- The live `build` path exercises the same semantics or an equivalent AOT path.
- No shim or eager direct-call lowering remains for the feature being marked complete.
- There is at least one end-to-end test that would fail if execution silently fell back to the old fake behavior.

## Hard Acceptance Criteria

The async runtime is only complete when these statements are true in the compiled execution path:

- A suspendable callee cannot be invoked with plain `f()` anywhere.
- `blocking f()` executes through coroutine machinery and may suspend without creating a user-visible `Task[T]`.
- `async f()` creates a real runtime task that executes independently of the caller.
- `resolve task` waits on the real task state machine and consumes exactly one result.
- `wait_cancel()` does more than `cancel()`; it waits for a terminal task state.
- `async scope` cancels unresolved children on scope exit and keeps unwind bounded.
- Worker threads actually run and steal green-thread work under load.
- Blocking native/disk work does not pin scheduler workers.
- Network I/O suspends and resumes tasks through the poller.
- GC traces live objects across multiple workers and suspended task stacks in the real runtime path.

## Required Proofs

These are the minimum proofs needed before this plan can be marked executed.

### Code-level proofs

- There is no direct-call lowering for `Spawn` in the production codegen path.
- There is no direct-call lowering for `BlockOn` in the production codegen path.
- Suspendable functions have distinct entry/resume code generation from non-suspendable functions.
- Production task helpers are backed by live runtime task records rather than immediate-value wrappers.

### End-to-end proofs

- A program with `async f()` and unrelated caller-side work shows true concurrency behavior, not eager execution.
- A program with `blocking f()` suspends the current green thread and lets other work run.
- A cancellation test proves `wait_cancel()` observes task termination instead of just flipping a bit.
- A `resolve_first` test proves the fastest task wins rather than "first in the list wins."
- A blocking-pool test proves worker threads remain available while blocking work is in flight.
- A network-wakeup test proves resumed work returns through the poller path.

### Benchmark proofs

- Web-shaped fan-out/fan-in benchmark runs on the live runtime.
- Many-sleeping-tasks benchmark exercises actual scheduling and wakeup behavior.
- Frequent-small-blocking-jobs benchmark exercises the real blocking executor path.
- GC pause benchmark runs under live multi-worker request-like load.

## Execution Order

Do the remaining work in this order:

1. Replace direct-call codegen for `Spawn` and `BlockOn`.
2. Land coroutine ABI and runtime entry/resume hooks.
3. Put the runtime substrate on the compiled execution path.
4. Start real scheduler workers and move `main()` under the scheduler.
5. Rebuild task state operations on the live runtime.
6. Finish async scope teardown and cleanup ordering.
7. Wire blocking pool and network poller.
8. Finish GC/runtime root integration.
9. Rebaseline combinators and end-to-end tests on the live runtime.
10. Only then mark this plan executed.

## Notes

- Channels and mutexes remain deferred until the core task/runtime system is truly finished.
- The current test matrix is useful. It is not wasted work. It just cannot be mistaken for full runtime completion.
- From this point on, "done" must mean "done in the live runtime path," not "done in syntax, FIR, or model tests."
