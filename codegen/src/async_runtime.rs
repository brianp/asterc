use std::collections::{HashSet, VecDeque};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TaskId {
    slot: usize,
    generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct HeapObjectId(usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BlockingJobId {
    slot: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NetworkWaitId {
    slot: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StackError {
    requested: usize,
    max: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GcError {
    WorkersNotSafepointed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeTaskState {
    Running,
    Ready(i64),
    Failed(i64),
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskResolveError {
    Failed(i64),
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CoroutineStep {
    Yield,
    Suspend,
    Block(BlockingRequest),
    WaitForNetwork(i64),
    Complete(i64),
    Fail(i64),
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockingKind {
    Native,
    Foreign,
    Disk,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockingRequest {
    kind: BlockingKind,
    payload: i64,
}

impl BlockingRequest {
    pub fn native(payload: i64) -> Self {
        Self {
            kind: BlockingKind::Native,
            payload,
        }
    }

    pub fn foreign(payload: i64) -> Self {
        Self {
            kind: BlockingKind::Foreign,
            payload,
        }
    }

    pub fn disk(payload: i64) -> Self {
        Self {
            kind: BlockingKind::Disk,
            payload,
        }
    }
}

pub trait CoroutineBody: Send {
    fn resume(&mut self, cx: &mut CoroutineContext<'_>) -> CoroutineStep;
}

pub struct CoroutineContext<'a> {
    stack: &'a mut SegmentedStack,
    cancelled: bool,
    blocking_result: Option<i64>,
    network_result: Option<i64>,
}

impl<'a> CoroutineContext<'a> {
    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }

    pub fn stack(&mut self) -> &mut SegmentedStack {
        self.stack
    }

    pub fn take_blocking_result(&mut self) -> Option<i64> {
        self.blocking_result.take()
    }

    pub fn take_network_result(&mut self) -> Option<i64> {
        self.network_result.take()
    }
}

struct StackSegment {
    bytes: Box<[u8]>,
    used: usize,
}

pub struct SegmentedStack {
    segments: Vec<StackSegment>,
    roots: Vec<HeapObjectId>,
    next_segment_size: usize,
    max_segment_size: usize,
}

impl SegmentedStack {
    pub fn new(initial_segment_size: usize, max_segment_size: usize) -> Self {
        assert!(
            initial_segment_size > 0,
            "segmented stacks need a positive segment size"
        );
        assert!(
            max_segment_size >= initial_segment_size,
            "max segment size must be at least the initial segment size"
        );

        Self {
            segments: vec![StackSegment {
                bytes: vec![0; initial_segment_size].into_boxed_slice(),
                used: 0,
            }],
            roots: Vec::new(),
            next_segment_size: initial_segment_size,
            max_segment_size,
        }
    }

    pub fn push_bytes(&mut self, len: usize) -> Result<(), StackError> {
        if len == 0 {
            return Ok(());
        }

        if len > self.max_segment_size {
            return Err(StackError {
                requested: len,
                max: self.max_segment_size,
            });
        }

        let current = self
            .segments
            .last_mut()
            .expect("segmented stack always has a segment");
        if current.bytes.len().saturating_sub(current.used) < len {
            let grown = self.next_segment_size.saturating_mul(2).max(len);
            let next_size = grown.min(self.max_segment_size);
            self.segments.push(StackSegment {
                bytes: vec![0; next_size].into_boxed_slice(),
                used: 0,
            });
            self.next_segment_size = next_size;
        }

        let current = self
            .segments
            .last_mut()
            .expect("segmented stack always has a segment");
        current.used += len;
        Ok(())
    }

    pub fn segment_bases(&self) -> Vec<*const u8> {
        self.segments
            .iter()
            .map(|segment| segment.bytes.as_ptr())
            .collect()
    }

    pub fn push_root(&mut self, root: HeapObjectId) {
        self.roots.push(root);
    }

    fn roots(&self) -> impl Iterator<Item = HeapObjectId> + '_ {
        self.roots.iter().copied()
    }
}

struct Coroutine {
    stack: SegmentedStack,
    body: Box<dyn CoroutineBody>,
}

struct TaskRecord {
    coroutine: Option<Coroutine>,
    state: RuntimeTaskState,
    home_worker: usize,
    cancel_requested: bool,
    suspended: bool,
    owned_roots: Vec<HeapObjectId>,
    result_root: Option<HeapObjectId>,
    blocking_result: Option<i64>,
    network_result: Option<i64>,
}

struct TaskSlot {
    generation: u64,
    record: Option<TaskRecord>,
}

impl TaskRecord {
    fn new(home_worker: usize, body: Box<dyn CoroutineBody>) -> Self {
        Self {
            coroutine: Some(Coroutine {
                stack: SegmentedStack::new(2048, 64 * 1024),
                body,
            }),
            state: RuntimeTaskState::Running,
            home_worker,
            cancel_requested: false,
            suspended: false,
            owned_roots: Vec::new(),
            result_root: None,
            blocking_result: None,
            network_result: None,
        }
    }

    fn is_terminal(&self) -> bool {
        matches!(
            self.state,
            RuntimeTaskState::Ready(_) | RuntimeTaskState::Failed(_) | RuntimeTaskState::Cancelled
        )
    }
}

struct Worker {
    local: VecDeque<TaskId>,
    safepointed: bool,
    roots: Vec<HeapObjectId>,
}

impl Worker {
    fn new() -> Self {
        Self {
            local: VecDeque::new(),
            safepointed: false,
            roots: Vec::new(),
        }
    }
}

struct HeapObject {
    references: Vec<HeapObjectId>,
}

struct RuntimeHeap {
    objects: Vec<Option<HeapObject>>,
}

impl RuntimeHeap {
    fn new() -> Self {
        Self {
            objects: Vec::new(),
        }
    }

    fn allocate(&mut self, references: Vec<HeapObjectId>) -> HeapObjectId {
        let id = HeapObjectId(self.objects.len());
        self.objects.push(Some(HeapObject { references }));
        id
    }

    fn is_live(&self, object: HeapObjectId) -> bool {
        self.objects
            .get(object.0)
            .and_then(Option::as_ref)
            .is_some()
    }

    fn collect(&mut self, roots: Vec<HeapObjectId>) -> usize {
        let mut marked = HashSet::new();
        let mut worklist = roots;

        while let Some(object) = worklist.pop() {
            if !marked.insert(object) {
                continue;
            }
            if let Some(Some(entry)) = self.objects.get(object.0) {
                worklist.extend(entry.references.iter().copied());
            }
        }

        let mut reclaimed = 0;
        for (index, slot) in self.objects.iter_mut().enumerate() {
            if slot.is_some() && !marked.contains(&HeapObjectId(index)) {
                *slot = None;
                reclaimed += 1;
            }
        }
        reclaimed
    }
}

struct BlockingJob {
    task: TaskId,
    request: BlockingRequest,
}

struct NetworkWait {
    task: TaskId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WakeupSource {
    SameWorkerSpawn,
    BlockingPool,
    Poller,
    Cancellation,
}

pub struct AsyncRuntime {
    workers: Vec<Worker>,
    injector: VecDeque<TaskId>,
    tasks: Vec<TaskSlot>,
    free_slots: Vec<usize>,
    heap: RuntimeHeap,
    blocking_jobs: Vec<Option<BlockingJob>>,
    network_waits: Vec<Option<NetworkWait>>,
    gc_requested: bool,
}

impl AsyncRuntime {
    pub fn new(worker_count: usize) -> Self {
        let worker_count = worker_count.max(1);
        Self {
            workers: (0..worker_count).map(|_| Worker::new()).collect(),
            injector: VecDeque::new(),
            tasks: Vec::new(),
            free_slots: Vec::new(),
            heap: RuntimeHeap::new(),
            blocking_jobs: Vec::new(),
            network_waits: Vec::new(),
            gc_requested: false,
        }
    }

    pub fn bootstrap_main(&mut self, body: Box<dyn CoroutineBody>) -> TaskId {
        self.spawn_on_worker(0, body)
    }

    pub fn spawn_on_worker(&mut self, worker: usize, body: Box<dyn CoroutineBody>) -> TaskId {
        let worker = worker % self.workers.len();
        let task = self.create_task(worker, body);
        self.enqueue_local(worker, task);
        task
    }

    pub fn spawn_external(&mut self, body: Box<dyn CoroutineBody>) -> TaskId {
        let task = self.create_task(0, body);
        self.injector.push_back(task);
        task
    }

    pub fn run_one_tick(&mut self) -> bool {
        for worker in 0..self.workers.len() {
            if let Some(task) = self.workers[worker].local.pop_front() {
                self.resume_task(worker, task);
                return true;
            }
        }

        if let Some(task) = self.injector.pop_front() {
            let Some(worker) = self.task_record(task).map(|record| record.home_worker) else {
                return true;
            };
            self.resume_task(worker, task);
            return true;
        }

        for thief in 0..self.workers.len() {
            for victim in 0..self.workers.len() {
                if thief == victim {
                    continue;
                }
                if self.steal_half(thief, victim) > 0
                    && let Some(task) = self.workers[thief].local.pop_front()
                {
                    self.resume_task(thief, task);
                    return true;
                }
            }
        }

        false
    }

    pub fn await_task_terminal(&mut self, task: TaskId) -> Option<RuntimeTaskState> {
        while !self.task_record(task)?.is_terminal() {
            if !self.run_one_tick() {
                return None;
            }
        }
        self.task_state(task)
    }

    pub fn mark_task_cancelled(&mut self, task: TaskId) {
        if let Some(record) = self.task_record_mut(task) {
            if record.is_terminal() {
                return;
            }
            record.cancel_requested = true;
            if record.suspended {
                self.injector.push_back(task);
            }
        }
    }

    pub fn wake_task(&mut self, task: TaskId, source: WakeupSource) {
        let Some(home_worker) = self.task_record_mut(task).and_then(|record| {
            if record.is_terminal() {
                return None;
            }
            record.suspended = false;
            Some(record.home_worker)
        }) else {
            return;
        };
        let workers_len = self.workers.len();
        let local_index = home_worker % workers_len;
        match source {
            WakeupSource::SameWorkerSpawn => self.workers[local_index].local.push_back(task),
            WakeupSource::BlockingPool | WakeupSource::Poller | WakeupSource::Cancellation => {
                self.injector.push_back(task);
            }
        }
    }

    pub fn pending_blocking_job_count(&self) -> usize {
        self.blocking_jobs.iter().flatten().count()
    }

    pub fn first_pending_blocking_job(&self) -> Option<BlockingJobId> {
        self.blocking_jobs
            .iter()
            .position(Option::is_some)
            .map(|slot| BlockingJobId { slot })
    }

    pub fn first_pending_blocking_job_kind(&self) -> Option<BlockingKind> {
        self.first_pending_blocking_job()
            .and_then(|job| self.blocking_jobs.get(job.slot))
            .and_then(|job| job.as_ref())
            .map(|job| job.request.kind)
    }

    pub fn complete_blocking_job(&mut self, job: BlockingJobId, result: i64) {
        let Some(blocking_job) = self.blocking_jobs.get_mut(job.slot).and_then(Option::take) else {
            return;
        };
        if let Some(task) = self.task_record_mut(blocking_job.task) {
            if task.is_terminal() {
                return;
            }
            task.blocking_result = Some(result);
        }
        self.wake_task(blocking_job.task, WakeupSource::BlockingPool);
    }

    pub fn pending_network_wait_count(&self) -> usize {
        self.network_waits.iter().flatten().count()
    }

    pub fn first_pending_network_wait(&self) -> Option<NetworkWaitId> {
        self.network_waits
            .iter()
            .position(Option::is_some)
            .map(|slot| NetworkWaitId { slot })
    }

    pub fn complete_network_wait(&mut self, wait: NetworkWaitId, result: i64) {
        let Some(network_wait) = self.network_waits.get_mut(wait.slot).and_then(Option::take)
        else {
            return;
        };
        if let Some(task) = self.task_record_mut(network_wait.task) {
            if task.is_terminal() {
                return;
            }
            task.network_result = Some(result);
        }
        self.wake_task(network_wait.task, WakeupSource::Poller);
    }

    pub fn resolve_all(&mut self, tasks: &[TaskId]) -> Result<Vec<i64>, TaskResolveError> {
        let mut resolved = Vec::with_capacity(tasks.len());
        for &task in tasks {
            match self.await_task_terminal(task) {
                Some(RuntimeTaskState::Ready(value)) => resolved.push(value),
                Some(RuntimeTaskState::Failed(error)) => {
                    return Err(TaskResolveError::Failed(error));
                }
                Some(RuntimeTaskState::Cancelled | RuntimeTaskState::Running) | None => {
                    return Err(TaskResolveError::Cancelled);
                }
            }
        }
        Ok(resolved)
    }

    pub fn resolve_first(&mut self, tasks: &[TaskId]) -> Result<i64, TaskResolveError> {
        let Some(winner) = self.first_terminal_task(tasks) else {
            return Err(TaskResolveError::Cancelled);
        };

        for &task in tasks {
            if task != winner {
                self.mark_task_cancelled(task);
            }
        }
        for &task in tasks {
            if task != winner {
                let _ = self.await_task_terminal(task);
            }
        }

        match self.task_state(winner) {
            Some(RuntimeTaskState::Ready(value)) => Ok(value),
            Some(RuntimeTaskState::Failed(error)) => Err(TaskResolveError::Failed(error)),
            Some(RuntimeTaskState::Cancelled | RuntimeTaskState::Running) | None => {
                Err(TaskResolveError::Cancelled)
            }
        }
    }

    pub fn task_is_suspended(&self, task: TaskId) -> Option<bool> {
        self.task_record(task).map(|record| record.suspended)
    }

    pub fn allocate_heap_object(&mut self, references: Vec<HeapObjectId>) -> HeapObjectId {
        self.heap.allocate(references)
    }

    pub fn heap_object_is_live(&self, object: HeapObjectId) -> bool {
        self.heap.is_live(object)
    }

    pub fn add_worker_root(&mut self, worker: usize, root: HeapObjectId) {
        let worker = worker % self.workers.len();
        self.workers[worker].roots.push(root);
    }

    pub fn add_task_stack_root(&mut self, task: TaskId, root: HeapObjectId) -> bool {
        let Some(record) = self.task_record_mut(task) else {
            return false;
        };
        let Some(coroutine) = record.coroutine.as_mut() else {
            return false;
        };
        coroutine.stack.push_root(root);
        true
    }

    pub fn add_task_record_root(&mut self, task: TaskId, root: HeapObjectId) -> bool {
        let Some(record) = self.task_record_mut(task) else {
            return false;
        };
        record.owned_roots.push(root);
        true
    }

    pub fn store_task_result_root(&mut self, task: TaskId, root: HeapObjectId) -> bool {
        let Some(record) = self.task_record_mut(task) else {
            return false;
        };
        record.result_root = Some(root);
        true
    }

    pub fn request_stop_the_world_collection(&mut self) {
        self.gc_requested = true;
        for worker in &mut self.workers {
            worker.safepointed = false;
        }
    }

    pub fn worker_reach_safepoint(&mut self, worker: usize) {
        let worker = worker % self.workers.len();
        self.workers[worker].safepointed = true;
    }

    pub fn collect_garbage(&mut self) -> Result<usize, GcError> {
        if self.gc_requested && self.workers.iter().any(|worker| !worker.safepointed) {
            return Err(GcError::WorkersNotSafepointed);
        }
        let reclaimed = self.heap.collect(self.gc_roots());
        self.gc_requested = false;
        for worker in &mut self.workers {
            worker.safepointed = false;
        }
        Ok(reclaimed)
    }

    pub fn steal_half(&mut self, thief: usize, victim: usize) -> usize {
        let thief = thief % self.workers.len();
        let victim = victim % self.workers.len();
        if thief == victim {
            return 0;
        }

        let available = self.workers[victim].local.len();
        let to_steal = available / 2;
        for _ in 0..to_steal {
            if let Some(task) = self.workers[victim].local.pop_back() {
                self.workers[thief].local.push_back(task);
            }
        }
        to_steal
    }

    pub fn task_state(&self, task: TaskId) -> Option<RuntimeTaskState> {
        self.task_record(task).map(|record| record.state.clone())
    }

    pub fn local_queue_len(&self, worker: usize) -> usize {
        self.workers[worker % self.workers.len()].local.len()
    }

    pub fn injector_len(&self) -> usize {
        self.injector.len()
    }

    pub fn reap_task(&mut self, task: TaskId) -> Option<RuntimeTaskState> {
        let slot = self.tasks.get_mut(task.slot)?;
        if slot.generation != task.generation {
            return None;
        }
        let record = slot.record.as_ref()?;
        if !record.is_terminal() {
            return None;
        }
        let terminal = slot.record.take()?.state;
        self.free_slots.push(task.slot);
        Some(terminal)
    }

    fn create_task(&mut self, home_worker: usize, body: Box<dyn CoroutineBody>) -> TaskId {
        if let Some(slot) = self.free_slots.pop() {
            let generation = self.tasks[slot].generation.saturating_add(1);
            self.tasks[slot] = TaskSlot {
                generation,
                record: Some(TaskRecord::new(home_worker, body)),
            };
            TaskId { slot, generation }
        } else {
            let slot = self.tasks.len();
            self.tasks.push(TaskSlot {
                generation: 0,
                record: Some(TaskRecord::new(home_worker, body)),
            });
            TaskId {
                slot,
                generation: 0,
            }
        }
    }

    fn enqueue_local(&mut self, worker: usize, task: TaskId) {
        let worker = worker % self.workers.len();
        self.workers[worker].local.push_back(task);
    }

    fn enqueue_blocking_job(&mut self, task: TaskId, request: BlockingRequest) {
        self.blocking_jobs.push(Some(BlockingJob { task, request }));
    }

    fn enqueue_network_wait(&mut self, task: TaskId, _interest: i64) {
        self.network_waits.push(Some(NetworkWait { task }));
    }

    fn gc_roots(&self) -> Vec<HeapObjectId> {
        let mut roots = Vec::new();
        for worker in &self.workers {
            roots.extend(worker.roots.iter().copied());
        }
        for slot in &self.tasks {
            let Some(record) = slot.record.as_ref() else {
                continue;
            };
            roots.extend(record.owned_roots.iter().copied());
            roots.extend(record.result_root);
            if let Some(coroutine) = record.coroutine.as_ref() {
                roots.extend(coroutine.stack.roots());
            }
        }
        roots
    }

    fn resume_task(&mut self, worker: usize, task: TaskId) {
        let Some(record) = self.task_record_mut(task) else {
            return;
        };
        if record.is_terminal() {
            return;
        }

        if record.cancel_requested {
            record.coroutine = None;
            record.suspended = false;
            record.state = RuntimeTaskState::Cancelled;
            return;
        }

        let Some(coroutine) = record.coroutine.as_mut() else {
            return;
        };

        let mut cx = CoroutineContext {
            stack: &mut coroutine.stack,
            cancelled: record.cancel_requested,
            blocking_result: record.blocking_result.take(),
            network_result: record.network_result.take(),
        };
        let step = coroutine.body.resume(&mut cx);

        match step {
            CoroutineStep::Yield => {
                record.state = RuntimeTaskState::Running;
                record.suspended = false;
                self.enqueue_local(worker, task);
            }
            CoroutineStep::Suspend => {
                record.state = RuntimeTaskState::Running;
                record.suspended = true;
            }
            CoroutineStep::Block(request) => {
                record.state = RuntimeTaskState::Running;
                record.suspended = true;
                self.enqueue_blocking_job(task, request);
            }
            CoroutineStep::WaitForNetwork(interest) => {
                record.state = RuntimeTaskState::Running;
                record.suspended = true;
                self.enqueue_network_wait(task, interest);
            }
            CoroutineStep::Complete(value) => {
                record.coroutine = None;
                record.suspended = false;
                record.state = RuntimeTaskState::Ready(value);
            }
            CoroutineStep::Fail(error) => {
                record.coroutine = None;
                record.suspended = false;
                record.state = RuntimeTaskState::Failed(error);
            }
            CoroutineStep::Cancelled => {
                record.coroutine = None;
                record.suspended = false;
                record.state = RuntimeTaskState::Cancelled;
            }
        }
    }

    fn task_record(&self, task: TaskId) -> Option<&TaskRecord> {
        let slot = self.tasks.get(task.slot)?;
        if slot.generation != task.generation {
            return None;
        }
        slot.record.as_ref()
    }

    fn task_record_mut(&mut self, task: TaskId) -> Option<&mut TaskRecord> {
        let slot = self.tasks.get_mut(task.slot)?;
        if slot.generation != task.generation {
            return None;
        }
        slot.record.as_mut()
    }

    fn first_terminal_task(&mut self, tasks: &[TaskId]) -> Option<TaskId> {
        loop {
            if let Some(task) = tasks.iter().copied().find(|task| {
                matches!(
                    self.task_state(*task),
                    Some(RuntimeTaskState::Ready(_))
                        | Some(RuntimeTaskState::Failed(_))
                        | Some(RuntimeTaskState::Cancelled)
                )
            }) {
                return Some(task);
            }
            if !self.run_one_tick() {
                return None;
            }
        }
    }
}
