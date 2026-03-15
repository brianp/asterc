pub const C_RUNTIME_SOURCE: &str = r#"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <pthread.h>
#include <sched.h>

void* aster_alloc(int64_t size) {
    if (size == 0) return (void*)8; /* aligned dangling */
    if (size < 0) { fprintf(stderr, "aster_alloc: negative size\n"); abort(); }
    void* p = malloc((size_t)size);
    if (!p) { fprintf(stderr, "out of memory\n"); abort(); }
    return p;
}

void* aster_class_alloc(int64_t size) { return aster_alloc(size); }

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

/* List handle indirection: list value = handle (ptr to ptr to data block).
   Data block: [len: i64][cap: i64][data: i64...] */

void* aster_list_new(int64_t cap) {
    if (cap < 4) cap = 4;
    void* block = aster_alloc(16 + cap * 8);
    *(int64_t*)block = 0;              /* len */
    *((int64_t*)block + 1) = cap;     /* cap */
    void* handle = aster_alloc(8);
    *(void**)handle = block;
    return handle;
}

int64_t aster_list_get(void* handle, int64_t index) {
    if (!handle) { fprintf(stderr, "aster_list_get: null list\n"); abort(); }
    void* block = *(void**)handle;
    int64_t len = *(int64_t*)block;
    if (index < 0 || index >= len) {
        fprintf(stderr, "list index out of bounds: %lld (len %lld)\n", (long long)index, (long long)len);
        abort();
    }
    return *((int64_t*)block + 2 + index);
}

void aster_list_set(void* handle, int64_t index, int64_t value) {
    if (!handle) { fprintf(stderr, "aster_list_set: null list\n"); abort(); }
    void* block = *(void**)handle;
    int64_t len = *(int64_t*)block;
    if (index < 0 || index >= len) {
        fprintf(stderr, "list index out of bounds: %lld (len %lld)\n", (long long)index, (long long)len);
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

static int aster_string_eq(void* a, void* b) {
    if (a == b) return 1;
    if (!a || !b) return 0;
    int64_t a_len = *(int64_t*)a;
    int64_t b_len = *(int64_t*)b;
    if (a_len != b_len || a_len < 0) return 0;
    return memcmp((char*)a + 8, (char*)b + 8, (size_t)a_len) == 0;
}

/* Map handle indirection: map value = handle (ptr to ptr to data block).
   Data block: [len: i64][cap: i64][entries: [key: i64, val: i64]...] */

void* aster_map_new(int64_t cap) {
    if (cap < 4) cap = 4;
    void* block = aster_alloc(16 + cap * 16);
    *(int64_t*)block = 0;              /* len */
    *((int64_t*)block + 1) = cap;     /* cap */
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

void aster_safepoint(void) {}

typedef struct AsterTask AsterTask;
typedef struct AsterAsyncScope AsterAsyncScope;

struct AsterTask {
    pthread_mutex_t mu;
    pthread_cond_t cv;
    int64_t state;
    int64_t consumed;
    int64_t payload;
    int64_t cancel_requested;
    int64_t entry_ptr;
    int64_t args_ptr;
};

struct AsterAsyncScope {
    pthread_mutex_t mu;
    int64_t len;
    int64_t cap;
    AsterTask** tasks;
};

enum {
    ASTER_TASK_QUEUED = 0,
    ASTER_TASK_RUNNING = 1,
    ASTER_TASK_READY = 2,
    ASTER_TASK_FAILED = 3,
    ASTER_TASK_CANCELLED = 4
};

static int64_t aster_task_wait_terminal(AsterTask* task) {
    pthread_mutex_lock(&task->mu);
    while (task->state == ASTER_TASK_QUEUED || task->state == ASTER_TASK_RUNNING) {
        pthread_cond_wait(&task->cv, &task->mu);
    }
    int64_t state = task->state;
    pthread_mutex_unlock(&task->mu);
    return state;
}

static void* aster_task_runner(void* raw_task) {
    AsterTask* task = (AsterTask*)raw_task;
    pthread_mutex_lock(&task->mu);
    if (task->cancel_requested) {
        task->state = ASTER_TASK_CANCELLED;
        pthread_cond_broadcast(&task->cv);
        pthread_mutex_unlock(&task->mu);
        return 0;
    }
    task->state = ASTER_TASK_RUNNING;
    int64_t entry_ptr = task->entry_ptr;
    int64_t args_ptr = task->args_ptr;
    pthread_mutex_unlock(&task->mu);

    int64_t (*entry)(const void*) = (int64_t (*)(const void*))entry_ptr;
    aster_error_check();
    int64_t value = entry((const void*)args_ptr);
    int8_t failed = aster_error_check();

    pthread_mutex_lock(&task->mu);
    if (task->cancel_requested) {
        task->state = ASTER_TASK_CANCELLED;
    } else if (failed) {
        task->state = ASTER_TASK_FAILED;
        task->payload = value;
    } else {
        task->state = ASTER_TASK_READY;
        task->payload = value;
    }
    pthread_cond_broadcast(&task->cv);
    pthread_mutex_unlock(&task->mu);
    return 0;
}

static void aster_scope_register(AsterAsyncScope* scope, AsterTask* task) {
    if (!scope) return;
    pthread_mutex_lock(&scope->mu);
    if (scope->len >= scope->cap) {
        int64_t next_cap = scope->cap == 0 ? 4 : scope->cap * 2;
        AsterTask** next = (AsterTask**)realloc(scope->tasks, (size_t)(next_cap * (int64_t)sizeof(AsterTask*)));
        if (!next) {
            fprintf(stderr, "out of memory\n");
            abort();
        }
        scope->tasks = next;
        scope->cap = next_cap;
    }
    scope->tasks[scope->len++] = task;
    pthread_mutex_unlock(&scope->mu);
}

void* aster_async_scope_enter(void) {
    AsterAsyncScope* scope = (AsterAsyncScope*)aster_alloc((int64_t)sizeof(AsterAsyncScope));
    pthread_mutex_init(&scope->mu, 0);
    scope->len = 0;
    scope->cap = 0;
    scope->tasks = 0;
    return scope;
}

void aster_async_scope_exit(void* scope_ptr) {
    if (!scope_ptr) return;
    AsterAsyncScope* scope = (AsterAsyncScope*)scope_ptr;
    pthread_mutex_lock(&scope->mu);
    int64_t len = scope->len;
    AsterTask** tasks = scope->tasks;
    scope->len = 0;
    scope->cap = 0;
    scope->tasks = 0;
    pthread_mutex_unlock(&scope->mu);

    for (int64_t i = 0; i < len; i++) {
        AsterTask* task = tasks[i];
        if (!task) continue;
        pthread_mutex_lock(&task->mu);
        task->cancel_requested = 1;
        if (task->state == ASTER_TASK_QUEUED) {
            task->state = ASTER_TASK_CANCELLED;
            pthread_cond_broadcast(&task->cv);
        }
        pthread_mutex_unlock(&task->mu);
    }
    for (int64_t i = 0; i < len; i++) {
        if (tasks[i]) aster_task_wait_terminal(tasks[i]);
    }
    free(tasks);
}

static void* aster_task_new_raw(int64_t payload, int8_t failed) {
    AsterTask* task = (AsterTask*)aster_alloc((int64_t)sizeof(AsterTask));
    pthread_mutex_init(&task->mu, 0);
    pthread_cond_init(&task->cv, 0);
    task->state = failed ? ASTER_TASK_FAILED : ASTER_TASK_READY;
    task->consumed = 0;
    task->payload = payload;
    task->cancel_requested = 0;
    task->entry_ptr = 0;
    task->args_ptr = 0;
    return task;
}

int64_t aster_task_spawn(int64_t entry_ptr, int64_t args_ptr, int64_t scope_ptr) {
    AsterTask* task = (AsterTask*)aster_alloc((int64_t)sizeof(AsterTask));
    pthread_mutex_init(&task->mu, 0);
    pthread_cond_init(&task->cv, 0);
    task->state = ASTER_TASK_QUEUED;
    task->consumed = 0;
    task->payload = 0;
    task->cancel_requested = 0;
    task->entry_ptr = entry_ptr;
    task->args_ptr = args_ptr;
    aster_scope_register((AsterAsyncScope*)scope_ptr, task);

    pthread_t thread;
    pthread_attr_t attr;
    pthread_attr_init(&attr);
    pthread_attr_setdetachstate(&attr, PTHREAD_CREATE_DETACHED);
    if (pthread_create(&thread, &attr, aster_task_runner, task) != 0) {
        pthread_attr_destroy(&attr);
        fprintf(stderr, "failed to create task thread\n");
        abort();
    }
    pthread_attr_destroy(&attr);
    return (int64_t)task;
}

int64_t aster_task_block_on(int64_t entry_ptr, int64_t args_ptr) {
    AsterTask* task = (AsterTask*)aster_task_spawn(entry_ptr, args_ptr, 0);
    int64_t state = aster_task_wait_terminal(task);
    if (state == ASTER_TASK_READY) {
        return task->payload;
    }
    aster_error_set();
    return 0;
}

void* aster_task_from_i64(int64_t value, int8_t failed) {
    return aster_task_new_raw(value, failed);
}

void* aster_task_from_f64(double value, int8_t failed) {
    int64_t bits = 0;
    memcpy(&bits, &value, sizeof(bits));
    return aster_task_new_raw(bits, failed);
}

void* aster_task_from_i8(int8_t value, int8_t failed) {
    return aster_task_new_raw((int64_t)value, failed);
}

int8_t aster_task_is_ready(void* task_ptr) {
    if (!task_ptr) return 0;
    AsterTask* task = (AsterTask*)task_ptr;
    pthread_mutex_lock(&task->mu);
    int8_t ready = task->state != ASTER_TASK_QUEUED && task->state != ASTER_TASK_RUNNING;
    pthread_mutex_unlock(&task->mu);
    return ready;
}

int64_t aster_task_cancel(void* task_ptr) {
    if (!task_ptr) return 0;
    AsterTask* task = (AsterTask*)task_ptr;
    pthread_mutex_lock(&task->mu);
    task->cancel_requested = 1;
    if (task->state == ASTER_TASK_QUEUED) {
        task->state = ASTER_TASK_CANCELLED;
        pthread_cond_broadcast(&task->cv);
    }
    pthread_mutex_unlock(&task->mu);
    return 0;
}

int64_t aster_task_wait_cancel(void* task_ptr) {
    aster_task_cancel(task_ptr);
    if (task_ptr) aster_task_wait_terminal((AsterTask*)task_ptr);
    return 0;
}

static int64_t aster_task_consume_payload(void* task_ptr) {
    if (!task_ptr) {
        aster_error_set();
        return 0;
    }
    AsterTask* task = (AsterTask*)task_ptr;
    int64_t state = aster_task_wait_terminal(task);
    pthread_mutex_lock(&task->mu);
    if (task->consumed) {
        pthread_mutex_unlock(&task->mu);
        aster_error_set();
        return 0;
    }
    task->consumed = 1;
    if (state == ASTER_TASK_READY) {
        int64_t payload = task->payload;
        pthread_mutex_unlock(&task->mu);
        return payload;
    }
    pthread_mutex_unlock(&task->mu);
    aster_error_set();
    return 0;
}

int64_t aster_task_resolve_i64(void* task_ptr) {
    return aster_task_consume_payload(task_ptr);
}

double aster_task_resolve_f64(void* task_ptr) {
    int64_t bits = aster_task_consume_payload(task_ptr);
    double value = 0.0;
    memcpy(&value, &bits, sizeof(value));
    return value;
}

int8_t aster_task_resolve_i8(void* task_ptr) {
    return (int8_t)aster_task_consume_payload(task_ptr);
}

void* aster_task_resolve_all_i64(void* tasks) {
    if (!tasks) {
        aster_error_set();
        return 0;
    }
    int64_t len = aster_list_len(tasks);
    void* out = aster_list_new(len);
    for (int64_t i = 0; i < len; i++) {
        int64_t task = aster_list_get(tasks, i);
        int64_t value = aster_task_resolve_i64((void*)task);
        if (aster_error_flag) return out;
        aster_list_push(out, value);
    }
    return out;
}

int64_t aster_task_resolve_first_i64(void* tasks) {
    if (!tasks) {
        aster_error_set();
        return 0;
    }
    int64_t len = aster_list_len(tasks);
    if (len == 0) {
        aster_error_set();
        return 0;
    }
    AsterTask* winner = 0;
    int64_t winner_index = -1;
    for (;;) {
        for (int64_t i = 0; i < len; i++) {
            AsterTask* task = (AsterTask*)aster_list_get(tasks, i);
            pthread_mutex_lock(&task->mu);
            int64_t state = task->state;
            pthread_mutex_unlock(&task->mu);
            if (state == ASTER_TASK_READY || state == ASTER_TASK_FAILED || state == ASTER_TASK_CANCELLED) {
                winner = task;
                winner_index = i;
                break;
            }
        }
        if (winner) break;
        sched_yield();
    }
    for (int64_t i = 0; i < len; i++) {
        if (i != winner_index) {
            aster_task_cancel((void*)aster_list_get(tasks, i));
        }
    }
    return aster_task_resolve_i64((void*)winner);
}

// GC stubs, the JIT runtime has real GC; the C AOT runtime
// uses a simple no-op strategy (OS reclaims on exit).
void aster_gc_push_roots(int64_t frame_addr, int64_t count) {
    (void)frame_addr; (void)count;
}
void aster_gc_pop_roots(void) {}
void aster_gc_collect(void) {}

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
