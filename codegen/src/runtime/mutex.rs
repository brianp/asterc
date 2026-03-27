use std::sync::{Arc, Mutex};

use crate::green::scheduler;
use crate::green::thread::GreenThread;

use super::error::aster_error_set;

// ---------------------------------------------------------------------------
// Mutex — Phase 7
// ---------------------------------------------------------------------------

/// Internal representation of a green-thread-aware mutex.
struct AsterMutex {
    inner: Mutex<AsterMutexState>,
}

struct AsterMutexState {
    locked: bool,
    owner: usize,
    value: i64,
    wait_queue: Vec<Arc<GreenThread>>,
}

/// Allocate a new Mutex wrapping the given value.
#[unsafe(no_mangle)]
pub extern "C" fn aster_mutex_new(value: i64) -> *mut u8 {
    let m = Box::new(AsterMutex {
        inner: Mutex::new(AsterMutexState {
            locked: false,
            owner: 0,
            value,
            wait_queue: Vec::new(),
        }),
    });
    Box::into_raw(m) as *mut u8
}

/// Acquire the mutex. If contended, suspend the current green thread.
/// Returns the inner value.
#[unsafe(no_mangle)]
pub extern "C" fn aster_mutex_lock(mutex: *mut u8) -> i64 {
    if mutex.is_null() {
        aster_error_set();
        return 0;
    }
    let m = unsafe { &*(mutex as *const AsterMutex) };
    let mut state = m.inner.lock().unwrap();
    if !state.locked {
        state.locked = true;
        state.owner = scheduler::current_thread_id();
        return state.value;
    }
    // Contended — suspend on the wait queue
    if let Some(current_arc) = scheduler::current_green_thread_arc() {
        state.wait_queue.push(current_arc);
        drop(state);
        scheduler::suspend_for_mutex();
        // Re-read value after wakeup — we now own the lock
        let mut state = m.inner.lock().unwrap();
        state.locked = true;
        state.owner = scheduler::current_thread_id();
        return state.value;
    }
    // Fallback for non-green-thread context: spin with timeout
    drop(state);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        std::thread::yield_now();
        let mut state = m.inner.lock().unwrap();
        if !state.locked {
            state.locked = true;
            state.owner = scheduler::current_thread_id();
            return state.value;
        }
        drop(state);
        if std::time::Instant::now() >= deadline {
            aster_error_set();
            return 0;
        }
    }
}

/// Release the mutex and store the updated value. Wakes the first waiter.
#[unsafe(no_mangle)]
pub extern "C" fn aster_mutex_unlock(mutex: *mut u8, value: i64) {
    if mutex.is_null() {
        return;
    }
    let m = unsafe { &*(mutex as *const AsterMutex) };
    let mut state = m.inner.lock().unwrap();
    state.value = value;
    if let Some(waiter) = state.wait_queue.pop() {
        // Transfer ownership to the waiter
        state.owner = 0; // waiter will set on resume
        drop(state);
        scheduler::wake_thread(waiter);
    } else {
        state.locked = false;
        state.owner = 0;
    }
}

/// Read the current value without locking (for debug/inspection only).
#[unsafe(no_mangle)]
pub extern "C" fn aster_mutex_get_value(mutex: *mut u8) -> i64 {
    if mutex.is_null() {
        return 0;
    }
    let m = unsafe { &*(mutex as *const AsterMutex) };
    let state = m.inner.lock().unwrap();
    state.value
}
