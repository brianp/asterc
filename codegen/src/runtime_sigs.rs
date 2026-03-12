//! Shared runtime function signature table for JIT and AOT backends.

use cranelift_codegen::ir::Type;
use cranelift_codegen::ir::types;

/// A runtime function signature: (name, param types, optional return type).
pub type RuntimeSig = (&'static str, &'static [Type], Option<Type>);

/// All runtime functions that must be declared in both JIT and AOT modules.
pub static RUNTIME_SIGS: &[RuntimeSig] = &[
    ("aster_alloc", &[types::I64], Some(types::I64)),
    ("aster_print_str", &[types::I64], None),
    ("aster_print_int", &[types::I64], None),
    ("aster_print_float", &[types::F64], None),
    ("aster_print_bool", &[types::I8], None),
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
    ("aster_list_new", &[types::I64], Some(types::I64)),
    (
        "aster_list_get",
        &[types::I64, types::I64],
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
    ("aster_map_new", &[types::I64], Some(types::I64)),
    (
        "aster_map_set",
        &[types::I64, types::I64, types::I64],
        Some(types::I64),
    ),
    ("aster_map_get", &[types::I64, types::I64], Some(types::I64)),
    ("aster_error_set", &[], None),
    ("aster_error_check", &[], Some(types::I8)),
    ("aster_panic", &[], None),
];
