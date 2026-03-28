//! Single-source runtime function signature table.
//!
//! The `runtime_functions!` macro defines every runtime function once.
//! It expands to both the Cranelift signature table (`RUNTIME_SIGS`)
//! used by the codegen to declare imports, and the JIT symbol table
//! (`runtime_builtin_symbols`) used to register function pointers.

use cranelift_codegen::ir::Type;
use cranelift_codegen::ir::types;

/// A runtime function signature: (name, param types, optional return type).
pub type RuntimeSig = (&'static str, &'static [Type], Option<Type>);

/// Map type shorthand tokens to Cranelift IR types.
macro_rules! clif_ty {
    (I64) => {
        types::I64
    };
    (F64) => {
        types::F64
    };
    (I8) => {
        types::I8
    };
    (I32) => {
        types::I32
    };
}

/// Define all runtime functions in one place. Expands to:
/// - `RUNTIME_SIGS`: `&[RuntimeSig]` for Cranelift import declarations
/// - `runtime_builtin_symbols()`: `Vec<(&str, *const u8)>` for JIT registration
macro_rules! runtime_functions {
    (
        $(
            fn $name:ident ( $($pty:ident),* ) $(-> $ret:ident)?;
        )*
    ) => {
        /// All runtime functions declared in both JIT and AOT modules.
        pub static RUNTIME_SIGS: &[RuntimeSig] = &[
            $(
                (
                    stringify!($name),
                    &[$(clif_ty!($pty)),*],
                    runtime_functions!(@ret $($ret)?),
                ),
            )*
        ];

        /// Return (name, function-pointer) pairs for JIT symbol registration.
        pub fn runtime_builtin_symbols() -> Vec<(&'static str, *const u8)> {
            use super::runtime::*;
            vec![
                $(
                    (stringify!($name), $name as *const u8),
                )*
            ]
        }
    };

    // Helper: optional return type
    (@ret) => { None };
    (@ret $ret:ident) => { Some(clif_ty!($ret)) };
}

runtime_functions! {
    // Allocation
    fn aster_alloc(I64) -> I64;
    // Printing
    fn aster_say_str(I64);
    fn aster_say_int(I64);
    fn aster_say_float(F64);
    fn aster_say_bool(I8);
    // String operations
    fn aster_string_new(I64, I64) -> I64;
    fn aster_string_concat(I64, I64) -> I64;
    fn aster_string_len(I64) -> I64;
    fn aster_string_char_len(I64) -> I64;
    fn aster_string_contains(I64, I64) -> I8;
    fn aster_string_starts_with(I64, I64) -> I8;
    fn aster_string_ends_with(I64, I64) -> I8;
    fn aster_string_trim(I64) -> I64;
    fn aster_string_to_upper(I64) -> I64;
    fn aster_string_to_lower(I64) -> I64;
    fn aster_string_slice(I64, I64, I64) -> I64;
    fn aster_string_replace(I64, I64, I64) -> I64;
    fn aster_string_split(I64, I64) -> I64;
    fn aster_string_eq(I64, I64) -> I8;
    fn aster_string_compare(I64, I64) -> I64;
    // List operations
    fn aster_list_new(I64, I64) -> I64;
    fn aster_list_get(I64, I64) -> I64;
    fn aster_list_random(I64) -> I64;
    fn aster_list_set(I64, I64, I64);
    fn aster_list_push(I64, I64) -> I64;
    fn aster_list_len(I64) -> I64;
    fn aster_list_insert(I64, I64, I64);
    fn aster_list_remove(I64, I64) -> I64;
    fn aster_list_pop(I64) -> I64;
    fn aster_list_contains(I64, I64, I64) -> I8;
    // Class / closure allocation
    fn aster_class_alloc(I64) -> I64;
    fn aster_class_alloc_typed(I64, I64) -> I64;
    fn aster_closure_alloc(I64) -> I64;
    // Integer arithmetic (checked overflow)
    fn aster_int_add(I64, I64) -> I64;
    fn aster_int_sub(I64, I64) -> I64;
    fn aster_int_mul(I64, I64) -> I64;
    fn aster_pow_int(I64, I64) -> I64;
    fn aster_pow_float(F64, F64) -> F64;
    // Int numeric methods
    fn aster_int_is_even(I64) -> I8;
    fn aster_int_is_odd(I64) -> I8;
    fn aster_int_abs(I64) -> I64;
    fn aster_int_clamp(I64, I64, I64) -> I64;
    fn aster_int_min(I64, I64) -> I64;
    fn aster_int_max(I64, I64) -> I64;
    // Float numeric methods
    fn aster_float_abs(F64) -> F64;
    fn aster_float_round(F64) -> I64;
    fn aster_float_floor(F64) -> I64;
    fn aster_float_ceil(F64) -> I64;
    fn aster_float_clamp(F64, F64, F64) -> F64;
    fn aster_float_min(F64, F64) -> F64;
    fn aster_float_max(F64, F64) -> F64;
    // String conversions
    fn aster_int_to_string(I64) -> I64;
    fn aster_float_to_string(F64) -> I64;
    fn aster_bool_to_string(I8) -> I64;
    fn aster_list_to_string(I64) -> I64;
    // Map operations
    fn aster_map_new(I64) -> I64;
    fn aster_map_set(I64, I64, I64) -> I64;
    fn aster_map_get(I64, I64) -> I64;
    fn aster_map_has_key(I64, I64) -> I64;
    // Error handling
    fn aster_error_set();
    fn aster_error_set_typed(I64, I64);
    fn aster_error_check() -> I8;
    fn aster_error_get_tag() -> I64;
    fn aster_error_get_value() -> I64;
    fn aster_safepoint();
    fn aster_panic();
    // Async scope
    fn aster_async_scope_enter() -> I64;
    fn aster_async_scope_exit(I64);
    // Task operations
    fn aster_task_spawn(I64, I64, I64) -> I64;
    fn aster_task_block_on(I64, I64) -> I64;
    fn aster_task_from_i64(I64, I8) -> I64;
    fn aster_task_from_f64(F64, I8) -> I64;
    fn aster_task_from_i8(I8, I8) -> I64;
    fn aster_task_is_ready(I64) -> I8;
    fn aster_task_cancel(I64) -> I64;
    fn aster_task_wait_cancel(I64) -> I64;
    fn aster_task_resolve_i64(I64) -> I64;
    fn aster_task_resolve_f64(I64) -> F64;
    fn aster_task_resolve_i8(I64) -> I8;
    fn aster_task_resolve_all_i64(I64) -> I64;
    fn aster_task_resolve_first_i64(I64) -> I64;
    // GC
    fn aster_gc_push_roots(I64, I64);
    fn aster_gc_pop_roots();
    fn aster_gc_collect();
    // I/O suspension
    fn aster_io_wait_read(I32);
    fn aster_io_wait_write(I32);
    fn aster_blocking_submit(I64, I64);
    // Mutex
    fn aster_mutex_new(I64) -> I64;
    fn aster_mutex_lock(I64) -> I64;
    fn aster_mutex_unlock(I64, I64);
    fn aster_mutex_get_value(I64) -> I64;
    // Channel
    fn aster_channel_new(I64) -> I64;
    fn aster_channel_send(I64, I64);
    fn aster_channel_wait_send(I64, I64);
    fn aster_channel_try_send(I64, I64);
    fn aster_channel_receive(I64) -> I64;
    fn aster_channel_wait_receive(I64) -> I64;
    fn aster_channel_try_receive(I64) -> I64;
    fn aster_channel_close(I64);
    // File I/O
    fn aster_file_read(I64) -> I64;
    fn aster_file_write(I64, I64);
    fn aster_file_append(I64, I64);
    // Range
    fn aster_range_new(I64, I64, I8) -> I64;
    fn aster_range_check(I64, I64, I8) -> I8;
    // Random
    fn aster_random_int(I64) -> I64;
    fn aster_random_float(F64) -> F64;
    fn aster_random_bool() -> I8;
    // Introspection
    fn aster_introspect_class_name(I64) -> I64;
    fn aster_introspect_fields(I64) -> I64;
    fn aster_introspect_methods(I64) -> I64;
    fn aster_introspect_ancestors(I64) -> I64;
    fn aster_introspect_children(I64) -> I64;
    fn aster_introspect_is_a(I64, I64) -> I8;
    fn aster_introspect_responds_to(I64, I64) -> I8;
}
