pub const C_RUNTIME_SOURCE: &str = r#"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <pthread.h>
#include <sched.h>
#include <sys/mman.h>
#include <unistd.h>
#include <stdatomic.h>

/* ===================================================================
 * Memory allocation
 * =================================================================== */

void* aster_alloc(int64_t size) {
    if (size == 0) return (void*)8; /* aligned dangling */
    if (size < 0) { fprintf(stderr, "aster_alloc: negative size\n"); abort(); }
    void* p = malloc((size_t)size);
    if (!p) { fprintf(stderr, "out of memory\n"); abort(); }
    return p;
}

void* aster_class_alloc(int64_t size) { return aster_alloc(size); }

/* ===================================================================
 * Printing
 * =================================================================== */

void aster_print_str(void* ptr) {
    if (!ptr) { printf("nil\n"); return; }
    int64_t len = *(int64_t*)ptr;
    if (len < 0) { printf("<invalid string>\n"); return; }
    char* data = (char*)ptr + 8;
    printf("%.*s\n", (int)len, data);
}

void aster_print_int(int64_t val) { printf("%lld\n", (long long)val); }
void aster_print_float(double val) { printf("%g\n", val); }
void aster_print_bool(int8_t val) { printf("%s\n", val ? "true" : "false"); }

/* ===================================================================
 * String operations
 * =================================================================== */

void* aster_string_new(void* data, int64_t len) {
    void* p = aster_alloc(8 + len);
    *(int64_t*)p = len;
    if (len > 0) memcpy((char*)p + 8, data, (size_t)len);
    return p;
}

void* aster_string_concat(void* a, void* b) {
    int64_t a_len = a ? *(int64_t*)a : 0;
    int64_t b_len = b ? *(int64_t*)b : 0;
    if (a_len < 0) a_len = 0;
    if (b_len < 0) b_len = 0;
    void* r = aster_alloc(8 + a_len + b_len);
    *(int64_t*)r = a_len + b_len;
    if (a_len > 0) memcpy((char*)r + 8, (char*)a + 8, (size_t)a_len);
    if (b_len > 0) memcpy((char*)r + 8 + a_len, (char*)b + 8, (size_t)b_len);
    return r;
}

int64_t aster_string_len(void* ptr) {
    if (!ptr) return 0;
    int64_t len = *(int64_t*)ptr;
    return len < 0 ? 0 : len;
}

int64_t aster_pow_int(int64_t base, int64_t exp) {
    if (exp < 0) return 0;
    int64_t result = 1;
    while (exp > 0) {
        if (exp & 1) result *= base;
        base *= base;
        exp >>= 1;
    }
    return result;
}

void* aster_int_to_string(int64_t val) {
    char buf[32];
    int len = snprintf(buf, sizeof(buf), "%lld", (long long)val);
    if (len < 0) len = 0;
    return aster_string_new(buf, (int64_t)len);
}

void* aster_float_to_string(double val) {
    char buf[64];
    int len = snprintf(buf, sizeof(buf), "%g", val);
    if (len < 0) len = 0;
    return aster_string_new(buf, (int64_t)len);
}

void* aster_bool_to_string(int8_t val) {
    const char* s = val ? "true" : "false";
    return aster_string_new((void*)s, val ? 4 : 5);
}

/* ===================================================================
 * List operations (handle-based indirection)
 * =================================================================== */

void* aster_list_new(int64_t cap) {
    if (cap < 4) cap = 4;
    void* block = aster_alloc(16 + cap * 8);
    *(int64_t*)block = 0;
    *((int64_t*)block + 1) = cap;
    void* handle = aster_alloc(8);
    *(void**)handle = block;
    return handle;
}

int64_t aster_list_get(void* handle, int64_t index) {
    if (!handle) { fprintf(stderr, "aster_list_get: null list\n"); abort(); }
    void* block = *(void**)handle;
    int64_t len = *(int64_t*)block;
    if (index < 0 || index >= len) {
        fprintf(stderr, "list index out of bounds: %lld (len %lld)\n",
                (long long)index, (long long)len);
        abort();
    }
    return *((int64_t*)block + 2 + index);
}

void aster_list_set(void* handle, int64_t index, int64_t value) {
    if (!handle) { fprintf(stderr, "aster_list_set: null list\n"); abort(); }
    void* block = *(void**)handle;
    int64_t len = *(int64_t*)block;
    if (index < 0 || index >= len) {
        fprintf(stderr, "list index out of bounds: %lld (len %lld)\n",
                (long long)index, (long long)len);
        abort();
    }
    *((int64_t*)block + 2 + index) = value;
}

void* aster_list_push(void* handle, int64_t value) {
    if (!handle) { fprintf(stderr, "aster_list_push: null list\n"); abort(); }
    void* block = *(void**)handle;
    int64_t len = *(int64_t*)block;
    int64_t cap = *((int64_t*)block + 1);
    if (len >= cap) {
        int64_t new_cap = cap * 2;
        if (new_cap < 4) new_cap = 4;
        void* new_block = aster_alloc(16 + new_cap * 8);
        memcpy(new_block, block, (size_t)(16 + len * 8));
        *((int64_t*)new_block + 1) = new_cap;
        free(block);
        *(void**)handle = new_block;
        block = new_block;
    }
    *((int64_t*)block + 2 + len) = value;
    *(int64_t*)block = len + 1;
    return handle;
}

int64_t aster_list_len(void* handle) {
    if (!handle) return 0;
    void* block = *(void**)handle;
    return *(int64_t*)block;
}

/* ===================================================================
 * Map operations (handle-based, linear scan)
 * =================================================================== */

static int aster_string_eq(void* a, void* b) {
    if (a == b) return 1;
    if (!a || !b) return 0;
    int64_t a_len = *(int64_t*)a;
    int64_t b_len = *(int64_t*)b;
    if (a_len != b_len || a_len < 0) return 0;
    return memcmp((char*)a + 8, (char*)b + 8, (size_t)a_len) == 0;
}

void* aster_map_new(int64_t cap) {
    if (cap < 4) cap = 4;
    void* block = aster_alloc(16 + cap * 16);
    *(int64_t*)block = 0;
    *((int64_t*)block + 1) = cap;
    void* handle = aster_alloc(8);
    *(void**)handle = block;
    return handle;
}

void* aster_map_set(void* handle, int64_t key, int64_t value) {
    if (!handle) { fprintf(stderr, "aster_map_set: null map\n"); abort(); }
    void* block = *(void**)handle;
    int64_t len = *(int64_t*)block;
    int64_t cap = *((int64_t*)block + 1);
    int64_t* entries = ((int64_t*)block) + 2;
    for (int64_t i = 0; i < len; i++) {
        if (aster_string_eq((void*)entries[i * 2], (void*)key)) {
            entries[i * 2 + 1] = value;
            return handle;
        }
    }
    if (len >= cap) {
        int64_t new_cap = cap * 2;
        if (new_cap < 4) new_cap = 4;
        void* new_block = aster_alloc(16 + new_cap * 16);
        memcpy(new_block, block, (size_t)(16 + len * 16));
        *((int64_t*)new_block + 1) = new_cap;
        free(block);
        *(void**)handle = new_block;
        block = new_block;
        entries = ((int64_t*)block) + 2;
    }
    entries[len * 2] = key;
    entries[len * 2 + 1] = value;
    *(int64_t*)block = len + 1;
    return handle;
}

int64_t aster_map_get(void* handle, int64_t key) {
    if (!handle) { fprintf(stderr, "aster_map_get: null map\n"); abort(); }
    void* block = *(void**)handle;
    int64_t len = *(int64_t*)block;
    int64_t* entries = ((int64_t*)block) + 2;
    for (int64_t i = 0; i < len; i++) {
        if (aster_string_eq((void*)entries[i * 2], (void*)key)) {
            return entries[i * 2 + 1];
        }
    }
    return 0;
}

/* ===================================================================
 * Error handling — per-thread flag (saved/restored per green thread)
 * =================================================================== */

static _Thread_local int aster_error_flag = 0;

void aster_error_set(void) { aster_error_flag = 1; }

int8_t aster_error_check(void) {
    int8_t was_set = aster_error_flag ? 1 : 0;
    aster_error_flag = 0;
    return was_set;
}

void aster_panic(void) {
    fprintf(stderr, "aster: uncaught error\n");
    abort();
}

/* GC stubs — the AOT runtime uses simple malloc/free, OS reclaims on exit */
void aster_gc_push_roots(int64_t frame_addr, int64_t count) {
    (void)frame_addr; (void)count;
}
void aster_gc_pop_roots(void) {}
void aster_gc_collect(void) {}

/* ===================================================================
 * Green thread infrastructure
 *
 * M:N scheduler: N OS worker threads run M green threads via assembly
 * context switching. Same architecture as the JIT Rust runtime.
 * =================================================================== */

/* --- MachineContext: must match assembly layout exactly --- */

#if defined(__aarch64__)
#define CONTEXT_REGS 21
#elif defined(__x86_64__)
#define CONTEXT_REGS 7
#else
#error "Unsupported architecture for green threads"
#endif

typedef struct {
    uint64_t regs[CONTEXT_REGS];
} MachineContext;

/* Assembly functions (linked from the .S file) */
extern void aster_context_switch(MachineContext *old_ctx, const MachineContext *new_ctx);
extern void aster_context_init(MachineContext *ctx, void *stack_top,
                               uintptr_t entry, uintptr_t arg);

/* --- Stack allocation --- */

#define GREEN_STACK_SIZE (64 * 1024)
#define GREEN_GUARD_SIZE 4096

typedef struct {
    void *base;
    size_t total;   /* guard + usable */
} GreenStack;

static GreenStack* green_stack_alloc(void) {
    size_t total = GREEN_STACK_SIZE + GREEN_GUARD_SIZE;
    void *mem = mmap(NULL, total, PROT_READ | PROT_WRITE,
                     MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (mem == MAP_FAILED) {
        fprintf(stderr, "mmap failed for green stack\n");
        abort();
    }
    /* Guard page at bottom */
    if (mprotect(mem, GREEN_GUARD_SIZE, PROT_NONE) != 0) {
        fprintf(stderr, "mprotect failed for guard page\n");
        abort();
    }
    GreenStack *s = (GreenStack*)malloc(sizeof(GreenStack));
    if (!s) { fprintf(stderr, "out of memory\n"); abort(); }
    s->base = mem;
    s->total = total;
    return s;
}

static void* green_stack_top(GreenStack *s) {
    return (char*)s->base + s->total;
}

static void green_stack_free(GreenStack *s) {
    if (s) {
        munmap(s->base, s->total);
        free(s);
    }
}

/* --- Stack pool --- */

#define STACK_POOL_MAX 64

typedef struct {
    GreenStack *stacks[STACK_POOL_MAX];
    size_t len;
    pthread_mutex_t mu;
} StackPool;

static StackPool stack_pool = { .len = 0, .mu = PTHREAD_MUTEX_INITIALIZER };

static GreenStack* stack_pool_get(void) {
    pthread_mutex_lock(&stack_pool.mu);
    if (stack_pool.len > 0) {
        GreenStack *s = stack_pool.stacks[--stack_pool.len];
        pthread_mutex_unlock(&stack_pool.mu);
        return s;
    }
    pthread_mutex_unlock(&stack_pool.mu);
    return green_stack_alloc();
}

static void stack_pool_put(GreenStack *s) {
    pthread_mutex_lock(&stack_pool.mu);
    if (stack_pool.len < STACK_POOL_MAX) {
        stack_pool.stacks[stack_pool.len++] = s;
        pthread_mutex_unlock(&stack_pool.mu);
    } else {
        pthread_mutex_unlock(&stack_pool.mu);
        green_stack_free(s);
    }
}

/* --- Green thread --- */

enum {
    GT_RUNNABLE  = 0,
    GT_RUNNING   = 1,
    GT_SUSPENDED = 2,
    GT_READY     = 3,
    GT_FAILED    = 4,
    GT_CANCELLED = 5,
};

typedef struct GreenThread GreenThread;
struct GreenThread {
    MachineContext context;
    GreenStack *stack;          /* NULL for terminal-allocated threads */
    int error_flag;
    void *shadow_stack_top;     /* unused in AOT (GC is no-op) */

    pthread_mutex_t mu;
    pthread_cond_t cv;
    int status;
    int cancel_requested;
    int consumed;
    int64_t result;
    int failed;

    GreenThread **waiters;
    size_t waiter_count;
    size_t waiter_cap;
};

static int gt_is_terminal(int status) {
    return status == GT_READY || status == GT_FAILED || status == GT_CANCELLED;
}

static GreenThread* gt_alloc(void) {
    GreenThread *t = (GreenThread*)calloc(1, sizeof(GreenThread));
    if (!t) { fprintf(stderr, "out of memory\n"); abort(); }
    pthread_mutex_init(&t->mu, NULL);
    pthread_cond_init(&t->cv, NULL);
    return t;
}

static void gt_add_waiter(GreenThread *target, GreenThread *waiter) {
    /* Must be called with target->mu held */
    if (target->waiter_count >= target->waiter_cap) {
        size_t new_cap = target->waiter_cap == 0 ? 4 : target->waiter_cap * 2;
        target->waiters = (GreenThread**)realloc(
            target->waiters, new_cap * sizeof(GreenThread*));
        if (!target->waiters) { fprintf(stderr, "out of memory\n"); abort(); }
        target->waiter_cap = new_cap;
    }
    target->waiters[target->waiter_count++] = waiter;
}

/* --- Work queue (mutex-protected FIFO) --- */

typedef struct {
    GreenThread **items;
    size_t head;
    size_t tail;
    size_t cap;
    pthread_mutex_t mu;
} WorkQueue;

static void wq_init(WorkQueue *q, size_t cap) {
    q->items = (GreenThread**)calloc(cap, sizeof(GreenThread*));
    if (!q->items) { fprintf(stderr, "out of memory\n"); abort(); }
    q->head = 0;
    q->tail = 0;
    q->cap = cap;
    pthread_mutex_init(&q->mu, NULL);
}

static void wq_push(WorkQueue *q, GreenThread *t) {
    pthread_mutex_lock(&q->mu);
    size_t next_tail = (q->tail + 1) % q->cap;
    if (next_tail == q->head) {
        /* Grow */
        size_t new_cap = q->cap * 2;
        GreenThread **new_items = (GreenThread**)calloc(new_cap, sizeof(GreenThread*));
        if (!new_items) { fprintf(stderr, "out of memory\n"); abort(); }
        size_t count = 0;
        size_t i = q->head;
        while (i != q->tail) {
            new_items[count++] = q->items[i];
            i = (i + 1) % q->cap;
        }
        free(q->items);
        q->items = new_items;
        q->head = 0;
        q->tail = count;
        q->cap = new_cap;
        next_tail = q->tail + 1;
    }
    q->items[q->tail] = t;
    q->tail = (q->tail + 1) % q->cap;
    pthread_mutex_unlock(&q->mu);
}

static GreenThread* wq_pop(WorkQueue *q) {
    pthread_mutex_lock(&q->mu);
    if (q->head == q->tail) {
        pthread_mutex_unlock(&q->mu);
        return NULL;
    }
    GreenThread *t = q->items[q->head];
    q->head = (q->head + 1) % q->cap;
    pthread_mutex_unlock(&q->mu);
    return t;
}

/* --- I/O Poller (Phase 5) --- */

#if defined(__APPLE__)
#include <sys/event.h>

static int poller_fd = -1;
static pthread_mutex_t poller_mu = PTHREAD_MUTEX_INITIALIZER;

static void poller_init(void) {
    poller_fd = kqueue();
    if (poller_fd < 0) {
        fprintf(stderr, "kqueue() failed\n");
        abort();
    }
}

static void poller_register_read(int fd, GreenThread *token) {
    struct kevent ev;
    EV_SET(&ev, fd, EVFILT_READ, EV_ADD | EV_ONESHOT, 0, 0, token);
    kevent(poller_fd, &ev, 1, NULL, 0, NULL);
}

static void poller_register_write(int fd, GreenThread *token) {
    struct kevent ev;
    EV_SET(&ev, fd, EVFILT_WRITE, EV_ADD | EV_ONESHOT, 0, 0, token);
    kevent(poller_fd, &ev, 1, NULL, 0, NULL);
}

static void poller_deregister(int fd) {
    struct kevent evs[2];
    EV_SET(&evs[0], fd, EVFILT_READ, EV_DELETE, 0, 0, NULL);
    EV_SET(&evs[1], fd, EVFILT_WRITE, EV_DELETE, 0, 0, NULL);
    kevent(poller_fd, evs, 2, NULL, 0, NULL);
}

static size_t poller_poll(GreenThread **out, size_t max_events) {
    struct kevent events[64];
    if (max_events > 64) max_events = 64;
    struct timespec ts = { 0, 0 }; /* non-blocking */
    int n = kevent(poller_fd, NULL, 0, events, (int)max_events, &ts);
    if (n < 0) return 0;
    for (int i = 0; i < n; i++) {
        out[i] = (GreenThread*)events[i].udata;
    }
    return (size_t)n;
}

#elif defined(__linux__)
#include <sys/epoll.h>

static int poller_fd = -1;
static pthread_mutex_t poller_mu = PTHREAD_MUTEX_INITIALIZER;

static void poller_init(void) {
    poller_fd = epoll_create1(EPOLL_CLOEXEC);
    if (poller_fd < 0) {
        fprintf(stderr, "epoll_create1() failed\n");
        abort();
    }
}

static void poller_register_read(int fd, GreenThread *token) {
    struct epoll_event ev = { .events = EPOLLIN | EPOLLONESHOT, .data.ptr = token };
    if (epoll_ctl(poller_fd, EPOLL_CTL_ADD, fd, &ev) < 0) {
        epoll_ctl(poller_fd, EPOLL_CTL_MOD, fd, &ev);
    }
}

static void poller_register_write(int fd, GreenThread *token) {
    struct epoll_event ev = { .events = EPOLLOUT | EPOLLONESHOT, .data.ptr = token };
    if (epoll_ctl(poller_fd, EPOLL_CTL_ADD, fd, &ev) < 0) {
        epoll_ctl(poller_fd, EPOLL_CTL_MOD, fd, &ev);
    }
}

static void poller_deregister(int fd) {
    epoll_ctl(poller_fd, EPOLL_CTL_DEL, fd, NULL);
}

static size_t poller_poll(GreenThread **out, size_t max_events) {
    struct epoll_event events[64];
    if (max_events > 64) max_events = 64;
    int n = epoll_wait(poller_fd, events, (int)max_events, 0);
    if (n < 0) return 0;
    for (int i = 0; i < n; i++) {
        out[i] = (GreenThread*)events[i].data.ptr;
    }
    return (size_t)n;
}

#endif

/* poll_io() and blocking pool are defined after global scheduler state below */

/* --- Yield reasons --- */

enum {
    YIELD_NONE               = 0,
    YIELD_PREEMPTED          = 1,
    YIELD_COMPLETED          = 2,
    YIELD_CANCELLED          = 3,
    YIELD_WAITING_ON_TASK    = 4,
    YIELD_WAITING_ON_IO      = 5,
    YIELD_WAITING_ON_BLOCKING = 6,
    YIELD_WAITING_ON_MUTEX    = 7,
    YIELD_WAITING_ON_CHAN_SEND = 8,
    YIELD_WAITING_ON_CHAN_RECV = 9,
};

/* --- Thread-local worker state --- */

static _Thread_local MachineContext worker_scheduler_ctx;
static _Thread_local GreenThread *worker_current_thread = NULL;
static _Thread_local int worker_yield_reason = YIELD_NONE;
static _Thread_local int64_t yield_result = 0;
static _Thread_local int yield_failed_flag = 0;
static _Thread_local GreenThread *yield_wait_target = NULL;
static _Thread_local uint32_t preempt_ticks = 0;
static _Thread_local int is_worker_thread = 0;
static _Thread_local int yield_io_fd = -1;
static _Thread_local int64_t (*yield_blocking_entry)(int64_t) = NULL;
static _Thread_local int64_t yield_blocking_arg = 0;

#define PREEMPT_THRESHOLD 1024

/* --- Global scheduler state --- */

#define MAX_WORKERS 32

static WorkQueue global_injector;
static WorkQueue worker_locals[MAX_WORKERS];
static int worker_count = 0;
static pthread_mutex_t park_mutex = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t park_cv = PTHREAD_COND_INITIALIZER;
static int scheduler_initialized = 0;
static _Thread_local int worker_id = -1;

/* Forward declarations */
static void wake_waiters(GreenThread **waiters, size_t count);
static void recycle_stack(GreenThread *t);

/* --- I/O poll (Phase 5) — uses global_injector and park_cv --- */

static void poll_io(void) {
    if (pthread_mutex_trylock(&poller_mu) != 0) return;
    GreenThread *ready[64];
    size_t n = poller_poll(ready, 64);
    pthread_mutex_unlock(&poller_mu);
    for (size_t i = 0; i < n; i++) {
        if (ready[i]) {
            wq_push(&global_injector, ready[i]);
        }
    }
    if (n > 0) {
        pthread_cond_broadcast(&park_cv);
    }
}

/* --- Blocking thread pool (Phase 5) --- */

typedef struct {
    GreenThread *task;
    int64_t (*entry)(int64_t);
    int64_t arg;
} BlockingJob;

#define BLOCKING_POOL_MAX 64

static BlockingJob blocking_jobs[BLOCKING_POOL_MAX];
static size_t blocking_job_count = 0;
static pthread_mutex_t blocking_mu = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t blocking_cv = PTHREAD_COND_INITIALIZER;

static void* blocking_worker(void *arg) {
    (void)arg;
    for (;;) {
        BlockingJob job;
        pthread_mutex_lock(&blocking_mu);
        while (blocking_job_count == 0) {
            pthread_cond_wait(&blocking_cv, &blocking_mu);
        }
        job = blocking_jobs[--blocking_job_count];
        pthread_mutex_unlock(&blocking_mu);

        int64_t result = job.entry(job.arg);

        /* Wake the green thread with the result */
        GreenThread *t = job.task;
        pthread_mutex_lock(&t->mu);
        t->result = result;
        t->failed = 0;
        t->status = GT_READY;
        pthread_cond_broadcast(&t->cv);
        GreenThread **waiters = t->waiters;
        size_t wcount = t->waiter_count;
        t->waiters = NULL;
        t->waiter_count = 0;
        t->waiter_cap = 0;
        pthread_mutex_unlock(&t->mu);
        wake_waiters(waiters, wcount);
        free(waiters);
    }
    return NULL;
}

#define BLOCKING_THREAD_COUNT 4

static int blocking_pool_initialized = 0;

static void ensure_blocking_pool(void) {
    if (blocking_pool_initialized) return;
    blocking_pool_initialized = 1;
    for (int i = 0; i < BLOCKING_THREAD_COUNT; i++) {
        pthread_t tid;
        pthread_attr_t attr;
        pthread_attr_init(&attr);
        pthread_attr_setdetachstate(&attr, PTHREAD_CREATE_DETACHED);
        pthread_create(&tid, &attr, blocking_worker, NULL);
        pthread_attr_destroy(&attr);
    }
}

static void blocking_pool_submit(GreenThread *task, int64_t (*entry)(int64_t), int64_t arg) {
    ensure_blocking_pool();
    pthread_mutex_lock(&blocking_mu);
    if (blocking_job_count >= BLOCKING_POOL_MAX) {
        fprintf(stderr, "blocking pool full\n");
        abort();
    }
    blocking_jobs[blocking_job_count++] = (BlockingJob){ .task = task, .entry = entry, .arg = arg };
    pthread_cond_signal(&blocking_cv);
    pthread_mutex_unlock(&blocking_mu);
}

/* --- Worker loop --- */

static GreenThread* find_task(int my_id) {
    /* 1. Local pop */
    GreenThread *t = wq_pop(&worker_locals[my_id]);
    if (t) return t;

    /* 2. Global injector */
    t = wq_pop(&global_injector);
    if (t) return t;

    /* 3. Steal from other workers */
    for (int i = 0; i < worker_count; i++) {
        if (i == my_id) continue;
        t = wq_pop(&worker_locals[i]);
        if (t) return t;
    }

    return NULL;
}

static void complete_thread(GreenThread *t, int64_t result, int failed) {
    pthread_mutex_lock(&t->mu);
    t->result = result;
    t->failed = failed;
    if (t->cancel_requested) {
        t->status = GT_CANCELLED;
    } else if (failed) {
        t->status = GT_FAILED;
    } else {
        t->status = GT_READY;
    }
    pthread_cond_broadcast(&t->cv);
    GreenThread **waiters = t->waiters;
    size_t wcount = t->waiter_count;
    t->waiters = NULL;
    t->waiter_count = 0;
    t->waiter_cap = 0;
    pthread_mutex_unlock(&t->mu);
    wake_waiters(waiters, wcount);
    free(waiters);
}

static void* worker_main(void *arg) {
    int my_id = (int)(intptr_t)arg;
    is_worker_thread = 1;
    worker_id = my_id;

    struct timespec ts;

    for (;;) {
        GreenThread *t = find_task(my_id);

        if (!t) {
            poll_io();
            t = find_task(my_id);
        }

        if (!t) {
            pthread_mutex_lock(&park_mutex);
            clock_gettime(CLOCK_REALTIME, &ts);
            ts.tv_nsec += 1000000; /* 1ms */
            if (ts.tv_nsec >= 1000000000) {
                ts.tv_sec += 1;
                ts.tv_nsec -= 1000000000;
            }
            pthread_cond_timedwait(&park_cv, &park_mutex, &ts);
            pthread_mutex_unlock(&park_mutex);
            continue;
        }

        /* Check cancel or already-terminal before running */
        pthread_mutex_lock(&t->mu);
        if (gt_is_terminal(t->status)) {
            pthread_mutex_unlock(&t->mu);
            continue;
        }
        if (t->cancel_requested) {
            t->status = GT_CANCELLED;
            pthread_cond_broadcast(&t->cv);
            GreenThread **waiters = t->waiters;
            size_t wcount = t->waiter_count;
            t->waiters = NULL;
            t->waiter_count = 0;
            t->waiter_cap = 0;
            pthread_mutex_unlock(&t->mu);
            wake_waiters(waiters, wcount);
            free(waiters);
            recycle_stack(t);
            continue;
        }
        t->status = GT_RUNNING;
        pthread_mutex_unlock(&t->mu);

        /* Set TLS for green thread */
        worker_current_thread = t;
        worker_yield_reason = YIELD_NONE;
        preempt_ticks = 0;

        /* Restore per-green-thread state */
        aster_error_flag = t->error_flag;

        /* Context switch to green thread */
        aster_context_switch(&worker_scheduler_ctx, &t->context);

        /* Green thread yielded back — save state */
        t->error_flag = aster_error_flag;
        worker_current_thread = NULL;

        switch (worker_yield_reason) {
        case YIELD_PREEMPTED:
            pthread_mutex_lock(&t->mu);
            t->status = GT_RUNNABLE;
            pthread_mutex_unlock(&t->mu);
            wq_push(&worker_locals[my_id], t);
            break;

        case YIELD_COMPLETED:
            complete_thread(t, yield_result, yield_failed_flag);
            recycle_stack(t);
            break;

        case YIELD_CANCELLED: {
            pthread_mutex_lock(&t->mu);
            t->status = GT_CANCELLED;
            pthread_cond_broadcast(&t->cv);
            GreenThread **waiters = t->waiters;
            size_t wcount = t->waiter_count;
            t->waiters = NULL;
            t->waiter_count = 0;
            t->waiter_cap = 0;
            pthread_mutex_unlock(&t->mu);
            wake_waiters(waiters, wcount);
            free(waiters);
            recycle_stack(t);
            break;
        }

        case YIELD_WAITING_ON_TASK: {
            GreenThread *target = yield_wait_target;
            pthread_mutex_lock(&target->mu);
            if (gt_is_terminal(target->status)) {
                pthread_mutex_unlock(&target->mu);
                pthread_mutex_lock(&t->mu);
                t->status = GT_RUNNABLE;
                pthread_mutex_unlock(&t->mu);
                wq_push(&worker_locals[my_id], t);
            } else {
                gt_add_waiter(target, t);
                pthread_mutex_unlock(&target->mu);
                pthread_mutex_lock(&t->mu);
                t->status = GT_SUSPENDED;
                pthread_mutex_unlock(&t->mu);
            }
            break;
        }

        case YIELD_WAITING_ON_IO:
            pthread_mutex_lock(&t->mu);
            t->status = GT_SUSPENDED;
            pthread_mutex_unlock(&t->mu);
            /* Thread is registered with the poller; it will be re-enqueued when I/O is ready */
            break;

        case YIELD_WAITING_ON_BLOCKING:
            pthread_mutex_lock(&t->mu);
            t->status = GT_SUSPENDED;
            pthread_mutex_unlock(&t->mu);
            /* Submit to blocking pool */
            blocking_pool_submit(t, yield_blocking_entry, yield_blocking_arg);
            break;

        case YIELD_WAITING_ON_MUTEX:
        case YIELD_WAITING_ON_CHAN_SEND:
        case YIELD_WAITING_ON_CHAN_RECV:
            pthread_mutex_lock(&t->mu);
            t->status = GT_SUSPENDED;
            pthread_mutex_unlock(&t->mu);
            /* Thread is on mutex/channel wait queue; will be re-enqueued when woken */
            break;

        default:
            /* YIELD_NONE — treat as preempted */
            pthread_mutex_lock(&t->mu);
            t->status = GT_RUNNABLE;
            pthread_mutex_unlock(&t->mu);
            wq_push(&worker_locals[my_id], t);
            break;
        }
    }

    return NULL;
}

static void wake_waiters(GreenThread **waiters, size_t count) {
    if (!waiters || count == 0) return;
    for (size_t i = 0; i < count; i++) {
        wq_push(&global_injector, waiters[i]);
    }
    pthread_cond_broadcast(&park_cv);
}

static void recycle_stack(GreenThread *t) {
    if (t->stack) {
        stack_pool_put(t->stack);
        t->stack = NULL;
    }
}

/* --- Scheduler init --- */

static void ensure_scheduler(void) {
    if (scheduler_initialized) return;
    scheduler_initialized = 1;

    long cpus = sysconf(_SC_NPROCESSORS_ONLN);
    worker_count = (int)(cpus > 2 ? cpus : 2);
    if (worker_count > MAX_WORKERS) worker_count = MAX_WORKERS;

    poller_init();
    ensure_blocking_pool();

    wq_init(&global_injector, 256);
    for (int i = 0; i < worker_count; i++) {
        wq_init(&worker_locals[i], 64);
    }

    for (int i = 0; i < worker_count; i++) {
        pthread_t tid;
        pthread_attr_t attr;
        pthread_attr_init(&attr);
        pthread_attr_setdetachstate(&attr, PTHREAD_CREATE_DETACHED);
        if (pthread_create(&tid, &attr, worker_main, (void*)(intptr_t)i) != 0) {
            fprintf(stderr, "failed to create worker thread\n");
            abort();
        }
        pthread_attr_destroy(&attr);
    }
}

/* --- Yield to scheduler --- */

static void yield_to_scheduler(int reason) {
    worker_yield_reason = reason;
    GreenThread *current = worker_current_thread;
    aster_context_switch(&current->context, &worker_scheduler_ctx);
    /* Execution resumes here when scheduler switches back to us */
}

/* --- Green thread exit (called from assembly trampoline) --- */

void aster_green_thread_exit(int64_t result) {
    int failed = aster_error_flag;
    aster_error_flag = 0;
    yield_result = result;
    yield_failed_flag = failed;
    yield_to_scheduler(YIELD_COMPLETED);
    /* unreachable */
    abort();
}

/* --- Safepoint --- */

void aster_safepoint(void) {
    GreenThread *current = worker_current_thread;
    if (!current) return;

    /* Check cancellation */
    pthread_mutex_lock(&current->mu);
    int cancel = current->cancel_requested;
    pthread_mutex_unlock(&current->mu);
    if (cancel) {
        yield_to_scheduler(YIELD_CANCELLED);
        return;
    }

    /* Tick-based preemption */
    preempt_ticks++;
    if (preempt_ticks >= PREEMPT_THRESHOLD) {
        preempt_ticks = 0;
        yield_to_scheduler(YIELD_PREEMPTED);
    }
}

/* ===================================================================
 * I/O suspension + blocking submit hooks (Phase 5)
 * =================================================================== */

void aster_io_wait_read(int fd) {
    GreenThread *current = worker_current_thread;
    if (!current) return;
    pthread_mutex_lock(&poller_mu);
    poller_register_read(fd, current);
    pthread_mutex_unlock(&poller_mu);
    yield_to_scheduler(YIELD_WAITING_ON_IO);
    /* Resumed when fd is readable */
    pthread_mutex_lock(&poller_mu);
    poller_deregister(fd);
    pthread_mutex_unlock(&poller_mu);
}

void aster_io_wait_write(int fd) {
    GreenThread *current = worker_current_thread;
    if (!current) return;
    pthread_mutex_lock(&poller_mu);
    poller_register_write(fd, current);
    pthread_mutex_unlock(&poller_mu);
    yield_to_scheduler(YIELD_WAITING_ON_IO);
    /* Resumed when fd is writable */
    pthread_mutex_lock(&poller_mu);
    poller_deregister(fd);
    pthread_mutex_unlock(&poller_mu);
}

void aster_blocking_submit(int64_t (*entry)(int64_t), int64_t arg) {
    GreenThread *current = worker_current_thread;
    if (!current) return;
    yield_blocking_entry = entry;
    yield_blocking_arg = arg;
    yield_to_scheduler(YIELD_WAITING_ON_BLOCKING);
    /* Resumed when blocking work completes — result is in current->result */
}

/* ===================================================================
 * Async scope
 * =================================================================== */

typedef struct {
    pthread_mutex_t mu;
    GreenThread **tasks;
    int64_t len;
    int64_t cap;
} AsterAsyncScope;

static void scope_register(AsterAsyncScope *scope, GreenThread *task) {
    if (!scope) return;
    pthread_mutex_lock(&scope->mu);
    if (scope->len >= scope->cap) {
        int64_t new_cap = scope->cap == 0 ? 4 : scope->cap * 2;
        scope->tasks = (GreenThread**)realloc(
            scope->tasks, (size_t)(new_cap * (int64_t)sizeof(GreenThread*)));
        if (!scope->tasks) { fprintf(stderr, "out of memory\n"); abort(); }
        scope->cap = new_cap;
    }
    scope->tasks[scope->len++] = task;
    pthread_mutex_unlock(&scope->mu);
}

void* aster_async_scope_enter(void) {
    AsterAsyncScope *scope = (AsterAsyncScope*)calloc(1, sizeof(AsterAsyncScope));
    if (!scope) { fprintf(stderr, "out of memory\n"); abort(); }
    pthread_mutex_init(&scope->mu, NULL);
    return scope;
}

/* Forward declarations for cancel/wait */
static void gt_cancel(GreenThread *t);
static void gt_wait_terminal(GreenThread *t);

void aster_async_scope_exit(void *scope_ptr) {
    if (!scope_ptr) return;
    AsterAsyncScope *scope = (AsterAsyncScope*)scope_ptr;
    pthread_mutex_lock(&scope->mu);
    int64_t len = scope->len;
    GreenThread **tasks = scope->tasks;
    scope->tasks = NULL;
    scope->len = 0;
    scope->cap = 0;
    pthread_mutex_unlock(&scope->mu);

    for (int64_t i = 0; i < len; i++) {
        if (tasks[i]) gt_cancel(tasks[i]);
    }
    for (int64_t i = 0; i < len; i++) {
        if (tasks[i]) gt_wait_terminal(tasks[i]);
    }
    free(tasks);
    free(scope);
}

/* ===================================================================
 * Task API — spawn, resolve, cancel
 * =================================================================== */

static void gt_cancel(GreenThread *t) {
    pthread_mutex_lock(&t->mu);
    t->cancel_requested = 1;
    switch (t->status) {
    case GT_RUNNABLE:
        t->status = GT_CANCELLED;
        pthread_cond_broadcast(&t->cv);
        {
            GreenThread **waiters = t->waiters;
            size_t wcount = t->waiter_count;
            t->waiters = NULL;
            t->waiter_count = 0;
            t->waiter_cap = 0;
            pthread_mutex_unlock(&t->mu);
            wake_waiters(waiters, wcount);
            free(waiters);
        }
        return;
    case GT_RUNNING:
        /* Flag set, safepoint will catch it */
        pthread_mutex_unlock(&t->mu);
        return;
    case GT_SUSPENDED:
        t->status = GT_CANCELLED;
        pthread_cond_broadcast(&t->cv);
        {
            GreenThread **waiters = t->waiters;
            size_t wcount = t->waiter_count;
            t->waiters = NULL;
            t->waiter_count = 0;
            t->waiter_cap = 0;
            pthread_mutex_unlock(&t->mu);
            wake_waiters(waiters, wcount);
            free(waiters);
        }
        recycle_stack(t);
        return;
    default:
        /* Already terminal */
        pthread_mutex_unlock(&t->mu);
        return;
    }
}

static void gt_wait_terminal(GreenThread *t) {
    if (is_worker_thread) {
        /* On a worker — yield as a green thread until target is terminal */
        for (;;) {
            pthread_mutex_lock(&t->mu);
            if (gt_is_terminal(t->status)) {
                pthread_mutex_unlock(&t->mu);
                return;
            }
            pthread_mutex_unlock(&t->mu);
            yield_wait_target = t;
            yield_to_scheduler(YIELD_WAITING_ON_TASK);
        }
    } else {
        /* On main or non-worker thread — block with condvar */
        pthread_mutex_lock(&t->mu);
        while (!gt_is_terminal(t->status)) {
            pthread_cond_wait(&t->cv, &t->mu);
        }
        pthread_mutex_unlock(&t->mu);
    }
}

static int64_t gt_consume_result(GreenThread *t) {
    gt_wait_terminal(t);

    pthread_mutex_lock(&t->mu);
    if (t->consumed) {
        pthread_mutex_unlock(&t->mu);
        aster_error_set();
        return 0;
    }
    t->consumed = 1;
    if (t->status == GT_READY) {
        int64_t val = t->result;
        pthread_mutex_unlock(&t->mu);
        return val;
    }
    pthread_mutex_unlock(&t->mu);
    aster_error_set();
    return 0;
}

int64_t aster_task_spawn(int64_t entry_ptr, int64_t args_ptr, int64_t scope_ptr) {
    ensure_scheduler();

    GreenStack *stack = stack_pool_get();
    GreenThread *t = gt_alloc();
    t->stack = stack;
    t->status = GT_RUNNABLE;

    aster_context_init(&t->context, green_stack_top(stack),
                       (uintptr_t)entry_ptr, (uintptr_t)args_ptr);

    scope_register((AsterAsyncScope*)(intptr_t)scope_ptr, t);

    wq_push(&global_injector, t);
    pthread_cond_broadcast(&park_cv);

    return (int64_t)(intptr_t)t;
}

int64_t aster_task_block_on(int64_t entry_ptr, int64_t args_ptr) {
    int64_t task = aster_task_spawn(entry_ptr, args_ptr, 0);
    return gt_consume_result((GreenThread*)(intptr_t)task);
}

static void* gt_new_terminal(int64_t payload, int8_t failed) {
    GreenThread *t = gt_alloc();
    t->status = failed ? GT_FAILED : GT_READY;
    t->result = payload;
    t->failed = failed;
    return t;
}

void* aster_task_from_i64(int64_t value, int8_t failed) {
    return gt_new_terminal(value, failed);
}

void* aster_task_from_f64(double value, int8_t failed) {
    int64_t bits = 0;
    memcpy(&bits, &value, sizeof(bits));
    return gt_new_terminal(bits, failed);
}

void* aster_task_from_i8(int8_t value, int8_t failed) {
    return gt_new_terminal((int64_t)value, failed);
}

int8_t aster_task_is_ready(void* task_ptr) {
    if (!task_ptr) return 0;
    GreenThread *t = (GreenThread*)task_ptr;
    pthread_mutex_lock(&t->mu);
    int8_t ready = gt_is_terminal(t->status) ? 1 : 0;
    pthread_mutex_unlock(&t->mu);
    return ready;
}

int64_t aster_task_cancel(void* task_ptr) {
    if (task_ptr) gt_cancel((GreenThread*)task_ptr);
    return 0;
}

int64_t aster_task_wait_cancel(void* task_ptr) {
    if (task_ptr) {
        gt_cancel((GreenThread*)task_ptr);
        gt_wait_terminal((GreenThread*)task_ptr);
    }
    return 0;
}

int64_t aster_task_resolve_i64(void* task_ptr) {
    if (!task_ptr) { aster_error_set(); return 0; }
    return gt_consume_result((GreenThread*)task_ptr);
}

double aster_task_resolve_f64(void* task_ptr) {
    if (!task_ptr) { aster_error_set(); return 0.0; }
    int64_t bits = gt_consume_result((GreenThread*)task_ptr);
    double value = 0.0;
    memcpy(&value, &bits, sizeof(value));
    return value;
}

int8_t aster_task_resolve_i8(void* task_ptr) {
    if (!task_ptr) { aster_error_set(); return 0; }
    return (int8_t)gt_consume_result((GreenThread*)task_ptr);
}

void* aster_task_resolve_all_i64(void* tasks) {
    if (!tasks) { aster_error_set(); return 0; }
    int64_t len = aster_list_len(tasks);
    void* out = aster_list_new(len);
    for (int64_t i = 0; i < len; i++) {
        int64_t task = aster_list_get(tasks, i);
        int64_t value = aster_task_resolve_i64((void*)(intptr_t)task);
        if (aster_error_flag) return out;
        aster_list_push(out, value);
    }
    return out;
}

int64_t aster_task_resolve_first_i64(void* tasks) {
    if (!tasks) { aster_error_set(); return 0; }
    int64_t len = aster_list_len(tasks);
    if (len == 0) { aster_error_set(); return 0; }

    GreenThread *winner = NULL;
    int64_t winner_index = -1;
    for (;;) {
        for (int64_t i = 0; i < len; i++) {
            GreenThread *t = (GreenThread*)(intptr_t)aster_list_get(tasks, i);
            pthread_mutex_lock(&t->mu);
            int done = gt_is_terminal(t->status);
            pthread_mutex_unlock(&t->mu);
            if (done) {
                winner = t;
                winner_index = i;
                break;
            }
        }
        if (winner) break;
        /* Yield or OS yield */
        if (is_worker_thread) {
            aster_safepoint();
        } else {
            sched_yield();
        }
    }
    for (int64_t i = 0; i < len; i++) {
        if (i != winner_index) {
            aster_task_cancel((void*)(intptr_t)aster_list_get(tasks, i));
        }
    }
    return aster_task_resolve_i64((void*)winner);
}

/* ===================================================================
 * Mutex[T]
 * =================================================================== */

typedef struct {
    pthread_mutex_t mu;
    int locked;
    int64_t value;
    GreenThread **waiters;
    int64_t wait_len;
    int64_t wait_cap;
} AsterMutex;

void* aster_mutex_new(int64_t value) {
    AsterMutex *m = (AsterMutex*)calloc(1, sizeof(AsterMutex));
    if (!m) { fprintf(stderr, "out of memory\n"); abort(); }
    pthread_mutex_init(&m->mu, NULL);
    m->locked = 0;
    m->value = value;
    m->waiters = NULL;
    m->wait_len = 0;
    m->wait_cap = 0;
    return m;
}

int64_t aster_mutex_lock(void *ptr) {
    AsterMutex *m = (AsterMutex*)ptr;
    if (!m) return 0;
    pthread_mutex_lock(&m->mu);
    while (m->locked) {
        /* Add self to wait queue */
        GreenThread *self = worker_current_thread;
        if (m->wait_len >= m->wait_cap) {
            int64_t new_cap = m->wait_cap == 0 ? 4 : m->wait_cap * 2;
            m->waiters = (GreenThread**)realloc(m->waiters, (size_t)(new_cap * (int64_t)sizeof(GreenThread*)));
            m->wait_cap = new_cap;
        }
        m->waiters[m->wait_len++] = self;
        pthread_mutex_unlock(&m->mu);
        if (self) {
            yield_to_scheduler(YIELD_WAITING_ON_MUTEX);
        }
        pthread_mutex_lock(&m->mu);
    }
    m->locked = 1;
    int64_t val = m->value;
    pthread_mutex_unlock(&m->mu);
    return val;
}

void aster_mutex_unlock(void *ptr, int64_t value) {
    AsterMutex *m = (AsterMutex*)ptr;
    if (!m) return;
    pthread_mutex_lock(&m->mu);
    m->value = value;
    m->locked = 0;
    if (m->wait_len > 0) {
        GreenThread *waiter = m->waiters[0];
        /* Shift wait queue */
        for (int64_t i = 1; i < m->wait_len; i++) {
            m->waiters[i-1] = m->waiters[i];
        }
        m->wait_len--;
        pthread_mutex_unlock(&m->mu);
        /* Re-enqueue the waiter */
        if (waiter) {
            pthread_mutex_lock(&waiter->mu);
            waiter->status = GT_RUNNABLE;
            pthread_mutex_unlock(&waiter->mu);
            wq_push(&worker_locals[0], waiter);
        }
    } else {
        pthread_mutex_unlock(&m->mu);
    }
}

int64_t aster_mutex_get_value(void *ptr) {
    AsterMutex *m = (AsterMutex*)ptr;
    if (!m) return 0;
    pthread_mutex_lock(&m->mu);
    int64_t val = m->value;
    pthread_mutex_unlock(&m->mu);
    return val;
}

/* ===================================================================
 * Channel[T]
 * =================================================================== */

typedef struct {
    pthread_mutex_t mu;
    int64_t *buffer;
    int64_t buf_len;
    int64_t buf_cap;
    int64_t capacity;
    int closed;
    GreenThread **send_waiters;
    int64_t send_wait_len;
    int64_t send_wait_cap;
    int64_t *send_values;       /* pending values for waiting senders */
    GreenThread **recv_waiters;
    int64_t recv_wait_len;
    int64_t recv_wait_cap;
} AsterChannel;

void* aster_channel_new(int64_t capacity) {
    AsterChannel *ch = (AsterChannel*)calloc(1, sizeof(AsterChannel));
    if (!ch) { fprintf(stderr, "out of memory\n"); abort(); }
    pthread_mutex_init(&ch->mu, NULL);
    ch->capacity = capacity > 0 ? capacity : 1;
    ch->buffer = (int64_t*)calloc((size_t)ch->capacity, sizeof(int64_t));
    ch->buf_len = 0;
    ch->buf_cap = ch->capacity;
    ch->closed = 0;
    return ch;
}

static void channel_wake_receiver(AsterChannel *ch) {
    if (ch->recv_wait_len > 0) {
        GreenThread *waiter = ch->recv_waiters[0];
        for (int64_t i = 1; i < ch->recv_wait_len; i++)
            ch->recv_waiters[i-1] = ch->recv_waiters[i];
        ch->recv_wait_len--;
        if (waiter) {
            pthread_mutex_lock(&waiter->mu);
            waiter->status = GT_RUNNABLE;
            pthread_mutex_unlock(&waiter->mu);
            wq_push(&worker_locals[0], waiter);
        }
    }
}

static void channel_wake_sender(AsterChannel *ch) {
    if (ch->send_wait_len > 0) {
        GreenThread *waiter = ch->send_waiters[0];
        for (int64_t i = 1; i < ch->send_wait_len; i++)
            ch->send_waiters[i-1] = ch->send_waiters[i];
        ch->send_wait_len--;
        if (waiter) {
            pthread_mutex_lock(&waiter->mu);
            waiter->status = GT_RUNNABLE;
            pthread_mutex_unlock(&waiter->mu);
            wq_push(&worker_locals[0], waiter);
        }
    }
}

void aster_channel_send(void *ptr, int64_t value) {
    AsterChannel *ch = (AsterChannel*)ptr;
    if (!ch) return;
    pthread_mutex_lock(&ch->mu);
    if (ch->closed || ch->buf_len >= ch->buf_cap) {
        /* Drop silently (fire-and-forget tier) */
        pthread_mutex_unlock(&ch->mu);
        return;
    }
    ch->buffer[ch->buf_len++] = value;
    channel_wake_receiver(ch);
    pthread_mutex_unlock(&ch->mu);
}

void aster_channel_wait_send(void *ptr, int64_t value) {
    AsterChannel *ch = (AsterChannel*)ptr;
    if (!ch) return;
    pthread_mutex_lock(&ch->mu);
    while (ch->buf_len >= ch->buf_cap && !ch->closed) {
        GreenThread *self = worker_current_thread;
        if (ch->send_wait_len >= ch->send_wait_cap) {
            int64_t new_cap = ch->send_wait_cap == 0 ? 4 : ch->send_wait_cap * 2;
            ch->send_waiters = (GreenThread**)realloc(ch->send_waiters, (size_t)(new_cap * (int64_t)sizeof(GreenThread*)));
            ch->send_wait_cap = new_cap;
        }
        ch->send_waiters[ch->send_wait_len++] = self;
        pthread_mutex_unlock(&ch->mu);
        if (self) {
            yield_to_scheduler(YIELD_WAITING_ON_CHAN_SEND);
        }
        pthread_mutex_lock(&ch->mu);
    }
    if (!ch->closed && ch->buf_len < ch->buf_cap) {
        ch->buffer[ch->buf_len++] = value;
        channel_wake_receiver(ch);
    }
    pthread_mutex_unlock(&ch->mu);
}

void aster_channel_try_send(void *ptr, int64_t value) {
    AsterChannel *ch = (AsterChannel*)ptr;
    if (!ch) { aster_error_set(); return; }
    pthread_mutex_lock(&ch->mu);
    if (ch->closed || ch->buf_len >= ch->buf_cap) {
        pthread_mutex_unlock(&ch->mu);
        aster_error_set();
        return;
    }
    ch->buffer[ch->buf_len++] = value;
    channel_wake_receiver(ch);
    pthread_mutex_unlock(&ch->mu);
}

int64_t aster_channel_receive(void *ptr) {
    AsterChannel *ch = (AsterChannel*)ptr;
    if (!ch) return 0;
    pthread_mutex_lock(&ch->mu);
    if (ch->buf_len > 0) {
        int64_t val = ch->buffer[0];
        for (int64_t i = 1; i < ch->buf_len; i++)
            ch->buffer[i-1] = ch->buffer[i];
        ch->buf_len--;
        channel_wake_sender(ch);
        pthread_mutex_unlock(&ch->mu);
        return val;
    }
    pthread_mutex_unlock(&ch->mu);
    return 0; /* nil / nullable */
}

int64_t aster_channel_wait_receive(void *ptr) {
    AsterChannel *ch = (AsterChannel*)ptr;
    if (!ch) return 0;
    pthread_mutex_lock(&ch->mu);
    while (ch->buf_len == 0 && !ch->closed) {
        GreenThread *self = worker_current_thread;
        if (ch->recv_wait_len >= ch->recv_wait_cap) {
            int64_t new_cap = ch->recv_wait_cap == 0 ? 4 : ch->recv_wait_cap * 2;
            ch->recv_waiters = (GreenThread**)realloc(ch->recv_waiters, (size_t)(new_cap * (int64_t)sizeof(GreenThread*)));
            ch->recv_wait_cap = new_cap;
        }
        ch->recv_waiters[ch->recv_wait_len++] = self;
        pthread_mutex_unlock(&ch->mu);
        if (self) {
            yield_to_scheduler(YIELD_WAITING_ON_CHAN_RECV);
        }
        pthread_mutex_lock(&ch->mu);
    }
    if (ch->buf_len > 0) {
        int64_t val = ch->buffer[0];
        for (int64_t i = 1; i < ch->buf_len; i++)
            ch->buffer[i-1] = ch->buffer[i];
        ch->buf_len--;
        channel_wake_sender(ch);
        pthread_mutex_unlock(&ch->mu);
        return val;
    }
    pthread_mutex_unlock(&ch->mu);
    if (ch->closed) aster_error_set();
    return 0;
}

int64_t aster_channel_try_receive(void *ptr) {
    AsterChannel *ch = (AsterChannel*)ptr;
    if (!ch) { aster_error_set(); return 0; }
    pthread_mutex_lock(&ch->mu);
    if (ch->buf_len > 0) {
        int64_t val = ch->buffer[0];
        for (int64_t i = 1; i < ch->buf_len; i++)
            ch->buffer[i-1] = ch->buffer[i];
        ch->buf_len--;
        channel_wake_sender(ch);
        pthread_mutex_unlock(&ch->mu);
        return val;
    }
    pthread_mutex_unlock(&ch->mu);
    aster_error_set();
    return 0;
}

void aster_channel_close(void *ptr) {
    AsterChannel *ch = (AsterChannel*)ptr;
    if (!ch) return;
    pthread_mutex_lock(&ch->mu);
    ch->closed = 1;
    /* Wake all waiters */
    while (ch->recv_wait_len > 0) channel_wake_receiver(ch);
    while (ch->send_wait_len > 0) channel_wake_sender(ch);
    pthread_mutex_unlock(&ch->mu);
}

/* ===================================================================
 * File I/O
 * =================================================================== */

/* Helper: extract C string from Aster heap string [len:i64][data:u8...] */
static char* aster_string_to_cstr(void *ptr) {
    if (!ptr) return strdup("");
    int64_t len = *(int64_t*)ptr;
    if (len <= 0) return strdup("");
    char *buf = (char*)malloc(len + 1);
    if (!buf) { fprintf(stderr, "out of memory\n"); abort(); }
    memcpy(buf, (char*)ptr + 8, len);
    buf[len] = '\0';
    return buf;
}

void* aster_file_read(void *path_ptr) {
    char *path = aster_string_to_cstr(path_ptr);
    FILE *f = fopen(path, "r");
    if (!f) { free(path); aster_error_set(); return aster_string_new(NULL, 0); }
    fseek(f, 0, SEEK_END);
    long sz = ftell(f);
    if (sz < 0) { fclose(f); free(path); aster_error_set(); return aster_string_new(NULL, 0); }
    fseek(f, 0, SEEK_SET);
    char *buf = (char*)malloc(sz);
    if (!buf) { fclose(f); free(path); aster_error_set(); return aster_string_new(NULL, 0); }
    size_t rd = fread(buf, 1, sz, f);
    fclose(f); free(path);
    void *result = aster_string_new((uint8_t*)buf, rd);
    free(buf);
    return result;
}

void aster_file_write(void *path_ptr, void *content_ptr) {
    char *path = aster_string_to_cstr(path_ptr);
    int64_t len = content_ptr ? *(int64_t*)content_ptr : 0;
    if (len < 0) len = 0;
    FILE *f = fopen(path, "w");
    if (!f) { free(path); aster_error_set(); return; }
    if (len > 0) fwrite((char*)content_ptr + 8, 1, len, f);
    fclose(f); free(path);
}

void aster_file_append(void *path_ptr, void *content_ptr) {
    char *path = aster_string_to_cstr(path_ptr);
    int64_t len = content_ptr ? *(int64_t*)content_ptr : 0;
    if (len < 0) len = 0;
    FILE *f = fopen(path, "a");
    if (!f) { free(path); aster_error_set(); return; }
    if (len > 0) fwrite((char*)content_ptr + 8, 1, len, f);
    fclose(f); free(path);
}

/* ===================================================================
 * Main entry point
 * =================================================================== */

int main(int argc, char** argv) {
    (void)argc; (void)argv;
    extern int64_t aster_main(void);
    int64_t result = aster_main();
    return (int)result;
}
"#;

pub fn c_runtime_source() -> &'static str {
    C_RUNTIME_SOURCE
}
