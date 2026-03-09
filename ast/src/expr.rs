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
        includes: Option<Vec<String>>,
        span: Span,
    },
    Trait {
        name: String,
        methods: Vec<Stmt>,
        is_public: bool,
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
        span: Span,
    },
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
    Literal(Expr, Span),
    Ident(String, Span),
    Wildcard(Span),
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
        is_async: bool,
        generic_params: Option<Vec<String>>,
        throws: Option<Type>,
        span: Span,
    },
    Call {
        func: Box<Expr>,
        args: Vec<Expr>,
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
        args: Vec<Expr>,
        span: Span,
    },
    ResolveCall {
        func: Box<Expr>,
        args: Vec<Expr>,
        span: Span,
    },
    DetachedCall {
        func: Box<Expr>,
        args: Vec<Expr>,
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
    AsyncScope {
        body: Vec<Stmt>,
        span: Span,
    },
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
            Expr::ResolveCall { span, .. } => *span,
            Expr::DetachedCall { span, .. } => *span,
            Expr::Propagate(_, s) => *s,
            Expr::Throw(_, s) => *s,
            Expr::ErrorOr { span, .. } => *span,
            Expr::ErrorOrElse { span, .. } => *span,
            Expr::ErrorCatch { span, .. } => *span,
            Expr::AsyncScope { span, .. } => *span,
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
