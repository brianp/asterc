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
