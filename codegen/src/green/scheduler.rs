use std::cell::Cell;
use std::os::fd::RawFd;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use crossbeam_deque::{Injector, Steal, Stealer, Worker};

use super::blocking::BlockingPool;
use super::context::MachineContext;
use super::poller::{self, Interest, Poller, Token};
use super::stack::StackPool;
use super::thread::{GreenThread, TaskState, ThreadPtr, ThreadStatus, YieldReason, is_terminal};

// ---------------------------------------------------------------------------
// Thread-local worker state
// ---------------------------------------------------------------------------

thread_local! {
    static WORKER_SCHEDULER_CTX: Cell<*mut MachineContext> = const { Cell::new(std::ptr::null_mut()) };
    // Stores a raw pointer to the current GreenThread. Does NOT own an Arc reference.
    // The Arc is kept alive by the worker loop's local `arc` variable during execution.
    static WORKER_CURRENT_THREAD: Cell<*const GreenThread> = const { Cell::new(std::ptr::null()) };
    static WORKER_YIELD_REASON: Cell<Option<YieldReason>> = const { Cell::new(None) };
    static PREEMPT_TICKS: Cell<u32> = const { Cell::new(0) };
    static IS_WORKER: Cell<bool> = const { Cell::new(false) };
}

const PREEMPT_THRESHOLD: u32 = 1024;
const BLOCKING_POOL_THREADS: usize = 4;

// Error flag and shadow stack live in runtime.rs — we use accessors.

// ---------------------------------------------------------------------------
// Scheduler
// ---------------------------------------------------------------------------

pub(crate) struct GreenScheduler {
    pub(crate) injector: Injector<ThreadPtr>,
    stealers: Vec<Stealer<ThreadPtr>>,
    pub(crate) stack_pool: StackPool,
    pub(crate) park_mutex: Mutex<()>,
    pub(crate) park_cv: Condvar,
    poller: Mutex<Box<dyn Poller>>,
    pub(crate) blocking_pool: Arc<BlockingPool>,
}

static SCHEDULER: OnceLock<GreenScheduler> = OnceLock::new();

pub(crate) fn sched() -> &'static GreenScheduler {
    SCHEDULER.get_or_init(|| {
        let worker_count = thread::available_parallelism()
            .map(|c| c.get())
            .unwrap_or(2)
            .max(2);

        let mut workers = Vec::with_capacity(worker_count);
        let mut stealers = Vec::with_capacity(worker_count);

        for _ in 0..worker_count {
            let w: Worker<ThreadPtr> = Worker::new_fifo();
            stealers.push(w.stealer());
            workers.push(w);
        }

        let scheduler = GreenScheduler {
            injector: Injector::new(),
            stealers,
            stack_pool: StackPool::new(worker_count * 16),
            park_mutex: Mutex::new(()),
            park_cv: Condvar::new(),
            poller: Mutex::new(poller::create_poller()),
            blocking_pool: BlockingPool::new(BLOCKING_POOL_THREADS),
        };

        for (id, w) in workers.into_iter().enumerate() {
            thread::spawn(move || worker_loop(id, w));
        }

        scheduler
    })
}

// ---------------------------------------------------------------------------
// FFI — assembly functions
// ---------------------------------------------------------------------------

unsafe extern "C" {
    fn aster_context_switch(old: *mut MachineContext, new: *const MachineContext);
    fn aster_context_init(ctx: *mut MachineContext, stack_top: *mut u8, entry: usize, arg: usize);
}

// ---------------------------------------------------------------------------
// Worker loop
// ---------------------------------------------------------------------------

fn worker_loop(_id: usize, local: Worker<ThreadPtr>) {
    IS_WORKER.set(true);

    let mut scheduler_ctx = MachineContext::new();
    WORKER_SCHEDULER_CTX.set(&raw mut scheduler_ctx);

    let sc = sched();

    loop {
        let task = find_task(&local, sc);

        let Some(ThreadPtr(arc)) = task else {
            // Idle path: poll I/O before parking
            poll_io(sc);
            let guard = sc.park_mutex.lock().unwrap();
            let _ = sc
                .park_cv
                .wait_timeout(guard, Duration::from_millis(1))
                .unwrap();
            continue;
        };

        let thread = &*arc;

        // Check cancel or already-terminal before running
        {
            let mut st = thread.state.lock().unwrap();
            if is_terminal(st.status) {
                // Already completed/cancelled (e.g. double-enqueue from cancel + wake_waiters race)
                continue;
            }
            if st.cancel_requested {
                st.status = ThreadStatus::Cancelled;
                thread.cv.notify_all();
                let waiters = std::mem::take(&mut st.green_waiters);
                drop(st);
                wake_waiters(waiters);
                recycle_stack(thread);
                continue;
            }
            st.status = ThreadStatus::Running;
        }

        // Release-mode guard: abort if two workers try to run the same thread.
        // This was previously a debug_assert; upgraded because the Send/Sync
        // impls on GreenThread rely on exclusive-access invariants.
        if thread
            .running_on_worker
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::Acquire,
                std::sync::atomic::Ordering::Relaxed,
            )
            .is_err()
        {
            eprintln!(
                "FATAL: double-scheduling detected: green thread is already running on another worker"
            );
            std::process::abort();
        }

        // Set TLS for the green thread. The raw pointer is valid as long as `arc` is alive.
        let thread_raw: *const GreenThread = Arc::as_ptr(&arc);
        WORKER_CURRENT_THREAD.set(thread_raw);
        WORKER_YIELD_REASON.set(None);
        PREEMPT_TICKS.set(0);

        // Restore per-green-thread TLS state
        unsafe {
            crate::runtime::error_flag_set(thread.get_error_flag());
            crate::runtime::shadow_stack_set(thread.get_shadow_stack());
        }

        // Context switch to green thread
        unsafe {
            aster_context_switch(&raw mut scheduler_ctx, thread.context_mut());
        }

        // Green thread yielded back — save per-green-thread TLS state
        thread
            .running_on_worker
            .store(false, std::sync::atomic::Ordering::Relaxed);
        unsafe {
            thread.set_error_flag(crate::runtime::error_flag_get());
            thread.set_shadow_stack(crate::runtime::shadow_stack_get());
        }
        WORKER_CURRENT_THREAD.set(std::ptr::null());

        match WORKER_YIELD_REASON.replace(None).unwrap_or(YieldReason::None) {
            YieldReason::Preempted => {
                thread.state.lock().unwrap().status = ThreadStatus::Runnable;
                local.push(ThreadPtr(arc));
            }

            YieldReason::Completed { result, failed } => {
                complete_thread(thread, result, failed);
                recycle_stack(thread);
                // arc drops here, decrementing refcount
            }

            YieldReason::Cancelled => {
                let mut st = thread.state.lock().unwrap();
                st.status = ThreadStatus::Cancelled;
                thread.cv.notify_all();
                let waiters = std::mem::take(&mut st.green_waiters);
                drop(st);
                wake_waiters(waiters);
                recycle_stack(thread);
                // arc drops here, decrementing refcount
            }

            YieldReason::WaitingOnTask(target_arc) => {
                let mut target_st = target_arc.state.lock().unwrap();
                if is_terminal(target_st.status) {
                    // Target already done — re-enqueue immediately
                    drop(target_st);
                    thread.state.lock().unwrap().status = ThreadStatus::Runnable;
                    local.push(ThreadPtr(arc));
                } else {
                    // Park until target completes
                    target_st.green_waiters.push(arc.clone());
                    drop(target_st);
                    thread.state.lock().unwrap().status = ThreadStatus::Suspended;
                    // arc drops here, but a clone was stored in green_waiters
                }
            }

            YieldReason::WaitingOnIo
            | YieldReason::WaitingOnBlockingPool
            | YieldReason::WaitingOnMutex
            | YieldReason::WaitingOnChannelSend
            | YieldReason::WaitingOnChannelRecv => {
                // Thread is suspended — the runtime will re-enqueue it when ready.
                // The Arc was transferred to the suspending subsystem (poller/blocking/mutex/channel)
                // which will push it back when the event fires.
                thread.state.lock().unwrap().status = ThreadStatus::Suspended;
                // arc drops here — the subsystem holds the reference it acquired
            }

            YieldReason::None => {
                // Should not happen — treat as preempted
                thread.state.lock().unwrap().status = ThreadStatus::Runnable;
                local.push(ThreadPtr(arc));
            }
        }
    }
}

fn find_task(local: &Worker<ThreadPtr>, sc: &GreenScheduler) -> Option<ThreadPtr> {
    // 1. Local pop
    if let Some(t) = local.pop() {
        return Some(t);
    }

    // 2. Steal batch from injector
    loop {
        match sc.injector.steal_batch_and_pop(local) {
            Steal::Success(t) => return Some(t),
            Steal::Empty => break,
            Steal::Retry => {}
        }
    }

    // 3. Steal from victims
    for stealer in &sc.stealers {
        loop {
            match stealer.steal() {
                Steal::Success(t) => return Some(t),
                Steal::Empty => break,
                Steal::Retry => {}
            }
        }
    }

    None
}

fn poll_io(sc: &GreenScheduler) {
    let mut poller = match sc.poller.try_lock() {
        Ok(p) => p,
        Err(_) => return, // Another worker is polling
    };
    let mut events = Vec::new();
    poller.poll(&mut events, Some(Duration::from_millis(0)));
    drop(poller);

    if events.is_empty() {
        return;
    }
    for event in events {
        let ptr = event.token.0 as *const GreenThread;
        if !ptr.is_null() {
            // Reconstruct the Arc from the raw pointer. The extra refcount was
            // incremented in io_wait_readable/io_wait_writable before registration.
            let arc = unsafe { Arc::from_raw(ptr) };
            sc.injector.push(ThreadPtr(arc));
        }
    }
    sc.park_cv.notify_all();
}

fn complete_thread(thread: &GreenThread, result: i64, failed: bool) {
    let mut st = thread.state.lock().unwrap();
    st.result = result;
    st.failed = failed;
    st.status = if st.cancel_requested {
        ThreadStatus::Cancelled
    } else if failed {
        ThreadStatus::Failed
    } else {
        ThreadStatus::Ready
    };
    thread.cv.notify_all();
    let waiters = std::mem::take(&mut st.green_waiters);
    drop(st);
    wake_waiters(waiters);
}

fn wake_waiters(waiters: Vec<Arc<GreenThread>>) {
    if waiters.is_empty() {
        return;
    }
    let sc = sched();
    for waiter in waiters {
        sc.injector.push(ThreadPtr(waiter));
    }
    sc.park_cv.notify_all();
}

fn recycle_stack(thread: &GreenThread) {
    if let Some(stack) = unsafe { thread.take_stack() } {
        sched().stack_pool.put(stack);
    }
}

// ---------------------------------------------------------------------------
// Public API — called from runtime.rs
// ---------------------------------------------------------------------------

pub(crate) fn spawn_green_thread(entry: usize, args: usize) -> *const GreenThread {
    let sc = sched();
    let stack = sc.stack_pool.get();
    let stack_top = stack.top();

    let arc = Arc::new(GreenThread {
        context: std::cell::UnsafeCell::new(super::context::MachineContext::new()),
        stack: std::cell::UnsafeCell::new(Some(stack)),
        error_flag: std::cell::UnsafeCell::new(false),
        shadow_stack_top: std::cell::UnsafeCell::new(std::ptr::null_mut()),
        state: Mutex::new(TaskState {
            status: ThreadStatus::Runnable,
            cancel_requested: false,
            consumed: false,
            scoped: false,
            result: 0,
            failed: false,
            green_waiters: Vec::new(),
        }),
        cv: std::sync::Condvar::new(),
        running_on_worker: std::sync::atomic::AtomicBool::new(false),
    });

    unsafe {
        aster_context_init(arc.context_mut(), stack_top, entry, args);
    }

    // The raw pointer represents the caller's Arc reference.
    let raw = Arc::into_raw(arc.clone());
    sc.injector.push(ThreadPtr(arc));
    sc.park_cv.notify_all();

    raw
}

pub(crate) fn allocate_terminal_thread(result: i64, failed: bool) -> *const GreenThread {
    let status = if failed {
        ThreadStatus::Failed
    } else {
        ThreadStatus::Ready
    };

    let arc = Arc::new(GreenThread {
        context: std::cell::UnsafeCell::new(super::context::MachineContext::new()),
        stack: std::cell::UnsafeCell::new(None),
        error_flag: std::cell::UnsafeCell::new(false),
        shadow_stack_top: std::cell::UnsafeCell::new(std::ptr::null_mut()),
        state: Mutex::new(TaskState {
            status,
            cancel_requested: false,
            consumed: false,
            scoped: false,
            result,
            failed,
            green_waiters: Vec::new(),
        }),
        cv: Condvar::new(),
        running_on_worker: std::sync::atomic::AtomicBool::new(false),
    });

    Arc::into_raw(arc)
}

pub(crate) fn cancel_thread(thread_ptr: *const GreenThread) {
    let thread = unsafe { &*thread_ptr };
    let mut st = thread.state.lock().unwrap();
    st.cancel_requested = true;

    match st.status {
        ThreadStatus::Runnable => {
            st.status = ThreadStatus::Cancelled;
            thread.cv.notify_all();
            let waiters = std::mem::take(&mut st.green_waiters);
            drop(st);
            wake_waiters(waiters);
        }
        ThreadStatus::Running => {
            // Currently executing — flag is set, safepoint will catch it
        }
        ThreadStatus::Suspended => {
            st.status = ThreadStatus::Cancelled;
            thread.cv.notify_all();
            let waiters = std::mem::take(&mut st.green_waiters);
            drop(st);
            wake_waiters(waiters);
            recycle_stack(thread);
        }
        _ => {
            // Already terminal — nothing to do
        }
    }
}

pub(crate) fn wait_cancel_thread(thread_ptr: *const GreenThread) {
    cancel_thread(thread_ptr);
    wait_for_terminal(thread_ptr);
}

pub(crate) fn is_thread_ready(thread_ptr: *const GreenThread) -> bool {
    let thread = unsafe { &*thread_ptr };
    let st = thread.state.lock().unwrap();
    is_terminal(st.status)
}

/// Consume the result of a terminal green thread.
/// Sets the error flag if the task failed or was cancelled.
/// Reconstructs the Arc from the raw pointer and drops it; if this was
/// the last reference, the GreenThread struct is freed automatically.
pub(crate) fn consume_thread_result(thread_ptr: *const GreenThread) -> i64 {
    wait_for_terminal(thread_ptr);

    let thread = unsafe { &*thread_ptr };
    let mut st = thread.state.lock().unwrap();

    if st.consumed {
        crate::runtime::error_flag_set(true);
        return 0;
    }
    st.consumed = true;

    let result = match st.status {
        ThreadStatus::Ready => st.result,
        ThreadStatus::Failed | ThreadStatus::Cancelled => {
            crate::runtime::error_flag_set(true);
            0
        }
        _ => {
            crate::runtime::error_flag_set(true);
            0
        }
    };
    drop(st);

    // Recycle the stack (64KB) now that the result is consumed.
    recycle_stack(thread);

    // Reconstruct and drop the caller's Arc reference. If no other references
    // remain (deque, waiters, etc.), the struct is freed here.
    let _arc = unsafe { Arc::from_raw(thread_ptr) };

    result
}

/// Clean up a scoped GreenThread. Called by async scope cleanup.
/// Scoped tasks are never freed by consume_thread_result (they defer to scope exit).
/// The stack may already be recycled if the task was consumed.
pub(crate) fn free_scoped_thread(thread_ptr: *const GreenThread) {
    let thread = unsafe { &*thread_ptr };
    // Recycle the stack if it hasn't been recycled already (unconsumed tasks).
    recycle_stack(thread);
    // Consume the scope's Arc reference. When refcount reaches zero, the struct is freed.
    let _arc = unsafe { Arc::from_raw(thread_ptr) };
}

fn wait_for_terminal(thread_ptr: *const GreenThread) {
    let thread = unsafe { &*thread_ptr };

    if IS_WORKER.get() {
        // On a worker thread — yield as a green thread
        loop {
            {
                let st = thread.state.lock().unwrap();
                if is_terminal(st.status) {
                    return;
                }
            }
            // Increment refcount so that the WaitingOnTask arc is valid
            unsafe { Arc::increment_strong_count(thread_ptr) };
            let wait_arc = unsafe { Arc::from_raw(thread_ptr) };
            yield_to_scheduler(YieldReason::WaitingOnTask(wait_arc));
        }
    } else {
        // On a non-worker thread (e.g., main) — block with Condvar
        let mut st = thread.state.lock().unwrap();
        while !is_terminal(st.status) {
            st = thread.cv.wait(st).unwrap();
        }
    }
}

// ---------------------------------------------------------------------------
// I/O suspension — Phase 5
// ---------------------------------------------------------------------------

/// Suspend the current green thread until `fd` is readable.
/// Must be called from a worker thread (inside a green thread).
pub(crate) fn io_wait_readable(fd: RawFd) {
    let current = WORKER_CURRENT_THREAD.get();
    assert!(!current.is_null(), "io_wait_readable outside green thread");

    let sc = sched();
    {
        let mut poller = sc.poller.lock().unwrap();
        // Increment refcount for the token stored in the poller. poll_io will
        // reconstruct the Arc via Arc::from_raw, consuming this extra count.
        unsafe { Arc::increment_strong_count(current) };
        poller.register(fd, Interest::Read, Token(current as usize));
    }
    yield_to_scheduler(YieldReason::WaitingOnIo);
    // Resumed here when fd is readable. The poller already consumed its Arc
    // reference via poll_io, so no cleanup needed here.
    {
        let mut poller = sc.poller.lock().unwrap();
        poller.deregister(fd);
    }
}

/// Suspend the current green thread until `fd` is writable.
/// Must be called from a worker thread (inside a green thread).
pub(crate) fn io_wait_writable(fd: RawFd) {
    let current = WORKER_CURRENT_THREAD.get();
    assert!(!current.is_null(), "io_wait_writable outside green thread");

    let sc = sched();
    {
        let mut poller = sc.poller.lock().unwrap();
        // Increment refcount for the token stored in the poller.
        unsafe { Arc::increment_strong_count(current) };
        poller.register(fd, Interest::Write, Token(current as usize));
    }
    yield_to_scheduler(YieldReason::WaitingOnIo);
    // Resumed here when fd is writable.
    {
        let mut poller = sc.poller.lock().unwrap();
        poller.deregister(fd);
    }
}

/// Submit blocking work to the thread pool, suspending the current green thread.
pub(crate) fn blocking_submit(work: Box<dyn FnOnce() -> i64 + Send>) {
    let current = WORKER_CURRENT_THREAD.get();
    assert!(!current.is_null(), "blocking_submit outside green thread");

    let sc = sched();
    // Increment refcount: the BlockingPool job holds a reference.
    unsafe { Arc::increment_strong_count(current) };
    let arc = unsafe { Arc::from_raw(current) };
    sc.blocking_pool.submit(arc, work);
    yield_to_scheduler(YieldReason::WaitingOnBlockingPool);
    // Resumed here when the blocking pool completes
}

// ---------------------------------------------------------------------------
// Yield — called from green thread code
// ---------------------------------------------------------------------------

pub(crate) fn yield_to_scheduler(reason: YieldReason) {
    WORKER_YIELD_REASON.set(Some(reason));
    let scheduler_ctx = WORKER_SCHEDULER_CTX.get();
    let current = WORKER_CURRENT_THREAD.get();
    assert!(!scheduler_ctx.is_null(), "yield outside worker thread");
    assert!(!current.is_null(), "yield with no current green thread");
    // SAFETY: current is non-null and the UnsafeCell pointer is valid while the
    // worker holds the Arc for the current green thread.
    unsafe {
        aster_context_switch((*current).context.get(), scheduler_ctx);
    }
    // Execution resumes here when the scheduler switches back to us
}

pub(crate) fn is_worker_thread() -> bool {
    IS_WORKER.get()
}

// ---------------------------------------------------------------------------
// Safepoint — Phase 3
// ---------------------------------------------------------------------------

pub(crate) fn safepoint() {
    let current = WORKER_CURRENT_THREAD.get();
    if current.is_null() {
        return; // Not on a worker thread
    }

    // Check cancellation
    let cancel = {
        let thread = unsafe { &*current };
        let st = thread.state.lock().unwrap();
        st.cancel_requested
    };
    if cancel {
        yield_to_scheduler(YieldReason::Cancelled);
        return;
    }

    // Tick-based preemption
    let ticks = PREEMPT_TICKS.get() + 1;
    if ticks >= PREEMPT_THRESHOLD {
        PREEMPT_TICKS.set(0);
        yield_to_scheduler(YieldReason::Preempted);
    } else {
        PREEMPT_TICKS.set(ticks);
    }
}

// ---------------------------------------------------------------------------
// Mutex / Channel support — Phase 7-8
// ---------------------------------------------------------------------------

/// Get a numeric ID for the current green thread (pointer value).
pub(crate) fn current_thread_id() -> usize {
    WORKER_CURRENT_THREAD.get() as usize
}

/// Suspend the current green thread waiting for a mutex.
pub(crate) fn suspend_for_mutex() {
    yield_to_scheduler(YieldReason::WaitingOnMutex);
}

/// Suspend the current green thread waiting to send on a channel.
pub(crate) fn suspend_for_channel_send() {
    yield_to_scheduler(YieldReason::WaitingOnChannelSend);
}

/// Suspend the current green thread waiting to receive from a channel.
/// Returns the value that was delivered by the sender via `wake_thread_with_value`.
pub(crate) fn suspend_for_channel_receive() -> i64 {
    let current = WORKER_CURRENT_THREAD.get();
    assert!(!current.is_null(), "channel receive outside green thread");
    yield_to_scheduler(YieldReason::WaitingOnChannelRecv);
    // After wakeup, the sender stored the value in our result field
    let thread = unsafe { &*current };
    let st = thread.state.lock().unwrap();
    st.result
}

/// Get an Arc for the current green thread, incrementing the refcount.
/// The caller is responsible for consuming or dropping the returned Arc.
pub(crate) fn current_green_thread_arc() -> Option<Arc<GreenThread>> {
    let current = WORKER_CURRENT_THREAD.get();
    if current.is_null() {
        return None;
    }
    // Increment refcount and reconstruct Arc without consuming the TLS pointer.
    unsafe { Arc::increment_strong_count(current) };
    Some(unsafe { Arc::from_raw(current) })
}

/// Wake a suspended green thread by re-enqueueing it.
pub(crate) fn wake_thread(arc: Arc<GreenThread>) {
    {
        let mut st = arc.state.lock().unwrap();
        if is_terminal(st.status) {
            return;
        }
        st.status = ThreadStatus::Runnable;
    }
    let sc = sched();
    sc.injector.push(ThreadPtr(arc));
    sc.park_cv.notify_all();
}

/// Wake a suspended green thread and deliver a value (for channel receive).
pub(crate) fn wake_thread_with_value(arc: Arc<GreenThread>, value: i64) {
    {
        let mut st = arc.state.lock().unwrap();
        if is_terminal(st.status) {
            return;
        }
        st.result = value;
        st.status = ThreadStatus::Runnable;
    }
    let sc = sched();
    sc.injector.push(ThreadPtr(arc));
    sc.park_cv.notify_all();
}

/// Wake a suspended green thread with an error (sets error flag).
pub(crate) fn wake_thread_with_error(arc: Arc<GreenThread>) {
    {
        let mut st = arc.state.lock().unwrap();
        if is_terminal(st.status) {
            return;
        }
        // Set error flag inside the lock, before making thread eligible for pickup
        unsafe { arc.set_error_flag(true) };
        st.status = ThreadStatus::Runnable;
    }
    let sc = sched();
    sc.injector.push(ThreadPtr(arc));
    sc.park_cv.notify_all();
}

// ---------------------------------------------------------------------------
// Green thread exit — called from assembly trampoline
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub extern "C" fn aster_green_thread_exit(result: i64) {
    let failed = crate::runtime::error_flag_get();
    crate::runtime::error_flag_set(false);
    yield_to_scheduler(YieldReason::Completed { result, failed });
    unreachable!("aster_green_thread_exit returned");
}
