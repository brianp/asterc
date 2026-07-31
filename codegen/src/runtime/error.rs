use std::cell::Cell;
use std::sync::{Arc, Mutex};

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

/// Construct a built-in error object carrying a single `message: String`
/// field and set it as the current typed error. `class_id` is the sentinel
/// ClassId of the error type declared in the builtin's `throws` signature
/// (see `ast::builtin_errors`). Both AOT and JIT reference this same helper,
/// so the runtime representation is identical across backends.
pub(crate) fn set_message_error(class_id: u32, message: &str) {
    use super::alloc::aster_class_alloc_typed;
    use super::string::aster_string_new_from_rust;
    use ast::builtin_errors::{MESSAGE_ONLY_PTR_COUNT, MESSAGE_ONLY_SIZE};

    // Pause GC across the object + string allocation: `obj` lives only in this
    // local (not on the shadow stack, and the error thread-local is not a GC
    // root), so a collection triggered by the string allocation would otherwise
    // sweep `obj` before we write into it.
    let obj = super::gc::pause_gc(|| {
        let obj = aster_class_alloc_typed(MESSAGE_ONLY_SIZE, MESSAGE_ONLY_PTR_COUNT);
        unsafe {
            *(obj as *mut i64) = aster_string_new_from_rust(message) as i64;
        }
        obj
    });
    aster_error_set_typed(class_id as i64, obj as i64);
}

/// Construct a `ProcessError` object (`message` + `command`) and set it as the
/// current typed error.
pub(crate) fn set_process_error(message: &str, command: &str) {
    use super::alloc::aster_class_alloc_typed;
    use super::string::aster_string_new_from_rust;
    use ast::builtin_errors::{
        PROCESS_ERROR_CLASS_ID, PROCESS_ERROR_PTR_COUNT, PROCESS_ERROR_SIZE,
    };

    // Pause GC across all three allocations: `obj` and the first string are
    // held only in locals until the whole graph is built, so a collection
    // triggered by a later string allocation would otherwise free them.
    let obj = super::gc::pause_gc(|| {
        let obj = aster_class_alloc_typed(PROCESS_ERROR_SIZE, PROCESS_ERROR_PTR_COUNT);
        unsafe {
            let base = obj as *mut i64;
            *base = aster_string_new_from_rust(message) as i64;
            *base.add(1) = aster_string_new_from_rust(command) as i64;
        }
        obj
    });
    aster_error_set_typed(PROCESS_ERROR_CLASS_ID as i64, obj as i64);
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
    /// Raw pointers from Arc::into_raw. Each represents one Arc reference
    /// owned by the scope. Freed in aster_async_scope_exit via free_scoped_thread.
    tasks: Vec<*const GreenThread>,
}

// SAFETY: *const GreenThread pointers are backed by Arc references and are valid
// until free_scoped_thread consumes them.
unsafe impl Send for AsyncScopeState {}

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

pub(super) fn register_task_with_scope(scope: *mut u8, task: *const GreenThread) {
    if let Some(scope) = live_scope(scope) {
        // Mark the task as scoped so consume_thread_result defers freeing to scope exit.
        let thread = unsafe { &*task };
        thread.state.lock().unwrap().scoped = true;
        // Increment the Arc refcount so the scope owns an independent reference.
        unsafe { Arc::increment_strong_count(task) };
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
    // Free all scoped task Arc references. free_scoped_thread consumes the scope's
    // Arc reference; when the refcount reaches zero, the struct is freed.
    for task in tasks {
        scheduler::free_scoped_thread(task);
    }
    // Free the scope handle itself
    unsafe { drop(Box::from_raw(scope as *mut AsyncScopeHandle)) };
}
