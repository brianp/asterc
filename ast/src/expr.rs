use crate::types::Type;

#[derive(Debug, Clone, PartialEq)]
pub struct Module {
    pub name: String,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Let {
        name: String,
        type_ann: Option<Type>,
        value: Expr,
        is_public: bool,
    },
    Class {
        name: String,
        fields: Vec<(String, Type)>,
        methods: Vec<Stmt>,
        is_public: bool,
        generic_params: Option<Vec<String>>,
        extends: Option<String>,
        includes: Option<Vec<String>>,
    },
    Trait {
        name: String,
        methods: Vec<Stmt>,
        is_public: bool,
    },
    Return(Expr),
    Expr(Expr),
    If {
        cond: Expr,
        then_body: Vec<Stmt>,
        elif_branches: Vec<(Expr, Vec<Stmt>)>,
        else_body: Vec<Stmt>,
    },
    While {
        cond: Expr,
        body: Vec<Stmt>,
    },
    For {
        var: String,
        iter: Expr,
        body: Vec<Stmt>,
    },
    Assignment {
        target: Expr,
        value: Expr,
    },
    Break,
    Continue,
    Use {
        path: Vec<String>,
        names: Option<Vec<String>>,
        alias: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum ErrorCatchPattern {
    /// `TypeName var` — matches errors of that type, binds to var
    Typed { error_type: String, var: String },
    /// `_` — wildcard catch-all
    Wildcard,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MatchPattern {
    Literal(Expr),
    Ident(String),
    Wildcard,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
    Nil,
    Ident(String),
    Member {
        object: Box<Expr>,
        field: String,
    },
    Lambda {
        params: Vec<(String, Type)>,
        ret_type: Type,
        body: Vec<Stmt>,
        is_async: bool,
        generic_params: Option<Vec<String>>,
        throws: Option<Type>,
    },
    Call {
        func: Box<Expr>,
        args: Vec<Expr>,
    },
    BinaryOp {
        left: Box<Expr>,
        op: BinOp,
        right: Box<Expr>,
    },
    UnaryOp {
        op: UnaryOp,
        operand: Box<Expr>,
    },
    ListLiteral(Vec<Expr>),
    Index {
        object: Box<Expr>,
        index: Box<Expr>,
    },
    Match {
        scrutinee: Box<Expr>,
        arms: Vec<(MatchPattern, Expr)>,
    },
    AsyncCall {
        func: Box<Expr>,
        args: Vec<Expr>,
    },
    ResolveCall {
        func: Box<Expr>,
        args: Vec<Expr>,
    },
    DetachedCall {
        func: Box<Expr>,
        args: Vec<Expr>,
    },
    Propagate(Box<Expr>),
    Throw(Box<Expr>),
    /// `expr!.or(default)` — eager fallback for throwing calls
    ErrorOr {
        expr: Box<Expr>,
        default: Box<Expr>,
    },
    /// `expr!.or_else(-> recovery)` — lazy fallback for throwing calls
    ErrorOrElse {
        expr: Box<Expr>,
        handler: Box<Expr>,
    },
    /// `expr!.catch` with match arms on error types
    ErrorCatch {
        expr: Box<Expr>,
        arms: Vec<(ErrorCatchPattern, Expr)>,
    },
    AsyncScope {
        body: Vec<Stmt>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_op_construction_and_match() {
        let expr = Expr::BinaryOp {
            left: Box::new(Expr::Int(1)),
            op: BinOp::Add,
            right: Box::new(Expr::Int(2)),
        };
        match expr {
            Expr::BinaryOp { left, op, right } => {
                assert_eq!(*left, Expr::Int(1));
                assert_eq!(op, BinOp::Add);
                assert_eq!(*right, Expr::Int(2));
            }
            _ => panic!("expected BinaryOp"),
        }
    }

    #[test]
    fn unary_op_construction_and_match() {
        let expr = Expr::UnaryOp {
            op: UnaryOp::Neg,
            operand: Box::new(Expr::Int(5)),
        };
        match expr {
            Expr::UnaryOp { op, operand } => {
                assert_eq!(op, UnaryOp::Neg);
                assert_eq!(*operand, Expr::Int(5));
            }
            _ => panic!("expected UnaryOp"),
        }
    }

    #[test]
    fn nested_binary_ops() {
        // (1 + 2) * 3
        let expr = Expr::BinaryOp {
            left: Box::new(Expr::BinaryOp {
                left: Box::new(Expr::Int(1)),
                op: BinOp::Add,
                right: Box::new(Expr::Int(2)),
            }),
            op: BinOp::Mul,
            right: Box::new(Expr::Int(3)),
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
