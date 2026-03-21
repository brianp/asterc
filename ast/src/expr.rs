use std::fmt;

use serde::{Deserialize, Serialize};

use crate::span::Span;
use crate::types::Type;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Module {
    pub name: String,
    pub body: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Stmt {
    Let {
        name: String,
        type_ann: Option<Type>,
        value: Expr,
        is_public: bool,
        span: Span,
    },
    Class {
        name: String,
        fields: Vec<(String, Type)>,
        methods: Vec<Stmt>,
        is_public: bool,
        generic_params: Option<Vec<String>>,
        extends: Option<String>,
        includes: Option<Vec<(String, Vec<Type>)>>,
        span: Span,
    },
    Trait {
        name: String,
        methods: Vec<Stmt>,
        is_public: bool,
        generic_params: Option<Vec<String>>,
        span: Span,
    },
    Return(Expr, Span),
    Expr(Expr, Span),
    If {
        cond: Expr,
        then_body: Vec<Stmt>,
        elif_branches: Vec<(Expr, Vec<Stmt>)>,
        else_body: Vec<Stmt>,
        span: Span,
    },
    While {
        cond: Expr,
        body: Vec<Stmt>,
        span: Span,
    },
    For {
        var: String,
        iter: Expr,
        body: Vec<Stmt>,
        span: Span,
    },
    Assignment {
        target: Expr,
        value: Expr,
        span: Span,
    },
    Break(Span),
    Continue(Span),
    Use {
        path: Vec<String>,
        names: Option<Vec<String>>,
        alias: Option<String>,
        is_public: bool,
        span: Span,
    },
    Enum {
        name: String,
        variants: Vec<EnumVariant>,
        methods: Vec<Stmt>,
        includes: Vec<(String, Vec<Type>)>,
        is_public: bool,
        span: Span,
    },
    Const {
        name: String,
        type_ann: Option<Type>,
        value: Expr,
        is_public: bool,
        span: Span,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnumVariant {
    pub name: String,
    pub fields: Vec<(String, Type)>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ErrorCatchPattern {
    /// `TypeName var` -- matches errors of that type, binds to var
    Typed {
        error_type: String,
        var: String,
        span: Span,
    },
    /// `_` -- wildcard catch-all
    Wildcard(Span),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MatchPattern {
    Literal(Box<Expr>, Span),
    Ident(String, Span),
    Wildcard(Span),
    EnumVariant {
        enum_name: String,
        variant: String,
        span: Span,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
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

impl fmt::Display for BinOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            BinOp::Add => "Add",
            BinOp::Sub => "Sub",
            BinOp::Mul => "Mul",
            BinOp::Div => "Div",
            BinOp::Mod => "Mod",
            BinOp::Pow => "Pow",
            BinOp::Eq => "Eq",
            BinOp::Neq => "Neq",
            BinOp::Lt => "Lt",
            BinOp::Gt => "Gt",
            BinOp::Lte => "Lte",
            BinOp::Gte => "Gte",
            BinOp::And => "And",
            BinOp::Or => "Or",
        };
        write!(f, "{}", name)
    }
}

impl fmt::Display for UnaryOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            UnaryOp::Neg => "Neg",
            UnaryOp::Not => "Not",
        };
        write!(f, "{}", name)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Expr {
    Int(i64, Span),
    Float(f64, Span),
    Str(String, Span),
    Bool(bool, Span),
    Nil(Span),
    Ident(String, Span),
    Member {
        object: Box<Expr>,
        field: String,
        span: Span,
    },
    Lambda {
        params: Vec<(String, Type)>,
        ret_type: Type,
        body: Vec<Stmt>,
        generic_params: Option<Vec<String>>,
        throws: Option<Box<Type>>,
        /// Generic type parameter constraints: `T extends Foo includes Bar`.
        type_constraints: Vec<(String, Vec<crate::types::TypeConstraint>)>,
        /// Default values for parameters, indexed by position. None = no default.
        defaults: Box<Vec<Option<Expr>>>,
        span: Span,
    },
    Call {
        func: Box<Expr>,
        args: Vec<(String, Span, Expr)>,
        span: Span,
    },
    BinaryOp {
        left: Box<Expr>,
        op: BinOp,
        right: Box<Expr>,
        span: Span,
    },
    UnaryOp {
        op: UnaryOp,
        operand: Box<Expr>,
        span: Span,
    },
    ListLiteral(Vec<Expr>, Span),
    Index {
        object: Box<Expr>,
        index: Box<Expr>,
        span: Span,
    },
    Match {
        scrutinee: Box<Expr>,
        arms: Vec<(MatchPattern, Expr)>,
        span: Span,
    },
    AsyncCall {
        func: Box<Expr>,
        args: Vec<(String, Span, Expr)>,
        span: Span,
    },
    BlockingCall {
        func: Box<Expr>,
        args: Vec<(String, Span, Expr)>,
        span: Span,
    },
    Resolve {
        expr: Box<Expr>,
        span: Span,
    },
    DetachedCall {
        func: Box<Expr>,
        args: Vec<(String, Span, Expr)>,
        span: Span,
    },
    Propagate(Box<Expr>, Span),
    Throw(Box<Expr>, Span),
    /// `expr!.or(default)` -- eager fallback for throwing calls
    ErrorOr {
        expr: Box<Expr>,
        default: Box<Expr>,
        span: Span,
    },
    /// `expr!.or_else(-> recovery)` -- lazy fallback for throwing calls
    ErrorOrElse {
        expr: Box<Expr>,
        handler: Box<Expr>,
        span: Span,
    },
    /// `expr!.catch` with match arms on error types
    ErrorCatch {
        expr: Box<Expr>,
        arms: Vec<(ErrorCatchPattern, Expr)>,
        span: Span,
    },
    StringInterpolation {
        parts: Vec<StringPart>,
        span: Span,
    },
    Map {
        entries: Vec<(Expr, Expr)>,
        span: Span,
    },
    Range {
        start: Box<Expr>,
        end: Box<Expr>,
        inclusive: bool,
        span: Span,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum StringPart {
    Literal(String),
    Expr(Box<Expr>),
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::Int(_, s) => *s,
            Expr::Float(_, s) => *s,
            Expr::Str(_, s) => *s,
            Expr::Bool(_, s) => *s,
            Expr::Nil(s) => *s,
            Expr::Ident(_, s) => *s,
            Expr::Member { span, .. } => *span,
            Expr::Lambda { span, .. } => *span,
            Expr::Call { span, .. } => *span,
            Expr::BinaryOp { span, .. } => *span,
            Expr::UnaryOp { span, .. } => *span,
            Expr::ListLiteral(_, s) => *s,
            Expr::Index { span, .. } => *span,
            Expr::Match { span, .. } => *span,
            Expr::AsyncCall { span, .. } => *span,
            Expr::BlockingCall { span, .. } => *span,
            Expr::Resolve { span, .. } => *span,
            Expr::DetachedCall { span, .. } => *span,
            Expr::Propagate(_, s) => *s,
            Expr::Throw(_, s) => *s,
            Expr::ErrorOr { span, .. } => *span,
            Expr::ErrorOrElse { span, .. } => *span,
            Expr::ErrorCatch { span, .. } => *span,
            Expr::StringInterpolation { span, .. } => *span,
            Expr::Map { span, .. } => *span,
            Expr::Range { span, .. } => *span,
        }
    }
}

impl Stmt {
    pub fn span(&self) -> Span {
        match self {
            Stmt::Let { span, .. } => *span,
            Stmt::Class { span, .. } => *span,
            Stmt::Trait { span, .. } => *span,
            Stmt::Return(_, s) => *s,
            Stmt::Expr(_, s) => *s,
            Stmt::If { span, .. } => *span,
            Stmt::While { span, .. } => *span,
            Stmt::For { span, .. } => *span,
            Stmt::Assignment { span, .. } => *span,
            Stmt::Break(s) => *s,
            Stmt::Continue(s) => *s,
            Stmt::Use { span, .. } => *span,
            Stmt::Enum { span, .. } => *span,
            Stmt::Const { span, .. } => *span,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_op_construction_and_match() {
        let s = Span::dummy();
        let expr = Expr::BinaryOp {
            left: Box::new(Expr::Int(1, s)),
            op: BinOp::Add,
            right: Box::new(Expr::Int(2, s)),
            span: s,
        };
        match expr {
            Expr::BinaryOp {
                left, op, right, ..
            } => {
                assert_eq!(*left, Expr::Int(1, s));
                assert_eq!(op, BinOp::Add);
                assert_eq!(*right, Expr::Int(2, s));
            }
            _ => panic!("expected BinaryOp"),
        }
    }

    #[test]
    fn unary_op_construction_and_match() {
        let s = Span::dummy();
        let expr = Expr::UnaryOp {
            op: UnaryOp::Neg,
            operand: Box::new(Expr::Int(5, s)),
            span: s,
        };
        match expr {
            Expr::UnaryOp { op, operand, .. } => {
                assert_eq!(op, UnaryOp::Neg);
                assert_eq!(*operand, Expr::Int(5, s));
            }
            _ => panic!("expected UnaryOp"),
        }
    }

    #[test]
    fn nested_binary_ops() {
        let s = Span::dummy();
        // (1 + 2) * 3
        let expr = Expr::BinaryOp {
            left: Box::new(Expr::BinaryOp {
                left: Box::new(Expr::Int(1, s)),
                op: BinOp::Add,
                right: Box::new(Expr::Int(2, s)),
                span: s,
            }),
            op: BinOp::Mul,
            right: Box::new(Expr::Int(3, s)),
            span: s,
        };
        match &expr {
            Expr::BinaryOp { left, op, .. } => {
                assert_eq!(*op, BinOp::Mul);
                assert!(matches!(**left, Expr::BinaryOp { .. }));
            }
            _ => panic!("expected nested BinaryOp"),
        }
    }

    #[test]
    fn binop_clone_and_eq() {
        let a = BinOp::Add;
        let b = a.clone();
        assert_eq!(a, b);

        let u = UnaryOp::Not;
        let v = u.clone();
        assert_eq!(u, v);
    }
}
