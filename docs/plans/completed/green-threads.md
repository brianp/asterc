# Green Threads & Async Runtime — Execution Plan

Status: PHASES 1-3 COMPLETE (JIT), PHASES 4-10 PLANNED
Depends on: async-rfc.md (DECIDED), codegen milestone 11+
Scope: Full RFC delivery — green threads, I/O poller, Mutex[T], channels, Drop/Close, must-consume, data sharing warnings
Parity: JIT (Rust) and AOT (C) runtimes both get green threads

---

## Current State

The async **programming model** is complete — syntax, parsing, typechecking, FIR lowering, and codegen all work end-to-end. But the runtime substrate is fake:

- **JIT** (`runtime.rs`): Each `async f()` spawns work on a real OS thread pool via `thread::spawn`. `Mutex`+`Condvar` for synchronization. `aster_safepoint()` is a no-op.
- **AOT** (`runtime_source.rs`): Each `async f()` does `pthread_create`. One OS thread per task, no pooling.
- **`async_runtime.rs`**: Has `CoroutineBody` trait, `SegmentedStack`, task scheduling logic — all simulated. No real stack switching. None of this is wired into the actual task execution path.

What's missing: real stack switching (assembly), cooperative preemption, I/O suspension, Mutex[T], channels, Drop/Close, must-consume enforcement, data sharing warnings.

---

## Phase 1: Assembly Stack Switching

**Goal**: A working `context_switch(old_ctx, new_ctx)` function that saves/restores registers and swaps the stack pointer. Two platforms: aarch64 (Apple Silicon — primary) and x86_64 (Linux).

### 1.1 — Define the context structure

Create `codegen/src/green/context.rs`:

```rust
#[repr(C)]
pub struct MachineContext {
    // Callee-saved registers + stack pointer + return address
    // Layout must match the assembly exactly
    regs: [u64; CONTEXT_REGS],  // platform-dependent count
}
```

- aarch64: x19-x28 (10 callee-saved), x29 (fp), x30 (lr), sp, d8-d15 (8 SIMD) = 22 slots
- x86_64: rbx, rbp, r12-r15 (6 callee-saved), rsp, rip = 8 slots

### 1.2 — Write assembly trampolines

Create `codegen/src/green/asm/`:
- `aarch64.s` — `aster_context_switch`, `aster_context_init`
- `x86_64.s` — `aster_context_switch`, `aster_context_init`

**`aster_context_switch(old: *mut MachineContext, new: *const MachineContext)`**:
1. Save callee-saved registers to `old`
2. Save current sp to `old.sp`
3. Load sp from `new.sp`
4. Restore callee-saved registers from `new`
5. Return (ret pops to the new context's return address)

**`aster_context_init(ctx: *mut MachineContext, stack_top: *mut u8, entry: fn(*mut u8), arg: *mut u8)`**:
1. Set `ctx.sp` to `stack_top` (aligned to 16 bytes)
2. Set up the stack frame so that when `context_switch` restores this context, execution begins at `entry(arg)`
3. Place a sentinel return address (trap/abort) below entry so a bare `ret` from entry doesn't go to garbage

### 1.3 — Stack allocation

Create `codegen/src/green/stack.rs`:

```rust
pub struct GreenStack {
    base: *mut u8,    // mmap'd region base
    size: usize,      // usable size (excluding guard page)
    guard: usize,     // guard page size
}
```

- Allocate via `mmap` with `MAP_ANONYMOUS | MAP_PRIVATE`
- Place a guard page at the bottom (`mprotect` with `PROT_NONE`) — stack overflow = SIGSEGV on the guard page, clean crash
- Default size: 8KB usable + 4KB guard. Growable stacks are a later optimization (the segmented stack in `async_runtime.rs` can be revisited then)
- Pool stacks for reuse — `StackPool` with a freelist, bounded size

### 1.4 — Rust FFI binding

Create `codegen/src/green/mod.rs`:

```rust
extern "C" {
    fn aster_context_switch(old: *mut MachineContext, new: *const MachineContext);
    fn aster_context_init(
        ctx: *mut MachineContext,
        stack_top: *mut u8,
        entry: extern "C" fn(*mut u8),
        arg: *mut u8,
    );
}
```

Link the assembly via `build.rs` using `cc::Build::new().file("src/green/asm/aarch64.s").compile("green_asm")`.

### 1.5 — Unit tests

- Test `context_switch` round-trips: create two contexts, switch between them, verify register state
- Test `context_init`: create a context pointing at a test function, switch to it, verify it runs and returns
- Test guard page: spawn a green thread that overflows its stack, verify SIGSEGV (not corruption)
- Test on both aarch64 and x86_64 (CI matrix)

### Deliverable
`context_switch` and `context_init` work on aarch64 and x86_64. Stacks are mmap'd with guard pages. Stack pooling works. All tested in isolation.

---

## Phase 2: M:N Scheduler Rewrite

**Goal**: Replace the current OS-thread-per-task model with a real M:N scheduler. N OS worker threads run M green threads using the assembly context switching from Phase 1.

### 2.1 — Green thread representation

Create `codegen/src/green/thread.rs`:

```rust
pub struct GreenThread {
    context: MachineContext,
    stack: GreenStack,
    id: TaskId,
    state: ThreadState,
    cancel_requested: bool,
    home_worker: usize,
}

enum ThreadState {
    Runnable,
    Running,
    Suspended,
    Blocked(BlockReason),
    Terminal(TerminalState),
}

enum TerminalState {
    Ready(i64),
    Failed(i64),
    Cancelled,
}

enum BlockReason {
    WaitingOnTask(TaskId),
    WaitingOnIo(RawFd),
    WaitingOnMutex(MutexId),
    WaitingOnChannel(ChannelId),
    WaitingOnBlockingPool,
}
```

### 2.2 — Worker thread loop

Rewrite `worker_loop` in `runtime.rs`:

```
loop:
  1. Pop green thread from local deque
  2. If empty, check global injector
  3. If empty, try steal half from random victim
  4. If empty, poll I/O (Phase 5) with timeout
  5. If still empty, park (condvar wait)

  When a green thread is found:
    context_switch(worker.scheduler_ctx, green_thread.context)
    // execution resumes here when green thread yields back
    handle the yield reason (reschedule, suspend, complete, etc.)
```

Each worker OS thread has its own `MachineContext` (the "scheduler context"). Switching to a green thread saves the scheduler context and loads the green thread's context. When the green thread yields (safepoint, I/O, completion), it switches back to the scheduler context.

### 2.3 — Task lifecycle

- **Spawn**: allocate `GreenThread` (stack from pool + `context_init` pointing at the task entry function), push to local deque
- **Yield**: green thread calls `aster_safepoint()` → switches back to scheduler context, scheduler re-enqueues the green thread
- **Suspend**: green thread suspends (waiting on I/O, mutex, resolve) → switches back to scheduler, green thread is NOT re-enqueued (stays in suspended state until woken)
- **Complete**: green thread's entry function returns → trampoline stores result, transitions to `Terminal`, switches back to scheduler
- **Cancel**: set `cancel_requested` flag, if suspended then wake it (re-enqueue), next safepoint check sees the flag and unwinds

### 2.4 — Replace `aster_task_spawn` and friends

Rewrite the runtime functions that codegen calls:
- `aster_task_spawn(entry, args, scope)` → allocate green thread, enqueue, return handle
- `aster_task_block_on(entry, args)` → spawn + suspend current green thread until result ready
- `aster_task_resolve_*` → suspend current green thread until target task is terminal
- `aster_safepoint()` → check preemption flag, if set then yield to scheduler
- `aster_async_scope_enter/exit` → unchanged API, but cancellation now cooperates with green thread unwinding

### 2.5 — Thread-local worker state

Each worker needs thread-local access to:
- Its own scheduler context
- The currently-running green thread
- The preemption flag (set by scheduler when a green thread has run too long)

Use `#[thread_local]` or `thread_local!` for this state.

### 2.6 — Work stealing deque

Replace `Mutex<VecDeque<TaskPtr>>` with a lock-free Chase-Lev work-stealing deque. The owner pushes/pops from one end, thieves steal from the other. This eliminates lock contention on the hot path.

Options:
- Use the `crossbeam-deque` crate (battle-tested)
- Or write a simple bounded deque — the contention surface is small

### 2.7 — Unit tests

- Spawn 1 green thread, verify it runs and completes
- Spawn 1000 green threads, verify all complete
- Verify work stealing: pin spawning to worker 0, verify workers 1..N steal and execute
- Verify `resolve` suspends the caller until the target completes
- Verify `block_on` works (spawn + wait)
- Verify cancellation: cancel a running green thread, verify it stops at next safepoint
- Stress test: spawn 10K tasks that each spawn 10 subtasks, resolve all

### 2.8 — End-to-end .aster tests

```
# test_green_spawn.aster
def work(n: Int) -> Int
  n * 2

let t = async work(n: 21)
let result = resolve t!
print(result)   # 42
```

```
# test_green_many.aster
def fib(n: Int) -> Int
  if n <= 1
    n
  else
    let a = async fib(n: n - 1)
    let b = async fib(n: n - 2)
    let ra = resolve a!
    let rb = resolve b!
    ra + rb

print(fib(n: 10))  # 55
```

### Deliverable
`async f()` spawns a real green thread. Green threads run on N OS worker threads via context switching. Work stealing balances load. `resolve`, `block_on`, cancellation all work. Old OS-thread-per-task model is gone.

---

## Phase 3: Safepoint Preemption

**Goal**: `aster_safepoint()` actually yields the green thread when the scheduler says it's been running too long. Prevents any single green thread from starving others.

### 3.1 — Preemption flag

Each worker has a flag that the scheduler sets when the current green thread has exceeded its time slice:

```rust
thread_local! {
    static PREEMPT: AtomicBool = AtomicBool::new(false);
}
```

The scheduler sets this flag via a timer or tick counter. When `aster_safepoint()` reads it as true, the green thread yields back to the scheduler.

### 3.2 — Implement `aster_safepoint()`

Replace the no-op:

```rust
pub extern "C" fn aster_safepoint() {
    // Fast path: no preemption needed — single atomic load
    if !PREEMPT.load(Ordering::Relaxed) {
        return;
    }
    PREEMPT.store(false, Ordering::Relaxed);
    // Yield: switch back to scheduler context
    yield_to_scheduler(YieldReason::Preempted);
}
```

Cost: one relaxed atomic load per safepoint. Negligible.

### 3.3 — Cancellation check in safepoint

Safepoints also check the current green thread's `cancel_requested` flag:

```rust
pub extern "C" fn aster_safepoint() {
    let thread = current_green_thread();
    if thread.cancel_requested {
        // Begin unwinding — run Drop/Close, then terminate
        unwind_green_thread(thread);
    }
    if !PREEMPT.load(Ordering::Relaxed) {
        return;
    }
    PREEMPT.store(false, Ordering::Relaxed);
    yield_to_scheduler(YieldReason::Preempted);
}
```

### 3.4 — Time slice enforcement

Two options for setting the preempt flag:
- **Tick counter**: increment a counter per safepoint, preempt after N ticks (simple, deterministic, no OS timer overhead)
- **Timer**: a background thread or signal-based timer sets the flag periodically (real time fairness, more complex)

Start with tick counter (e.g., yield after 1024 safepoints). Add timer-based preemption later if needed.

### 3.5 — Verify safepoint insertion in codegen

Audit `translate.rs` to confirm safepoints are emitted at:
- Every function call site
- Every loop back-edge (`while`, `for`)
- Every `match` arm entry (for recursive match patterns)

If any are missing, add them in `translate.rs`.

### 3.6 — Tests

- Spawn a green thread that runs an infinite loop — verify it yields and doesn't starve other threads
- Spawn 100 threads, one of which is a tight loop — verify all others make progress
- Cancel a thread in a tight loop — verify it stops at the next safepoint
- Benchmark safepoint overhead: measure cost of the atomic load in a hot loop

### Deliverable
Green threads yield cooperatively. No thread can starve the scheduler. Cancellation is checked at safepoints. The cost is one atomic load per function call and loop iteration.

---

## Phase 4: AOT Runtime Parity

**Goal**: The C runtime (`runtime_source.rs`) gets the same green thread model as the JIT runtime. `asterc build` produces binaries with real green threads.

### 4.1 — Assembly files for AOT

The same `.s` files from Phase 1 are compiled into the AOT binary. Update `runtime_source.rs` (or the build process in `aot.rs`) to include:
- `asm/aarch64.s` or `asm/x86_64.s` based on target
- Link them via the C compiler invocation

### 4.2 — C runtime scheduler

Rewrite the C runtime's task infrastructure:

```c
typedef struct {
    MachineContext context;
    void *stack_base;
    size_t stack_size;
    int64_t id;
    int state;           // RUNNABLE, RUNNING, SUSPENDED, TERMINAL_*
    int cancel_requested;
    int64_t result;
    // ... wake/wait bookkeeping
} GreenThread;

typedef struct {
    GreenThread **deque;  // local work-stealing deque
    size_t deque_len;
    size_t deque_cap;
    MachineContext scheduler_ctx;
    int preempt_flag;
} Worker;
```

Port the same worker loop logic from Phase 2:
1. Pop from local deque
2. Check global injector
3. Steal from victim
4. Park

### 4.3 — Stack allocation in C

```c
void *green_stack_alloc(size_t size, size_t guard) {
    size_t total = size + guard;
    void *mem = mmap(NULL, total, PROT_READ | PROT_WRITE,
                     MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    mprotect(mem, guard, PROT_NONE);  // guard page
    return mem;
}
```

Stack pooling: simple freelist with a cap.

### 4.4 — Port safepoint, spawn, resolve

Rewrite `aster_task_spawn`, `aster_task_block_on`, `aster_task_resolve_*`, `aster_safepoint` in C with the same semantics as the Rust JIT versions. The assembly `aster_context_switch` is shared.

### 4.5 — Synchronization in C

Replace `pthread_mutex_t` + `pthread_cond_t` per-task with:
- Atomic state transitions on `GreenThread.state`
- Scheduler-level wake queues (when a task completes, wake anyone waiting on it)
- No per-task OS synchronization — the scheduler handles all coordination

### 4.6 — Tests

- All existing `tests/aot_cli.rs` tests continue to pass
- New AOT-specific async tests: spawn, resolve, cancellation, scope cleanup
- Compare JIT and AOT output for the same `.aster` programs (must be identical)
- Stress test AOT binary with 10K tasks

### Deliverable
`asterc build` produces binaries with real green threads. Same M:N scheduling, same safepoints, same semantics as JIT. The assembly is shared between both runtimes.

---

## Phase 5: I/O Poller Infrastructure

**Goal**: Green threads can suspend on I/O without blocking an OS thread. Build the poller infrastructure and hook points — actual I/O primitives (file API, socket API) are a separate RFC.

### 5.1 — Platform poller abstraction

Create `codegen/src/green/poller.rs`:

```rust
pub trait Poller: Send {
    fn register(&mut self, fd: RawFd, interest: Interest, token: Token);
    fn deregister(&mut self, fd: RawFd);
    fn poll(&mut self, events: &mut Vec<Event>, timeout: Option<Duration>) -> usize;
}

pub enum Interest { Read, Write, ReadWrite }
pub struct Token(pub usize);  // maps back to a TaskId
pub struct Event { pub token: Token, pub readable: bool, pub writable: bool }
```

Implementations:
- `KqueuePoller` for macOS (`kqueue`, `kevent`)
- `EpollPoller` for Linux (`epoll_create1`, `epoll_ctl`, `epoll_wait`)

### 5.2 — Integrate poller into scheduler

The scheduler's idle path (step 4 in the worker loop) polls for I/O:

```
// Worker loop step 4: poll I/O
if no runnable green threads:
    let events = poller.poll(timeout: 1ms)
    for event in events:
        wake the green thread associated with event.token
```

One poller per runtime (not per worker). Workers coordinate access via a lock or designated I/O thread.

### 5.3 — Runtime I/O suspension API

Internal functions (not user-facing) that green threads call to suspend on I/O:

```rust
/// Suspend current green thread until `fd` is readable.
pub fn io_wait_readable(fd: RawFd) {
    let task_id = current_task_id();
    POLLER.register(fd, Interest::Read, Token(task_id));
    yield_to_scheduler(YieldReason::WaitingOnIo(fd));
    // resumed here when fd is readable
    POLLER.deregister(fd);
}

/// Suspend current green thread until `fd` is writable.
pub fn io_wait_writable(fd: RawFd) { ... }
```

### 5.4 — Blocking thread pool for disk I/O

Disk I/O can't use epoll/kqueue (they report disk fds as always ready). Create a small thread pool for blocking operations:

```rust
pub struct BlockingPool {
    threads: Vec<JoinHandle<()>>,
    sender: crossbeam_channel::Sender<BlockingJob>,
}

struct BlockingJob {
    task_id: TaskId,
    work: Box<dyn FnOnce() -> i64 + Send>,
}
```

When a green thread needs to do disk I/O:
1. Submit the operation to the blocking pool
2. Suspend the green thread
3. When the pool thread completes the work, wake the green thread

### 5.5 — C runtime parity

Port the poller to C:
- `kqueue_poller.c` for macOS
- `epoll_poller.c` for Linux
- Same blocking pool pattern with pthreads

### 5.6 — Runtime function hooks

Register new runtime functions that codegen can call:
- `aster_io_wait_read(fd: i32)` — suspend until readable
- `aster_io_wait_write(fd: i32)` — suspend until writable
- `aster_blocking_submit(entry: fn, arg: *mut u8)` — submit to blocking pool, suspend

These are internal plumbing — not exposed to Aster code yet. The future I/O RFC will design the language-level API and emit calls to these hooks.

### 5.7 — Tests

- Unit test the poller: register a pipe fd, write to it, verify poll returns the event
- Test I/O suspension: green thread waits on a pipe, another thread writes to it, verify the green thread wakes and resumes
- Test blocking pool: submit a slow operation, verify the green thread suspends and resumes with the result
- Test interaction: mix I/O-blocked and CPU-bound green threads, verify all make progress
- Test cancellation of I/O-blocked threads

### Deliverable
Green threads can suspend on file descriptors and blocking operations without tying up an OS thread. The poller is integrated into the scheduler idle path. Hook functions are registered for future I/O primitives.

---

## Phase 6: `Drop` and `Close` Traits

**Goal**: Implement `Drop` (synchronous cleanup) and `Close` (async-capable cleanup) traits. These are essential for cancellation — when a green thread is cancelled, its resources must be cleaned up properly.

### 6.1 — Trait definitions

Add to the virtual stdlib:

```
trait Drop
  def drop() -> Unit

trait Close
  def close() throws -> Unit
```

- `Drop` runs synchronously during stack unwinding. Cannot perform I/O. Cannot throw (errors are logged).
- `Close` runs on the green thread before it exits. Can perform I/O (the green thread is still alive). Can throw (errors are logged, never propagated).

### 6.2 — Parser + typecheck support

- Parse `includes Drop` / `includes Close` on class definitions
- Validate that `drop()` takes no arguments and returns `Unit`
- Validate that `close()` takes no arguments, returns `Unit`, may throw
- Auto-derive: classes that hold resources (files, connections) must implement `Close` or `Drop` explicitly — no auto-derive for these

### 6.3 — FIR lowering

- Track which locals in a scope implement `Drop` or `Close`
- On scope exit (normal, error, cancellation), emit cleanup calls in reverse declaration order
- `Drop` calls are emitted inline (synchronous)
- `Close` calls are emitted as blocking calls (can suspend the green thread)

### 6.4 — Codegen

- Emit `drop()` calls at scope exit for `Drop` implementors
- Emit `close()` calls at scope exit for `Close` implementors
- On cancellation unwind: run `Close` first (async cleanup while thread is alive), then `Drop` (sync cleanup)
- If cleanup throws, call `aster_log_cleanup_error` and continue

### 6.5 — Cancellation integration

Update the cancellation path:
1. `cancel_requested` flag set on green thread
2. Next safepoint detects it
3. Begin unwinding: run `Close` for all live scoped resources (reverse order)
4. Run `Drop` for all live scoped resources (reverse order)
5. Green thread transitions to `Cancelled`

### 6.6 — Tests

- Class with `Drop`: verify `drop()` called on scope exit
- Class with `Close`: verify `close()` called on scope exit
- Verify reverse order: A created before B → B.drop() before A.drop()
- Cancellation: verify cleanup runs when green thread is cancelled
- Cleanup error: verify errors in `drop()`/`close()` are logged, not propagated
- Nested scopes: verify cleanup runs for each scope independently

### Deliverable
`Drop` and `Close` traits work. Resources are cleaned up on scope exit, error propagation, and cancellation. Cleanup order is reverse declaration order. Cleanup errors are logged.

---

## Phase 7: `Mutex[T]`

**Goal**: Green-thread-aware mutual exclusion. `Mutex[T]` yields the green thread instead of blocking the OS thread. Scoped `.lock()` with escape analysis. Timeout support.

### 7.1 — Runtime data structure

```rust
pub struct AsterMutex {
    locked: AtomicBool,
    owner: AtomicUsize,        // TaskId of current holder
    wait_queue: Mutex<VecDeque<TaskId>>,  // green threads waiting to acquire
    timeout_ms: u64,
}
```

When a green thread tries to acquire a locked mutex:
1. Add itself to the wait queue
2. Suspend (yield to scheduler with `BlockReason::WaitingOnMutex`)
3. When the holder releases, the first waiter is woken

### 7.2 — Language-level API

Typecheck + codegen for:

```
let m = Mutex(initial_value)

# Scoped lock — lambda receives the inner value
m.lock(value ->
  value.field = 42
)

# With timeout
m.lock(timeout: 5000, value ->
  value.field = 42
)
```

### 7.3 — Escape analysis

The compiler must enforce that nothing obtained inside `.lock()` escapes the block:

```
let m = Mutex(data)
let leaked = nil
m.lock(value ->
  leaked = value    # COMPILE ERROR: lock-scoped reference cannot escape
)
```

This requires tracking the lambda parameter's scope in the typechecker and rejecting assignments to outer variables.

### 7.4 — Timeout and `LockTimeoutError`

- Default timeout configurable (runtime-level, e.g., `ASTER_LOCK_TIMEOUT_MS`)
- If timeout expires, throw `LockTimeoutError`
- Implementation: when suspending on a mutex, register a timer. If the timer fires before the lock is acquired, wake the green thread with an error.

### 7.5 — Manual API

```
m.acquire()    # blocks (suspends green thread) until acquired
# ... use m.value ...
m.release()    # releases, wakes first waiter
```

No escape analysis on manual API — sharp knife, user's responsibility.

### 7.6 — Codegen

- `Mutex(value)` → `aster_mutex_new(value)` allocates the mutex
- `.lock(fn)` → `aster_mutex_lock(mutex)` (suspend if contended), call fn, `aster_mutex_unlock(mutex)` (wake first waiter)
- `.acquire()` → `aster_mutex_lock(mutex)`
- `.release()` → `aster_mutex_unlock(mutex)`
- Integrate with `Drop` — if a green thread is cancelled while holding a mutex, release it during cleanup

### 7.7 — C runtime parity

Port `AsterMutex` to C. Same semantics: green-thread-aware wait queue, timeout, wake-on-release.

### 7.8 — Tests

- Basic lock/unlock: single thread, verify mutual exclusion
- Contention: two green threads competing for a lock, verify no data corruption
- Timeout: verify `LockTimeoutError` thrown after timeout expires
- Escape analysis: verify compiler rejects leaked references
- Cancellation: green thread cancelled while waiting on mutex — verify it's removed from wait queue
- Cancellation while holding: verify mutex is released during cleanup
- Deadlock prevention: verify timeout prevents infinite deadlock
- Stress test: 100 green threads hammering a single mutex

### Deliverable
`Mutex[T]` works with green-thread-aware suspension. Scoped `.lock()` with escape analysis. Timeout prevents deadlocks. Cleanup on cancellation.

---

## Phase 8: Channels

**Goal**: Typed message-passing between green threads. Three channel types, three-tier API, green-thread-aware suspension.

### 8.1 — Runtime data structure

```rust
pub struct AsterChannel<T> {
    buffer: VecDeque<T>,
    capacity: usize,
    closed: bool,
    send_waiters: VecDeque<TaskId>,    // threads waiting to send (buffer full)
    recv_waiters: VecDeque<TaskId>,    // threads waiting to receive (buffer empty)
}
```

### 8.2 — Channel types

- `Channel[T]` — single sender, single receiver (enforced at typecheck: using send from two green threads is a compile error)
- `MultiSend[T]` — multiple senders, single receiver
- `MultiReceive[T]` — single sender, multiple receivers

For `MultiSend`/`MultiReceive`, the runtime data structure is the same but access patterns differ. `MultiSend` allows concurrent `send()` calls (lock on the buffer). `MultiReceive` distributes messages round-robin to receivers.

### 8.3 — Three-tier API

**Send side:**

| Method | Buffer full behavior | Return |
|--------|---------------------|--------|
| `ch.send(value)` | Drop silently | `Unit` |
| `ch.wait_send(value)` | Suspend green thread | `Unit` |
| `ch.try_send(value)` | Throw `ChannelFullError` | `throws` |

**Receive side:**

| Method | Buffer empty behavior | Return |
|--------|-----------------------|--------|
| `ch.receive()` | Return `nil` | `T?` |
| `ch.wait_receive()` | Suspend green thread | `T` |
| `ch.try_receive()` | Throw `ChannelEmptyError` | `throws` |

### 8.4 — Close semantics

`ch.close()`:
- Subsequent sends throw `ChannelClosedError`
- `receive()` drains buffer, then returns `nil`
- `wait_receive()` drains buffer, then throws `ChannelClosedError`
- Wake all suspended senders/receivers with appropriate errors

### 8.5 — Typecheck

- `Channel[T].new()` / `Channel[T].new(capacity: N)` — constructor with optional capacity
- Type-check all six methods with correct return types and error types
- Enforce sender/receiver cardinality for `Channel[T]` (single-sender, single-receiver) at typecheck time where possible, or runtime error for dynamic cases

### 8.6 — FIR + Codegen

- Lower channel operations to runtime calls
- `ch.wait_send(value)` → `aster_channel_wait_send(ch, value)` which suspends the green thread if buffer is full
- `ch.wait_receive()` → `aster_channel_wait_receive(ch)` which suspends if buffer is empty
- Wake mechanics: when a send completes and there are receive waiters, wake the first one. Vice versa.

### 8.7 — Data ownership

Sending through a channel shallow-copies the value (same as `async` thread boundary). The data sharing warning system (Phase 10) covers the "used after send" case.

### 8.8 — C runtime parity

Port channel data structure and operations to C. Same buffer, same wait queues, same semantics.

### 8.9 — Tests

- Single producer, single consumer: send N values, receive N values, verify order
- Buffered channel: fill buffer, verify `send()` drops, `try_send()` throws
- Suspension: `wait_send` on full buffer, verify sender resumes when receiver drains
- Suspension: `wait_receive` on empty buffer, verify receiver resumes when sender sends
- Close: verify close semantics (drain, then error)
- Multi-send: two senders, one receiver, verify all messages arrive (order may interleave)
- Multi-receive: one sender, two receivers, verify messages are distributed
- Cancellation: green thread waiting on channel is cancelled, verify it's removed from wait queue
- Stress test: 100 producers, 1 consumer, 10K messages each

### Deliverable
All three channel types work with green-thread-aware suspension. Three-tier API. Close semantics. Shallow copy at boundaries.

---

## Phase 9: Must-Consume `Task[T]` Enforcement

**Goal**: The compiler rejects programs that drop a `Task[T]` without resolving, passing to `resolve_all`/`resolve_first`, or being inside an `async scope` that handles cleanup.

### 9.1 — Linear type tracking in typechecker

Add a `consumed` flag to `Task[T]` bindings in the type environment. Track consumption via:
- `resolve task` → consumed
- Passed to `resolve_all([..., task, ...])` → consumed
- Passed to `resolve_first([..., task, ...])` → consumed
- Inside an `async scope` → scope exit handles cleanup (consumed by scope)
- Returned from a function → consumed (caller's responsibility)

### 9.2 — Scope exit analysis

At the end of every scope (function body, if/else branches, match arms), check for unconsumed `Task[T]` bindings. Emit error:

```
error[E026]: task 'task_name' is never consumed
  --> file.aster:10:5
   |
10 |   let t = async fetch_user(id: 1)
   |       ^ task created here but never resolved
   |
   = help: use `resolve t!` to consume, or wrap in `async scope`
   = help: use `detached async f()` for fire-and-forget
```

### 9.3 — Branch analysis

All branches must consume or not consume consistently:

```
if condition
  let t = async work()
  resolve t!              # consumed in if branch
else
  let t = async work()
  # NOT consumed — error
```

### 9.4 — Tests

- Verify consumed via `resolve` — no error
- Verify consumed via `resolve_all` — no error
- Verify consumed via `resolve_first` — no error
- Verify consumed in `async scope` — no error
- Verify unconsumed — compile error E026
- Verify branch inconsistency — compile error
- Verify returned from function — no error (caller's problem)
- Verify `detached async` — no task to consume, no error

### Deliverable
Dropping a `Task[T]` without consuming it is a compile error. No orphaned green threads.

---

## Phase 10: Data Sharing Warnings

**Goal**: The compiler warns when a variable is used after being passed to a thread boundary (`async`, channel send, `Shared()`).

### 10.1 — Thread boundary tracking

In the typechecker, track when a variable crosses a thread boundary:
- Passed as argument to `async f(x)` → `x` is "boundary-crossed"
- Passed to `ch.send(x)` → `x` is "boundary-crossed"
- Wrapped in `Shared(x)` → `x` is "boundary-crossed"

### 10.2 — Post-boundary use detection

After a variable is marked boundary-crossed, any subsequent use of it (read or write) emits a warning:

```
warning: 'user' has been copied to an async context
  --> file.aster:5:1
   |
3  |   let task = async save_user(user)
   |                               ---- copied here
5  |   user.name = "Rick"
   |   ^^^^ mutation only affects local copy
   |
   = note: allow(copied_non_shared_resource) to suppress
```

### 10.3 — Suppression

- `allow(copied_non_shared_resource)` annotation suppresses the warning
- `user.copy()` at the boundary (explicit copy) suppresses the warning on the original
- Variable not used after boundary crossing → no warning

### 10.4 — Scope

This is local-scope analysis only (within a single function body). No cross-function or cross-module alias tracking. The warning is best-effort for common cases.

### 10.5 — Tests

- Variable used after `async` pass → warning
- Variable not used after → no warning
- Explicit `.copy()` at boundary → no warning
- `allow` annotation → suppressed
- Multiple boundaries: `async` then `send` → warning on both
- Reassignment after boundary → no warning (new value)

### Deliverable
Compiler warns about the most common data-sharing footgun. Local scope analysis, suppressible, best-effort.

---

## Testing Strategy

Each phase includes unit tests for runtime internals and end-to-end `.aster` programs for integration.

### Runtime unit tests (`codegen/src/tests.rs`, `codegen/src/green/tests.rs`)
- Assembly context switching correctness
- Stack allocation and guard pages
- Scheduler: spawn, steal, suspend, wake
- Safepoint preemption under load
- Mutex contention and timeout
- Channel buffer management and suspension
- I/O poller event delivery

### End-to-end .aster tests (`tests/`)
- `tests/green_threads.rs` — spawn, resolve, cancellation, scope cleanup
- `tests/green_stress.rs` — 10K tasks, recursive spawn, work stealing validation
- `tests/green_mutex.rs` — contention, timeout, cleanup
- `tests/green_channels.rs` — producer/consumer, multi-send, close semantics
- `tests/green_io.rs` — I/O suspension (when I/O primitives exist)
- `tests/must_consume.rs` — compile errors for unconsumed tasks
- `tests/data_sharing.rs` — warnings for post-boundary use

### AOT parity tests
- Every end-to-end test runs with both `asterc run` (JIT) and `asterc build` + execute (AOT)
- Output must be identical

### Stress / correctness tests
- ThreadSanitizer on the Rust runtime (detect data races)
- AddressSanitizer on the C runtime (detect memory errors)
- Valgrind on AOT binaries (detect leaks)
- Run stress tests under heavy load for extended periods

---

## Dependency Graph

```
Phase 1 (Assembly)
  │
  ▼
Phase 2 (M:N Scheduler) ──────────────────┐
  │                                        │
  ▼                                        ▼
Phase 3 (Safepoints)              Phase 4 (AOT Parity)
  │                                        │
  ├────────────────────────────────────────┘
  ▼
Phase 5 (I/O Poller)
  │
  ▼
Phase 6 (Drop/Close) ◄── needed by Phase 7, 8
  │
  ├──────────────┐
  ▼              ▼
Phase 7        Phase 8
(Mutex)      (Channels)
  │              │
  └──────┬───────┘
         ▼
Phase 9 (Must-Consume) ◄── independent, can start after Phase 2
         │
         ▼
Phase 10 (Data Sharing) ◄── independent, can start after Phase 2
```

Phases 9 and 10 are typecheck-only — they can be developed in parallel with Phases 5-8.

Phases 7 and 8 are independent of each other and can be developed in parallel after Phase 6.

Phase 4 (AOT parity) can proceed in parallel with Phase 3 once Phase 2 is done.

---

## Risk Register

| Risk | Impact | Mitigation |
|------|--------|------------|
| Assembly bugs (register clobbering, alignment) | Segfaults, corruption | Extensive unit tests, run under sanitizers, test on both platforms early |
| Stack overflow on small green thread stacks | Crash | Guard pages catch it cleanly. Start with 8KB, monitor. Add growable stacks later if needed |
| Work-stealing deque correctness | Rare deadlocks or lost tasks | Use `crossbeam-deque` (proven) or write extensive stress tests |
| Poller platform differences (kqueue vs epoll) | Subtle behavioral differences | Abstract behind `Poller` trait, test on both platforms in CI |
| Mutex escape analysis too restrictive | False positives frustrate users | Start conservative, relax based on user feedback |
| C runtime parity drift | AOT behaves differently than JIT | Run identical test suite against both, compare output byte-for-byte |
| Cancellation + cleanup ordering | Resource leaks or double-free | Formal specification of cleanup order, extensive cancellation tests |
