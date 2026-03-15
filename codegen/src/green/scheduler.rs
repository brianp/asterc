use std::cell::Cell;
use std::sync::{Condvar, Mutex, OnceLock};
use std::thread;

use crossbeam_deque::{Injector, Steal, Stealer, Worker};

use super::context::MachineContext;
use super::stack::StackPool;
use super::thread::{GreenThread, TaskState, ThreadPtr, ThreadStatus, YieldReason, is_terminal};

// ---------------------------------------------------------------------------
// Thread-local worker state
// ---------------------------------------------------------------------------

thread_local! {
    static WORKER_SCHEDULER_CTX: Cell<*mut MachineContext> = const { Cell::new(std::ptr::null_mut()) };
    static WORKER_CURRENT_THREAD: Cell<*mut GreenThread> = const { Cell::new(std::ptr::null_mut()) };
    static WORKER_YIELD_REASON: Cell<YieldReason> = const { Cell::new(YieldReason::None) };
    static PREEMPT_TICKS: Cell<u32> = const { Cell::new(0) };
    static IS_WORKER: Cell<bool> = const { Cell::new(false) };
}

const PREEMPT_THRESHOLD: u32 = 1024;

// Error flag and shadow stack live in runtime.rs — we use accessors.

// ---------------------------------------------------------------------------
// Scheduler
// ---------------------------------------------------------------------------

struct GreenScheduler {
    injector: Injector<ThreadPtr>,
    stealers: Vec<Stealer<ThreadPtr>>,
    stack_pool: StackPool,
    park_mutex: Mutex<()>,
    park_cv: Condvar,
}

static SCHEDULER: OnceLock<GreenScheduler> = OnceLock::new();

fn sched() -> &'static GreenScheduler {
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

        let Some(ThreadPtr(thread_ptr)) = task else {
            let guard = sc.park_mutex.lock().unwrap();
            let _ = sc
                .park_cv
                .wait_timeout(guard, std::time::Duration::from_millis(1))
                .unwrap();
            continue;
        };

        let thread = unsafe { &mut *thread_ptr };

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

        // Set TLS for the green thread
        WORKER_CURRENT_THREAD.set(thread_ptr);
        WORKER_YIELD_REASON.set(YieldReason::None);
        PREEMPT_TICKS.set(0);

        // Restore per-green-thread TLS state
        crate::runtime::error_flag_set(thread.error_flag);
        crate::runtime::shadow_stack_set(thread.shadow_stack_top);

        // Context switch to green thread
        unsafe {
            aster_context_switch(&raw mut scheduler_ctx, &raw const thread.context);
        }

        // Green thread yielded back — save per-green-thread TLS state
        thread.error_flag = crate::runtime::error_flag_get();
        thread.shadow_stack_top = crate::runtime::shadow_stack_get();
        WORKER_CURRENT_THREAD.set(std::ptr::null_mut());

        match WORKER_YIELD_REASON.get() {
            YieldReason::Preempted => {
                thread.state.lock().unwrap().status = ThreadStatus::Runnable;
                local.push(ThreadPtr(thread_ptr));
            }

            YieldReason::Completed { result, failed } => {
                complete_thread(thread, result, failed);
                recycle_stack(thread);
            }

            YieldReason::Cancelled => {
                let mut st = thread.state.lock().unwrap();
                st.status = ThreadStatus::Cancelled;
                thread.cv.notify_all();
                let waiters = std::mem::take(&mut st.green_waiters);
                drop(st);
                wake_waiters(waiters);
                recycle_stack(thread);
            }

            YieldReason::WaitingOnTask(target_ptr) => {
                let target = unsafe { &*target_ptr };
                let mut target_st = target.state.lock().unwrap();
                if is_terminal(target_st.status) {
                    // Target already done — re-enqueue immediately
                    drop(target_st);
                    thread.state.lock().unwrap().status = ThreadStatus::Runnable;
                    local.push(ThreadPtr(thread_ptr));
                } else {
                    // Park until target completes
                    target_st.green_waiters.push(thread_ptr);
                    drop(target_st);
                    thread.state.lock().unwrap().status = ThreadStatus::Suspended;
                }
            }

            YieldReason::None => {
                // Should not happen — treat as preempted
                thread.state.lock().unwrap().status = ThreadStatus::Runnable;
                local.push(ThreadPtr(thread_ptr));
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

fn wake_waiters(waiters: Vec<*mut GreenThread>) {
    if waiters.is_empty() {
        return;
    }
    let sc = sched();
    for waiter in waiters {
        sc.injector.push(ThreadPtr(waiter));
    }
    sc.park_cv.notify_all();
}

fn recycle_stack(thread: &mut GreenThread) {
    if let Some(stack) = thread.stack.take() {
        sched().stack_pool.put(stack);
    }
}

// ---------------------------------------------------------------------------
// Public API — called from runtime.rs
// ---------------------------------------------------------------------------

pub(crate) fn spawn_green_thread(entry: usize, args: usize) -> *mut GreenThread {
    let sc = sched();
    let stack = sc.stack_pool.get();
    let stack_top = stack.top();

    let thread = Box::into_raw(Box::new(GreenThread {
        context: MachineContext::new(),
        stack: Some(stack),
        error_flag: false,
        shadow_stack_top: std::ptr::null_mut(),
        state: Mutex::new(TaskState {
            status: ThreadStatus::Runnable,
            cancel_requested: false,
            consumed: false,
            result: 0,
            failed: false,
            green_waiters: Vec::new(),
        }),
        cv: std::sync::Condvar::new(),
    }));

    unsafe {
        aster_context_init(&raw mut (*thread).context, stack_top, entry, args);
    }

    sc.injector.push(ThreadPtr(thread));
    sc.park_cv.notify_all();

    thread
}

pub(crate) fn allocate_terminal_thread(result: i64, failed: bool) -> *mut GreenThread {
    let status = if failed {
        ThreadStatus::Failed
    } else {
        ThreadStatus::Ready
    };

    Box::into_raw(Box::new(GreenThread {
        context: MachineContext::new(),
        stack: None,
        error_flag: false,
        shadow_stack_top: std::ptr::null_mut(),
        state: Mutex::new(TaskState {
            status,
            cancel_requested: false,
            consumed: false,
            result,
            failed,
            green_waiters: Vec::new(),
        }),
        cv: Condvar::new(),
    }))
}

pub(crate) fn cancel_thread(thread_ptr: *mut GreenThread) {
    let thread = unsafe { &*thread_ptr };
    let mut st = thread.state.lock().unwrap();
    st.cancel_requested = true;

    match st.status {
        ThreadStatus::Runnable => {
            // Not yet running — mark terminal immediately.
            // Worker loop's is_terminal check will skip it when dequeued.
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
            // Waiting on another task — mark terminal immediately
            st.status = ThreadStatus::Cancelled;
            thread.cv.notify_all();
            let waiters = std::mem::take(&mut st.green_waiters);
            drop(st);
            wake_waiters(waiters);
            recycle_stack(unsafe { &mut *thread_ptr });
        }
        _ => {
            // Already terminal — nothing to do
        }
    }
}

pub(crate) fn wait_cancel_thread(thread_ptr: *mut GreenThread) {
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
pub(crate) fn consume_thread_result(thread_ptr: *mut GreenThread) -> i64 {
    wait_for_terminal(thread_ptr);

    let thread = unsafe { &*thread_ptr };
    let mut st = thread.state.lock().unwrap();

    if st.consumed {
        crate::runtime::error_flag_set(true);
        return 0;
    }
    st.consumed = true;

    match st.status {
        ThreadStatus::Ready => st.result,
        ThreadStatus::Failed | ThreadStatus::Cancelled => {
            crate::runtime::error_flag_set(true);
            0
        }
        _ => {
            crate::runtime::error_flag_set(true);
            0
        }
    }
}

fn wait_for_terminal(thread_ptr: *mut GreenThread) {
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
            yield_to_scheduler(YieldReason::WaitingOnTask(thread_ptr));
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
// Yield — called from green thread code
// ---------------------------------------------------------------------------

pub(crate) fn yield_to_scheduler(reason: YieldReason) {
    WORKER_YIELD_REASON.set(reason);
    let scheduler_ctx = WORKER_SCHEDULER_CTX.get();
    let current = WORKER_CURRENT_THREAD.get();
    assert!(!scheduler_ctx.is_null(), "yield outside worker thread");
    assert!(!current.is_null(), "yield with no current green thread");
    unsafe {
        aster_context_switch(&raw mut (*current).context, scheduler_ctx);
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
// Green thread exit — called from assembly trampoline
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub extern "C" fn aster_green_thread_exit(result: i64) {
    let failed = crate::runtime::error_flag_get();
    crate::runtime::error_flag_set(false);
    yield_to_scheduler(YieldReason::Completed { result, failed });
    unreachable!("aster_green_thread_exit returned");
}
