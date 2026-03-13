pub mod diagnostic;
pub mod expr;
pub mod span;
pub mod type_env;
pub mod type_table;
pub mod types;

pub use diagnostic::{Diagnostic, Label, Severity};
pub use expr::{
    BinOp, EnumVariant, ErrorCatchPattern, Expr, MatchPattern, Module, Stmt, StringPart, UnaryOp,
};
pub use span::Span;
pub use type_env::{ClassInfo, EnumInfo, NamespaceInfo, TraitInfo, TypeEnv};
pub use type_table::TypeTable;
pub use types::{Type, TypeConstraint};

/// Result of parsing with error recovery — best-effort AST plus all diagnostics.
#[derive(Debug, Clone)]
pub struct ParseResult {
    pub module: Module,
    pub diagnostics: Vec<Diagnostic>,
}
