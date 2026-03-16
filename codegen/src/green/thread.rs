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
    pub(crate) result: i64,
    pub(crate) failed: bool,
    pub(crate) green_waiters: Vec<*mut GreenThread>,
}

pub(crate) struct GreenThread {
    pub(crate) context: MachineContext,
    pub(crate) stack: Option<GreenStack>,
    pub(crate) error_flag: bool,
    pub(crate) shadow_stack_top: *mut u8,
    pub(crate) state: Mutex<TaskState>,
    pub(crate) cv: Condvar,
}

unsafe impl Send for GreenThread {}
// SAFETY: Non-Mutex fields (context, error_flag, shadow_stack_top) are only
// accessed by the single worker currently running the green thread. The scheduler
// guarantees exclusive access: a thread is either running on exactly one worker,
// sitting in a deque, or terminal. The Mutex<TaskState> serializes all status
// transitions that make a thread available to a different worker.
unsafe impl Sync for GreenThread {}

/// Newtype wrapper so `*mut GreenThread` can go into crossbeam deques.
pub(crate) struct ThreadPtr(pub *mut GreenThread);

unsafe impl Send for ThreadPtr {}

#[derive(Clone, Copy)]
pub(crate) enum YieldReason {
    None,
    Preempted,
    Completed { result: i64, failed: bool },
    Cancelled,
    WaitingOnTask(*mut GreenThread),
    WaitingOnIo,
    WaitingOnBlockingPool,
}
