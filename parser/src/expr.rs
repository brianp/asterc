use ast::{BinOp, Expr, MatchPattern, UnaryOp};
use lexer::TokenKind;

use crate::{MAX_COLLECTION_SIZE, MAX_NESTING_DEPTH, Parser};

impl Parser {
    pub(crate) fn parse_expr(&mut self) -> Result<Expr, String> {
        self.depth += 1;
        if self.depth > MAX_NESTING_DEPTH {
            return Err(format!(
                "Nesting depth exceeds maximum of {}",
                MAX_NESTING_DEPTH
            ));
        }
        while self.at(&TokenKind::Newline) {
            self.advance();
        }
        let result = self.parse_or();
        self.depth -= 1;
        result
    }

    fn parse_or(&mut self) -> Result<Expr, String> {
        self.parse_binop(0)
    }

    /// Table-driven precedence parser for left-associative binary operators.
    /// Levels: 0=Or, 1=And, 2=Equality, 3=Comparison, 4=Additive, 5=Multiplicative
    fn parse_binop(&mut self, level: usize) -> Result<Expr, String> {
        if level >= Self::BINOP_TABLE.len() {
            return self.parse_exponent();
        }
        let mut left = self.parse_binop(level + 1)?;
        loop {
            let op = Self::BINOP_TABLE[level]
                .iter()
                .find_map(|(tk, bo)| if self.at(tk) { Some(bo.clone()) } else { None });
            let Some(op) = op else { break };
            self.advance();
            let right = self.parse_binop(level + 1)?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    const BINOP_TABLE: &[&[(TokenKind, BinOp)]] = &[
        &[(TokenKind::Or, BinOp::Or)],
        &[(TokenKind::And, BinOp::And)],
        &[
            (TokenKind::EqualEqual, BinOp::Eq),
            (TokenKind::BangEqual, BinOp::Neq),
        ],
        &[
            (TokenKind::Less, BinOp::Lt),
            (TokenKind::Greater, BinOp::Gt),
            (TokenKind::LessEqual, BinOp::Lte),
            (TokenKind::GreaterEqual, BinOp::Gte),
        ],
        &[
            (TokenKind::Plus, BinOp::Add),
            (TokenKind::Minus, BinOp::Sub),
        ],
        &[
            (TokenKind::Star, BinOp::Mul),
            (TokenKind::Slash, BinOp::Div),
            (TokenKind::Percent, BinOp::Mod),
        ],
    ];

    fn parse_exponent(&mut self) -> Result<Expr, String> {
        let base = self.parse_unary()?;
        if self.at(&TokenKind::StarStar) {
            self.advance();
            let exp = self.parse_exponent()?; // right-associative
            Ok(Expr::BinaryOp {
                left: Box::new(base),
                op: BinOp::Pow,
                right: Box::new(exp),
            })
        } else {
            Ok(base)
        }
    }

    fn parse_unary(&mut self) -> Result<Expr, String> {
        self.depth += 1;
        if self.depth > MAX_NESTING_DEPTH {
            return Err(format!(
                "Nesting depth exceeds maximum of {}",
                MAX_NESTING_DEPTH
            ));
        }
        let result = self.parse_unary_inner();
        self.depth -= 1;
        result
    }

    fn parse_unary_inner(&mut self) -> Result<Expr, String> {
        if self.at(&TokenKind::Minus) {
            self.advance();
            let operand = self.parse_unary()?;
            return Ok(Expr::UnaryOp {
                op: UnaryOp::Neg,
                operand: Box::new(operand),
            });
        }
        if self.at(&TokenKind::Not) {
            self.advance();
            let operand = self.parse_unary()?;
            return Ok(Expr::UnaryOp {
                op: UnaryOp::Not,
                operand: Box::new(operand),
            });
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<Expr, String> {
        let mut expr = self.parse_primary()?;
        loop {
            if self.at(&TokenKind::LParen) {
                self.advance();
                let mut args = Vec::new();
                if !self.at(&TokenKind::RParen) {
                    loop {
                        args.push(self.parse_expr()?);
                        if args.len() > MAX_COLLECTION_SIZE {
                            return Err(format!(
                                "Function call exceeds maximum of {} arguments",
                                MAX_COLLECTION_SIZE
                            ));
                        }
                        if self.at(&TokenKind::Comma) {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                }
                self.expect(TokenKind::RParen)?;
                expr = Expr::Call {
                    func: Box::new(expr),
                    args,
                };
            } else if self.at(&TokenKind::LBracket) {
                self.advance();
                let index = self.parse_expr()?;
                self.expect(TokenKind::RBracket)?;
                expr = Expr::Index {
                    object: Box::new(expr),
                    index: Box::new(index),
                };
            } else if self.at(&TokenKind::Bang) {
                self.advance();
                // Check for !.or(), !.or_else(), !.catch
                if self.at(&TokenKind::Dot) {
                    // Peek at the token after dot to detect !.or(), !.or_else(), !.catch
                    let next = self.tokens.get(self.pos + 1).map(|t| &t.kind);
                    let is_or = matches!(next, Some(TokenKind::Or));
                    let is_or_else = matches!(next, Some(TokenKind::Ident(n)) if n == "or_else");
                    let is_catch = matches!(next, Some(TokenKind::Catch));
                    if is_or {
                        self.advance(); // consume .
                        self.advance(); // consume "or"
                        // Could be !.or(...) or !.or_else(...)
                        // Check if next is _else to distinguish
                        // Actually "or" is a keyword token, "or_else" is an ident
                        // So this is !.or(default)
                        self.expect(TokenKind::LParen)?;
                        let default = self.parse_expr()?;
                        self.expect(TokenKind::RParen)?;
                        expr = Expr::ErrorOr {
                            expr: Box::new(expr),
                            default: Box::new(default),
                        };
                    } else if is_or_else {
                        self.advance(); // consume .
                        self.advance(); // consume "or_else"
                        self.expect(TokenKind::LParen)?;
                        self.expect(TokenKind::Arrow)?; // consume ->
                        let handler = self.parse_expr()?;
                        self.expect(TokenKind::RParen)?;
                        expr = Expr::ErrorOrElse {
                            expr: Box::new(expr),
                            handler: Box::new(handler),
                        };
                    } else if is_catch {
                        self.advance(); // consume .
                        self.advance(); // consume "catch"
                        expr = self.parse_error_catch(expr)?;
                    } else {
                        // Plain ! propagation
                        expr = Expr::Propagate(Box::new(expr));
                    }
                } else {
                    expr = Expr::Propagate(Box::new(expr));
                }
            } else if self.at(&TokenKind::Dot) {
                self.advance();
                // Accept identifiers and keyword tokens that can be method names
                let field = match &self.advance().kind {
                    TokenKind::Ident(n) => n.clone(),
                    TokenKind::Or => "or".to_string(),
                    TokenKind::Catch => "catch".to_string(),
                    t => return Err(format!("Expected field name after '.', got {:?}", t)),
                };
                expr = Expr::Member {
                    object: Box::new(expr),
                    field,
                };
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn parse_error_catch(&mut self, call_expr: Expr) -> Result<Expr, String> {
        use TokenKind::*;
        use ast::ErrorCatchPattern;
        self.consume_newlines();
        self.expect(Indent)?;
        let mut arms = Vec::new();
        while !self.at(&Dedent) && !self.at(&EOF) {
            self.consume_newlines();
            if self.at(&Dedent) || self.at(&EOF) {
                break;
            }
            let pattern = match &self.peek().kind {
                Ident(name) if name == "_" => {
                    self.advance();
                    ErrorCatchPattern::Wildcard
                }
                Ident(type_name) => {
                    let tname = type_name.clone();
                    self.advance();
                    let var = match &self.advance().kind {
                        Ident(v) => v.clone(),
                        t => {
                            return Err(format!(
                                "Expected variable name after error type '{}', got {:?}",
                                tname, t
                            ));
                        }
                    };
                    ErrorCatchPattern::Typed {
                        error_type: tname,
                        var,
                    }
                }
                t => {
                    return Err(format!(
                        "Expected error type or '_' in catch arm, got {:?}",
                        t
                    ));
                }
            };
            self.expect(Arrow)?;
            let value = self.parse_expr()?;
            arms.push((pattern, value));
            self.consume_newlines();
        }
        self.expect(Dedent)?;
        Ok(Expr::ErrorCatch {
            expr: Box::new(call_expr),
            arms,
        })
    }

    fn parse_match_pattern(&mut self) -> Result<MatchPattern, String> {
        use TokenKind::*;
        match &self.peek().kind {
            Int(v) => {
                let val = *v;
                self.advance();
                Ok(MatchPattern::Literal(Expr::Int(val)))
            }
            Float(v) => {
                let val = *v;
                self.advance();
                Ok(MatchPattern::Literal(Expr::Float(val)))
            }
            Str(s) => {
                let lit = s.clone();
                self.advance();
                Ok(MatchPattern::Literal(Expr::Str(lit)))
            }
            True => {
                self.advance();
                Ok(MatchPattern::Literal(Expr::Bool(true)))
            }
            False => {
                self.advance();
                Ok(MatchPattern::Literal(Expr::Bool(false)))
            }
            Nil => {
                self.advance();
                Ok(MatchPattern::Literal(Expr::Nil))
            }
            Ident(n) => {
                let name = n.clone();
                self.advance();
                if name == "_" {
                    Ok(MatchPattern::Wildcard)
                } else {
                    Ok(MatchPattern::Ident(name))
                }
            }
            t => Err(format!("Expected match pattern, got {:?}", t)),
        }
    }

    fn parse_primary(&mut self) -> Result<Expr, String> {
        use TokenKind::*;
        // -> expr shorthand for zero-arg lambda (used in .or_else())
        if self.at(&Arrow) {
            self.advance();
            let body_expr = self.parse_expr()?;
            return Ok(body_expr);
        }
        match &self.peek().kind {
            Ident(n) => {
                let name = n.clone();
                self.advance();
                Ok(Expr::Ident(name))
            }
            Str(s) => {
                let lit = s.clone();
                self.advance();
                Ok(Expr::Str(lit))
            }
            Int(v) => {
                let val = *v;
                self.advance();
                Ok(Expr::Int(val))
            }
            Float(v) => {
                let val = *v;
                self.advance();
                Ok(Expr::Float(val))
            }
            True => {
                self.advance();
                Ok(Expr::Bool(true))
            }
            False => {
                self.advance();
                Ok(Expr::Bool(false))
            }
            Nil => {
                self.advance();
                Ok(Expr::Nil)
            }
            LParen => {
                if self.depth >= MAX_NESTING_DEPTH {
                    return Err(format!(
                        "Nesting depth exceeds maximum of {}",
                        MAX_NESTING_DEPTH
                    ));
                }
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(RParen)?;
                Ok(expr)
            }
            LBracket => {
                self.advance();
                let mut elems = Vec::new();
                if !self.at(&RBracket) {
                    loop {
                        if self.at(&RBracket) {
                            break; // trailing comma
                        }
                        elems.push(self.parse_expr()?);
                        if elems.len() > MAX_COLLECTION_SIZE {
                            return Err(format!(
                                "List literal exceeds maximum of {} elements",
                                MAX_COLLECTION_SIZE
                            ));
                        }
                        if self.at(&Comma) {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                }
                self.expect(RBracket)?;
                Ok(Expr::ListLiteral(elems))
            }
            Match => {
                self.advance();
                let scrutinee = self.parse_expr()?;
                self.consume_newlines();
                self.expect(Indent)?;
                let mut arms = Vec::new();
                while !self.at(&Dedent) && !self.at(&TokenKind::EOF) {
                    self.consume_newlines();
                    if self.at(&Dedent) || self.at(&TokenKind::EOF) {
                        break;
                    }
                    let pattern = self.parse_match_pattern()?;
                    self.expect(FatArrow)?;
                    let value = self.parse_expr()?;
                    arms.push((pattern, value));
                    self.consume_newlines();
                }
                self.expect(Dedent)?;
                Ok(Expr::Match {
                    scrutinee: Box::new(scrutinee),
                    arms,
                })
            }
            Resolve => {
                self.advance();
                let func_expr = self.parse_postfix()?;
                match func_expr {
                    Expr::Call { func, args } => Ok(Expr::ResolveCall { func, args }),
                    _ => Err("Expected function call after 'resolve'".to_string()),
                }
            }
            Async => {
                self.advance();
                let func_expr = self.parse_postfix()?;
                match func_expr {
                    Expr::Call { func, args } => Ok(Expr::AsyncCall { func, args }),
                    _ => Err("Expected function call after 'async'".to_string()),
                }
            }
            Detached => {
                self.advance();
                if !self.at(&Async) {
                    return Err("Expected 'async' after 'detached'".to_string());
                }
                self.advance();
                let func_expr = self.parse_postfix()?;
                match func_expr {
                    Expr::Call { func, args } => Ok(Expr::DetachedCall { func, args }),
                    _ => Err("Expected function call after 'detached async'".to_string()),
                }
            }
            Throw => {
                self.advance();
                let expr = self.parse_expr()?;
                Ok(Expr::Throw(Box::new(expr)))
            }
            t => Err(format!("unexpected token in expression: {:?}", t)),
        }
    }
}
