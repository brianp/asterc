---
status: executed
created: 2026-03-16 18:00
executed: 2026-03-17
---

# Implementation Plan: Close Remaining Async Gaps

## Prerequisites

- Green thread runtime is fully operational (scheduler, context switching, work stealing)
- Phases 1-10 of green-threads.md are implemented
- Drop/Close traits: typechecker validates, FIR emits cleanup calls, codegen executes them (confirmed by `drop_called_on_scope_exit`, `drop_reverse_order`, `drop_called_on_explicit_return` tests)
- Mutex[T]: constructor, acquire/release, runtime wait queue — all working
- Channel[T]: constructor, send/wait_send/try_send, receive/wait_receive/try_receive, close — all working
- I/O poller infrastructure: kqueue (macOS), epoll (Linux), blocking pool — all working but not exposed to user code
- Must-consume (E027) and data sharing warnings (W002) — complete

## Codebase Analysis

**What's done:**
- `codegen/src/green/scheduler.rs` — work-stealing scheduler with full yield/suspend/wake lifecycle
- `codegen/src/green/poller.rs` — kqueue/epoll abstraction with register/deregister/poll
- `codegen/src/green/blocking.rs` — 4-thread blocking pool with job submission
- `codegen/src/runtime.rs` — `aster_io_wait_read`, `aster_io_wait_write`, `aster_blocking_submit` registered as runtime functions
- `codegen/src/runtime_source.rs` — C runtime parity for all of the above
- `codegen/src/runtime_sigs.rs` — signatures registered for io_wait_read/write and blocking_submit
- `fir/src/lower.rs` — `cleanup_locals` tracking, `emit_cleanup_calls()` emits Drop/Close before returns
- `typecheck/src/check_call.rs` — `resolve_all()` and `resolve_first()` typechecked
- `typecheck/src/check_expr.rs` — Mutex/Channel constructor and method type-checking

**What's NOT done (the gaps):**
1. Scoped `mutex.lock(value -> ...)` — no parser syntax, no typechecker escape analysis, no codegen
2. MultiSend[T] / MultiReceive[T] — not implemented anywhere
3. User-facing I/O primitives — poller + blocking pool exist but no language-level file/socket API
4. Drop/Close on early exit paths other than `return` — FIR emits cleanups before returns and at implicit function end, but NOT for break/continue or error propagation paths within nested scopes
5. Cancellation + Drop/Close integration — `unwind_green_thread` in scheduler references cleanup but doesn't actually call user Drop/Close methods

## Research Findings

**Key insight from analysis:** These gaps are coupled. Scoped locking and multi-channel close semantics both depend on reliable cleanup. The I/O primitives are the biggest "M:N credibility" gap — without them, users will accidentally block OS threads.

**Prioritization (from research):**
1. Drop/Close cleanup completeness (prerequisite — makes everything else sound)
2. Scoped mutex.lock() (most-requested safety feature, depends on cleanup)
3. I/O primitives (exposes existing poller to users, makes M:N actually useful)
4. MultiSend/MultiReceive (capability gap, but Channel[T] covers many patterns)

**Anti-patterns to avoid:**
- Don't implement scoped lock as sugar over acquire/release without guaranteed unlock on all exit paths
- Don't expose I/O APIs that can silently block an OS thread
- Don't add multi-channel without Drop-based close semantics (last sender drops → receiver sees EOF)

## Task Breakdown

### 1. Drop/Close Cleanup Completeness

- **Files to modify:**
  - `fir/src/lower<dot>rs` (emit cleanup calls on break/continue/error paths)
  - `fir/src/stmts<dot>rs` (no changes needed — cleanup emits as regular Call stmts)
  - `codegen/src/runtime<dot>rs` (add `aster_log_cleanup_error` for cleanup failures)
  - `codegen/src/runtime_source<dot>rs` (C parity for cleanup error logging)
- **Files to create:** None
- **Dependencies:** None — this is the foundation
- **Approach:** Currently `emit_cleanup_calls()` runs at two points: before implicit return (end of function body) and before explicit `Stmt::Return`. It needs to also run at: (a) `Stmt::Break` — emit cleanups for locals declared since the loop started, (b) `Stmt::Continue` — same, (c) error propagation (`!` operator) — emit cleanups before propagating. This requires tracking cleanup-local scopes (push on loop/block entry, pop on exit) so we know which locals to clean up at each exit point.
- **Integration points:** `lower_stmt_inner` for Break/Continue/Return handling; `lower_expr` for `!` error propagation
- **Key decisions:**
  - Scoped cleanup tracking: add a `cleanup_scope_stack: Vec<usize>` where each entry is the index into `cleanup_locals` at the start of that scope. On break/continue, clean up locals from current index back to the scope start.
  - Error in cleanup: log and continue (never propagate cleanup errors — matches RFC)
- **Implementation notes:**
  - Break/Continue: before emitting the FIR Break/Continue, call a new `emit_cleanup_calls_since(scope_start)` that only cleans up locals declared in the current loop body
  - Error propagation: the `!` lowering already generates error-check branches; the error branch needs cleanup calls before returning the error
  - Track scope depth so nested loops clean up correctly
- **Potential issues:**
  - Double cleanup: if a local is cleaned up at break AND at function end, we'd run Drop twice. Solution: cleanup_locals entries consumed/marked at their first cleanup point, removed from the outer scope's list.

### 2. Scoped `mutex.lock(value -> ...)`

- **Files to modify:**
  - `parser/src/expr<dot>rs` (parse `m.lock(value -> ...)` syntax)
  - `ast/src/expr<dot>rs` (add `MutexLock` AST variant or reuse method call + lambda)
  - `typecheck/src/check_expr<dot>rs` (type-check lock block, add escape analysis)
  - `typecheck/src/check_call<dot>rs` (handle `.lock()` method on Mutex[T])
  - `fir/src/lower<dot>rs` (lower to acquire/call/release with cleanup guarantee)
  - `codegen/src/runtime<dot>rs` (lock timeout support)
  - `codegen/src/runtime_source<dot>rs` (C parity for timeout)
- **Files to create:** None
- **Dependencies:** Task 1 (cleanup completeness — unlock must happen on all exit paths)
- **Approach:** The syntax `m.lock(value -> ...)` is already parseable as a method call with a trailing lambda argument. The typechecker recognizes `.lock()` on `Mutex[T]`, validates the lambda takes one parameter of type `T`, and runs escape analysis on the lambda body. FIR lowers this to: `acquire → let value = mutex_get → call lambda body → release`, with release guaranteed via the cleanup mechanism from Task 1.
- **Integration points:** Existing Mutex runtime functions (acquire/release); existing lambda/closure parsing
- **Key decisions:**
  - Escape analysis scope: restrict to the lambda body. The lambda parameter `value` is marked as "no-escape". Any assignment of `value` (or a field of `value`) to an outer-scope variable is a compile error. Passing `value` to a function is allowed (the function receives a copy — Aster's shallow copy semantics protect us). Capturing `value` in a nested closure that outlives the lock block is an error.
  - No-escape enforcement: add a `no_escape_vars: HashSet<String>` to the typechecker. When checking assignments, reject if the RHS references a no-escape var and the LHS is in an outer scope. When checking closure captures, reject if a no-escape var is captured.
  - Timeout: `m.lock(timeout: 5000, value -> ...)` — optional named arg. Default from `ASTER_LOCK_TIMEOUT_MS` env var (10_000ms). Throws `LockTimeoutError`.
- **Implementation notes:**
  - The lambda is NOT a real closure — it's inlined at the call site. FIR lowers the lock block as a sequence (acquire, body stmts, release), not as a closure call. This avoids allocation and simplifies escape analysis.
  - The `release` call is emitted via the cleanup mechanism: register the mutex as a cleanup-local with a synthetic "MutexGuard" that calls release on drop.
- **Potential issues:**
  - Nested locks: `m1.lock(a -> m2.lock(b -> ...))` — must work, cleanup in reverse order. The cleanup_locals stack handles this naturally.
  - Return inside lock block: must release before returning. Task 1's break/return cleanup handles this.

### 3. User-Facing I/O Primitives

- **Files to modify:**
  - `typecheck/src/check_call<dot>rs` (register I/O built-in functions)
  - `typecheck/src/typechecker<dot>rs` (register I/O types and functions)
  - `fir/src/lower<dot>rs` (lower I/O calls to RuntimeCall)
  - `codegen/src/translate<dot>rs` (translate I/O FIR nodes to runtime calls)
  - `codegen/src/runtime<dot>rs` (implement file/socket operations on top of existing poller)
  - `codegen/src/runtime_source<dot>rs` (C parity)
  - `codegen/src/runtime_sigs<dot>rs` (register new runtime function signatures)
- **Files to create:**
  - `tests/io_primitives<dot>rs` (end-to-end I/O tests)
- **Dependencies:** Task 1 (cleanup for file handles via Drop/Close)
- **Approach:** Expose a minimal I/O surface that covers the most common use cases: file read/write and TCP sockets. All operations are fiber-blocking by default (park the green thread, not the OS thread). The existing `aster_io_wait_read/write` and `aster_blocking_submit` runtime functions are the foundation — we build user-facing APIs on top. File I/O goes through the blocking pool (disk fds can't use kqueue/epoll meaningfully). Socket I/O uses the poller directly.
- **Integration points:** `codegen/src/green/poller.rs` (existing), `codegen/src/green/blocking.rs` (existing), `codegen/src/green/scheduler.rs` (io_wait_readable/writable already implemented)
- **Key decisions:**
  - Minimal surface: `File.read(path)`, `File.write(path, content)`, `File.append(path, content)` — return String/Void, throw IOError. These are blocking-pool operations (disk I/O).
  - TCP: `TcpListener.bind(port)`, `listener.accept()`, `TcpStream.connect(host, port)`, `stream.read()`, `stream.write(data)`, `stream.close()`. These use the poller for readiness.
  - All I/O types implement Close (async cleanup) — when a file handle or socket is closed, it deregisters from the poller.
  - Error types: `IOError` (general), `ConnectionError`, `TimeoutError` — all built-in.
  - No `async` keyword needed — these operations are fiber-blocking by default. Caller can still do `async File.read(path: p)` to get a Task[String].
- **Implementation notes:**
  - File operations: runtime function opens fd, submits read/write to blocking pool via `aster_blocking_submit`, green thread suspends, resumes with result.
  - Socket operations: runtime function creates nonblocking socket, registers with poller. On `read()`: try nonblocking read, if EWOULDBLOCK then `io_wait_readable(fd)` (suspends green thread), retry on wake. On `write()`: similar with `io_wait_writable`.
  - Handle objects: allocated on heap, tracked by runtime. Include fd, registered-with-poller flag, closed flag.
  - Close semantics: deregister from poller, close fd, wake any green threads blocked on this handle with IOError.
- **Potential issues:**
  - Lost wakeups: between "try nonblocking" and "register with poller", readiness could fire. Solution: always register first, then try nonblocking. If it succeeds, deregister. If EWOULDBLOCK, we're already registered and will wake correctly.
  - Platform differences: TCP connect on macOS vs Linux kqueue/epoll edge cases. Solution: abstract behind poller trait (already done).

### 4. MultiSend[T] and MultiReceive[T]

- **Files to modify:**
  - `typecheck/src/check_expr<dot>rs` (recognize MultiSend/MultiReceive constructors)
  - `typecheck/src/check_call<dot>rs` (type-check methods on multi-channel types)
  - `typecheck/src/typechecker<dot>rs` (register types)
  - `ast/src/types<dot>rs` (add MultiSend/MultiReceive type variants, or reuse Channel with a mode flag)
  - `fir/src/lower<dot>rs` (lower multi-channel operations to runtime calls)
  - `codegen/src/runtime<dot>rs` (multi-channel runtime with refcounted handles)
  - `codegen/src/runtime_source<dot>rs` (C parity)
  - `codegen/src/runtime_sigs<dot>rs` (new runtime function signatures)
- **Files to create:**
  - `tests/multi_channels<dot>rs` (multi-producer/consumer tests)
- **Dependencies:** Task 1 (Drop-based close semantics for last-sender/last-receiver EOF)
- **Approach:** MultiSend[T] and MultiReceive[T] are wrappers around the existing Channel runtime with reference-counted sender/receiver handles. When the last sender handle is dropped, the channel transitions to "sender-closed" state (receivers drain buffer then get EOF). When the last receiver is dropped, senders get ChannelClosedError. The runtime data structure is the same `AsterChannel` with added refcounts.
- **Integration points:** Existing `AsterChannel` struct, existing channel runtime functions
- **Key decisions:**
  - Constructor: `MultiSend(capacity: N)` returns a tuple-like pair `(sender: Sender[T], receiver: Receiver[T])`. Or: `let ch = MultiSend(capacity: 10)` returns a channel where `.send()` is multi-safe but `.receive()` is single-consumer. MultiReceive is the inverse.
  - Cloning: `sender.clone()` creates another sender handle (increments refcount). `receiver.clone()` for MultiReceive.
  - API: same three-tier as Channel[T] — send/wait_send/try_send, receive/wait_receive/try_receive, close.
  - Runtime difference: MultiSend wraps buffer access in a lock (already true for AsterChannel). MultiReceive distributes to the first waiting receiver (round-robin or FIFO — use FIFO for fairness).
- **Implementation notes:**
  - Add `sender_count: AtomicUsize` and `receiver_count: AtomicUsize` to `AsterChannelState`.
  - `clone_sender()` increments `sender_count`. `drop_sender()` decrements; if zero, mark channel sender-closed and wake all receivers.
  - `clone_receiver()` increments `receiver_count`. `drop_receiver()` decrements; if zero, mark channel receiver-closed and wake all senders with error.
  - Drop integration: MultiSend sender/receiver handles implement Drop. When dropped, decrement refcount. This is why Task 1 (cleanup completeness) must be done first.
- **Potential issues:**
  - Refcount race: decrement + check-zero must be atomic. Use `AtomicUsize::fetch_sub` + check for 1 (was last).
  - Wake correctness: when last sender drops and there are receivers blocked on wait_receive, all must be woken with a "channel closed" signal, not a value. Must not lose wake events.

## Potential Challenges & Mitigations

1. **Challenge:** Double cleanup — a local cleaned up at `break` might also be in the function-level cleanup list.
   **Mitigation:** Use a scope-indexed cleanup list. When cleanup fires at an inner scope exit (break/continue), remove those entries from the outer scope's list. Alternatively, mark entries as "already cleaned" with a flag.

2. **Challenge:** Escape analysis false positives — overly strict rules that reject valid programs.
   **Mitigation:** Start conservative (reject any outer-scope assignment of lock-scoped value). Users have the manual `acquire/release` API as an escape hatch. Relax rules based on real usage feedback.

3. **Challenge:** I/O lost wakeups between nonblocking-try and poller registration.
   **Mitigation:** Register-first pattern: register with poller before attempting nonblocking I/O. If the operation succeeds immediately, deregister. If EWOULDBLOCK, the registration is already active.

4. **Challenge:** Multi-channel refcount races on last-sender/last-receiver drop.
   **Mitigation:** Use `fetch_sub(1, Ordering::AcqRel)` and check if previous value was 1. This is the standard pattern (same as Arc).

## Unwired Code Audit

- [x] Scoped lock `release` has guaranteed execution path (Task 2 depends on Task 1 cleanup completeness)
- [x] I/O handle `close` has guaranteed execution path (Task 3 I/O types implement Close, Task 1 ensures it fires)
- [x] Multi-channel sender/receiver Drop decrements refcount (Task 4 depends on Task 1)
- [x] Every runtime function registered in `runtime_sigs.rs` has a caller in `translate.rs` or `lower.rs`
- [x] Every new type (File, TcpStream, etc.) has constructor + methods + error types all wired
- [x] Lock timeout has both the timer-set side (runtime) and the error-throw side (typechecker knows LockTimeoutError)
- [x] Multi-channel close triggers wake of all blocked fibers (runtime close path wakes waiters)

## Validation Steps

- All existing 1199+ tests pass unchanged
- `tests/drop_close.rs`: add tests for break-inside-loop cleanup, continue cleanup, error-propagation cleanup, nested scope cleanup
- `tests/mutex.rs`: add scoped lock tests — basic usage, escape rejection, timeout, nested locks, return-inside-lock cleanup
- `tests/io_primitives.rs`: file read/write round-trip, TCP echo server/client, I/O error handling, I/O + async interaction
- `tests/multi_channels.rs`: MPSC basic, last-sender-close wakes receiver, multiple receivers round-robin, stress test
- AOT parity: all new tests run with both `asterc run` and `asterc build` + execute
- Manual stress test: 1000 green threads doing concurrent file reads through blocking pool
- Manual stress test: 100 producers, 10 consumers on MultiSend channel
