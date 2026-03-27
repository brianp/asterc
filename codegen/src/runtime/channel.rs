use std::sync::{Arc, Mutex};

use crate::green::scheduler;
use crate::green::thread::GreenThread;

use super::error::aster_error_set;

// ---------------------------------------------------------------------------
// Channel — Phase 8
// ---------------------------------------------------------------------------

struct AsterChannel {
    inner: Mutex<AsterChannelState>,
}

struct AsterChannelState {
    buffer: std::collections::VecDeque<i64>,
    capacity: usize,
    closed: bool,
    send_waiters: Vec<(Arc<GreenThread>, i64)>,
    recv_waiters: Vec<Arc<GreenThread>>,
}

/// Create a new channel with the given capacity (0 = unbounded, default 64).
#[unsafe(no_mangle)]
pub extern "C" fn aster_channel_new(capacity: i64) -> *mut u8 {
    let cap = if capacity <= 0 { 64 } else { capacity as usize };
    let ch = Box::new(AsterChannel {
        inner: Mutex::new(AsterChannelState {
            buffer: std::collections::VecDeque::with_capacity(cap),
            capacity: cap,
            closed: false,
            send_waiters: Vec::new(),
            recv_waiters: Vec::new(),
        }),
    });
    Box::into_raw(ch) as *mut u8
}

/// Non-blocking send. Drops the value silently if buffer is full or channel is closed.
#[unsafe(no_mangle)]
pub extern "C" fn aster_channel_send(ch: *mut u8, value: i64) {
    if ch.is_null() {
        return;
    }
    let c = unsafe { &*(ch as *const AsterChannel) };
    let mut state = c.inner.lock().unwrap();
    if state.closed {
        return;
    }
    // Direct delivery only when buffer is empty (preserves FIFO ordering)
    if state.buffer.is_empty()
        && let Some(waiter) = state.recv_waiters.pop()
    {
        drop(state);
        scheduler::wake_thread_with_value(waiter, value);
        return;
    }
    if state.buffer.len() < state.capacity {
        state.buffer.push_back(value);
        // Wake a receiver now that there's data in the buffer
        if let Some(waiter) = state.recv_waiters.pop() {
            drop(state);
            scheduler::wake_thread(waiter);
        }
    }
    // else: drop silently (fire-and-forget send semantics)
}

/// Blocking send. Suspends if buffer is full.
#[unsafe(no_mangle)]
pub extern "C" fn aster_channel_wait_send(ch: *mut u8, value: i64) {
    if ch.is_null() {
        aster_error_set();
        return;
    }
    let c = unsafe { &*(ch as *const AsterChannel) };
    let mut state = c.inner.lock().unwrap();
    if state.closed {
        aster_error_set();
        return;
    }
    // Direct delivery only when buffer is empty (preserves FIFO)
    if state.buffer.is_empty()
        && let Some(waiter) = state.recv_waiters.pop()
    {
        drop(state);
        scheduler::wake_thread_with_value(waiter, value);
        return;
    }
    if state.buffer.len() < state.capacity {
        state.buffer.push_back(value);
        if let Some(waiter) = state.recv_waiters.pop() {
            drop(state);
            scheduler::wake_thread(waiter);
        }
        return;
    }
    // Buffer full — suspend
    if let Some(current_arc) = scheduler::current_green_thread_arc() {
        state.send_waiters.push((current_arc, value));
        drop(state);
        scheduler::suspend_for_channel_send();
    } else {
        // Fallback for non-green-thread context: spin with timeout
        drop(state);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            std::thread::yield_now();
            let mut state = c.inner.lock().unwrap();
            if state.buffer.len() < state.capacity || state.closed {
                if !state.closed {
                    state.buffer.push_back(value);
                }
                break;
            }
            drop(state);
            if std::time::Instant::now() >= deadline {
                aster_error_set();
                break;
            }
        }
    }
}

/// Try-send. Sets error flag if buffer full or closed.
#[unsafe(no_mangle)]
pub extern "C" fn aster_channel_try_send(ch: *mut u8, value: i64) {
    if ch.is_null() {
        aster_error_set();
        return;
    }
    let c = unsafe { &*(ch as *const AsterChannel) };
    let mut state = c.inner.lock().unwrap();
    if state.closed {
        aster_error_set();
        return;
    }
    // Direct delivery only when buffer is empty (preserves FIFO)
    if state.buffer.is_empty()
        && let Some(waiter) = state.recv_waiters.pop()
    {
        drop(state);
        scheduler::wake_thread_with_value(waiter, value);
        return;
    }
    if state.buffer.len() < state.capacity {
        state.buffer.push_back(value);
    } else {
        aster_error_set();
    }
}

/// Non-blocking receive. Returns 0 and sets error_flag=false if empty (nil semantics).
/// Returns value if available.
#[unsafe(no_mangle)]
pub extern "C" fn aster_channel_receive(ch: *mut u8) -> i64 {
    if ch.is_null() {
        return 0;
    }
    let c = unsafe { &*(ch as *const AsterChannel) };
    let mut state = c.inner.lock().unwrap();
    if let Some(value) = state.buffer.pop_front() {
        // Wake a send waiter if one is pending
        if let Some((waiter, send_val)) = state.send_waiters.pop() {
            state.buffer.push_back(send_val);
            drop(state);
            scheduler::wake_thread(waiter);
        }
        return value;
    }
    0 // nil
}

/// Blocking receive. Suspends if buffer is empty.
#[unsafe(no_mangle)]
pub extern "C" fn aster_channel_wait_receive(ch: *mut u8) -> i64 {
    if ch.is_null() {
        aster_error_set();
        return 0;
    }
    let c = unsafe { &*(ch as *const AsterChannel) };
    let mut state = c.inner.lock().unwrap();
    if let Some(value) = state.buffer.pop_front() {
        if let Some((waiter, send_val)) = state.send_waiters.pop() {
            state.buffer.push_back(send_val);
            drop(state);
            scheduler::wake_thread(waiter);
        }
        return value;
    }
    if state.closed {
        aster_error_set();
        return 0;
    }
    // Empty — suspend
    if let Some(current_arc) = scheduler::current_green_thread_arc() {
        state.recv_waiters.push(current_arc);
        drop(state);
        return scheduler::suspend_for_channel_receive();
    }
    // Fallback: spin with timeout
    drop(state);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        std::thread::yield_now();
        let mut state = c.inner.lock().unwrap();
        if let Some(value) = state.buffer.pop_front() {
            return value;
        }
        if state.closed {
            aster_error_set();
            return 0;
        }
        drop(state);
        if std::time::Instant::now() >= deadline {
            aster_error_set();
            return 0;
        }
    }
}

/// Try-receive. Sets error flag if empty or closed.
#[unsafe(no_mangle)]
pub extern "C" fn aster_channel_try_receive(ch: *mut u8) -> i64 {
    if ch.is_null() {
        aster_error_set();
        return 0;
    }
    let c = unsafe { &*(ch as *const AsterChannel) };
    let mut state = c.inner.lock().unwrap();
    if let Some(value) = state.buffer.pop_front() {
        if let Some((waiter, send_val)) = state.send_waiters.pop() {
            state.buffer.push_back(send_val);
            drop(state);
            scheduler::wake_thread(waiter);
        }
        return value;
    }
    aster_error_set();
    0
}

/// Close the channel. Wake all waiters with errors.
#[unsafe(no_mangle)]
pub extern "C" fn aster_channel_close(ch: *mut u8) {
    if ch.is_null() {
        return;
    }
    let c = unsafe { &*(ch as *const AsterChannel) };
    let mut state = c.inner.lock().unwrap();
    state.closed = true;
    let send_waiters: Vec<_> = state.send_waiters.drain(..).collect();
    let recv_waiters: Vec<_> = state.recv_waiters.drain(..).collect();
    drop(state);
    for (waiter, _) in send_waiters {
        scheduler::wake_thread_with_error(waiter);
    }
    for waiter in recv_waiters {
        scheduler::wake_thread_with_error(waiter);
    }
}
