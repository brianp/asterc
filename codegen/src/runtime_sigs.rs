//! Shared runtime function signature table for JIT and AOT backends.

use cranelift_codegen::ir::Type;
use cranelift_codegen::ir::types;

/// A runtime function signature: (name, param types, optional return type).
pub type RuntimeSig = (&'static str, &'static [Type], Option<Type>);

/// All runtime functions that must be declared in both JIT and AOT modules.
pub static RUNTIME_SIGS: &[RuntimeSig] = &[
    ("aster_alloc", &[types::I64], Some(types::I64)),
    ("aster_say_str", &[types::I64], None),
    ("aster_say_int", &[types::I64], None),
    ("aster_say_float", &[types::F64], None),
    ("aster_say_bool", &[types::I8], None),
    (
        "aster_string_new",
        &[types::I64, types::I64],
        Some(types::I64),
    ),
    (
        "aster_string_concat",
        &[types::I64, types::I64],
        Some(types::I64),
    ),
    ("aster_string_len", &[types::I64], Some(types::I64)),
    ("aster_list_new", &[types::I64, types::I64], Some(types::I64)),
    (
        "aster_list_get",
        &[types::I64, types::I64],
        Some(types::I64),
    ),
    (
        "aster_list_random",
        &[types::I64],
        Some(types::I64),
    ),
    (
        "aster_list_set",
        &[types::I64, types::I64, types::I64],
        None,
    ),
    (
        "aster_list_push",
        &[types::I64, types::I64],
        Some(types::I64),
    ),
    ("aster_list_len", &[types::I64], Some(types::I64)),
    ("aster_class_alloc", &[types::I64], Some(types::I64)),
    ("aster_pow_int", &[types::I64, types::I64], Some(types::I64)),
    ("aster_int_to_string", &[types::I64], Some(types::I64)),
    ("aster_float_to_string", &[types::F64], Some(types::I64)),
    ("aster_bool_to_string", &[types::I8], Some(types::I64)),
    ("aster_list_to_string", &[types::I64], Some(types::I64)),
    ("aster_map_new", &[types::I64], Some(types::I64)),
    (
        "aster_map_set",
        &[types::I64, types::I64, types::I64],
        Some(types::I64),
    ),
    ("aster_map_get", &[types::I64, types::I64], Some(types::I64)),
    ("aster_error_set", &[], None),
    ("aster_error_check", &[], Some(types::I8)),
    ("aster_safepoint", &[], None),
    ("aster_panic", &[], None),
    ("aster_async_scope_enter", &[], Some(types::I64)),
    ("aster_async_scope_exit", &[types::I64], None),
    (
        "aster_task_spawn",
        &[types::I64, types::I64, types::I64],
        Some(types::I64),
    ),
    (
        "aster_task_block_on",
        &[types::I64, types::I64],
        Some(types::I64),
    ),
    (
        "aster_task_from_i64",
        &[types::I64, types::I8],
        Some(types::I64),
    ),
    (
        "aster_task_from_f64",
        &[types::F64, types::I8],
        Some(types::I64),
    ),
    (
        "aster_task_from_i8",
        &[types::I8, types::I8],
        Some(types::I64),
    ),
    ("aster_task_is_ready", &[types::I64], Some(types::I8)),
    ("aster_task_cancel", &[types::I64], Some(types::I64)),
    ("aster_task_wait_cancel", &[types::I64], Some(types::I64)),
    ("aster_task_resolve_i64", &[types::I64], Some(types::I64)),
    ("aster_task_resolve_f64", &[types::I64], Some(types::F64)),
    ("aster_task_resolve_i8", &[types::I64], Some(types::I8)),
    (
        "aster_task_resolve_all_i64",
        &[types::I64],
        Some(types::I64),
    ),
    (
        "aster_task_resolve_first_i64",
        &[types::I64],
        Some(types::I64),
    ),
    ("aster_gc_push_roots", &[types::I64, types::I64], None),
    ("aster_gc_pop_roots", &[], None),
    ("aster_gc_collect", &[], None),
    // I/O suspension hooks (Phase 5)
    ("aster_io_wait_read", &[types::I32], None),
    ("aster_io_wait_write", &[types::I32], None),
    ("aster_blocking_submit", &[types::I64, types::I64], None),
    // Mutex[T] (Phase 7)
    ("aster_mutex_new", &[types::I64], Some(types::I64)),
    ("aster_mutex_lock", &[types::I64], Some(types::I64)),
    ("aster_mutex_unlock", &[types::I64, types::I64], None),
    ("aster_mutex_get_value", &[types::I64], Some(types::I64)),
    // Channel[T] (Phase 8)
    ("aster_channel_new", &[types::I64], Some(types::I64)),
    ("aster_channel_send", &[types::I64, types::I64], None),
    ("aster_channel_wait_send", &[types::I64, types::I64], None),
    ("aster_channel_try_send", &[types::I64, types::I64], None),
    ("aster_channel_receive", &[types::I64], Some(types::I64)),
    (
        "aster_channel_wait_receive",
        &[types::I64],
        Some(types::I64),
    ),
    ("aster_channel_try_receive", &[types::I64], Some(types::I64)),
    ("aster_channel_close", &[types::I64], None),
    // File I/O
    ("aster_file_read", &[types::I64], Some(types::I64)),
    ("aster_file_write", &[types::I64, types::I64], None),
    ("aster_file_append", &[types::I64, types::I64], None),
    // Range
    (
        "aster_range_new",
        &[types::I64, types::I64, types::I8],
        Some(types::I64),
    ),
    (
        "aster_range_check",
        &[types::I64, types::I64, types::I8],
        Some(types::I8),
    ),
    // Random
    ("aster_random_int", &[types::I64], Some(types::I64)),
    ("aster_random_float", &[types::F64], Some(types::F64)),
    ("aster_random_bool", &[], Some(types::I8)),
];
