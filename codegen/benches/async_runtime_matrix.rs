use std::hint::black_box;
use std::time::{Duration, Instant};

use codegen::async_runtime::{
    AsyncRuntime, BlockingRequest, CoroutineBody, CoroutineContext, CoroutineStep,
};

struct ImmediateCoroutine {
    value: i64,
}

impl CoroutineBody for ImmediateCoroutine {
    fn resume(&mut self, _cx: &mut CoroutineContext<'_>) -> CoroutineStep {
        CoroutineStep::Complete(self.value)
    }
}

struct SleepingCoroutine {
    remaining_yields: usize,
    value: i64,
}

impl CoroutineBody for SleepingCoroutine {
    fn resume(&mut self, _cx: &mut CoroutineContext<'_>) -> CoroutineStep {
        if self.remaining_yields == 0 {
            CoroutineStep::Complete(self.value)
        } else {
            self.remaining_yields -= 1;
            CoroutineStep::Yield
        }
    }
}

struct SmallBlockingCoroutine {
    started: bool,
    payload: i64,
}

impl CoroutineBody for SmallBlockingCoroutine {
    fn resume(&mut self, cx: &mut CoroutineContext<'_>) -> CoroutineStep {
        if self.started {
            CoroutineStep::Complete(
                cx.take_blocking_result()
                    .expect("blocking result should be available on resume"),
            )
        } else {
            self.started = true;
            CoroutineStep::Block(BlockingRequest::native(self.payload))
        }
    }
}

fn main() {
    run_benchmark("web_fan_out_fan_in", benchmark_web_fan_out_fan_in);
    run_benchmark("many_sleeping_tasks", benchmark_many_sleeping_tasks);
    run_benchmark(
        "frequent_small_blocking_jobs",
        benchmark_small_blocking_jobs,
    );
    run_benchmark("gc_pause_request_load", benchmark_gc_pause_request_load);
}

fn run_benchmark(name: &str, mut bench: impl FnMut() -> i64) {
    let started = Instant::now();
    let result = black_box(bench());
    let elapsed = started.elapsed();
    println!(
        "{name}: result={result} elapsed={}",
        format_duration(elapsed)
    );
}

fn format_duration(duration: Duration) -> String {
    format!("{:.3}ms", duration.as_secs_f64() * 1_000.0)
}

fn benchmark_web_fan_out_fan_in() -> i64 {
    const TASKS: usize = 256;
    let mut runtime = AsyncRuntime::new(4);
    let handles: Vec<_> = (0..TASKS)
        .map(|value| {
            runtime.spawn_external(Box::new(ImmediateCoroutine {
                value: value as i64,
            }))
        })
        .collect();
    runtime
        .resolve_all(&handles)
        .expect("fan-out/fan-in should resolve")
        .into_iter()
        .sum()
}

fn benchmark_many_sleeping_tasks() -> i64 {
    const TASKS: usize = 512;
    let mut runtime = AsyncRuntime::new(4);
    let handles: Vec<_> = (0..TASKS)
        .map(|value| {
            runtime.spawn_external(Box::new(SleepingCoroutine {
                remaining_yields: 8,
                value: value as i64,
            }))
        })
        .collect();
    runtime
        .resolve_all(&handles)
        .expect("sleeping tasks should resolve")
        .into_iter()
        .sum()
}

fn benchmark_small_blocking_jobs() -> i64 {
    const TASKS: usize = 256;
    let mut runtime = AsyncRuntime::new(4);
    let handles: Vec<_> = (0..TASKS)
        .map(|value| {
            runtime.spawn_external(Box::new(SmallBlockingCoroutine {
                started: false,
                payload: value as i64,
            }))
        })
        .collect();

    while runtime.pending_blocking_job_count() > 0
        || handles.iter().any(|task| {
            !matches!(
                runtime.task_state(*task),
                Some(codegen::async_runtime::RuntimeTaskState::Ready(_))
            )
        })
    {
        while let Some(job) = runtime.first_pending_blocking_job() {
            runtime.complete_blocking_job(job, 1);
        }
        if !runtime.run_one_tick() && runtime.pending_blocking_job_count() == 0 {
            break;
        }
    }

    runtime
        .resolve_all(&handles)
        .expect("blocking jobs should resolve")
        .into_iter()
        .sum()
}

fn benchmark_gc_pause_request_load() -> i64 {
    const TASKS: usize = 64;
    const WORKERS: usize = 4;

    let mut runtime = AsyncRuntime::new(WORKERS);
    let handles: Vec<_> = (0..TASKS)
        .map(|value| {
            runtime.spawn_external(Box::new(ImmediateCoroutine {
                value: value as i64,
            }))
        })
        .collect();

    for worker in 0..WORKERS {
        for _ in 0..64 {
            let root = runtime.allocate_heap_object(Vec::new());
            runtime.add_worker_root(worker, root);
        }
    }

    runtime.request_stop_the_world_collection();
    for worker in 0..WORKERS {
        runtime.worker_reach_safepoint(worker);
    }
    let reclaimed = runtime
        .collect_garbage()
        .expect("gc should collect with all workers safepointed") as i64;

    reclaimed
        + runtime
            .resolve_all(&handles)
            .expect("request-like tasks should resolve")
            .into_iter()
            .sum::<i64>()
}
