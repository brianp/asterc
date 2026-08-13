//! Sentinel ClassIds and object layouts for the built-in runtime error classes.
//!
//! Runtime builtins that declare `throws T` (fs I/O, process spawn, integer
//! parsing, channels, async task cancellation) construct a real error object of
//! type `T` on failure and set it as the current typed error. Just like
//! `EvalError` (see `ast::eval_error`) and the introspection classes
//! (FieldInfo/ParamInfo/MethodInfo), each such class is pre-registered in the
//! lowerer with a fixed sentinel ClassId that never collides with a
//! user-defined class. The runtime references these same constants when it
//! allocates the object, so both the AOT and JIT backends agree on the layout.
//!
//! Sentinel ClassId assignment (highest first):
//! - FieldInfo:          u32::MAX
//! - ParamInfo:          u32::MAX - 1
//! - MethodInfo:         u32::MAX - 2
//! - EvalError:          u32::MAX - 3
//! - IOError:            u32::MAX - 4
//! - ProcessError:       u32::MAX - 5
//! - IntParseError:      u32::MAX - 6
//! - ChannelFullError:   u32::MAX - 7
//! - ChannelEmptyError:  u32::MAX - 8
//! - ChannelClosedError: u32::MAX - 9
//! - LockTimeoutError:   u32::MAX - 10
//! - CancelledError:     u32::MAX - 11
//! - Frame:              u32::MAX - 12  (not an error; a stack-trace frame)

/// Shared floor for every compiler-reserved sentinel ClassId.
///
/// Any ClassId `>= SENTINEL_CLASS_ID_FLOOR` is a reserved sentinel (the
/// introspection classes and the built-in error classes) and must never be
/// remapped as a user class during cross-module merge. The guard-band checks in
/// the lowerer all key off this constant, so adding a new sentinel below the
/// current lowest only requires staying above this floor. Kept well below the
/// lowest sentinel (`u32::MAX - 11`) so more can be added without touching the
/// guard band again.
pub const SENTINEL_CLASS_ID_FLOOR: u32 = u32::MAX - 64;

pub const IO_ERROR_CLASS_ID: u32 = u32::MAX - 4;
pub const PROCESS_ERROR_CLASS_ID: u32 = u32::MAX - 5;
pub const INT_PARSE_ERROR_CLASS_ID: u32 = u32::MAX - 6;
pub const CHANNEL_FULL_ERROR_CLASS_ID: u32 = u32::MAX - 7;
pub const CHANNEL_EMPTY_ERROR_CLASS_ID: u32 = u32::MAX - 8;
pub const CHANNEL_CLOSED_ERROR_CLASS_ID: u32 = u32::MAX - 9;
pub const LOCK_TIMEOUT_ERROR_CLASS_ID: u32 = u32::MAX - 10;
pub const CANCELLED_ERROR_CLASS_ID: u32 = u32::MAX - 11;

/// Byte offset of the inherited `message: String` field, shared by every
/// built-in error class. The runtime writes the message string pointer here and
/// catch-arm field access reads `e.message` from the same offset.
pub const BUILTIN_ERROR_MESSAGE_OFFSET: usize = 0;

/// A built-in error carrying only `message` occupies a single pointer field.
pub const MESSAGE_ONLY_SIZE: usize = 8;

/// Number of leading GC-traceable pointer fields for a message-only error.
pub const MESSAGE_ONLY_PTR_COUNT: i64 = 1;

/// Byte offset of `ProcessError.command` (own field, follows `message`).
pub const PROCESS_ERROR_COMMAND_OFFSET: usize = 8;

/// `ProcessError` carries `message` + `command`, two pointer fields.
pub const PROCESS_ERROR_SIZE: usize = 16;

/// Number of leading GC-traceable pointer fields for `ProcessError`.
pub const PROCESS_ERROR_PTR_COUNT: i64 = 2;

// ---------------------------------------------------------------------------
// Frame — a single stack-trace frame produced by `error.trace()`.
//
// Not an error class, but a compiler-generated built-in class registered the
// same way (fixed sentinel ClassId + fixed layout shared by the runtime that
// constructs it and the lowerer that resolves `frame.function` / `frame.file`
// / `frame.line`). Pointer fields come first so the GC traces them.
// ---------------------------------------------------------------------------

/// Sentinel ClassId for the `Frame` built-in class.
pub const FRAME_CLASS_ID: u32 = u32::MAX - 12;

/// `Frame.function: String` — offset 0 (pointer).
pub const FRAME_FUNCTION_OFFSET: usize = 0;

/// `Frame.file: String` — offset 8 (pointer).
pub const FRAME_FILE_OFFSET: usize = 8;

/// `Frame.line: Int` — offset 16 (value).
pub const FRAME_LINE_OFFSET: usize = 16;

/// A `Frame` occupies two pointer fields (`function`, `file`) followed by one
/// integer field (`line`): 24 bytes total.
pub const FRAME_SIZE: usize = 24;

/// Number of leading GC-traceable pointer fields for a `Frame`.
pub const FRAME_PTR_COUNT: i64 = 2;
