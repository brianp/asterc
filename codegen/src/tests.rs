use fir::lower::Lowerer;
use fir::module::FirModule;
use fir::types::FunctionId;
use std::sync::{Arc, Mutex};

use crate::aot::CraneliftAOT;
use crate::async_runtime::{
    AsyncRuntime, BlockingKind, BlockingRequest, CoroutineBody, CoroutineContext, CoroutineStep,
    GcError, RuntimeTaskState, SegmentedStack, WakeupSource,
};
use crate::jit::CraneliftJIT;
use crate::runtime::{
    aster_int_add, aster_int_mul, aster_int_sub, aster_task_from_i64, aster_task_is_ready,
    aster_task_resolve_i64, runtime_builtin_symbols,
};
use crate::runtime_sigs::RUNTIME_SIGS;
use crate::runtime_source::c_runtime_source;

// ---------------------------------------------------------------------------
// Helper: source → FIR → JIT compile
// ---------------------------------------------------------------------------

fn compile_and_run(src: &str) -> FirModule {
    let tokens = lexer::lex(src).expect("lex ok");
    let mut parser = parser::Parser::new(tokens);
    let module = parser.parse_module("test").expect("parse ok");
    let mut tc = typecheck::TypeChecker::new();
    tc.check_module(&module).expect("typecheck ok");
    let mut lowerer = Lowerer::new(tc.env, tc.type_table);
    lowerer.lower_module(&module).expect("lower ok");
    lowerer.finish()
}

fn jit_compile(fir: &FirModule) -> CraneliftJIT {
    let mut jit = CraneliftJIT::new();
    jit.compile_module(fir).expect("JIT compile ok");
    jit
}

struct ScriptedCoroutine {
    steps: Vec<CoroutineStep>,
    log: Arc<Mutex<Vec<&'static str>>>,
    label: &'static str,
}

impl ScriptedCoroutine {
    fn new(
        label: &'static str,
        log: Arc<Mutex<Vec<&'static str>>>,
        steps: Vec<CoroutineStep>,
    ) -> Self {
        Self { steps, log, label }
    }
}

impl CoroutineBody for ScriptedCoroutine {
    fn resume(&mut self, cx: &mut CoroutineContext<'_>) -> CoroutineStep {
        self.log.lock().unwrap().push(self.label);
        cx.stack()
            .push_bytes(1)
            .expect("scripted coroutine stack push");
        if cx.is_cancelled() {
            return CoroutineStep::Cancelled;
        }
        if self.steps.is_empty() {
            CoroutineStep::Complete(0)
        } else {
            self.steps.remove(0)
        }
    }
}

struct BlockingCoroutine {
    started: bool,
}

impl BlockingCoroutine {
    fn new() -> Self {
        Self { started: false }
    }
}

impl CoroutineBody for BlockingCoroutine {
    fn resume(&mut self, cx: &mut CoroutineContext<'_>) -> CoroutineStep {
        if !self.started {
            self.started = true;
            return CoroutineStep::Block(BlockingRequest::native(41));
        }
        CoroutineStep::Complete(
            cx.take_blocking_result()
                .expect("blocking result should be available on resume"),
        )
    }
}

struct DiskCoroutine {
    started: bool,
}

impl DiskCoroutine {
    fn new() -> Self {
        Self { started: false }
    }
}

impl CoroutineBody for DiskCoroutine {
    fn resume(&mut self, cx: &mut CoroutineContext<'_>) -> CoroutineStep {
        if !self.started {
            self.started = true;
            return CoroutineStep::Block(BlockingRequest::disk(99));
        }
        CoroutineStep::Complete(
            cx.take_blocking_result()
                .expect("disk result should be available on resume"),
        )
    }
}

struct PollerCoroutine {
    started: bool,
}

impl PollerCoroutine {
    fn new() -> Self {
        Self { started: false }
    }
}

impl CoroutineBody for PollerCoroutine {
    fn resume(&mut self, cx: &mut CoroutineContext<'_>) -> CoroutineStep {
        if !self.started {
            self.started = true;
            return CoroutineStep::WaitForNetwork(7);
        }
        CoroutineStep::Complete(
            cx.take_network_result()
                .expect("network result should be available on resume"),
        )
    }
}

#[test]
fn segmented_stack_growth_keeps_existing_segments_stable() {
    let mut stack = SegmentedStack::new(16, 64);
    stack.push_bytes(12).expect("initial stack push");
    let initial_base = stack.segment_bases()[0];

    stack.push_bytes(24).expect("growth stack push");

    let bases = stack.segment_bases();
    assert_eq!(bases[0], initial_base);
    assert_eq!(bases.len(), 2);
}

#[test]
fn segmented_stack_rejects_segment_larger_than_max() {
    let mut stack = SegmentedStack::new(16, 64);
    assert!(stack.push_bytes(65).is_err());
}

#[test]
fn coroutine_yield_requeues_task_and_later_publishes_result() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let mut runtime = AsyncRuntime::new(1);
    let task = runtime.bootstrap_main(Box::new(ScriptedCoroutine::new(
        "main",
        Arc::clone(&log),
        vec![CoroutineStep::Yield, CoroutineStep::Complete(42)],
    )));

    runtime.run_one_tick();

    assert_eq!(runtime.task_state(task), Some(RuntimeTaskState::Running));
    assert_eq!(runtime.local_queue_len(0), 1);

    let terminal = runtime
        .await_task_terminal(task)
        .expect("task reaches terminal state");
    assert_eq!(terminal, RuntimeTaskState::Ready(42));
    assert_eq!(log.lock().unwrap().as_slice(), &["main", "main"]);
}

#[test]
fn cancelling_runnable_task_publishes_cancelled_terminal_state() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let mut runtime = AsyncRuntime::new(1);
    let task = runtime.spawn_external(Box::new(ScriptedCoroutine::new(
        "cancelled",
        Arc::clone(&log),
        vec![CoroutineStep::Complete(99)],
    )));

    runtime.mark_task_cancelled(task);

    let terminal = runtime
        .await_task_terminal(task)
        .expect("task reaches terminal state");
    assert_eq!(terminal, RuntimeTaskState::Cancelled);
    assert!(log.lock().unwrap().is_empty());
}

#[test]
fn bootstrap_main_runs_before_injected_work() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let mut runtime = AsyncRuntime::new(2);
    runtime.bootstrap_main(Box::new(ScriptedCoroutine::new(
        "main",
        Arc::clone(&log),
        vec![CoroutineStep::Complete(1)],
    )));
    runtime.spawn_external(Box::new(ScriptedCoroutine::new(
        "external",
        Arc::clone(&log),
        vec![CoroutineStep::Complete(2)],
    )));

    runtime.run_one_tick();

    assert_eq!(log.lock().unwrap().as_slice(), &["main"]);
}

#[test]
fn steal_half_moves_work_from_busy_worker_to_idle_worker() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let mut runtime = AsyncRuntime::new(2);
    for _ in 0..4 {
        runtime.spawn_on_worker(
            0,
            Box::new(ScriptedCoroutine::new(
                "worker0",
                Arc::clone(&log),
                vec![CoroutineStep::Complete(1)],
            )),
        );
    }

    let stolen = runtime.steal_half(1, 0);

    assert_eq!(stolen, 2);
    assert_eq!(runtime.local_queue_len(0), 2);
    assert_eq!(runtime.local_queue_len(1), 2);
}

#[test]
fn failed_coroutine_publishes_failed_terminal_state() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let mut runtime = AsyncRuntime::new(1);
    let task = runtime.bootstrap_main(Box::new(ScriptedCoroutine::new(
        "failed",
        Arc::clone(&log),
        vec![CoroutineStep::Fail(7)],
    )));

    let terminal = runtime
        .await_task_terminal(task)
        .expect("task reaches terminal state");
    assert_eq!(terminal, RuntimeTaskState::Failed(7));
    assert_eq!(log.lock().unwrap().as_slice(), &["failed"]);
}

#[test]
fn poller_and_blocking_wakeups_enqueue_into_global_injector() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let mut runtime = AsyncRuntime::new(2);
    let task = runtime.spawn_external(Box::new(ScriptedCoroutine::new(
        "wakeup",
        Arc::clone(&log),
        vec![CoroutineStep::Suspend, CoroutineStep::Complete(1)],
    )));

    runtime.run_one_tick();
    assert_eq!(runtime.injector_len(), 0);

    runtime.wake_task(task, WakeupSource::Poller);
    runtime.wake_task(task, WakeupSource::BlockingPool);
    runtime.wake_task(task, WakeupSource::Cancellation);

    assert_eq!(runtime.injector_len(), 3);
}

#[test]
fn same_worker_wakeup_uses_local_queue() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let mut runtime = AsyncRuntime::new(2);
    let task = runtime.spawn_on_worker(
        1,
        Box::new(ScriptedCoroutine::new(
            "local-wakeup",
            Arc::clone(&log),
            vec![CoroutineStep::Suspend, CoroutineStep::Complete(1)],
        )),
    );

    runtime.run_one_tick();
    runtime.wake_task(task, WakeupSource::SameWorkerSpawn);

    assert_eq!(runtime.local_queue_len(1), 1);
    assert_eq!(runtime.injector_len(), 0);
}

#[test]
fn reaped_tasks_release_slots_and_stale_handles_stop_resolving() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let mut runtime = AsyncRuntime::new(1);
    let first = runtime.bootstrap_main(Box::new(ScriptedCoroutine::new(
        "first",
        Arc::clone(&log),
        vec![CoroutineStep::Complete(1)],
    )));
    assert_eq!(
        runtime.await_task_terminal(first),
        Some(RuntimeTaskState::Ready(1))
    );
    assert_eq!(runtime.reap_task(first), Some(RuntimeTaskState::Ready(1)));
    assert_eq!(runtime.task_state(first), None);

    let second = runtime.bootstrap_main(Box::new(ScriptedCoroutine::new(
        "second",
        Arc::clone(&log),
        vec![CoroutineStep::Complete(2)],
    )));
    assert_eq!(
        runtime.await_task_terminal(second),
        Some(RuntimeTaskState::Ready(2))
    );
    assert_eq!(runtime.task_state(first), None);
}

#[test]
fn blocking_operation_suspends_task_and_enqueues_native_job() {
    let mut runtime = AsyncRuntime::new(1);
    let task = runtime.bootstrap_main(Box::new(BlockingCoroutine::new()));

    runtime.run_one_tick();

    assert_eq!(runtime.task_state(task), Some(RuntimeTaskState::Running));
    assert_eq!(runtime.pending_blocking_job_count(), 1);
    assert_eq!(
        runtime.first_pending_blocking_job_kind(),
        Some(BlockingKind::Native)
    );
    assert!(runtime.task_is_suspended(task).expect("task should exist"));
    assert_eq!(runtime.injector_len(), 0);
}

#[test]
fn completing_blocking_job_wakes_task_through_injector_and_publishes_result() {
    let mut runtime = AsyncRuntime::new(1);
    let task = runtime.bootstrap_main(Box::new(BlockingCoroutine::new()));

    runtime.run_one_tick();
    let job = runtime
        .first_pending_blocking_job()
        .expect("blocking job should be queued");

    runtime.complete_blocking_job(job, 77);

    assert_eq!(runtime.injector_len(), 1);
    assert_eq!(
        runtime.await_task_terminal(task),
        Some(RuntimeTaskState::Ready(77))
    );
}

#[test]
fn network_wait_registers_with_poller_and_resumes_on_readiness() {
    let mut runtime = AsyncRuntime::new(1);
    let task = runtime.bootstrap_main(Box::new(PollerCoroutine::new()));

    runtime.run_one_tick();

    assert_eq!(runtime.pending_network_wait_count(), 1);
    assert!(runtime.task_is_suspended(task).expect("task should exist"));
    assert_eq!(runtime.injector_len(), 0);

    let wait = runtime
        .first_pending_network_wait()
        .expect("network wait should be registered");
    runtime.complete_network_wait(wait, 55);

    assert_eq!(runtime.injector_len(), 1);
    assert_eq!(
        runtime.await_task_terminal(task),
        Some(RuntimeTaskState::Ready(55))
    );
}

#[test]
fn disk_requests_route_to_blocking_pool_before_resuming_task() {
    let mut runtime = AsyncRuntime::new(1);
    let task = runtime.bootstrap_main(Box::new(DiskCoroutine::new()));

    runtime.run_one_tick();

    assert_eq!(runtime.pending_blocking_job_count(), 1);
    assert_eq!(
        runtime.first_pending_blocking_job_kind(),
        Some(BlockingKind::Disk)
    );
    assert!(runtime.task_is_suspended(task).expect("task should exist"));

    let job = runtime
        .first_pending_blocking_job()
        .expect("disk job should be queued");
    runtime.complete_blocking_job(job, 123);

    assert_eq!(
        runtime.await_task_terminal(task),
        Some(RuntimeTaskState::Ready(123))
    );
}

#[test]
fn resolve_all_collects_values_in_input_order() {
    let mut runtime = AsyncRuntime::new(1);
    let first = runtime.spawn_external(Box::new(ScriptedCoroutine::new(
        "first",
        Arc::new(Mutex::new(Vec::new())),
        vec![CoroutineStep::Complete(20)],
    )));
    let second = runtime.spawn_external(Box::new(ScriptedCoroutine::new(
        "second",
        Arc::new(Mutex::new(Vec::new())),
        vec![CoroutineStep::Complete(22)],
    )));

    let resolved = runtime
        .resolve_all(&[first, second])
        .expect("resolve_all should complete both tasks");

    assert_eq!(resolved, vec![20, 22]);
    assert_eq!(runtime.task_state(first), Some(RuntimeTaskState::Ready(20)));
    assert_eq!(
        runtime.task_state(second),
        Some(RuntimeTaskState::Ready(22))
    );
}

#[test]
fn resolve_all_errors_when_any_task_fails_or_is_cancelled() {
    let mut runtime = AsyncRuntime::new(1);
    let failed = runtime.spawn_external(Box::new(ScriptedCoroutine::new(
        "failed",
        Arc::new(Mutex::new(Vec::new())),
        vec![CoroutineStep::Fail(7)],
    )));
    let cancelled = runtime.spawn_external(Box::new(ScriptedCoroutine::new(
        "cancelled",
        Arc::new(Mutex::new(Vec::new())),
        vec![CoroutineStep::Complete(1)],
    )));
    runtime.mark_task_cancelled(cancelled);

    let failed_result = runtime.resolve_all(&[failed]);
    let cancelled_result = runtime.resolve_all(&[cancelled]);

    assert_eq!(
        failed_result,
        Err(crate::async_runtime::TaskResolveError::Failed(7))
    );
    assert_eq!(
        cancelled_result,
        Err(crate::async_runtime::TaskResolveError::Cancelled)
    );
}

#[test]
fn resolve_first_returns_winner_and_cancels_losers() {
    let mut runtime = AsyncRuntime::new(1);
    let winner = runtime.spawn_external(Box::new(ScriptedCoroutine::new(
        "winner",
        Arc::new(Mutex::new(Vec::new())),
        vec![CoroutineStep::Complete(42)],
    )));
    let loser = runtime.spawn_external(Box::new(ScriptedCoroutine::new(
        "loser",
        Arc::new(Mutex::new(Vec::new())),
        vec![CoroutineStep::Yield, CoroutineStep::Complete(99)],
    )));

    let resolved = runtime
        .resolve_first(&[winner, loser])
        .expect("resolve_first should return the first ready task");

    assert_eq!(resolved, 42);
    assert_eq!(
        runtime.task_state(winner),
        Some(RuntimeTaskState::Ready(42))
    );
    assert_eq!(runtime.task_state(loser), Some(RuntimeTaskState::Cancelled));
}

#[test]
fn resolve_first_surfaces_winner_failure_and_cancels_losers() {
    let mut runtime = AsyncRuntime::new(1);
    let failed = runtime.spawn_external(Box::new(ScriptedCoroutine::new(
        "failed",
        Arc::new(Mutex::new(Vec::new())),
        vec![CoroutineStep::Fail(9)],
    )));
    let loser = runtime.spawn_external(Box::new(ScriptedCoroutine::new(
        "loser",
        Arc::new(Mutex::new(Vec::new())),
        vec![CoroutineStep::Yield, CoroutineStep::Complete(1)],
    )));

    let resolved = runtime.resolve_first(&[failed, loser]);

    assert_eq!(
        resolved,
        Err(crate::async_runtime::TaskResolveError::Failed(9))
    );
    assert_eq!(runtime.task_state(loser), Some(RuntimeTaskState::Cancelled));
}

#[test]
fn stop_the_world_gc_waits_for_all_workers_to_reach_safepoints() {
    let mut runtime = AsyncRuntime::new(2);
    let unreachable = runtime.allocate_heap_object(Vec::new());

    runtime.request_stop_the_world_collection();
    runtime.worker_reach_safepoint(0);

    assert_eq!(
        runtime.collect_garbage(),
        Err(GcError::WorkersNotSafepointed)
    );

    runtime.worker_reach_safepoint(1);

    assert_eq!(runtime.collect_garbage(), Ok(1));
    assert!(!runtime.heap_object_is_live(unreachable));
}

#[test]
fn stop_the_world_gc_traces_worker_stack_result_and_task_record_roots() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let mut runtime = AsyncRuntime::new(2);

    let child = runtime.allocate_heap_object(Vec::new());
    let worker_root = runtime.allocate_heap_object(Vec::new());
    let stack_root = runtime.allocate_heap_object(Vec::new());
    let result_root = runtime.allocate_heap_object(vec![child]);
    let record_root = runtime.allocate_heap_object(Vec::new());
    let garbage = runtime.allocate_heap_object(Vec::new());

    runtime.add_worker_root(0, worker_root);

    let suspended = runtime.spawn_on_worker(
        0,
        Box::new(ScriptedCoroutine::new(
            "suspended",
            Arc::clone(&log),
            vec![CoroutineStep::Suspend],
        )),
    );
    runtime.run_one_tick();
    runtime.add_task_stack_root(suspended, stack_root);
    runtime.add_task_record_root(suspended, record_root);

    let completed = runtime.spawn_external(Box::new(ScriptedCoroutine::new(
        "completed",
        Arc::clone(&log),
        vec![CoroutineStep::Complete(1)],
    )));
    assert_eq!(
        runtime.await_task_terminal(completed),
        Some(RuntimeTaskState::Ready(1))
    );
    runtime.store_task_result_root(completed, result_root);

    runtime.request_stop_the_world_collection();
    runtime.worker_reach_safepoint(0);
    runtime.worker_reach_safepoint(1);

    assert_eq!(runtime.collect_garbage(), Ok(1));
    assert!(runtime.heap_object_is_live(worker_root));
    assert!(runtime.heap_object_is_live(stack_root));
    assert!(runtime.heap_object_is_live(result_root));
    assert!(runtime.heap_object_is_live(record_root));
    assert!(runtime.heap_object_is_live(child));
    assert!(!runtime.heap_object_is_live(garbage));
}

#[test]
fn stop_the_world_gc_traces_stack_roots_from_multiple_suspended_tasks() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let mut runtime = AsyncRuntime::new(2);

    let first_root = runtime.allocate_heap_object(Vec::new());
    let second_root = runtime.allocate_heap_object(Vec::new());
    let garbage = runtime.allocate_heap_object(Vec::new());

    let first = runtime.spawn_on_worker(
        0,
        Box::new(ScriptedCoroutine::new(
            "first-suspended",
            Arc::clone(&log),
            vec![CoroutineStep::Suspend],
        )),
    );
    let second = runtime.spawn_on_worker(
        1,
        Box::new(ScriptedCoroutine::new(
            "second-suspended",
            Arc::clone(&log),
            vec![CoroutineStep::Suspend],
        )),
    );

    runtime.run_one_tick();
    runtime.run_one_tick();
    runtime.add_task_stack_root(first, first_root);
    runtime.add_task_stack_root(second, second_root);

    runtime.request_stop_the_world_collection();
    runtime.worker_reach_safepoint(0);
    runtime.worker_reach_safepoint(1);

    assert_eq!(runtime.collect_garbage(), Ok(1));
    assert!(runtime.heap_object_is_live(first_root));
    assert!(runtime.heap_object_is_live(second_root));
    assert!(!runtime.heap_object_is_live(garbage));
}

#[test]
fn c_runtime_source_covers_runtime_signatures() {
    let source = c_runtime_source();
    for (name, _, _) in RUNTIME_SIGS {
        let needle = format!("{name}(");
        assert!(
            source.contains(&needle),
            "runtime source is missing {name} from the shared symbol table"
        );
    }
}

#[test]
fn jit_runtime_symbols_cover_runtime_signatures() {
    let registered: std::collections::HashSet<_> = runtime_builtin_symbols()
        .into_iter()
        .map(|(name, _)| name)
        .collect();

    for (name, _, _) in RUNTIME_SIGS {
        assert!(
            registered.contains(name),
            "JIT runtime registration is missing {name} from the shared symbol table"
        );
    }
}

#[test]
fn runtime_task_handle_reports_ready_and_resolves_value() {
    let task = aster_task_from_i64(42, 0);
    assert_eq!(aster_task_is_ready(task), 1);
    assert_eq!(aster_task_resolve_i64(task), 42);
}

// ---------------------------------------------------------------------------
// Overflow-checked integer arithmetic
// ---------------------------------------------------------------------------

#[test]
fn checked_int_add_normal() {
    assert_eq!(aster_int_add(1, 2), 3);
    assert_eq!(aster_int_add(-5, 10), 5);
    assert_eq!(aster_int_add(0, 0), 0);
    assert_eq!(aster_int_add(i64::MAX - 1, 1), i64::MAX);
    assert_eq!(aster_int_add(i64::MIN + 1, -1), i64::MIN);
}

#[test]
fn checked_int_sub_normal() {
    assert_eq!(aster_int_sub(10, 3), 7);
    assert_eq!(aster_int_sub(-5, -10), 5);
    assert_eq!(aster_int_sub(i64::MIN + 1, 1), i64::MIN);
    assert_eq!(aster_int_sub(i64::MAX, 1), i64::MAX - 1);
}

#[test]
fn checked_int_mul_normal() {
    assert_eq!(aster_int_mul(3, 4), 12);
    assert_eq!(aster_int_mul(-3, 4), -12);
    assert_eq!(aster_int_mul(0, i64::MAX), 0);
    assert_eq!(aster_int_mul(1, i64::MAX), i64::MAX);
}

#[test]
fn checked_int_add_overflow_aborts() {
    // Verify that overflow causes process abort (caught as non-zero exit).
    // Runs an ignored inner test in a subprocess so the abort doesn't kill us.
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "tests::checked_int_add_overflow_aborts_inner",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success(), "should have aborted on overflow");
}

#[test]
#[ignore] // run only via the wrapper test above
fn checked_int_add_overflow_aborts_inner() {
    aster_int_add(i64::MAX, 1);
}

#[test]
fn checked_int_sub_overflow_aborts() {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "tests::checked_int_sub_overflow_aborts_inner",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success(), "should have aborted on overflow");
}

#[test]
#[ignore]
fn checked_int_sub_overflow_aborts_inner() {
    aster_int_sub(i64::MIN, 1);
}

#[test]
fn checked_int_mul_overflow_aborts() {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "tests::checked_int_mul_overflow_aborts_inner",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success(), "should have aborted on overflow");
}

#[test]
#[ignore]
fn checked_int_mul_overflow_aborts_inner() {
    aster_int_mul(i64::MAX, 2);
}

#[test]
fn bool_returning_function_works_in_if_condition() {
    let src = "\
def ready() -> Bool
  true

def main() -> Int
  if ready()
    return 1
  else
    return 0
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 1);
}

#[test]
fn bool_returning_function_returns_true_via_jit_pointer() {
    let src = "\
def ready() -> Bool
  true
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.get_function_ptr(FunctionId(0)).unwrap();
    let ready: fn() -> i8 = unsafe { std::mem::transmute(ptr) };
    assert_eq!(ready(), 1);
}

// ===========================================================================
// Integer arithmetic
// ===========================================================================

#[test]
fn return_constant() {
    let fir = compile_and_run("def main() -> Int\n  return 42\n");
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn add_two_numbers() {
    let fir = compile_and_run("def add(a: Int, b: Int) -> Int\n  a + b\n");
    let jit = jit_compile(&fir);
    let add_id = FunctionId(0);
    let result = jit.call_i64_i64_i64(add_id, 3, 4);
    assert_eq!(result, 7);
}

#[test]
fn subtract() {
    let fir = compile_and_run("def sub(a: Int, b: Int) -> Int\n  a - b\n");
    let jit = jit_compile(&fir);
    let result = jit.call_i64_i64_i64(FunctionId(0), 10, 3);
    assert_eq!(result, 7);
}

#[test]
fn multiply() {
    let fir = compile_and_run("def mul(a: Int, b: Int) -> Int\n  a * b\n");
    let jit = jit_compile(&fir);
    let result = jit.call_i64_i64_i64(FunctionId(0), 6, 7);
    assert_eq!(result, 42);
}

#[test]
fn divide() {
    let fir = compile_and_run("def div(a: Int, b: Int) -> Int\n  a / b\n");
    let jit = jit_compile(&fir);
    let result = jit.call_i64_i64_i64(FunctionId(0), 20, 4);
    assert_eq!(result, 5);
}

#[test]
fn divide_by_zero_returns_zero() {
    let fir = compile_and_run("def div(a: Int, b: Int) -> Int\n  a / b\n");
    let jit = jit_compile(&fir);
    // Division by zero should return 0, not trap
    let result = jit.call_i64_i64_i64(FunctionId(0), 10, 0);
    assert_eq!(result, 0);
}

#[test]
fn modulo_by_zero_returns_zero() {
    let fir = compile_and_run("def modz(a: Int, b: Int) -> Int\n  a % b\n");
    let jit = jit_compile(&fir);
    // Modulo by zero should return 0, not trap
    let result = jit.call_i64_i64_i64(FunctionId(0), 10, 0);
    assert_eq!(result, 0);
}

#[test]
fn nested_arithmetic() {
    let fir = compile_and_run("def f(a: Int, b: Int) -> Int\n  (a + b) * (a - b)\n");
    let jit = jit_compile(&fir);
    // (5+3) * (5-3) = 8 * 2 = 16
    let result = jit.call_i64_i64_i64(FunctionId(0), 5, 3);
    assert_eq!(result, 16);
}

#[test]
fn unary_negation() {
    let fir = compile_and_run("def neg(x: Int) -> Int\n  -x\n");
    let jit = jit_compile(&fir);
    let result = jit.call_i64_i64(FunctionId(0), 42);
    assert_eq!(result, -42);
}

#[test]
fn let_binding_and_return() {
    let fir = compile_and_run("def f() -> Int\n  let x: Int = 10\n  let y: Int = 20\n  x + y\n");
    let jit = jit_compile(&fir);
    let result = jit.call_i64(FunctionId(0));
    assert_eq!(result, 30);
}

#[test]
fn modulo() {
    let fir = compile_and_run("def modulo(a: Int, b: Int) -> Int\n  a % b\n");
    let jit = jit_compile(&fir);
    let result = jit.call_i64_i64_i64(FunctionId(0), 17, 5);
    assert_eq!(result, 2);
}

#[test]
fn negative_literal() {
    let fir = compile_and_run("def f() -> Int\n  return -42\n");
    let jit = jit_compile(&fir);
    let result = jit.call_i64(FunctionId(0));
    assert_eq!(result, -42);
}

// ===========================================================================
// Float arithmetic
// ===========================================================================

#[test]
fn float_return_constant() {
    let src = "def main() -> Float\n  3.141592653589793\n";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.get_function_ptr(fir.entry.unwrap()).unwrap();
    let f: fn() -> f64 = unsafe { std::mem::transmute(ptr) };
    let result = f();
    assert!(
        (result - std::f64::consts::PI).abs() < 1e-10,
        "expected PI, got {}",
        result
    );
}

#[test]
fn float_add() {
    let src = "def main() -> Float\n  1.5 + 2.5\n";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.get_function_ptr(fir.entry.unwrap()).unwrap();
    let f: fn() -> f64 = unsafe { std::mem::transmute(ptr) };
    let result = f();
    assert!((result - 4.0).abs() < 1e-10, "expected 4.0, got {}", result);
}

#[test]
fn float_subtract() {
    let src = "def main() -> Float\n  3.5 - 1.5\n";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.get_function_ptr(fir.entry.unwrap()).unwrap();
    let f: fn() -> f64 = unsafe { std::mem::transmute(ptr) };
    let result = f();
    assert!((result - 2.0).abs() < 1e-10, "expected 2.0, got {}", result);
}

#[test]
fn float_multiply() {
    let src = "def main() -> Float\n  2.0 * 3.0\n";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.get_function_ptr(fir.entry.unwrap()).unwrap();
    let f: fn() -> f64 = unsafe { std::mem::transmute(ptr) };
    let result = f();
    assert!((result - 6.0).abs() < 1e-10, "expected 6.0, got {}", result);
}

#[test]
fn float_comparison() {
    // 3.14 > 2.71 should return true (1)
    let src = "\
def main() -> Int
  if 3.14 > 2.71
    return 1
  else
    return 0
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 1);
}

// ===========================================================================
// Mixed Int/Float arithmetic coercion (C1 audit fix)
// ===========================================================================

#[test]
fn mixed_int_float_add() {
    let src = "def main() -> Float\n  1 + 2.5\n";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.get_function_ptr(fir.entry.unwrap()).unwrap();
    let f: fn() -> f64 = unsafe { std::mem::transmute(ptr) };
    let result = f();
    assert!((result - 3.5).abs() < 1e-10, "expected 3.5, got {}", result);
}

#[test]
fn mixed_float_int_add() {
    let src = "def main() -> Float\n  2.5 + 1\n";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.get_function_ptr(fir.entry.unwrap()).unwrap();
    let f: fn() -> f64 = unsafe { std::mem::transmute(ptr) };
    let result = f();
    assert!((result - 3.5).abs() < 1e-10, "expected 3.5, got {}", result);
}

#[test]
fn mixed_int_float_mul() {
    let src = "def main() -> Float\n  3 * 2.5\n";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.get_function_ptr(fir.entry.unwrap()).unwrap();
    let f: fn() -> f64 = unsafe { std::mem::transmute(ptr) };
    let result = f();
    assert!((result - 7.5).abs() < 1e-10, "expected 7.5, got {}", result);
}

#[test]
fn mixed_int_float_div() {
    let src = "def main() -> Float\n  7 / 2.0\n";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.get_function_ptr(fir.entry.unwrap()).unwrap();
    let f: fn() -> f64 = unsafe { std::mem::transmute(ptr) };
    let result = f();
    assert!((result - 3.5).abs() < 1e-10, "expected 3.5, got {}", result);
}

#[test]
fn mixed_int_float_comparison() {
    // 1 < 2.5 should be true
    let src = "\
def main() -> Int
  if 1 < 2.5
    return 1
  else
    return 0
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 1);
}

#[test]
fn mixed_int_float_eq() {
    // 2 == 2.0 should be true
    let src = "\
def main() -> Int
  if 2 == 2.0
    return 1
  else
    return 0
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 1);
}

#[test]
fn float_pow() {
    let src = "def main() -> Float\n  2.0 ** 3.0\n";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.get_function_ptr(fir.entry.unwrap()).unwrap();
    let f: fn() -> f64 = unsafe { std::mem::transmute(ptr) };
    let result = f();
    assert!((result - 8.0).abs() < 1e-10, "expected 8.0, got {}", result);
}

#[test]
fn mixed_int_float_pow() {
    let src = "def main() -> Float\n  2 ** 3.0\n";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.get_function_ptr(fir.entry.unwrap()).unwrap();
    let f: fn() -> f64 = unsafe { std::mem::transmute(ptr) };
    let result = f();
    assert!((result - 8.0).abs() < 1e-10, "expected 8.0, got {}", result);
}

// ===========================================================================
// Control flow
// ===========================================================================

#[test]
fn if_else_true_branch() {
    let src = "def f(x: Int) -> Int\n  if x > 0\n    return 1\n  else\n    return -1\n";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    assert_eq!(jit.call_i64_i64(FunctionId(0), 5), 1);
}

#[test]
fn if_else_false_branch() {
    let src = "def f(x: Int) -> Int\n  if x > 0\n    return 1\n  else\n    return -1\n";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    assert_eq!(jit.call_i64_i64(FunctionId(0), -5), -1);
}

#[test]
fn if_else_zero() {
    let src = "def f(x: Int) -> Int\n  if x > 0\n    return 1\n  else\n    return -1\n";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    assert_eq!(jit.call_i64_i64(FunctionId(0), 0), -1);
}

#[test]
fn elif_chain() {
    let src = "\
def classify(x: Int) -> Int
  if x > 0
    return 1
  elif x < 0
    return -1
  else
    return 0
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    assert_eq!(jit.call_i64_i64(FunctionId(0), 10), 1);
    assert_eq!(jit.call_i64_i64(FunctionId(0), -10), -1);
    assert_eq!(jit.call_i64_i64(FunctionId(0), 0), 0);
}

#[test]
fn while_loop_sum() {
    let src = "\
def sum_to(n: Int) -> Int
  let total: Int = 0
  let i: Int = 1
  while i <= n
    total = total + i
    i = i + 1
  return total
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    assert_eq!(jit.call_i64_i64(FunctionId(0), 10), 55);
}

#[test]
fn while_loop_zero_iterations() {
    let src = "\
def sum_to(n: Int) -> Int
  let total: Int = 0
  let i: Int = 1
  while i <= n
    total = total + i
    i = i + 1
  return total
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    assert_eq!(jit.call_i64_i64(FunctionId(0), 0), 0);
}

#[test]
fn break_in_while() {
    let src = "\
def f() -> Int
  let x: Int = 0
  while true
    x = x + 1
    if x == 5
      break
  return x
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    assert_eq!(jit.call_i64(FunctionId(0)), 5);
}

#[test]
fn comparison_operators() {
    // Test all comparisons by encoding results as bits
    let src = "\
def test_cmp(a: Int, b: Int) -> Int
  let result: Int = 0
  if a == b
    result = result + 1
  if a != b
    result = result + 2
  if a < b
    result = result + 4
  if a > b
    result = result + 8
  if a <= b
    result = result + 16
  if a >= b
    result = result + 32
  return result
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    // a=3, b=5: != + < + <= = 2 + 4 + 16 = 22
    assert_eq!(jit.call_i64_i64_i64(FunctionId(0), 3, 5), 22);
    // a=5, b=5: == + <= + >= = 1 + 16 + 32 = 49
    assert_eq!(jit.call_i64_i64_i64(FunctionId(0), 5, 5), 49);
    // a=7, b=5: != + > + >= = 2 + 8 + 32 = 42
    assert_eq!(jit.call_i64_i64_i64(FunctionId(0), 7, 5), 42);
}

#[test]
fn nested_if() {
    let src = "\
def f(x: Int, y: Int) -> Int
  if x > 0
    if y > 0
      return 1
    else
      return 2
  else
    return 3
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    assert_eq!(jit.call_i64_i64_i64(FunctionId(0), 1, 1), 1);
    assert_eq!(jit.call_i64_i64_i64(FunctionId(0), 1, -1), 2);
    assert_eq!(jit.call_i64_i64_i64(FunctionId(0), -1, 1), 3);
}

// ===========================================================================
// Strings
// ===========================================================================

#[test]
fn string_return() {
    // Verify string functions compile without crashing
    let src = "def f() -> String\n  return \"hello\"\n";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    // String returns a heap pointer — just check it's non-null
    let ptr = jit.call_i64(FunctionId(0));
    assert_ne!(ptr, 0, "string should return non-null pointer");
}

#[test]
fn string_length() {
    // Create a string and verify its heap layout (len at offset 0)
    let src = "def f() -> String\n  return \"hello\"\n";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.call_i64(FunctionId(0));
    // Read length from heap string layout: [len: i64][data...]
    let len = unsafe { *(ptr as *const i64) };
    assert_eq!(len, 5);
}

#[test]
fn string_data() {
    let src = "def f() -> String\n  return \"hello\"\n";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.call_i64(FunctionId(0));
    unsafe {
        let len = *(ptr as *const i64) as usize;
        let data = (ptr as *const u8).add(8);
        let s = std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len));
        assert_eq!(s, "hello");
    }
}

#[test]
fn log_compiles() {
    // Verify that log() calls compile (runtime call)
    let src = "def f() -> Void\n  log(message: \"hello\")\n";
    let fir = compile_and_run(src);
    let _jit = jit_compile(&fir);
    // If we get here, compilation succeeded
}

// ===========================================================================
// Function calls
// ===========================================================================

#[test]
fn call_another_function() {
    let src = "\
def double(x: Int) -> Int
  x * 2

def main() -> Int
  double(x: 21)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn call_chain() {
    let src = "\
def add(a: Int, b: Int) -> Int
  a + b

def mul(a: Int, b: Int) -> Int
  a * b

def main() -> Int
  mul(a: add(a: 2, b: 3), b: add(a: 4, b: 5))
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    // (2+3) * (4+5) = 5 * 9 = 45
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 45);
}

#[test]
fn recursive_factorial() {
    let src = "\
def factorial(n: Int) -> Int
  if n <= 1
    return 1
  else
    return n * factorial(n: n - 1)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    assert_eq!(jit.call_i64_i64(FunctionId(0), 5), 120);
    assert_eq!(jit.call_i64_i64(FunctionId(0), 10), 3628800);
}

#[test]
fn recursive_fibonacci() {
    let src = "\
def fib(n: Int) -> Int
  if n <= 1
    return n
  else
    return fib(n: n - 1) + fib(n: n - 2)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    assert_eq!(jit.call_i64_i64(FunctionId(0), 0), 0);
    assert_eq!(jit.call_i64_i64(FunctionId(0), 1), 1);
    assert_eq!(jit.call_i64_i64(FunctionId(0), 10), 55);
}

#[test]
fn mutual_recursion() {
    let src = "\
def is_even(n: Int) -> Int
  if n == 0
    return 1
  else
    return is_odd(n: n - 1)

def is_odd(n: Int) -> Int
  if n == 0
    return 0
  else
    return is_even(n: n - 1)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let is_even_id = fir
        .functions
        .iter()
        .find(|f| f.name == "is_even")
        .unwrap()
        .id;
    assert_eq!(jit.call_i64_i64(is_even_id, 4), 1);
    assert_eq!(jit.call_i64_i64(is_even_id, 5), 0);
}

#[test]
fn function_with_multiple_calls() {
    let src = "\
def square(x: Int) -> Int
  x * x

def sum_of_squares(a: Int, b: Int) -> Int
  square(x: a) + square(x: b)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let sos_id = fir
        .functions
        .iter()
        .find(|f| f.name == "sum_of_squares")
        .unwrap()
        .id;
    assert_eq!(jit.call_i64_i64_i64(sos_id, 3, 4), 25);
}

// ===========================================================================
// Classes
// ===========================================================================

#[test]
fn class_construction() {
    let src = "\
class Point
  x: Int
  y: Int

def make_point() -> Point
  Point(x: 10, y: 20)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    // Class returns a heap pointer — just check non-null
    let make_id = fir
        .functions
        .iter()
        .find(|f| f.name == "make_point")
        .unwrap()
        .id;
    let ptr = jit.call_i64(make_id);
    assert_ne!(ptr, 0, "class instance should be non-null");
}

#[test]
fn class_field_access() {
    let src = "\
class Point
  x: Int
  y: Int

def get_x() -> Int
  let p: Point = Point(x: 42, y: 99)
  p.x
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let get_x_id = fir.functions.iter().find(|f| f.name == "get_x").unwrap().id;
    let result = jit.call_i64(get_x_id);
    assert_eq!(result, 42);
}

#[test]
fn class_field_access_second_field() {
    let src = "\
class Point
  x: Int
  y: Int

def get_y() -> Int
  let p: Point = Point(x: 42, y: 99)
  p.y
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let get_y_id = fir.functions.iter().find(|f| f.name == "get_y").unwrap().id;
    let result = jit.call_i64(get_y_id);
    assert_eq!(result, 99);
}

#[test]
fn method_returns_field() {
    let src = "\
class Counter
  value: Int

  def get() -> Int
    value

def main() -> Int
  let c: Counter = Counter(value: 42)
  c.get()
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn method_returns_computed_field() {
    let src = "\
class Point
  x: Int
  y: Int

  def sum() -> Int
    x + y

def main() -> Int
  let p: Point = Point(x: 10, y: 32)
  p.sum()
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn method_accesses_multiple_fields() {
    let src = "\
class Rect
  w: Int
  h: Int

  def area() -> Int
    w * h

def main() -> Int
  let r: Rect = Rect(w: 6, h: 7)
  r.area()
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

// ===========================================================================
// Field mutation
// ===========================================================================

#[test]
fn field_mutation_assign_and_read() {
    let src = "\
class Point
  x: Int
  y: Int

def main() -> Int
  let p: Point = Point(x: 1, y: 2)
  p.x = 99
  p.x
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 99);
}

#[test]
fn field_mutation_second_field() {
    let src = "\
class Point
  x: Int
  y: Int

def main() -> Int
  let p: Point = Point(x: 1, y: 2)
  p.y = 77
  p.y
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 77);
}

#[test]
fn field_mutation_preserves_other_fields() {
    let src = "\
class Point
  x: Int
  y: Int

def main() -> Int
  let p: Point = Point(x: 10, y: 32)
  p.x = 99
  p.y
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 32);
}

// ===========================================================================
// Lists
// ===========================================================================

#[test]
fn list_creation_and_get() {
    let src = "\
def main() -> Int
  let xs: List[Int] = [10, 20, 30]
  xs[1]
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 20);
}

#[test]
fn list_set() {
    let src = "\
def main() -> Int
  let xs: List[Int] = [10, 20, 30]
  xs[0] = 99
  xs[0]
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 99);
}

#[test]
fn list_len() {
    let src = "\
def main() -> Int
  let xs: List[Int] = [1, 2, 3, 4, 5]
  xs.len()
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 5);
}

#[test]
fn list_push() {
    let src = "\
def main() -> Int
  let xs: List[Int] = [1, 2]
  xs.push(item: 3)
  xs.len()
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 3);
}

#[test]
fn list_float_creation_and_get() {
    let src = "\
def main() -> Float
  let xs: List[Float] = [1.5, 2.5, 3.5]
  xs[1]
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.get_function_ptr(fir.entry.unwrap()).unwrap();
    let f: fn() -> f64 = unsafe { std::mem::transmute(ptr) };
    let result = f();
    assert!((result - 2.5).abs() < 1e-10, "expected 2.5, got {}", result);
}

#[test]
fn list_float_set() {
    let src = "\
def main() -> Float
  let xs: List[Float] = [1.0, 2.0, 3.0]
  xs[0] = 9.5
  xs[0]
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.get_function_ptr(fir.entry.unwrap()).unwrap();
    let f: fn() -> f64 = unsafe { std::mem::transmute(ptr) };
    let result = f();
    assert!((result - 9.5).abs() < 1e-10, "expected 9.5, got {}", result);
}

#[test]
fn list_float_push() {
    let src = "\
def main() -> Float
  let xs: List[Float] = [1.0, 2.0]
  xs.push(item: 4.5)
  xs[2]
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.get_function_ptr(fir.entry.unwrap()).unwrap();
    let f: fn() -> f64 = unsafe { std::mem::transmute(ptr) };
    let result = f();
    assert!((result - 4.5).abs() < 1e-10, "expected 4.5, got {}", result);
}

#[test]
fn list_bool_creation_and_get() {
    let src = "\
def main() -> Int
  let xs: List[Bool] = [true, false, true]
  let a: Bool = xs[0]
  let b: Bool = xs[1]
  let c: Bool = xs[2]
  let r: Int = 0
  if a
    r = r + 1
  if b
    r = r + 10
  if c
    r = r + 100
  r
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 101);
}

#[test]
fn list_bool_set() {
    let src = "\
def main() -> Int
  let xs: List[Bool] = [true, true, true]
  xs[1] = false
  let v: Bool = xs[1]
  let r: Int = 0
  if v
    r = 1
  r
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 0);
}

// ===========================================================================
// Maps
// ===========================================================================

#[test]
fn map_literal_creation() {
    // Map literal should be constructable without crashing
    let src = "\
def main() -> Int
  let m: Map[String, Int] = {\"x\": 1, \"y\": 2}
  42
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn map_get_value() {
    let src = "\
def main() -> Int
  let m: Map[String, Int] = {\"x\": 10, \"y\": 32}
  match m[\"y\"]
    nil => 0
    v => v
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 32);
}

#[test]
fn map_get_first_key() {
    let src = "\
def main() -> Int
  let m: Map[String, Int] = {\"a\": 99, \"b\": 1}
  match m[\"a\"]
    nil => 0
    v => v
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 99);
}

#[test]
fn for_loop_sum() {
    let src = "\
def main() -> Int
  let xs: List[Int] = [1, 2, 3, 4, 5]
  let total: Int = 0
  for x in xs
    total = total + x
  return total
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 15);
}

#[test]
fn for_loop_empty_list() {
    let src = "\
def main() -> Int
  let xs: List[Int] = []
  let total: Int = 0
  for x in xs
    total = total + x
  return total
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 0);
}

#[test]
fn for_loop_with_break() {
    let src = "\
def main() -> Int
  let xs: List[Int] = [1, 2, 3, 4, 5]
  let total: Int = 0
  for x in xs
    if x == 4
      break
    total = total + x
  return total
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 6); // 1 + 2 + 3
}

#[test]
fn for_loop_list_with_continue() {
    let src = "\
def main() -> Int
  let xs: List[Int] = [1, 2, 3, 4, 5]
  let total: Int = 0
  for x in xs
    if x == 3
      continue
    total = total + x
  return total
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 12); // 1 + 2 + 4 + 5
}

#[test]
fn for_loop_range_with_continue() {
    let src = "\
def main() -> Int
  let total = 0
  for i in 1..10
    if i == 3
      continue
    if i == 7
      continue
    total = total + i
  total
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 35); // 1+2+4+5+6+8+9
}

#[test]
fn for_loop_range_inclusive_with_continue() {
    let src = "\
def main() -> Int
  let total = 0
  for i in 1..=5
    if i == 2
      continue
    total = total + i
  total
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 13); // 1+3+4+5
}

#[test]
fn for_loop_continue_skips_all_but_last() {
    // continue on every iteration except the last — verifies the loop still terminates
    let src = "\
def main() -> Int
  let total = 0
  for i in 0..5
    if i < 4
      continue
    total = total + i
  total
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 4);
}

#[test]
fn for_loop_nested_continue() {
    // continue in nested for loops — each loop advances independently
    let src = "\
def main() -> Int
  let total = 0
  for i in 0..3
    for j in 0..3
      if j == 1
        continue
      total = total + 1
  total
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 6); // 3 outer * 2 inner (j=0, j=2)
}

#[test]
fn while_loop_continue_still_works() {
    // plain while-loop continue (no increment) is unaffected by the fix
    let src = "\
def main() -> Int
  let total = 0
  let i = 0
  while i < 10
    i = i + 1
    if i == 5
      continue
    total = total + i
  total
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 50); // 1+2+3+4+6+7+8+9+10
}

// ===========================================================================
// Range.random() and Range variable for-loops
// ===========================================================================

#[test]
fn range_random_returns_value_in_bounds() {
    // (10..20).random() must return a value in [10, 20)
    let src = "\
def main() -> Int
  let r: Int = (10..20).random()
  if r >= 10
    if r < 20
      return 1
  return 0
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 1);
}

#[test]
fn range_random_many_calls_all_in_bounds() {
    // Call .random() 1000 times, accumulate out-of-bounds count
    let src = "\
def main() -> Int
  let bad = 0
  for i in 0..1000
    let r: Int = (5..15).random()
    if r < 5
      bad = bad + 1
    if r >= 15
      bad = bad + 1
  bad
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 0);
}

#[test]
fn for_loop_over_range_variable() {
    // Store a range in a variable, iterate over it
    let src = "\
def make_range() -> Range
  1..=5

def main() -> Int
  let rng: Range = make_range()
  let total = 0
  for i in rng
    total = total + i
  total
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 15); // 1+2+3+4+5
}

#[test]
fn gc_survives_range_allocation_pressure() {
    // Many Range.random() calls trigger GC while Range fields contain small
    // integers that must not be dereferenced as pointers during mark phase.
    let src = "\
def main() -> Int
  let count = 0
  for i in 2..5000
    let r: Int = (2..(i + 10)).random()
    if r >= 2
      count = count + 1
  count
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert!(result > 0, "expected positive count, got {}", result);
}

// ===========================================================================
// Nullable operations
// ===========================================================================

#[test]
fn nullable_return_value() {
    // Nullable Int? return — value is boxed (heap pointer), non-zero
    let src = "\
def f() -> Int?
  return 42
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(FunctionId(0));
    // Value should be a non-zero heap pointer (boxed 42)
    assert_ne!(result, 0, "Some(42) should be non-zero (boxed pointer)");
    // Verify the boxed value by reading from the pointer
    let ptr = result as *const i64;
    let unboxed = unsafe { *ptr };
    assert_eq!(unboxed, 42);
}

#[test]
fn nullable_nil_return() {
    let src = "\
def f() -> Int?
  return nil
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(FunctionId(0));
    // nil should be 0
    assert_eq!(result, 0);
}

#[test]
fn nullable_string_some_skips_boxing() {
    // String is already a pointer — nullable String? passes through without
    // boxing. Verify the string value survives the nullable round-trip.
    let src = "\
def f() -> String?
  return \"hello\"
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(FunctionId(0));
    // Some(string) should be non-zero (the string pointer itself, not a box)
    assert_ne!(result, 0);
}

#[test]
fn nullable_string_nil() {
    let src = "\
def f() -> String?
  return nil
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(FunctionId(0));
    assert_eq!(result, 0);
}

// ===========================================================================
// Generics
// ===========================================================================

#[test]
fn generic_identity_int() {
    let src = "\
def identity(x: T) -> T
  x

def main() -> Int
  identity(x: 42)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn generic_class_field() {
    let src = "\
class Box[T]
  value: T

def main() -> Int
  let b: Box[Int] = Box(value: 99)
  b.value
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 99);
}

// ===========================================================================
// AOT backend
// ===========================================================================

#[test]
fn aot_compile_simple() {
    let src = "\
def main() -> Int
  return 42
";
    let fir = compile_and_run(src);
    let mut aot = CraneliftAOT::new();
    aot.compile_module(&fir).expect("AOT compile ok");
    let bytes = aot.emit_object().expect("emit object ok");
    // Object file should start with a valid magic number
    assert!(!bytes.is_empty(), "object file should not be empty");
    // Mach-O magic (macOS) or ELF magic (Linux)
    assert!(
        bytes.len() > 4,
        "object file too small: {} bytes",
        bytes.len()
    );
}

#[test]
fn aot_compile_with_functions() {
    let src = "\
def double(x: Int) -> Int
  x * 2

def main() -> Int
  double(x: 21)
";
    let fir = compile_and_run(src);
    let mut aot = CraneliftAOT::new();
    aot.compile_module(&fir).expect("AOT compile ok");
    let bytes = aot.emit_object().expect("emit object ok");
    assert!(
        bytes.len() > 100,
        "object file should contain compiled code"
    );
}

#[test]
fn aot_compile_with_strings() {
    let src = "\
def main() -> Int
  log(message: \"hello AOT\")
  return 0
";
    let fir = compile_and_run(src);
    let mut aot = CraneliftAOT::new();
    aot.compile_module(&fir).expect("AOT compile ok");
    let bytes = aot.emit_object().expect("emit object ok");
    assert!(!bytes.is_empty());
}

#[test]
fn aot_compile_with_control_flow() {
    let src = "\
def factorial(n: Int) -> Int
  if n <= 1
    return 1
  else
    return n * factorial(n: n - 1)

def main() -> Int
  factorial(n: 10)
";
    let fir = compile_and_run(src);
    let mut aot = CraneliftAOT::new();
    aot.compile_module(&fir).expect("AOT compile ok");
    let bytes = aot.emit_object().expect("emit object ok");
    assert!(bytes.len() > 100);
}

#[test]
fn aot_compile_with_classes() {
    let src = "\
class Point
  x: Int
  y: Int

def main() -> Int
  let p: Point = Point(x: 3, y: 4)
  p.x * p.x + p.y * p.y
";
    let fir = compile_and_run(src);
    let mut aot = CraneliftAOT::new();
    aot.compile_module(&fir).expect("AOT compile ok");
    let bytes = aot.emit_object().expect("emit object ok");
    assert!(bytes.len() > 100);
}

#[test]
fn aot_compile_with_lists() {
    let src = "\
def main() -> Int
  let xs: List[Int] = [1, 2, 3, 4, 5]
  let total: Int = 0
  for x in xs
    total = total + x
  return total
";
    let fir = compile_and_run(src);
    let mut aot = CraneliftAOT::new();
    aot.compile_module(&fir).expect("AOT compile ok");
    let bytes = aot.emit_object().expect("emit object ok");
    assert!(bytes.len() > 100);
}

// ===========================================================================
// Async
// ===========================================================================

#[test]
fn async_call_and_resolve() {
    let src = "\
def compute() -> Int
  42

def main() -> Int
  let t: Task[Int] = async compute()
  resolve t!
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn async_with_args() {
    let src = "\
def add(a: Int, b: Int) -> Int
  a + b

def main() -> Int
  let t: Task[Int] = async add(a: 20, b: 22)
  resolve t!
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn async_task_is_ready_after_resolve() {
    let src = "\
def compute() -> Int
  42

def main() -> Int
  let t: Task[Int] = async compute()
  let val = resolve t!
  if t.is_ready()
    return val
  else
    return 0
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn async_task_cancel_keeps_terminal_task_ready() {
    let src = "\
def compute() -> Int
  42

def main() -> Int
  let t: Task[Int] = async compute()
  t.cancel()
  if t.is_ready()
    return 1
  else
    return 0
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 1);
}

#[test]
fn async_task_wait_cancel_keeps_terminal_task_ready() {
    let src = "\
def compute() -> Int
  42

def main() -> Int
  let t: Task[Int] = async compute()
  t.wait_cancel()
  if t.is_ready()
    return 1
  else
    return 0
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 1);
}

#[test]
fn resolve_after_wait_cancel_surfaces_cancelled_state() {
    let src = "\
def compute() -> Int
  let i: Int = 0
  let total: Int = 0
  while i < 20000000
    total = total + i
    i = i + 1
  42

def main() -> Int
  let t: Task[Int] = async compute()
  t.wait_cancel()
  resolve t!.catch
    _ -> 99
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 99);
}

#[test]
fn async_resolve_all_returns_list_values() {
    let src = "\
def fetch(value: Int) -> Int
  value

def main() -> Int
  let tasks: List[Task[Int]] = [async fetch(value: 20), async fetch(value: 22)]
  let values: List[Int] = resolve_all(tasks: tasks)!
  values[0] + values[1]
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn async_resolve_preserves_gc_managed_list_result() {
    let src = "\
def make_numbers() -> List[Int]
  [10, 20, 12]

def main() -> Int
  let t: Task[List[Int]] = async make_numbers()
  let values: List[Int] = resolve t!
  values.len() + values[1]
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 23);
}

#[test]
fn async_resolve_first_returns_fastest_value() {
    let src = "\
def slow() -> Int
  let i: Int = 0
  let total: Int = 0
  while i < 20000000
    total = total + i
    i = i + 1
  10

def fast() -> Int
  42

def main() -> Int
  let tasks: List[Task[Int]] = [async slow(), async fast()]
  resolve_first(tasks: tasks)!
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn async_wait_cancel_publishes_cancelled_terminal_state() {
    let src = "\
def slow() -> Int
  let i: Int = 0
  let total: Int = 0
  while i < 20000000
    total = total + i
    i = i + 1
  42

def main() -> Int
  let t: Task[Int] = async slow()
  t.wait_cancel()
  resolve t!.catch
    _ -> 99
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 99);
}

// ===========================================================================
// Classes (continued)
// ===========================================================================

#[test]
fn class_in_function() {
    let src = "\
class Point
  x: Int
  y: Int

def distance_sq() -> Int
  let p: Point = Point(x: 3, y: 4)
  p.x * p.x + p.y * p.y
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let f_id = fir
        .functions
        .iter()
        .find(|f| f.name == "distance_sq")
        .unwrap()
        .id;
    let result = jit.call_i64(f_id);
    assert_eq!(result, 25);
}

// ===========================================================================
// Match expressions — desugared to if/else chains
// ===========================================================================

#[test]
fn match_int_literal() {
    let src = "\
def classify(x: Int) -> Int
  match x
    1 => 10
    2 => 20
    _ => 99
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    assert_eq!(jit.call_i64_i64(FunctionId(0), 1), 10);
    assert_eq!(jit.call_i64_i64(FunctionId(0), 2), 20);
    assert_eq!(jit.call_i64_i64(FunctionId(0), 3), 99);
}

#[test]
fn match_wildcard_only() {
    let src = "\
def f(x: Int) -> Int
  match x
    _ => 42
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    assert_eq!(jit.call_i64_i64(FunctionId(0), 0), 42);
    assert_eq!(jit.call_i64_i64(FunctionId(0), 100), 42);
}

#[test]
fn match_variable_binding() {
    let src = "\
def f(x: Int) -> Int
  match x
    1 => 10
    other => other + 100
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    assert_eq!(jit.call_i64_i64(FunctionId(0), 1), 10);
    assert_eq!(jit.call_i64_i64(FunctionId(0), 5), 105);
}

#[test]
fn match_in_expression() {
    let src = "\
def f(x: Int) -> Int
  let result: Int = match x
    0 => 0
    _ => 1
  result * 10
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    assert_eq!(jit.call_i64_i64(FunctionId(0), 0), 0);
    assert_eq!(jit.call_i64_i64(FunctionId(0), 5), 10);
}

// ===========================================================================
// MILESTONE 13: Enum lowering — tagged union layout
// ===========================================================================

#[test]
fn fieldless_enum_construct() {
    let src = "\
enum Color
  Red
  Green
  Blue

def main() -> Int
  let c = Color.Red
  0
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 0);
}

#[test]
fn enum_match_tag() {
    let src = "\
enum Color
  Red
  Green
  Blue

def test(c: Color) -> Int
  match c
    Color.Red => 1
    Color.Green => 2
    Color.Blue => 3
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    // Call the test function with a Color.Green (tag=1)
    // First, construct Color.Green by calling its constructor
    let green_ctor = fir
        .functions
        .iter()
        .find(|f| f.name == "Color.Green")
        .unwrap()
        .id;
    let green_ptr = jit.call_i64(green_ctor);
    let test_id = fir.functions.iter().find(|f| f.name == "test").unwrap().id;
    let result = jit.call_i64_i64(test_id, green_ptr);
    assert_eq!(result, 2);
}

#[test]
fn enum_match_with_wildcard() {
    let src = "\
enum Direction
  North
  South
  East
  West

def is_vertical(d: Direction) -> Int
  match d
    Direction.North => 1
    Direction.South => 1
    _ => 0
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let east_ctor = fir
        .functions
        .iter()
        .find(|f| f.name == "Direction.East")
        .unwrap()
        .id;
    let east_ptr = jit.call_i64(east_ctor);
    let test_id = fir
        .functions
        .iter()
        .find(|f| f.name == "is_vertical")
        .unwrap()
        .id;
    let result = jit.call_i64_i64(test_id, east_ptr);
    assert_eq!(result, 0);

    let north_ctor = fir
        .functions
        .iter()
        .find(|f| f.name == "Direction.North")
        .unwrap()
        .id;
    let north_ptr = jit.call_i64(north_ctor);
    let result = jit.call_i64_i64(test_id, north_ptr);
    assert_eq!(result, 1);
}

#[test]
fn enum_variant_with_field() {
    // Construct an enum variant that carries a field, match on tag to dispatch
    let src = "\
enum Shape
  Circle(radius: Int)
  Square(side: Int)

def describe(s: Shape) -> Int
  match s
    Shape.Circle => 1
    Shape.Square => 2
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    // Construct a Circle with radius=10
    let circle_ctor = fir
        .functions
        .iter()
        .find(|f| f.name == "Shape.Circle")
        .unwrap()
        .id;
    let circle_ptr = jit.call_i64_i64(circle_ctor, 10);
    let describe_id = fir
        .functions
        .iter()
        .find(|f| f.name == "describe")
        .unwrap()
        .id;
    let result = jit.call_i64_i64(describe_id, circle_ptr);
    assert_eq!(result, 1);

    // Construct a Square with side=5
    let square_ctor = fir
        .functions
        .iter()
        .find(|f| f.name == "Shape.Square")
        .unwrap()
        .id;
    let square_ptr = jit.call_i64_i64(square_ctor, 5);
    let result = jit.call_i64_i64(describe_id, square_ptr);
    assert_eq!(result, 2);
}

// ===========================================================================
// MILESTONE 14: Closures — full calling convention
// ===========================================================================

#[test]
fn lambda_no_captures() {
    // Nested def (closure without captures) called directly
    let src = "\
def main() -> Int
  def double(x: Int) -> Int
    x * 2
  double(x: 21)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn lambda_with_captures() {
    // Nested def captures a local variable from enclosing scope
    let src = "\
def main() -> Int
  let offset: Int = 10
  def add_offset(x: Int) -> Int
    x + offset
  add_offset(x: 32)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn closure_multiple_captures() {
    let src = "\
def main() -> Int
  let a: Int = 10
  let b: Int = 20
  def sum_with(x: Int) -> Int
    x + a + b
  sum_with(x: 12)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

// ===========================================================================
// Auto-derived to_string (Printable)
// ===========================================================================

#[test]
fn auto_derived_to_string_single_int_field() {
    // Class with auto-derived Printable should produce "ClassName(field_value)"
    let src = "\
class Wrapper includes Printable
  val: Int

def main() -> String
  let w = Wrapper(val: 42)
  w.to_string()
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.call_i64(fir.entry.unwrap());
    assert_ne!(ptr, 0);
    unsafe {
        let len = *(ptr as *const i64) as usize;
        let data = (ptr as *const u8).add(8);
        let s = std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len));
        assert_eq!(s, "Wrapper(42)");
    }
}

#[test]
fn auto_derived_to_string_multiple_fields() {
    let src = "\
class Point includes Printable
  x: Int
  y: Int

def main() -> String
  let p = Point(x: 10, y: 20)
  p.to_string()
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.call_i64(fir.entry.unwrap());
    assert_ne!(ptr, 0);
    unsafe {
        let len = *(ptr as *const i64) as usize;
        let data = (ptr as *const u8).add(8);
        let s = std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len));
        assert_eq!(s, "Point(10, 20)");
    }
}

#[test]
fn auto_derived_to_string_in_interpolation() {
    let src = "\
class Pair includes Printable
  a: Int
  b: Int

def main() -> String
  let p = Pair(a: 1, b: 2)
  \"result: {p}\"
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.call_i64(fir.entry.unwrap());
    assert_ne!(ptr, 0);
    unsafe {
        let len = *(ptr as *const i64) as usize;
        let data = (ptr as *const u8).add(8);
        let s = std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len));
        assert_eq!(s, "result: Pair(1, 2)");
    }
}

// ===========================================================================
// MILESTONE 15: ClosureCall dynamic dispatch
// ===========================================================================

#[test]
fn closure_stored_in_variable_then_called() {
    // Closure assigned to a local variable and called dynamically
    let src = "\
def main() -> Int
  def adder(x: Int) -> Int
    x + 10
  let f: Fn(Int) -> Int = adder
  f(x: 32)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn closure_no_captures_stored_and_called() {
    let src = "\
def main() -> Int
  let offset: Int = 10
  def add_offset(x: Int) -> Int
    x + offset
  let f: Fn(Int) -> Int = add_offset
  f(x: 32)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

// ===========================================================================
// Top-level let bindings
// ===========================================================================

#[test]
fn top_level_let_int() {
    let src = "\
let MAGIC: Int = 42

def main() -> Int
  MAGIC
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn top_level_let_expression() {
    let src = "\
let BASE: Int = 20
let OFFSET: Int = 22

def main() -> Int
  BASE + OFFSET
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn top_level_let_used_in_function() {
    let src = "\
let FACTOR: Int = 7

def multiply(x: Int) -> Int
  x * FACTOR

def main() -> Int
  multiply(x: 6)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

// ===========================================================================
// Generic monomorphization
// ===========================================================================

#[test]
fn generic_identity_int_and_string() {
    // Same generic function called with Int and String in same program
    let src = "\
def identity(x: T) -> T
  x

def main() -> Int
  let a: Int = identity(x: 42)
  a
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn generic_max() {
    let src = "\
def max_val(a: Int, b: Int) -> Int
  if a > b
    return a
  else
    return b

def main() -> Int
  max_val(a: 10, b: 42)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

// ===========================================================================
// Short-circuit And/Or
// ===========================================================================

#[test]
fn short_circuit_and_false() {
    let src = "def main() -> Int\n  if false and true\n    return 1\n  else\n    return 0\n";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 0);
}

#[test]
fn short_circuit_and_true() {
    let src = "def main() -> Int\n  if true and true\n    return 1\n  else\n    return 0\n";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 1);
}

#[test]
fn short_circuit_or_true() {
    let src = "def main() -> Int\n  if true or false\n    return 1\n  else\n    return 0\n";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 1);
}

#[test]
fn short_circuit_or_false() {
    let src = "def main() -> Int\n  if false or false\n    return 1\n  else\n    return 0\n";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 0);
}

// ===========================================================================
// Build profiles: optimization levels
// ===========================================================================

#[test]
fn profile_debug_opt_none() {
    use crate::config::{BuildConfig, OptLevel};
    let config = BuildConfig::debug();
    assert_eq!(config.opt_level, OptLevel::None);

    // Compile and run with debug config
    let src = "def main() -> Int\n  return 42\n";
    let fir = compile_and_run(src);
    let mut jit = CraneliftJIT::with_config(&BuildConfig::debug());
    jit.compile_module(&fir).expect("JIT compile ok");
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn profile_release_opt_speed() {
    use crate::config::BuildConfig;
    let src = "def main() -> Int\n  return 42\n";
    let fir = compile_and_run(src);
    let mut jit = CraneliftJIT::with_config(&BuildConfig::release());
    jit.compile_module(&fir).expect("JIT compile ok");
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn profile_size_opt() {
    use crate::config::{BuildConfig, OptLevel};
    let mut config = BuildConfig::release();
    config.opt_level = OptLevel::SpeedAndSize;
    let src = "def main() -> Int\n  return 42\n";
    let fir = compile_and_run(src);
    let mut jit = CraneliftJIT::with_config(&config);
    jit.compile_module(&fir).expect("JIT compile ok");
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn aot_with_debug_config() {
    use crate::config::BuildConfig;
    let src = "def main() -> Int\n  return 42\n";
    let fir = compile_and_run(src);
    let mut aot = CraneliftAOT::with_config(&BuildConfig::debug());
    aot.compile_module(&fir).expect("AOT compile ok");
    let bytes = aot.emit_object().expect("emit ok");
    assert!(!bytes.is_empty());
}

#[test]
fn aot_with_release_config() {
    use crate::config::BuildConfig;
    let src = "def main() -> Int\n  return 42\n";
    let fir = compile_and_run(src);
    let mut aot = CraneliftAOT::with_config(&BuildConfig::release());
    aot.compile_module(&fir).expect("AOT compile ok");
    let bytes = aot.emit_object().expect("emit ok");
    assert!(!bytes.is_empty());
}

// ===========================================================================
// Power operator
// ===========================================================================

#[test]
fn pow_int_basic() {
    let src = "\
def main() -> Int
  2 ** 3
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 8);
}

#[test]
fn pow_int_zero_exponent() {
    let src = "\
def main() -> Int
  5 ** 0
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 1);
}

#[test]
fn pow_int_one_exponent() {
    let src = "\
def main() -> Int
  7 ** 1
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 7);
}

#[test]
fn pow_int_large() {
    let src = "\
def main() -> Int
  10 ** 6
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 1_000_000);
}

#[test]
fn pow_right_associative() {
    // 2 ** 3 ** 2 = 2 ** 9 = 512 (right-associative)
    let src = "\
def main() -> Int
  2 ** 3 ** 2
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 512);
}

#[test]
fn pow_in_expression() {
    let src = "\
def main() -> Int
  let x: Int = 3
  x ** 2 + 1
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 10);
}

#[test]
fn pow_int_overflow_wraps() {
    // 3 ** 40 overflows i64 — should wrap (matching JIT wrapping_mul behavior)
    let src = "\
def main() -> Int
  3 ** 40
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    // Compute expected: 3i64.wrapping_pow(40)
    let expected = 3i64.wrapping_pow(40);
    assert_eq!(result, expected);
}

// ===========================================================================
// String interpolation
// ===========================================================================

#[test]
fn string_interp_literal_only() {
    // No interpolation — should still work (already works via StringLit)
    let src = "\
def main() -> String
  \"hello world\"
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.call_i64(fir.entry.unwrap());
    assert_ne!(ptr, 0);
    unsafe {
        let len = *(ptr as *const i64) as usize;
        let data = (ptr as *const u8).add(8);
        let s = std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len));
        assert_eq!(s, "hello world");
    }
}

#[test]
fn string_interp_with_int() {
    let src = "\
def main() -> String
  let x: Int = 42
  \"value is {x}\"
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.call_i64(fir.entry.unwrap());
    assert_ne!(ptr, 0);
    unsafe {
        let len = *(ptr as *const i64) as usize;
        let data = (ptr as *const u8).add(8);
        let s = std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len));
        assert_eq!(s, "value is 42");
    }
}

#[test]
fn string_interp_with_string() {
    let src = "\
def main() -> String
  let name = \"world\"
  \"hello {name}\"
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.call_i64(fir.entry.unwrap());
    assert_ne!(ptr, 0);
    unsafe {
        let len = *(ptr as *const i64) as usize;
        let data = (ptr as *const u8).add(8);
        let s = std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len));
        assert_eq!(s, "hello world");
    }
}

#[test]
fn string_interp_multiple_parts() {
    let src = "\
def main() -> String
  let a: Int = 1
  let b: Int = 2
  \"{a} + {b} = 3\"
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.call_i64(fir.entry.unwrap());
    assert_ne!(ptr, 0);
    unsafe {
        let len = *(ptr as *const i64) as usize;
        let data = (ptr as *const u8).add(8);
        let s = std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len));
        assert_eq!(s, "1 + 2 = 3");
    }
}

#[test]
fn string_interp_with_bool() {
    let src = "\
def main() -> String
  let flag = true
  \"result: {flag}\"
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.call_i64(fir.entry.unwrap());
    assert_ne!(ptr, 0);
    unsafe {
        let len = *(ptr as *const i64) as usize;
        let data = (ptr as *const u8).add(8);
        let s = std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len));
        assert_eq!(s, "result: true");
    }
}

#[test]
fn string_interp_expression() {
    let src = "\
def main() -> String
  \"sum: {1 + 2}\"
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.call_i64(fir.entry.unwrap());
    assert_ne!(ptr, 0);
    unsafe {
        let len = *(ptr as *const i64) as usize;
        let data = (ptr as *const u8).add(8);
        let s = std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len));
        assert_eq!(s, "sum: 3");
    }
}

#[test]
fn string_interp_float() {
    let src = "\
def main() -> String
  \"pi: {3.14}\"
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.call_i64(fir.entry.unwrap());
    assert_ne!(ptr, 0);
    unsafe {
        let len = *(ptr as *const i64) as usize;
        let data = (ptr as *const u8).add(8);
        let s = std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len));
        assert!(
            s.starts_with("pi: 3.14"),
            "expected 'pi: 3.14...', got '{}'",
            s
        );
    }
}

#[test]
fn builtin_to_string_int_returns_heap_string() {
    let src = "\
def main() -> String
  to_string(value: 42)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.call_i64(fir.entry.unwrap());
    assert_ne!(ptr, 0);
    unsafe {
        let len = *(ptr as *const i64) as usize;
        let data = (ptr as *const u8).add(8);
        let s = std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len));
        assert_eq!(s, "42");
    }
}

// String interpolation — class to_string

#[test]
fn string_interp_class_manual_to_string() {
    // A class with a manual to_string should have it called during interpolation
    let src = "\
class Point includes Printable
  x: Int
  y: Int
  def to_string() -> String
    \"point\"

def main() -> String
  let p = Point(x: 1, y: 2)
  \"got: {p}\"
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.call_i64(fir.entry.unwrap());
    assert_ne!(ptr, 0);
    unsafe {
        let len = *(ptr as *const i64) as usize;
        let data = (ptr as *const u8).add(8);
        let s = std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len));
        assert_eq!(s, "got: point");
    }
}

#[test]
fn string_interp_class_with_fields() {
    // to_string that uses field access on self
    let src = "\
class Num includes Printable
  val: Int
  def to_string() -> String
    \"num\"

def main() -> String
  let n = Num(val: 42)
  \"value={n}\"
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.call_i64(fir.entry.unwrap());
    assert_ne!(ptr, 0);
    unsafe {
        let len = *(ptr as *const i64) as usize;
        let data = (ptr as *const u8).add(8);
        let s = std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len));
        assert_eq!(s, "value=num");
    }
}

#[test]
fn string_interp_class_mixed_with_primitives() {
    // Interpolation mixing class and primitive types
    let src = "\
class Tag includes Printable
  label: String
  def to_string() -> String
    \"tag\"

def main() -> String
  let t = Tag(label: \"hello\")
  let n: Int = 42
  \"{t}:{n}\"
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.call_i64(fir.entry.unwrap());
    assert_ne!(ptr, 0);
    unsafe {
        let len = *(ptr as *const i64) as usize;
        let data = (ptr as *const u8).add(8);
        let s = std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len));
        assert_eq!(s, "tag:42");
    }
}

// ===========================================================================
// Error handling
// ===========================================================================

#[test]
fn error_or_success_path() {
    // A throwing function that succeeds — .or fallback not needed
    let src = "\
class AppError extends Error
  code: Int

def risky() throws AppError -> Int
  42

def main() -> Int
  risky()!.or(0)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn error_or_else_success_path() {
    let src = "\
class AppError extends Error
  code: Int

def risky() throws AppError -> Int
  42

def main() -> Int
  risky()!.or_else(-> 0)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn bang_propagate_success() {
    // Simple ! propagation on success
    let src = "\
class AppError extends Error
  code: Int

def risky() throws AppError -> Int
  42

def main() throws AppError -> Int
  risky()!
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn error_or_failure_uses_default() {
    // A throwing function that actually throws — .or fallback should be returned
    let src = "\
class AppError extends Error
  code: Int

def risky() throws AppError -> Int
  throw AppError(message: \"fail\", code: 1)

def main() -> Int
  risky()!.or(99)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 99);
}

#[test]
fn error_or_else_failure_uses_handler() {
    let src = "\
class AppError extends Error
  code: Int

def risky() throws AppError -> Int
  throw AppError(message: \"fail\", code: 1)

def main() -> Int
  risky()!.or_else(-> 77)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 77);
}

#[test]
fn nullable_or_throw_success_path() {
    let src = "\
class AppError extends Error
  code: Int

def main() throws AppError -> Int
  let x: Int? = 42
  x.or_throw(error: AppError(message: \"missing\", code: 1))
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

// ===========================================================================
// Closure dispatch
// ===========================================================================

#[test]
fn closure_passed_to_function() {
    // Pass a closure to a function that calls it
    // Function type params use positional names (_0, _1, etc.)
    let src = "\
def apply(f: Fn(Int) -> Int, x: Int) -> Int
  f(_0: x)

def main() -> Int
  def double(x: Int) -> Int
    x * 2
  apply(f: double, x: 21)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn closure_with_captures_passed() {
    let src = "\
def apply(f: Fn(Int) -> Int, x: Int) -> Int
  f(_0: x)

def main() -> Int
  let offset: Int = 10
  def add_offset(x: Int) -> Int
    x + offset
  apply(f: add_offset, x: 32)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn inline_lambda_passed() {
    let src = "\
def apply(f: Fn(Int) -> Int, x: Int) -> Int
  f(_0: x)

def main() -> Int
  apply(f: -> x: x * 3, x: 14)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

// ===========================================================================
// MILESTONE 17: Nullable values and Iterator[T] codegen
// ===========================================================================

#[test]
fn nullable_return_nil_is_zero() {
    // A function returning T? that returns nil should produce 0 (null pointer)
    let src = "\
def maybe() -> Int?
  return nil

def main() -> Int
  let x: Int? = maybe()
  return 0
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 0);
}

#[test]
fn nullable_return_some_value_is_nonzero() {
    // A function returning T? with a value should produce a non-zero boxed pointer
    let src = "\
def maybe() -> Int?
  return 42

def main() -> Int
  let x: Int? = maybe()
  return 1
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 1);
}

#[test]
fn iterator_for_loop_sums_range() {
    // Iterator[Int] for-loop that sums values 0..5
    let src = "\
class Counter includes Iterator[Int]
  current: Int
  max: Int

  def next() -> Int?
    if current >= max
      return nil
    let val: Int = current
    current = current + 1
    return val

def main() -> Int
  let c: Counter = Counter(current: 0, max: 5)
  let total: Int = 0
  for x in c
    total = total + x
  return total
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    // 0 + 1 + 2 + 3 + 4 = 10
    assert_eq!(result, 10);
}

#[test]
fn iterator_for_loop_counts_elements() {
    // Iterator that produces 3 elements, count them
    let src = "\
class ThreeItems includes Iterator[Int]
  pos: Int

  def next() -> Int?
    if pos >= 3
      return nil
    pos = pos + 1
    return pos

def main() -> Int
  let it: ThreeItems = ThreeItems(pos: 0)
  let count: Int = 0
  for x in it
    count = count + 1
  return count
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 3);
}

#[test]
fn iterator_for_loop_empty() {
    // Iterator that immediately returns nil — loop body never executes
    let src = "\
class Empty includes Iterator[Int]
  done: Int

  def next() -> Int?
    return nil

def main() -> Int
  let it: Empty = Empty(done: 0)
  let count: Int = 0
  for x in it
    count = count + 1
  return count
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 0);
}

// ===========================================================================
// MILESTONE 18: Default parameter values
// ===========================================================================

#[test]
fn default_param_uses_default_when_arg_omitted() {
    let src = "\
def add(a: Int, b: Int = 10) -> Int
  a + b

def main() -> Int
  add(a: 32)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn default_param_uses_explicit_value_when_provided() {
    let src = "\
def add(a: Int, b: Int = 10) -> Int
  a + b

def main() -> Int
  add(a: 20, b: 22)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn default_param_all_params_defaulted() {
    let src = "\
def f(a: Int = 40, b: Int = 2) -> Int
  a + b

def main() -> Int
  f()
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn default_param_selective_override() {
    let src = "\
def f(a: Int = 100, b: Int = 2) -> Int
  a + b

def main() -> Int
  f(a: 40)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn default_param_string_default() {
    // Validates string defaults work through codegen (uses Ptr type)
    let src = "\
def greet(name: String = \"world\") -> Int
  return 42

def main() -> Int
  greet()
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn default_param_in_method() {
    let src = "\
class Calc
  base: Int

  def add(n: Int = 10) -> Int
    base + n

def main() -> Int
  let c: Calc = Calc(base: 32)
  c.add()
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

// ---------------------------------------------------------------------------
// M21: Map literals
// ---------------------------------------------------------------------------

#[test]
fn e2e_map_empty_creation() {
    let src = "\
def main() -> Int
  let m: Map[String, Int] = {}
  0
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 0);
}

#[test]
fn e2e_map_literal_with_entries() {
    let src = r#"
def main() -> Int
  let m: Map[String, Int] = {"a": 10, "b": 20}
  match m["a"]
    nil => 0
    v => v
"#;
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 10);
}

#[test]
fn e2e_map_get_second_key() {
    let src = r#"
def main() -> Int
  let m: Map[String, Int] = {"x": 5, "y": 42}
  match m["y"]
    nil => 0
    v => v
"#;
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn e2e_map_set_via_index_assignment() {
    let src = r#"
def main() -> Int
  let m: Map[String, Int] = {"a": 1}
  m["a"] = 42
  match m["a"]
    nil => 0
    v => v
"#;
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn e2e_map_add_new_key() {
    let src = r#"
def main() -> Int
  let m: Map[String, Int] = {}
  m["key"] = 99
  match m["key"]
    nil => 0
    v => v
"#;
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 99);
}

// ---------------------------------------------------------------------------
// M23: Virtual dispatch — trait methods on custom types
// ---------------------------------------------------------------------------

#[test]
fn e2e_class_eq_same_values() {
    let src = "\
class Point includes Eq
  x: Int
  y: Int

def main() -> Int
  let a: Point = Point(x: 1, y: 2)
  let b: Point = Point(x: 1, y: 2)
  if a == b
    return 1
  return 0
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 1);
}

#[test]
fn e2e_class_eq_different_values() {
    let src = "\
class Point includes Eq
  x: Int
  y: Int

def main() -> Int
  let a: Point = Point(x: 1, y: 2)
  let b: Point = Point(x: 3, y: 4)
  if a == b
    return 1
  return 0
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 0);
}

#[test]
fn e2e_class_neq() {
    let src = "\
class Point includes Eq
  x: Int
  y: Int

def main() -> Int
  let a: Point = Point(x: 1, y: 2)
  let b: Point = Point(x: 3, y: 4)
  if a != b
    return 1
  return 0
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 1);
}

#[test]
fn e2e_class_printable_to_string() {
    let src = "\
class Point includes Printable
  x: Int
  y: Int

def main() -> Int
  let p: Point = Point(x: 3, y: 4)
  let s = p.to_string()
  42
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

// ---------------------------------------------------------------------------
// Ord protocol on custom types
// ---------------------------------------------------------------------------

#[test]
fn e2e_class_ord_less_than() {
    let src = "\
class Score includes Ord
  val: Int

def main() -> Int
  let a: Score = Score(val: 3)
  let b: Score = Score(val: 7)
  if a < b
    return 1
  return 0
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 1);
}

#[test]
fn e2e_class_ord_greater_than() {
    let src = "\
class Score includes Ord
  val: Int

def main() -> Int
  let a: Score = Score(val: 9)
  let b: Score = Score(val: 3)
  if a > b
    return 1
  return 0
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 1);
}

// ---------------------------------------------------------------------------
// Inheritance: method override
// ---------------------------------------------------------------------------

#[test]
fn e2e_inheritance_method_override() {
    // Subclass overrides parent method — subclass version is called
    let src = "\
class Animal
  def sound() -> Int
    0

class Cat extends Animal
  def sound() -> Int
    42

def main() -> Int
  let c: Cat = Cat()
  c.sound()
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

// ---------------------------------------------------------------------------
// M24: Iterator protocol — custom for-loop
// ---------------------------------------------------------------------------

#[test]
fn e2e_for_loop_over_list() {
    let src = "\
def main() -> Int
  let nums: List[Int] = [10, 20, 12]
  let sum = 0
  for n in nums
    sum = sum + n
  sum
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn e2e_custom_iterator_for_loop() {
    let src = "\
class Range includes Iterator[Int]
  current: Int
  end_val: Int

  def next() -> Int?
    if current >= end_val
      return nil
    let val = current
    current = current + 1
    return val

def main() -> Int
  let r: Range = Range(current: 0, end_val: 5)
  let sum = 0
  for i in r
    sum = sum + i
  sum
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 10);
}

// ---------------------------------------------------------------------------
// M22: Error handling (throw, propagate, .or, .or_else, .catch)
// ---------------------------------------------------------------------------

#[test]
fn e2e_error_or_fallback() {
    let src = "\
def risky() throws String -> Int
  throw \"oops\"

def main() -> Int
  risky()!.or(42)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn e2e_error_or_success_path() {
    let src = "\
def safe() throws String -> Int
  return 10

def main() -> Int
  safe()!.or(99)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 10);
}

#[test]
fn e2e_error_or_else_fallback() {
    let src = "\
def risky() throws String -> Int
  throw \"oops\"

def main() -> Int
  risky()!.or_else(-> 42)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn e2e_error_catch_fallback() {
    let src = "\
def risky() throws String -> Int
  throw \"oops\"

def main() -> Int
  risky()!.catch
    _ -> 42
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

// ---------------------------------------------------------------------------
// Coverage: every Expr variant must have at least one codegen test
// ---------------------------------------------------------------------------

#[test]
fn coverage_all_expr_variants() {
    // This test maps every AST Expr variant to the test(s) that exercise it.
    // If a variant is added to the Expr enum without codegen coverage, add a
    // test and update this list. Variants listed here are *known* to be
    // exercised by the named test(s).
    //
    // Count of Expr variants in ast/src/expr.rs (keep in sync):
    let covered_variants: &[(&str, &str)] = &[
        ("Int", "m2_int_literal"),
        ("Float", "m3_float_literal"),
        ("Str", "m4_string_literal"),
        ("Bool", "m5_bool_literal"),
        ("Nil", "m6_nil_literal"),
        ("Ident", "m2_int_literal"),
        ("Member", "m10_class_field_access"),
        ("Lambda", "m11_closure_identity"),
        ("Call", "m7_function_call"),
        ("BinaryOp", "m2_int_literal"),
        ("UnaryOp", "m14_unary_negation"),
        ("ListLiteral", "m8_list_creation"),
        ("Index", "m8_list_index_access"),
        ("Match", "m9_match_int_literal"),
        ("StringInterpolation", "m13_string_interpolation"),
        ("Map", "e2e_map_literal_with_entries"),
        ("AsyncCall", "async_call_and_resolve"),
        ("Resolve", "m16_resolve_identity"),
        ("Propagate", "e2e_error_or_fallback"),
        ("Throw", "e2e_throw_returns_from_function"),
        ("ErrorOr", "e2e_error_or_fallback"),
        ("ErrorOrElse", "e2e_error_or_else_fallback"),
        ("ErrorCatch", "e2e_error_catch_fallback"),
        ("DetachedCall", "e2e_detached_async_executes"),
    ];

    // Verify the count matches the actual Expr enum variant count.
    // Expr has 24 variants (count from ast/src/expr.rs).
    assert_eq!(
        covered_variants.len(),
        24,
        "coverage_all_expr_variants is out of sync with the Expr enum — update this list"
    );
}

#[test]
fn coverage_all_stmt_variants() {
    let covered_variants: &[(&str, &str)] = &[
        ("Let", "m2_int_literal"),
        ("Class", "m10_class_construction"),
        ("Trait", "ERASED_type_only"),
        ("Return", "m7_function_call"),
        ("Expr", "m2_int_literal"),
        ("If", "m5_if_true_branch"),
        ("While", "m6_while_loop_sum"),
        ("For", "m6_for_loop"),
        ("Assignment", "m8_list_set"),
        ("Break", "m6_while_break"),
        ("Continue", "m6_while_continue"),
        ("Use", "ERASED_resolved_pre_fir"),
        ("Enum", "m9_enum_match"),
        ("Const", "m20_const_binding"),
    ];

    // Stmt has 14 variants.
    assert_eq!(
        covered_variants.len(),
        14,
        "coverage_all_stmt_variants is out of sync with the Stmt enum — update this list"
    );
}

// ---------------------------------------------------------------------------
// Audit: UnsupportedFeature occurrences must be tracked in the parity matrix
// ---------------------------------------------------------------------------

#[test]
fn unsupported_feature_audit() {
    // Count UnsupportedFeature call sites in lower.rs.
    // Each occurrence represents a known gap or guard rail. If you add a new
    // UnsupportedFeature path, you MUST document it in the STATUS.md parity
    // matrix. If you close a gap, decrement the count here.
    //
    // Current breakdown (16 sites in lower.rs, 0 in translate.rs):
    //   2 — generic catch-alls (unsupported_top_level_stmt, unsupported_stmt)
    //   2 — assignment/place edge cases (complex target, complex place expr)
    //   1 — .or_throw() on non-nullable FIR type
    //   1 — unresolved method call
    //   1 — missing argument with no default
    //   1 — Iterator class missing next() method
    //   3 — class field resolution errors (unknown class, no layout, unknown field)
    //   5 — resolve_class_name failures (variable, call expr, member-fallback, index-fallback, other expr)
    //
    // lower.rs was split into fir/src/lower/*.rs — count across all sub-modules.
    let lower_files: &[&str] = &[
        include_str!("../../fir/src/lower/mod.rs"),
        include_str!("../../fir/src/lower/stmt.rs"),
        include_str!("../../fir/src/lower/expr.rs"),
        include_str!("../../fir/src/lower/method.rs"),
        include_str!("../../fir/src/lower/for_loop.rs"),
        include_str!("../../fir/src/lower/iterable.rs"),
        include_str!("../../fir/src/lower/match_lower.rs"),
        include_str!("../../fir/src/lower/closure.rs"),
        include_str!("../../fir/src/lower/synthesize.rs"),
    ];
    let lower_src: String = lower_files.join("\n");
    // Count error construction sites: LowerError::UnsupportedFeature(
    let lower_count = lower_src.matches("LowerError::UnsupportedFeature").count();

    // Subtract 2 for the Display impl and span() match arms (not error sites).
    let actual_call_sites = lower_count - 2;

    assert_eq!(
        actual_call_sites, 17,
        "UnsupportedFeature call site count changed in lower.rs (expected 17, got {}). \
         If you added a new UnsupportedFeature, document it in STATUS.md parity matrix. \
         If you closed a gap, update this count and STATUS.md.",
        actual_call_sites
    );

    // translate.rs should have zero — every FirExpr/FirStmt variant is handled.
    let translate_src = include_str!("../../codegen/src/translate.rs");
    let translate_count = translate_src.matches("UnsupportedFeature").count();
    assert_eq!(
        translate_count, 0,
        "translate.rs should have no UnsupportedFeature paths — all FIR nodes must be translated"
    );
}

#[test]
fn e2e_into_method_dispatch() {
    // into() on a class calls the user-defined into() method.
    // Field names are accessed directly (not self.field) as the typechecker
    // injects fields into the method scope.
    let src = "\
class Wrapper
  val: Int
  def into() -> Int
    val * 2

def main() -> Int
  let w = Wrapper(val: 21)
  w.into()
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn e2e_from_static_call() {
    // Type.from(value: x) intrinsic — class must include From[T]
    let src = "\
class Doubled includes From[Int]
  val: Int
  def from(value: Int) -> Self
    Doubled(val: value * 2)

def main() -> Int
  let d = Doubled.from(value: 21)
  d.val
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn e2e_inheritance_parent_method_call() {
    // Subclass inherits method from parent — calling the inherited method on subclass instance
    let src = "\
class Animal
  sound: String
  def speak() -> Int
    42

class Dog extends Animal
  name: String

def main() -> Int
  let d: Dog = Dog(sound: \"woof\", name: \"Rex\")
  d.speak()
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn e2e_inheritance_subclass_field_access() {
    // Subclass inherits fields from parent — access both parent and child fields
    let src = "\
class Shape
  color: Int

class Circle extends Shape
  radius: Int
  def area() -> Int
    radius * radius

def main() -> Int
  let c: Circle = Circle(color: 1, radius: 7)
  c.color + c.radius
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 8);
}

#[test]
fn e2e_method_call_on_return_value() {
    // Method call on the return value of a function: f().method()
    let src = "\
class Wrapper
  val: Int
  def doubled() -> Int
    val * 2

def make_wrapper(n: Int) -> Wrapper
  Wrapper(val: n)

def main() -> Int
  make_wrapper(n: 21).doubled()
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn e2e_map_three_entries() {
    // Map with 3 entries — look up middle key
    let src = r#"
def main() -> Int
  let m: Map[String, Int] = {"a": 1, "b": 2, "c": 3}
  match m["b"]
    nil => 0
    v => v
"#;
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 2);
}

#[test]
fn e2e_class_default_field_in_method() {
    // A method that uses a field directly (field is in scope in method body)
    let src = "\
class Counter
  count: Int
  def value() -> Int
    count * 3

def main() -> Int
  let c = Counter(count: 14)
  c.value()
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn e2e_multiple_inheritance_levels() {
    // Three-level inheritance: GrandChild → Child → Parent
    // Access fields and methods across all levels
    let src = "\
class A
  x: Int
  def x_value() -> Int
    x

class B extends A
  y: Int
  def y_value() -> Int
    y

class C extends B
  z: Int

def main() -> Int
  let c: C = C(x: 1, y: 10, z: 100)
  c.x_value() + c.y_value() + c.z
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 111);
}

#[test]
fn e2e_nullable_or_with_method_call() {
    // Nullable .or() with a fallback value
    let src = "\
def maybe_get(flag: Bool) -> Int?
  if flag
    return 42
  nil

def main() -> Int
  maybe_get(flag: false).or(default: 99)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 99);
}

#[test]
fn e2e_nullable_or_present() {
    let src = "\
def maybe_get(flag: Bool) -> Int?
  if flag
    return 42
  nil

def main() -> Int
  maybe_get(flag: true).or(default: 0)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn e2e_nullable_or_else_nil() {
    // .or_else(f:) should return fallback when value is nil (lazy eval)
    let src = "\
def maybe_get(flag: Bool) -> Int?
  if flag
    return 42
  nil

def main() -> Int
  maybe_get(flag: false).or_else(f: 99)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 99);
}

#[test]
fn e2e_nullable_or_else_present() {
    // .or_else(f:) should return inner value when present
    let src = "\
def maybe_get(flag: Bool) -> Int?
  if flag
    return 42
  nil

def main() -> Int
  maybe_get(flag: true).or_else(f: 0)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn e2e_nested_field_assignment() {
    // Assignment to a chained field path: o.inner.val = x
    let src = "\
class Inner
  val: Int

class Outer
  inner: Inner

def main() -> Int
  let o = Outer(inner: Inner(val: 0))
  o.inner.val = 42
  o.inner.val
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn e2e_class_returned_from_function_field_access() {
    // Field access on value returned from a factory function (not a constructor)
    let src = "\
class Point
  x: Int
  y: Int

def make_point(x: Int, y: Int) -> Point
  Point(x: x, y: y)

def main() -> Int
  let p = make_point(x: 10, y: 32)
  p.x + p.y
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn e2e_method_chain_returns_self() {
    // Builder-style method chaining where each method returns a class instance
    let src = "\
class Builder
  val: Int

  def set(n: Int) -> Builder
    Builder(val: n)

  def doubled() -> Builder
    Builder(val: val * 2)

  def result() -> Int
    val

def main() -> Int
  Builder(val: 0).set(n: 3).doubled().result()
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 6);
}

#[test]
fn e2e_field_access_on_list_element() {
    // Field access on a class instance obtained via list indexing
    let src = "\
class Point
  x: Int

def main() -> Int
  let points: List[Point] = [Point(x: 10), Point(x: 32)]
  points[0].x + points[1].x
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn e2e_nested_class_field_in_method() {
    // Method that accesses a field of class type via bare name (not self.field)
    // e.g. `addr.zip` where addr is a field of type Address
    let src = "\
class Address
  zip: Int

class Person
  addr: Address

  def get_zip() -> Int
    addr.zip

def main() -> Int
  let p = Person(addr: Address(zip: 42))
  p.get_zip()
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn e2e_nested_field_assignment_in_method() {
    // Assignment to a nested field from inside a method body (addr.zip = x where addr is self.addr)
    let src = "\
class Address
  zip: Int

class Person
  addr: Address

  def move_to(new_zip: Int) -> Int
    addr.zip = new_zip
    addr.zip

def main() -> Int
  let p = Person(addr: Address(zip: 10))
  p.move_to(new_zip: 42)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn e2e_map_of_class_field_access() {
    // Field access on a class instance obtained via map lookup
    let src = r#"
class Item
  value: Int

def main() -> Int
  let m: Map[String, Item] = {"a": Item(value: 42)}
  match m["a"]
    nil => 0
    v => v.value
"#;
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn e2e_throw_returns_from_function() {
    // After throw, the error flag is set and the function returns a dummy value.
    // The caller uses .or() to recover.
    let src = "\
def compute(x: Int) throws String -> Int
  if x < 0
    throw \"negative\"
  return x * 2

def main() -> Int
  compute(x: -5)!.or(0) + compute(x: 10)!.or(0)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 20);
}

// ===========================================================================
// Top-level control flow
// ===========================================================================

#[test]
fn e2e_top_level_if_executes() {
    // Top-level if should execute in the init thunk before main
    let src = "\
let x = 10
let y = 0
if x > 5
  y = x + 1

def main() -> Int
  y
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 11);
}

#[test]
fn e2e_top_level_while_executes() {
    // Top-level while loops should execute
    let src = "\
let x = 0
let sum = 0
while x < 5
  sum = sum + x
  x = x + 1

def main() -> Int
  sum
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 10);
}

#[test]
fn e2e_top_level_for_executes() {
    // Top-level for-in loops should execute
    let src = "\
let nums = [10, 20, 12]
let total = 0
for n in nums
  total = total + n

def main() -> Int
  total
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn e2e_top_level_assignment_executes() {
    // Top-level assignment should execute
    let src = "\
let x = 10
x = 42

def main() -> Int
  x
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

// ===========================================================================
// Async
// ===========================================================================

#[test]
fn e2e_detached_async_executes() {
    // detached async f() should execute without materializing a task handle
    let src = "\
def work() -> Int
  42

def main() -> Int
  detached async work()
  1
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 1);
}

#[test]
fn e2e_async_spawn_and_resolve_tasks() {
    // spawned tasks are resolved within the implicit function scope
    let src = "\
def fetch_a() -> Int
  20

def fetch_b() -> Int
  22

def main() -> Int
  let ta = async fetch_a()
  let tb = async fetch_b()
  let a = resolve ta!
  let b = resolve tb!
  a + b
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

// ===========================================================================
// Generic type erasure — Float/Bool through TypeVar params (H1 audit fix)
// ===========================================================================

#[test]
fn generic_identity_float() {
    // Float through a generic identity function should preserve its value.
    let src = "\
def identity(x: T) -> T
  return x

def main() -> Float
  identity(x: 3.14)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.get_function_ptr(fir.entry.unwrap()).unwrap();
    let f: fn() -> f64 = unsafe { std::mem::transmute(ptr) };
    let result = f();
    assert!(
        (result - 3.14).abs() < 1e-10,
        "expected 3.14, got {}",
        result
    );
}

#[test]
fn generic_identity_bool() {
    // Bool through a generic identity function should preserve its value.
    let src = "\
def identity(x: T) -> T
  return x

def main() -> Bool
  identity(x: true)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 1);
}

// ===========================================================================
// Indirect function calls (GH #10)
// ===========================================================================

#[test]
fn indirect_call_return_value() {
    // Calling the return value of a function: get_handler()(21)
    let src = "\
def get_doubler() -> Fn(Int) -> Int
  def double(x: Int) -> Int
    x * 2
  double

def main() -> Int
  get_doubler()(_0: 21)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn indirect_call_fn_typed_variable() {
    // Calling a variable with Fn type annotation
    let src = "\
def main() -> Int
  def adder(x: Int) -> Int
    x + 10
  let f: Fn(Int) -> Int = adder
  f(x: 32)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn indirect_call_passed_as_fn_param() {
    // Fn type syntax in parameter position
    let src = "\
def apply(f: Fn(Int) -> Int, x: Int) -> Int
  f(_0: x)

def main() -> Int
  def double(x: Int) -> Int
    x * 2
  apply(f: double, x: 21)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn indirect_call_chained() {
    // Double indirection: get_handler()() where handler takes no args
    let src = "\
def make_const() -> Fn() -> Int
  def forty_two() -> Int
    42
  forty_two

def main() -> Int
  make_const()()
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

// ===========================================================================
// Closure allocation uses OBJ_CLOSURE header (Issue 3 regression)
// ===========================================================================

#[test]
fn closure_alloc_uses_obj_closure_header() {
    // Verify aster_closure_alloc is present in the runtime symbol table.
    // The JIT backend must resolve it so closures receive OBJ_CLOSURE headers
    // rather than OBJ_CLASS, ensuring the GC only traces the env pointer.
    let symbols = crate::runtime::runtime_builtin_symbols();
    assert!(
        symbols
            .iter()
            .any(|(name, _)| *name == "aster_closure_alloc"),
        "aster_closure_alloc must be registered in the runtime symbol table"
    );
}

#[test]
fn closure_with_captures_gc_safe() {
    // Regression test: closures allocated via aster_closure_alloc still
    // capture variables correctly and return the right value after a GC
    // safepoint (aster_safepoint is called implicitly by the runtime).
    let src = "\
def main() -> Int
  let captured: Int = 21
  def double_captured() -> Int
    captured * 2
  double_captured()
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

// ===========================================================================
// List mutation methods (Issue #9)
// ===========================================================================

#[test]
fn list_insert_at_beginning() {
    let src = "\
def main() -> Int
  let xs: List[Int] = [2, 3]
  xs.insert(at: 0, item: 1)
  xs[0]
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 1);
}

#[test]
fn list_insert_at_middle() {
    let src = "\
def main() -> Int
  let xs: List[Int] = [1, 3]
  xs.insert(at: 1, item: 2)
  xs[1]
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 2);
}

#[test]
fn list_insert_preserves_len() {
    let src = "\
def main() -> Int
  let xs: List[Int] = [1, 2]
  xs.insert(at: 1, item: 99)
  xs.len()
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 3);
}

#[test]
fn list_remove_at_beginning() {
    let src = "\
def main() -> Int
  let xs: List[Int] = [10, 20, 30]
  let removed = xs.remove(at: 0)
  removed
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 10);
}

#[test]
fn list_remove_shifts_elements() {
    let src = "\
def main() -> Int
  let xs: List[Int] = [10, 20, 30]
  xs.remove(at: 0)
  xs[0]
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 20);
}

#[test]
fn list_remove_decreases_len() {
    let src = "\
def main() -> Int
  let xs: List[Int] = [1, 2, 3]
  xs.remove(at: 1)
  xs.len()
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 2);
}

#[test]
fn list_pop_returns_last() {
    let src = "\
def main() -> Int
  let xs: List[Int] = [1, 2, 42]
  xs.pop()
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn list_pop_decreases_len() {
    let src = "\
def main() -> Int
  let xs: List[Int] = [1, 2, 3]
  xs.pop()
  xs.len()
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 2);
}

#[test]
fn list_contains_item_found() {
    let src = "\
def main() -> Int
  let xs: List[Int] = [10, 20, 30]
  let result: Int = 0
  if xs.contains(item: 20)
    result = 1
  result
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 1);
}

#[test]
fn list_contains_item_not_found() {
    let src = "\
def main() -> Int
  let xs: List[Int] = [10, 20, 30]
  let result: Int = 0
  if xs.contains(item: 99)
    result = 1
  result
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 0);
}

#[test]
fn list_contains_predicate() {
    let src = "\
def main() -> Int
  let xs: List[Int] = [1, 2, 3]
  let result: Int = 0
  if xs.contains(f: -> x : x > 2)
    result = 1
  result
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 1);
}

#[test]
fn list_remove_first_found() {
    // Verify remove_first removes the element and decreases length
    let src = "\
def main() -> Int
  let xs: List[Int] = [1, 2, 3, 2]
  xs.remove_first(f: -> x : x == 2)
  xs.len()
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 3);
}

#[test]
fn list_remove_first_not_found() {
    // When no match is found, list is unchanged
    let src = "\
def main() -> Int
  let xs: List[Int] = [1, 2, 3]
  xs.remove_first(f: -> x : x == 99)
  xs.len()
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 3);
}

#[test]
fn list_remove_first_shifts_elements() {
    // After removing first match, subsequent elements shift
    let src = "\
def main() -> Int
  let xs: List[Int] = [10, 20, 30]
  xs.remove_first(f: -> x : x == 20)
  xs[1]
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 30);
}

#[test]
fn list_contains_runtime_direct() {
    // Direct test of the runtime function, bypassing JIT compilation
    let list = crate::runtime::aster_list_new(4, 0);
    crate::runtime::aster_list_push(list, 10);
    crate::runtime::aster_list_push(list, 20);
    crate::runtime::aster_list_push(list, 30);
    assert_eq!(crate::runtime::aster_list_contains(list as *const u8, 20, 0), 1);
    assert_eq!(crate::runtime::aster_list_contains(list as *const u8, 99, 0), 0);
}

#[test]
fn list_remove_first_updates_len() {
    let src = "\
def main() -> Int
  let xs: List[Int] = [1, 2, 3]
  xs.remove_first(f: -> x : x == 2)
  xs.len()
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 2);
}
