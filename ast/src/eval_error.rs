use crate::types::Type;

/// Sentinel ClassId for the built-in `EvalError` class.
///
/// Follows the existing sentinel pattern:
/// - FieldInfo:  u32::MAX
/// - ParamInfo:  u32::MAX - 1
/// - MethodInfo: u32::MAX - 2
/// - EvalError:  u32::MAX - 3
pub const EVAL_ERROR_CLASS_ID: u32 = u32::MAX - 3;

/// Number of pointer fields in EvalError (kind, message). Both are String (heap ptr).
pub const EVAL_ERROR_PTR_COUNT: i64 = 2;

/// Size of an EvalError instance in bytes (2 pointer-sized fields).
pub const EVAL_ERROR_SIZE: usize = 2 * 8;

/// Field offset for `kind: String` (bytes from object start).
pub const EVAL_ERROR_KIND_OFFSET: usize = 0;

/// Field offset for `message: String` (bytes from object start).
pub const EVAL_ERROR_MESSAGE_OFFSET: usize = 8;

/// The AST `Type` for EvalError.
pub fn eval_error_type() -> Type {
    Type::Custom("EvalError".into(), Vec::new())
}
