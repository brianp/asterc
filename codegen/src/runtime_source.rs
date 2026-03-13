pub const C_RUNTIME_SOURCE: &str = r#"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>

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

static int aster_error_flag = 0;

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
