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

#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// Compare two `f64` values by their bit pattern so that `NaN == NaN`
/// (and `-0.0 != 0.0`). This gives AST nodes structural equality rather
/// than IEEE 754 numeric equality.
fn f64_bitwise_eq(a: &f64, b: &f64) -> bool {
    a.to_bits() == b.to_bits()
}

impl PartialEq for Expr {
    fn eq(&self, other: &Self) -> bool {
        // Match on `self` exhaustively so adding a new Expr variant
        // produces a compile error until its equality is defined.
        match self {
            Expr::Int(a, sa) => matches!(other, Expr::Int(b, sb) if a == b && sa == sb),
            Expr::Float(a, sa) => {
                matches!(other, Expr::Float(b, sb) if f64_bitwise_eq(a, b) && sa == sb)
            }
            Expr::Str(a, sa) => matches!(other, Expr::Str(b, sb) if a == b && sa == sb),
            Expr::Bool(a, sa) => matches!(other, Expr::Bool(b, sb) if a == b && sa == sb),
            Expr::Nil(sa) => matches!(other, Expr::Nil(sb) if sa == sb),
            Expr::Ident(a, sa) => matches!(other, Expr::Ident(b, sb) if a == b && sa == sb),
            Expr::Member {
                object: o1,
                field: f1,
                span: s1,
            } => matches!(other, Expr::Member { object: o2, field: f2, span: s2 } if o1 == o2 && f1 == f2 && s1 == s2),
            Expr::Lambda {
                params: p1,
                ret_type: r1,
                body: b1,
                generic_params: g1,
                throws: t1,
                type_constraints: tc1,
                defaults: d1,
                span: s1,
            } => matches!(other, Expr::Lambda {
                params: p2, ret_type: r2, body: b2, generic_params: g2,
                throws: t2, type_constraints: tc2, defaults: d2, span: s2,
            } if p1 == p2 && r1 == r2 && b1 == b2 && g1 == g2 && t1 == t2 && tc1 == tc2 && d1 == d2 && s1 == s2),
            Expr::Call {
                func: f1,
                args: a1,
                span: s1,
            } => matches!(other, Expr::Call { func: f2, args: a2, span: s2 } if f1 == f2 && a1 == a2 && s1 == s2),
            Expr::BinaryOp {
                left: l1,
                op: o1,
                right: r1,
                span: s1,
            } => matches!(other, Expr::BinaryOp { left: l2, op: o2, right: r2, span: s2 } if l1 == l2 && o1 == o2 && r1 == r2 && s1 == s2),
            Expr::UnaryOp {
                op: o1,
                operand: a1,
                span: s1,
            } => matches!(other, Expr::UnaryOp { op: o2, operand: a2, span: s2 } if o1 == o2 && a1 == a2 && s1 == s2),
            Expr::ListLiteral(a, sa) => {
                matches!(other, Expr::ListLiteral(b, sb) if a == b && sa == sb)
            }
            Expr::Index {
                object: o1,
                index: i1,
                span: s1,
            } => matches!(other, Expr::Index { object: o2, index: i2, span: s2 } if o1 == o2 && i1 == i2 && s1 == s2),
            Expr::Match {
                scrutinee: sc1,
                arms: a1,
                span: s1,
            } => matches!(other, Expr::Match { scrutinee: sc2, arms: a2, span: s2 } if sc1 == sc2 && a1 == a2 && s1 == s2),
            Expr::AsyncCall {
                func: f1,
                args: a1,
                span: s1,
            } => matches!(other, Expr::AsyncCall { func: f2, args: a2, span: s2 } if f1 == f2 && a1 == a2 && s1 == s2),
            Expr::BlockingCall {
                func: f1,
                args: a1,
                span: s1,
            } => matches!(other, Expr::BlockingCall { func: f2, args: a2, span: s2 } if f1 == f2 && a1 == a2 && s1 == s2),
            Expr::Resolve {
                expr: e1,
                span: s1,
            } => matches!(other, Expr::Resolve { expr: e2, span: s2 } if e1 == e2 && s1 == s2),
            Expr::DetachedCall {
                func: f1,
                args: a1,
                span: s1,
            } => matches!(other, Expr::DetachedCall { func: f2, args: a2, span: s2 } if f1 == f2 && a1 == a2 && s1 == s2),
            Expr::Propagate(e1, s1) => {
                matches!(other, Expr::Propagate(e2, s2) if e1 == e2 && s1 == s2)
            }
            Expr::Throw(e1, s1) => matches!(other, Expr::Throw(e2, s2) if e1 == e2 && s1 == s2),
            Expr::ErrorOr {
                expr: e1,
                default: d1,
                span: s1,
            } => matches!(other, Expr::ErrorOr { expr: e2, default: d2, span: s2 } if e1 == e2 && d1 == d2 && s1 == s2),
            Expr::ErrorOrElse {
                expr: e1,
                handler: h1,
                span: s1,
            } => matches!(other, Expr::ErrorOrElse { expr: e2, handler: h2, span: s2 } if e1 == e2 && h1 == h2 && s1 == s2),
            Expr::ErrorCatch {
                expr: e1,
                arms: a1,
                span: s1,
            } => matches!(other, Expr::ErrorCatch { expr: e2, arms: a2, span: s2 } if e1 == e2 && a1 == a2 && s1 == s2),
            Expr::StringInterpolation {
                parts: p1,
                span: s1,
            } => matches!(other, Expr::StringInterpolation { parts: p2, span: s2 } if p1 == p2 && s1 == s2),
            Expr::Map {
                entries: e1,
                span: s1,
            } => matches!(other, Expr::Map { entries: e2, span: s2 } if e1 == e2 && s1 == s2),
            Expr::Range {
                start: st1,
                end: en1,
                inclusive: i1,
                span: s1,
            } => matches!(other, Expr::Range { start: st2, end: en2, inclusive: i2, span: s2 } if st1 == st2 && en1 == en2 && i1 == i2 && s1 == s2),
        }
    }
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

    #[test]
    fn float_nan_equality() {
        let s = Span::dummy();
        let a = Expr::Float(f64::NAN, s);
        let b = Expr::Float(f64::NAN, s);
        assert_eq!(a, b, "two NaN float literals should be equal as AST nodes");
    }

    #[test]
    fn float_nan_in_nested_expr() {
        let s = Span::dummy();
        let a = Expr::BinaryOp {
            left: Box::new(Expr::Float(f64::NAN, s)),
            op: BinOp::Add,
            right: Box::new(Expr::Float(1.0, s)),
            span: s,
        };
        let b = a.clone();
        assert_eq!(a, b, "cloned expr containing NaN should be equal");
    }

    #[test]
    fn float_normal_equality_preserved() {
        let s = Span::dummy();
        assert_eq!(Expr::Float(1.0, s), Expr::Float(1.0, s));
        assert_ne!(Expr::Float(1.0, s), Expr::Float(2.0, s));
    }

    #[test]
    fn float_neg_zero_not_equal_pos_zero() {
        let s = Span::dummy();
        // -0.0 and 0.0 have different bits but same IEEE value;
        // for AST structural comparison we compare bits, so they differ.
        // This documents the behavior: -0.0 != 0.0 at the AST level.
        assert_ne!(
            Expr::Float(-0.0, s),
            Expr::Float(0.0, s),
            "-0.0 and 0.0 are distinct float literals"
        );
    }

    #[test]
    fn clone_equals_self_for_all_variants() {
        let s = Span::dummy();
        let int_lit = Box::new(Expr::Int(1, s));
        let variants: Vec<Expr> = vec![
            Expr::Int(42, s),
            Expr::Float(3.14, s),
            Expr::Str("hello".into(), s),
            Expr::Bool(true, s),
            Expr::Nil(s),
            Expr::Ident("x".into(), s),
            Expr::Member {
                object: int_lit.clone(),
                field: "f".into(),
                span: s,
            },
            Expr::Lambda {
                params: vec![],
                ret_type: Type::Int,
                body: vec![],
                generic_params: None,
                throws: None,
                type_constraints: vec![],
                defaults: Box::new(vec![]),
                span: s,
            },
            Expr::Call {
                func: int_lit.clone(),
                args: vec![],
                span: s,
            },
            Expr::BinaryOp {
                left: int_lit.clone(),
                op: BinOp::Add,
                right: int_lit.clone(),
                span: s,
            },
            Expr::UnaryOp {
                op: UnaryOp::Neg,
                operand: int_lit.clone(),
                span: s,
            },
            Expr::ListLiteral(vec![], s),
            Expr::Index {
                object: int_lit.clone(),
                index: int_lit.clone(),
                span: s,
            },
            Expr::Match {
                scrutinee: int_lit.clone(),
                arms: vec![],
                span: s,
            },
            Expr::AsyncCall {
                func: int_lit.clone(),
                args: vec![],
                span: s,
            },
            Expr::BlockingCall {
                func: int_lit.clone(),
                args: vec![],
                span: s,
            },
            Expr::Resolve {
                expr: int_lit.clone(),
                span: s,
            },
            Expr::DetachedCall {
                func: int_lit.clone(),
                args: vec![],
                span: s,
            },
            Expr::Propagate(int_lit.clone(), s),
            Expr::Throw(int_lit.clone(), s),
            Expr::ErrorOr {
                expr: int_lit.clone(),
                default: int_lit.clone(),
                span: s,
            },
            Expr::ErrorOrElse {
                expr: int_lit.clone(),
                handler: int_lit.clone(),
                span: s,
            },
            Expr::ErrorCatch {
                expr: int_lit.clone(),
                arms: vec![],
                span: s,
            },
            Expr::StringInterpolation {
                parts: vec![],
                span: s,
            },
            Expr::Map {
                entries: vec![],
                span: s,
            },
            Expr::Range {
                start: int_lit.clone(),
                end: int_lit.clone(),
                inclusive: false,
                span: s,
            },
        ];
        for (i, expr) in variants.iter().enumerate() {
            assert_eq!(
                expr,
                &expr.clone(),
                "clone equality failed for variant index {i}"
            );
        }
    }
}
