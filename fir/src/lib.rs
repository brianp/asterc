#![deny(unsafe_code)]

pub mod builtins;
pub mod eval_context;
pub mod exprs;
pub mod lower;
pub mod module;
pub mod stmts;
pub mod types;
pub mod validate;

pub use eval_context::EvalContext;
pub use exprs::{BinOp, FirExpr, UnaryOp};
pub use lower::{FirCache, LowerError, Lowerer};
pub use module::{FirClass, FirFunction, FirModule};
pub use stmts::{FirPlace, FirStmt};
pub use types::{ClassId, FirType, FunctionId, LocalId};

#[cfg(test)]
mod tests;
