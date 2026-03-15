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
    },
    AsyncScope {
        scope: LocalId,
        body: Vec<FirStmt>,
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
