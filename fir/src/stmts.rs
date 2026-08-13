use serde::{Deserialize, Serialize};

use crate::exprs::FirExpr;
use crate::types::{FirType, LocalId};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FirStmt {
    Let {
        name: LocalId,
        ty: FirType,
        value: FirExpr,
    },
    Assign {
        target: FirPlace,
        value: FirExpr,
    },
    Return(FirExpr),
    If {
        cond: FirExpr,
        then_body: Vec<FirStmt>,
        else_body: Vec<FirStmt>,
    },
    While {
        cond: FirExpr,
        body: Vec<FirStmt>,
        /// Statements to run after the body (and on `continue`) before re-checking
        /// the condition.  Used by for-loop lowering to hold the loop variable increment.
        increment: Vec<FirStmt>,
    },
    Break,
    Continue,
    Expr(FirExpr),
    Block(Vec<FirStmt>),
    /// Placeholder for statements that produce no runtime value (e.g. nested
    /// type definitions). Codegen should skip these entirely.
    NoOp,
    /// Source-line marker (1-based) for stack-trace symbolization. Emitted by
    /// the lowerer before each source statement; codegen sets the current
    /// Cranelift srcloc from it so every instruction that follows carries the
    /// originating statement's line, giving per-statement (not per-function)
    /// line granularity in captured traces. Produces no runtime code.
    SrcLine(u32),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FirPlace {
    Local(LocalId),
    Field {
        object: Box<FirExpr>,
        offset: usize,
    },
    Index {
        list: Box<FirExpr>,
        index: Box<FirExpr>,
    },
    MapIndex {
        map: Box<FirExpr>,
        key: Box<FirExpr>,
    },
}
