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
}

/// Subset of class metadata needed for JIT typechecking of evaluated code.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SnapshotClassInfo {
    /// Field names and types in declaration order.
    pub fields: Vec<(String, Type)>,
    /// Method signatures (name -> function type).
    pub methods: HashMap<String, Type>,
    /// Whether this class includes DynamicReceiver.
    pub has_dynamic_receiver: bool,
}
