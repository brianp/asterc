use std::sync::{Arc, Condvar, Mutex};
use std::thread;

use super::thread::GreenThread;

type BlockingWork = Box<dyn FnOnce() -> i64 + Send>;

struct Job {
    task_ptr: *mut GreenThread,
    work: BlockingWork,
}

unsafe impl Send for Job {}

pub(crate) struct BlockingPool {
    sender: Mutex<Vec<Job>>,
    cv: Condvar,
    _workers: Vec<thread::JoinHandle<()>>,
}

impl BlockingPool {
    pub(crate) fn new(thread_count: usize) -> Arc<Self> {
        let pool = Arc::new(Self {
            sender: Mutex::new(Vec::new()),
            cv: Condvar::new(),
            _workers: Vec::new(),
        });

        // Spawn worker threads — they live for the process lifetime
        for _ in 0..thread_count {
            let pool_ref = Arc::clone(&pool);
            thread::spawn(move || blocking_worker(pool_ref));
        }

        pool
    }

    pub(crate) fn submit(&self, task_ptr: *mut GreenThread, work: BlockingWork) {
        let mut queue = self.sender.lock().unwrap();
        queue.push(Job { task_ptr, work });
        self.cv.notify_one();
    }
}

fn blocking_worker(pool: Arc<BlockingPool>) {
    loop {
        let job = {
            let mut queue = pool.sender.lock().unwrap();
            loop {
                if let Some(job) = queue.pop() {
                    break job;
                }
                queue = pool.cv.wait(queue).unwrap();
            }
        };

        // Execute the blocking work
        let result = (job.work)();

        // Wake the green thread with the result
        let thread = unsafe { &*job.task_ptr };
        let mut st = thread.state.lock().unwrap();
        st.result = result;
        st.failed = false;
        st.status = super::thread::ThreadStatus::Ready;
        thread.cv.notify_all();
        let waiters = std::mem::take(&mut st.green_waiters);
        drop(st);

        // Re-enqueue waiters
        if !waiters.is_empty() {
            let sc = super::scheduler::sched();
            for waiter in waiters {
                sc.injector.push(super::thread::ThreadPtr(waiter));
            }
            sc.park_cv.notify_all();
        }
    }
}
