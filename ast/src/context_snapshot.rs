use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::types::Type;

/// Snapshot of the type-level context at an `evaluate()` call site.
///
/// Captured during FIR lowering and serialized into the FIR module so the
/// runtime JIT compiler can reconstruct a valid typechecker environment
/// for the evaluated code string.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContextSnapshot {
    /// Class name if `evaluate()` is called inside a class method.
    pub current_class: Option<String>,
    /// Class field and method info (present when `current_class` is set).
    pub class_info: Option<SnapshotClassInfo>,
    /// Local variable types visible at the call site.
    pub variables: HashMap<String, Type>,
    /// Available function signatures in scope (name -> function type).
    pub functions: HashMap<String, Type>,
    /// Ordered layout of captured variables for the runtime env struct.
    /// Each entry is (variable_name, type), stored at offset `i * 8`.
    /// `None` means no env is passed (e.g. snapshot used only for typechecking).
    pub env_layout: Option<Vec<(String, Type)>>,
}

/// Subset of class metadata needed for JIT typechecking of evaluated code.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SnapshotClassInfo {
    /// Field names and types in declaration order.
    pub fields: Vec<(String, Type)>,
    /// Method signatures (name -> function type).
    pub methods: HashMap<String, Type>,
    /// DynamicReceiver info, if the class includes DynamicReceiver.
    pub dynamic_receiver: Option<SnapshotDynamicReceiver>,
}

/// Serializable subset of [`DynamicReceiverInfo`] for the context snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SnapshotDynamicReceiver {
    /// The Map value type from method_missing's second parameter.
    pub args_value_ty: Type,
    /// The return type of method_missing.
    pub return_ty: Type,
    /// Known dynamic method names (closed set), or None for open dispatch.
    pub known_names: Option<Vec<String>>,
}
