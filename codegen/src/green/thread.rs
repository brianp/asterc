use std::cell::UnsafeCell;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::{Condvar, Mutex};

use super::context::MachineContext;
use super::stack::GreenStack;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ThreadStatus {
    Runnable,
    Running,
    Suspended,
    Ready,
    Failed,
    Cancelled,
}

pub(crate) fn is_terminal(status: ThreadStatus) -> bool {
    matches!(
        status,
        ThreadStatus::Ready | ThreadStatus::Failed | ThreadStatus::Cancelled
    )
}

pub(crate) struct TaskState {
    pub(crate) status: ThreadStatus,
    pub(crate) cancel_requested: bool,
    pub(crate) consumed: bool,
    /// True if this task is owned by an async scope, which is responsible for
    /// freeing the struct. Unscoped tasks are freed by consume_thread_result.
    pub(crate) scoped: bool,
    pub(crate) result: i64,
    pub(crate) failed: bool,
    pub(crate) green_waiters: Vec<Arc<GreenThread>>,
}

pub(crate) struct GreenThread {
    /// Scheduler-exclusive fields wrapped in UnsafeCell. Only the single worker
    /// currently executing this green thread may access these.
    pub(crate) context: UnsafeCell<MachineContext>,
    pub(crate) stack: UnsafeCell<Option<GreenStack>>,
    pub(crate) error_flag: UnsafeCell<bool>,
    pub(crate) shadow_stack_top: UnsafeCell<*mut u8>,
    pub(crate) state: Mutex<TaskState>,
    pub(crate) cv: Condvar,
    /// Debug-mode guard: detects if two workers try to run the same thread.
    pub(crate) running_on_worker: AtomicBool,
}

unsafe impl Send for GreenThread {}
// SAFETY: Non-Mutex fields (context, error_flag, shadow_stack_top, stack) are wrapped
// in UnsafeCell and only accessed by the single worker currently running the green thread.
// The scheduler guarantees exclusive access: a thread is either running on exactly one
// worker, sitting in a deque, or terminal. The Mutex<TaskState> serializes all status
// transitions that make a thread available to a different worker.
unsafe impl Sync for GreenThread {}

impl GreenThread {
    /// # Safety
    /// Caller must have exclusive access (running on the worker that owns this thread).
    pub(crate) unsafe fn context_mut(&self) -> *mut MachineContext {
        self.context.get()
    }

    /// # Safety
    /// Caller must have exclusive access.
    pub(crate) unsafe fn set_error_flag(&self, val: bool) {
        unsafe {
            *self.error_flag.get() = val;
        }
    }

    /// # Safety
    /// Caller must have exclusive access.
    pub(crate) unsafe fn get_error_flag(&self) -> bool {
        unsafe { *self.error_flag.get() }
    }

    /// # Safety
    /// Caller must have exclusive access.
    pub(crate) unsafe fn set_shadow_stack(&self, val: *mut u8) {
        unsafe {
            *self.shadow_stack_top.get() = val;
        }
    }

    /// # Safety
    /// Caller must have exclusive access.
    pub(crate) unsafe fn get_shadow_stack(&self) -> *mut u8 {
        unsafe { *self.shadow_stack_top.get() }
    }

    /// # Safety
    /// Caller must have exclusive access.
    pub(crate) unsafe fn take_stack(&self) -> Option<GreenStack> {
        unsafe { (*self.stack.get()).take() }
    }
}

/// Newtype wrapper so `Arc<GreenThread>` can go into crossbeam deques.
pub(crate) struct ThreadPtr(pub Arc<GreenThread>);

#[derive(Clone)]
pub(crate) enum YieldReason {
    None,
    Preempted,
    Completed { result: i64, failed: bool },
    Cancelled,
    WaitingOnTask(Arc<GreenThread>),
    WaitingOnIo,
    WaitingOnBlockingPool,
    WaitingOnMutex,
    WaitingOnChannelSend,
    WaitingOnChannelRecv,
}
