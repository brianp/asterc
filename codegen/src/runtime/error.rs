use std::cell::Cell;
use std::sync::Mutex;

use crate::green::scheduler;
use crate::green::thread::GreenThread;

// ---------------------------------------------------------------------------
// Error handling — per-thread error flag (saved/restored per green thread)
// ---------------------------------------------------------------------------

thread_local! {
    static ERROR_FLAG: Cell<bool> = const { Cell::new(false) };
    static ERROR_TYPE_TAG: Cell<i64> = const { Cell::new(0) };
    static ERROR_VALUE: Cell<i64> = const { Cell::new(0) };
}

#[unsafe(no_mangle)]
pub extern "C" fn aster_error_set() {
    ERROR_FLAG.set(true);
}

/// Set error flag with a type tag and the error object pointer.
#[unsafe(no_mangle)]
pub extern "C" fn aster_error_set_typed(type_tag: i64, value: i64) {
    ERROR_FLAG.set(true);
    ERROR_TYPE_TAG.set(type_tag);
    ERROR_VALUE.set(value);
}

#[unsafe(no_mangle)]
pub extern "C" fn aster_error_check() -> i8 {
    let was = ERROR_FLAG.get();
    ERROR_FLAG.set(false);
    was as i8
}

/// Return the type tag of the current error (valid after error_check returns true).
#[unsafe(no_mangle)]
pub extern "C" fn aster_error_get_tag() -> i64 {
    ERROR_TYPE_TAG.get()
}

/// Return the error object pointer (valid after error_check returns true).
#[unsafe(no_mangle)]
pub extern "C" fn aster_error_get_value() -> i64 {
    ERROR_VALUE.get()
}

pub(crate) fn error_flag_get() -> bool {
    ERROR_FLAG.get()
}

pub(crate) fn error_flag_set(val: bool) {
    ERROR_FLAG.set(val);
}

#[unsafe(no_mangle)]
pub extern "C" fn aster_safepoint() {
    scheduler::safepoint();
}

#[unsafe(no_mangle)]
pub extern "C" fn aster_panic() {
    eprintln!("aster: uncaught error");
    std::process::abort();
}

// ---------------------------------------------------------------------------
// Async scope
// ---------------------------------------------------------------------------

struct AsyncScopeState {
    tasks: Vec<*mut GreenThread>,
}

struct AsyncScopeHandle {
    state: Mutex<AsyncScopeState>,
}

fn live_scope(scope: *const u8) -> Option<&'static AsyncScopeHandle> {
    if scope.is_null() {
        None
    } else {
        Some(unsafe { &*(scope as *const AsyncScopeHandle) })
    }
}

pub(super) fn register_task_with_scope(scope: *mut u8, task: *mut GreenThread) {
    if let Some(scope) = live_scope(scope) {
        // Mark the task as scoped so consume_thread_result defers freeing to scope exit.
        let thread = unsafe { &*task };
        thread.state.lock().unwrap().scoped = true;
        let mut state = scope.state.lock().unwrap();
        state.tasks.push(task);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn aster_async_scope_enter() -> *mut u8 {
    Box::into_raw(Box::new(AsyncScopeHandle {
        state: Mutex::new(AsyncScopeState { tasks: Vec::new() }),
    })) as *mut u8
}

#[unsafe(no_mangle)]
pub extern "C" fn aster_async_scope_exit(scope: *mut u8) {
    if scope.is_null() {
        return;
    }
    let scope_handle = unsafe { &*(scope as *const AsyncScopeHandle) };
    let tasks = {
        let mut state = scope_handle.state.lock().unwrap();
        std::mem::take(&mut state.tasks)
    };
    for &task in &tasks {
        scheduler::cancel_thread(task);
    }
    for &task in &tasks {
        scheduler::wait_cancel_thread(task);
    }
    // Free all scoped task structs. Scoped tasks defer freeing to scope exit,
    // so even consumed tasks still need their struct freed here.
    for task in tasks {
        scheduler::free_scoped_thread(task);
    }
    // Free the scope handle itself
    unsafe { drop(Box::from_raw(scope as *mut AsyncScopeHandle)) };
}
