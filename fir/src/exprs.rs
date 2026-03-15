use serde::{Deserialize, Serialize};

use crate::types::{ClassId, FirType, FunctionId, LocalId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Neq,
    Lt,
    Gt,
    Lte,
    Gte,
    And,
    Or,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FirExpr {
    IntLit(i64),
    FloatLit(f64),
    BoolLit(bool),
    StringLit(String),
    NilLit,
    LocalVar(LocalId, FirType),
    BinaryOp {
        left: Box<FirExpr>,
        op: BinOp,
        right: Box<FirExpr>,
        result_ty: FirType,
    },
    UnaryOp {
        op: UnaryOp,
        operand: Box<FirExpr>,
        result_ty: FirType,
    },
    Call {
        func: FunctionId,
        args: Vec<FirExpr>,
        ret_ty: FirType,
    },
    Spawn {
        func: FunctionId,
        args: Vec<FirExpr>,
        ret_ty: FirType,
        result_ty: FirType,
        scope: Option<LocalId>,
    },
    BlockOn {
        func: FunctionId,
        args: Vec<FirExpr>,
        ret_ty: FirType,
    },
    ResolveTask {
        task: Box<FirExpr>,
        ret_ty: FirType,
    },
    CancelTask {
        task: Box<FirExpr>,
    },
    WaitCancel {
        task: Box<FirExpr>,
    },
    Safepoint,
    FieldGet {
        object: Box<FirExpr>,
        offset: usize,
        ty: FirType,
    },
    FieldSet {
        object: Box<FirExpr>,
        offset: usize,
        value: Box<FirExpr>,
    },
    Construct {
        class: ClassId,
        fields: Vec<FirExpr>,
        ty: FirType,
    },
    ListNew {
        elements: Vec<FirExpr>,
        elem_ty: FirType,
    },
    ListGet {
        list: Box<FirExpr>,
        index: Box<FirExpr>,
        elem_ty: FirType,
    },
    ListSet {
        list: Box<FirExpr>,
        index: Box<FirExpr>,
        value: Box<FirExpr>,
    },
    /// Tagged union construction (nullable wrap, result wrap).
    TagWrap {
        tag: u8,
        value: Box<FirExpr>,
        ty: FirType,
    },
    /// Tagged union unwrap (nullable unwrap, result unwrap).
    TagUnwrap {
        value: Box<FirExpr>,
        expected_tag: u8,
        ty: FirType,
    },
    /// Tagged union tag check.
    TagCheck {
        value: Box<FirExpr>,
        tag: u8,
    },
    /// Runtime function (print, alloc, string ops, etc.).
    RuntimeCall {
        name: String,
        args: Vec<FirExpr>,
        ret_ty: FirType,
    },
    /// Create a closure: pairs a function ID with an environment pointer.
    /// Layout: [func_id_as_ptr: i64][env_ptr: i64]
    ClosureCreate {
        func: FunctionId,
        env: Box<FirExpr>,
        ret_ty: FirType,
    },
    /// Call a closure (indirect call through closure struct).
    ClosureCall {
        closure: Box<FirExpr>,
        args: Vec<FirExpr>,
        ret_ty: FirType,
    },
    /// Load a captured variable from the environment pointer.
    EnvLoad {
        env: Box<FirExpr>,
        offset: usize,
        ty: FirType,
    },
    /// Get the function ID stored in a global slot (for closure indirect calls).
    GlobalFunc(FunctionId),
}
