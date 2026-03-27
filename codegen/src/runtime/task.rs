use crate::green::scheduler;
use crate::green::thread::GreenThread;

use super::error::{aster_error_set, error_flag_get, register_task_with_scope};
use super::list::{aster_list_get, aster_list_len, aster_list_new, aster_list_push};

// ---------------------------------------------------------------------------
// Task spawn / resolve / cancel — backed by green threads
// ---------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub extern "C" fn aster_task_spawn(entry: usize, args: *mut u8, scope: *mut u8) -> *mut u8 {
    let thread = scheduler::spawn_green_thread(entry, args as usize);
    register_task_with_scope(scope, thread);
    thread as *mut u8
}

#[unsafe(no_mangle)]
pub extern "C" fn aster_task_block_on(entry: usize, args: *mut u8) -> i64 {
    let task = aster_task_spawn(entry, args, std::ptr::null_mut());
    scheduler::consume_thread_result(task as *mut GreenThread)
}

#[unsafe(no_mangle)]
pub extern "C" fn aster_task_from_i64(value: i64, failed: i8) -> *mut u8 {
    scheduler::allocate_terminal_thread(value, failed != 0) as *mut u8
}

#[unsafe(no_mangle)]
pub extern "C" fn aster_task_from_f64(value: f64, failed: i8) -> *mut u8 {
    scheduler::allocate_terminal_thread(value.to_bits() as i64, failed != 0) as *mut u8
}

#[unsafe(no_mangle)]
pub extern "C" fn aster_task_from_i8(value: i8, failed: i8) -> *mut u8 {
    scheduler::allocate_terminal_thread(value as i64, failed != 0) as *mut u8
}

#[unsafe(no_mangle)]
pub extern "C" fn aster_task_is_ready(task: *const u8) -> i8 {
    if task.is_null() {
        return 0;
    }
    scheduler::is_thread_ready(task as *const GreenThread) as i8
}

#[unsafe(no_mangle)]
pub extern "C" fn aster_task_cancel(task: *mut u8) -> i64 {
    if !task.is_null() {
        scheduler::cancel_thread(task as *mut GreenThread);
    }
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn aster_task_wait_cancel(task: *mut u8) -> i64 {
    if !task.is_null() {
        scheduler::wait_cancel_thread(task as *mut GreenThread);
    }
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn aster_task_resolve_i64(task: *mut u8) -> i64 {
    if task.is_null() {
        aster_error_set();
        return 0;
    }
    scheduler::consume_thread_result(task as *mut GreenThread)
}

#[unsafe(no_mangle)]
pub extern "C" fn aster_task_resolve_f64(task: *mut u8) -> f64 {
    if task.is_null() {
        aster_error_set();
        return 0.0;
    }
    let bits = scheduler::consume_thread_result(task as *mut GreenThread) as u64;
    f64::from_bits(bits)
}

#[unsafe(no_mangle)]
pub extern "C" fn aster_task_resolve_i8(task: *mut u8) -> i8 {
    if task.is_null() {
        aster_error_set();
        return 0;
    }
    scheduler::consume_thread_result(task as *mut GreenThread) as i8
}

#[unsafe(no_mangle)]
pub extern "C" fn aster_task_resolve_all_i64(tasks: *mut u8) -> *mut u8 {
    if tasks.is_null() {
        aster_error_set();
        return std::ptr::null_mut();
    }
    let len = aster_list_len(tasks);
    let out = aster_list_new(len, 0);
    for index in 0..len {
        let task = aster_list_get(tasks, index) as *mut u8;
        let value = aster_task_resolve_i64(task);
        if error_flag_get() {
            return out;
        }
        aster_list_push(out, value);
    }
    out
}

#[unsafe(no_mangle)]
pub extern "C" fn aster_task_resolve_first_i64(tasks: *mut u8) -> i64 {
    if tasks.is_null() {
        aster_error_set();
        return 0;
    }
    let len = aster_list_len(tasks);
    if len == 0 {
        aster_error_set();
        return 0;
    }
    let task_handles: Vec<*mut u8> = (0..len)
        .map(|index| aster_list_get(tasks, index) as *mut u8)
        .collect();
    loop {
        for (winner_index, &task) in task_handles.iter().enumerate() {
            if task.is_null() {
                continue;
            }
            if scheduler::is_thread_ready(task as *const GreenThread) {
                for (index, other) in task_handles.iter().enumerate() {
                    if index != winner_index && !other.is_null() {
                        scheduler::cancel_thread(*other as *mut GreenThread);
                    }
                }
                return aster_task_resolve_i64(task);
            }
        }
        // Yield to scheduler if on a worker, otherwise OS yield
        if scheduler::is_worker_thread() {
            scheduler::safepoint();
        } else {
            std::thread::yield_now();
        }
    }
}
