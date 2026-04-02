use serde::{Deserialize, Serialize};

/// Serialized context snapshot embedded in a FIR module for `evaluate()` call sites.
///
/// Each entry corresponds to one `evaluate()` call and carries the JSON-serialized
/// [`ast::ContextSnapshot`] that the runtime JIT compiler needs to reconstruct
/// the typechecker environment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalContext {
    /// JSON-serialized `ContextSnapshot`.
    pub snapshot_json: Vec<u8>,
}
