#![deny(unsafe_code)]

pub mod context_snapshot;
pub mod diagnostic;
pub mod eval_error;
pub mod expr;
pub mod span;
pub mod symbol_index;
pub mod templates;
pub mod type_env;
pub mod type_table;
pub mod types;

pub use context_snapshot::{ContextSnapshot, SnapshotClassInfo, SnapshotDynamicReceiver};
pub use diagnostic::{Diagnostic, Label, Severity};
pub use expr::{
    BinOp, EnumVariant, ErrorCatchPattern, Expr, MatchPattern, Module, Stmt, StringPart, UnaryOp,
};
pub use span::Span;
pub use symbol_index::{SymbolIndex, SymbolInfo, SymbolKind};
pub use type_env::{
    Binding, ClassInfo, DynamicReceiverInfo, EnumInfo, NamespaceInfo, TraitInfo, TypeEnv,
};
pub use type_table::TypeTable;
pub use types::{Type, TypeConstraint};

/// Result of parsing with error recovery — best-effort AST plus all diagnostics.
#[derive(Debug, Clone)]
pub struct ParseResult {
    pub module: Module,
    pub diagnostics: Vec<Diagnostic>,
}
