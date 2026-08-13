use std::sync::{Arc, Condvar, Mutex};
use std::thread;

use super::thread::GreenThread;

type BlockingWork = Box<dyn FnOnce() -> i64 + Send>;

struct Job {
    task: Arc<GreenThread>,
    work: BlockingWork,
}

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

    pub(crate) fn submit(&self, task: Arc<GreenThread>, work: BlockingWork) {
        let mut queue = self.sender.lock().unwrap();
        queue.push(Job { task, work });
        self.cv.notify_one();
    }
}

fn blocking_worker(pool: Arc<BlockingPool>) {
    // Record this blocking-pool OS thread's stack bounds so an error thrown
    // while running blocking work walks within bounds.
    crate::runtime::stacktrace::record_os_thread_bounds();
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

        // Execute the blocking work. If it throws, the error flag/tag/value are
        // set on THIS blocking-pool OS thread's TLS and the trace is captured in
        // the global (pointer-keyed) trace map at the throw site.
        let result = (job.work)();

        // Snapshot the error state this worker's TLS holds after the work, then
        // clear it so the next job on this worker starts clean.
        let failed = crate::runtime::error_flag_get();
        let error_tag = crate::runtime::error_tag_get();
        let error_value = crate::runtime::error_value_get();
        crate::runtime::error_flag_set(false);
        crate::runtime::error_tag_set(0);
        crate::runtime::error_value_set(0);

        // Hand the thrown error's tag/value across the thread boundary on the
        // suspended green thread's struct (exclusive access: it is parked, not
        // running on any worker). The switch-in that resumes it restores these
        // into the resuming worker's TLS, so the `!` after the blocking call
        // propagates the typed error and `e.trace()` resolves. Writes are
        // published to the resuming worker by the state mutex + injector push
        // below.
        unsafe {
            job.task.set_error_flag(failed);
            job.task.set_error_tag(error_tag);
            job.task.set_error_value(error_value);
        }

        // Resume the green thread with the result.
        // The thread yielded via blocking_submit and expects to continue
        // execution after the yield point. Setting Runnable (not Ready)
        // and pushing to the injector matches wake_thread_with_value semantics.
        {
            let mut st = job.task.state.lock().unwrap();
            st.result = result;
            st.failed = failed;
            st.status = super::thread::ThreadStatus::Runnable;
        }
        let sc = super::scheduler::sched();
        sc.injector.push(super::thread::ThreadPtr(job.task));
        sc.park_cv.notify_all();
    }
}
