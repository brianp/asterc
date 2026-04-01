use std::collections::HashSet;

use ast::{
    BinOp, Diagnostic, Expr, MatchPattern, Span, UnaryOp,
    templates::{DiagnosticTemplate, parse_errors::*},
};
use lexer::TokenKind;

use crate::{MAX_COLLECTION_SIZE, MAX_NESTING_DEPTH, Parser};

impl Parser {
    /// Parse argument list. Non-named arguments get synthesized names `_0`, `_1`, etc.
    /// The opening `(` must already be consumed. Does NOT consume the closing `)`.
    fn parse_args_inner(&mut self) -> Result<Vec<(String, Span, Expr)>, Diagnostic> {
        let mut args = Vec::new();
        let mut seen_names = HashSet::new();
        let mut positional_index = 0usize;
        self.consume_newlines();
        if !self.at(&TokenKind::RParen) {
            loop {
                self.consume_newlines();
                if self.at(&TokenKind::RParen) {
                    break; // trailing comma
                }
                let arg_start = self.start_span();
                // Detect named argument: `ident: expr`
                let is_named = matches!(self.peek_kind(), TokenKind::Ident(_))
                    && self.peek_second_kind() == Some(&TokenKind::Colon);
                if is_named {
                    let name_tok = self.advance();
                    let name_span = name_tok.span();
                    let name = match name_tok.kind {
                        TokenKind::Ident(n) => n,
                        _ => unreachable!(),
                    };
                    if !seen_names.insert(name.clone()) {
                        return Err(Diagnostic::from_template(
                            DiagnosticTemplate::UnexpectedToken(UnexpectedToken {
                                expected: "unique argument name".to_string(),
                                found: format!("duplicate '{}'", name),
                            }),
                        )
                        .with_label(self.span_from(arg_start), "duplicate argument"));
                    }
                    self.expect(TokenKind::Colon)?;
                    let value = self.parse_expr()?;
                    args.push((name, name_span, value));
                } else {
                    // Positional argument — synthesize name like `_0`, `_1`.
                    // For constructors this is valid; for regular calls the
                    // typechecker will reject it with a hint about the real param name.
                    let value = self.parse_expr()?;
                    let name = format!("_{}", positional_index);
                    positional_index += 1;
                    args.push((name, value.span(), value));
                }
                if args.len() > MAX_COLLECTION_SIZE {
                    return Err(
                        Diagnostic::from_template(DiagnosticTemplate::UnexpectedToken(
                            UnexpectedToken {
                                expected: format!("at most {} arguments", MAX_COLLECTION_SIZE),
                                found: "too many arguments".to_string(),
                            },
                        ))
                        .with_label(self.span_from(arg_start), "too many arguments"),
                    );
                }
                self.consume_newlines();
                if self.at(&TokenKind::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
        }
        self.consume_newlines();
        Ok(args)
    }

    pub(crate) fn parse_expr(&mut self) -> Result<Expr, Diagnostic> {
        self.depth += 1;
        if self.depth > MAX_NESTING_DEPTH {
            self.depth -= 1;
            return Err(Diagnostic::from_template(
                DiagnosticTemplate::NestingTooDeep(NestingTooDeep),
            ));
        }
        while self.at(&TokenKind::Newline) {
            self.advance();
        }
        let start = self.start_span();
        let left = self.parse_binop(0)?;
        // Check for range operators `..` and `..=` (lowest precedence)
        let result = if self.at(&TokenKind::DotDot) {
            self.advance();
            let right = self.parse_binop(0)?;
            Ok(Expr::Range {
                start: Box::new(left),
                end: Box::new(right),
                inclusive: false,
                span: self.span_from(start),
            })
        } else if self.at(&TokenKind::DotDotEq) {
            self.advance();
            let right = self.parse_binop(0)?;
            Ok(Expr::Range {
                start: Box::new(left),
                end: Box::new(right),
                inclusive: true,
                span: self.span_from(start),
            })
        } else {
            Ok(left)
        };
        self.depth -= 1;
        result
    }

    /// Table-driven precedence parser for left-associative binary operators.
    /// Levels: 0=Or, 1=And, 2=Equality, 3=Comparison, 4=Additive, 5=Multiplicative
    fn parse_binop(&mut self, level: usize) -> Result<Expr, Diagnostic> {
        if level >= Self::BINOP_TABLE.len() {
            return self.parse_exponent();
        }
        let start = self.start_span();
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
                span: self.span_from(start),
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

    fn parse_exponent(&mut self) -> Result<Expr, Diagnostic> {
        let start = self.start_span();
        let base = self.parse_unary()?;
        if self.at(&TokenKind::StarStar) {
            self.advance();
            let exp = self.parse_exponent()?; // right-associative
            Ok(Expr::BinaryOp {
                left: Box::new(base),
                op: BinOp::Pow,
                right: Box::new(exp),
                span: self.span_from(start),
            })
        } else {
            Ok(base)
        }
    }

    fn parse_unary(&mut self) -> Result<Expr, Diagnostic> {
        self.depth += 1;
        if self.depth > MAX_NESTING_DEPTH {
            self.depth -= 1;
            return Err(Diagnostic::from_template(
                DiagnosticTemplate::NestingTooDeep(NestingTooDeep),
            ));
        }
        let result = self.parse_unary_inner();
        self.depth -= 1;
        result
    }

    fn parse_unary_inner(&mut self) -> Result<Expr, Diagnostic> {
        let start = self.start_span();
        if self.at(&TokenKind::Minus) {
            self.advance();
            let operand = self.parse_unary()?;
            return Ok(Expr::UnaryOp {
                op: UnaryOp::Neg,
                operand: Box::new(operand),
                span: self.span_from(start),
            });
        }
        if self.at(&TokenKind::Not) {
            self.advance();
            let operand = self.parse_unary()?;
            return Ok(Expr::UnaryOp {
                op: UnaryOp::Not,
                operand: Box::new(operand),
                span: self.span_from(start),
            });
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<Expr, Diagnostic> {
        self.parse_postfix_impl(true)
    }

    /// Like parse_postfix but stops at `!` — used for `resolve expr` so that
    /// `resolve task!` parses as `Propagate(Resolve(task))` not `Resolve(Propagate(task))`.
    fn parse_postfix_no_bang(&mut self) -> Result<Expr, Diagnostic> {
        self.parse_postfix_impl(false)
    }

    fn parse_postfix_impl(&mut self, allow_bang: bool) -> Result<Expr, Diagnostic> {
        let start = self.start_span();
        let mut expr = self.parse_primary()?;
        loop {
            if self.at(&TokenKind::LParen) {
                self.advance();
                let args = self.parse_args_inner()?;
                self.expect(TokenKind::RParen)?;
                expr = Expr::Call {
                    func: Box::new(expr),
                    args,
                    span: self.span_from(start),
                };
            } else if self.at(&TokenKind::LBracket) {
                self.advance();
                let index = self.parse_expr()?;
                self.expect(TokenKind::RBracket)?;
                expr = Expr::Index {
                    object: Box::new(expr),
                    index: Box::new(index),
                    span: self.span_from(start),
                };
            } else if allow_bang && self.at(&TokenKind::Bang) {
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
                        self.expect(TokenKind::LParen)?;
                        let default = self.parse_expr()?;
                        self.expect(TokenKind::RParen)?;
                        expr = Expr::ErrorOr {
                            expr: Box::new(expr),
                            default: Box::new(default),
                            span: self.span_from(start),
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
                            span: self.span_from(start),
                        };
                    } else if is_catch {
                        self.advance(); // consume .
                        self.advance(); // consume "catch"
                        expr = self.parse_error_catch(expr, start)?;
                    } else {
                        // Plain ! propagation
                        expr = Expr::Propagate(Box::new(expr), self.span_from(start));
                    }
                } else {
                    expr = Expr::Propagate(Box::new(expr), self.span_from(start));
                }
            } else if self.at(&TokenKind::Dot) {
                self.advance();
                // Accept identifiers and keyword tokens that can be method names
                let next = self.advance();
                let field = match &next.kind {
                    TokenKind::Ident(n) => n.clone(),
                    TokenKind::Or => "or".to_string(),
                    TokenKind::Catch => "catch".to_string(),
                    t => {
                        return Err(Diagnostic::from_template(
                            DiagnosticTemplate::UnexpectedToken(UnexpectedToken {
                                expected: "field name after '.'".to_string(),
                                found: format!("`{}`", t),
                            }),
                        )
                        .with_label(next.span(), "unexpected token"));
                    }
                };
                expr = Expr::Member {
                    object: Box::new(expr),
                    field,
                    span: self.span_from(start),
                };
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn parse_error_catch(&mut self, call_expr: Expr, start: usize) -> Result<Expr, Diagnostic> {
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
            let arm_start = self.start_span();
            let pattern =
                match &self.peek().kind {
                    Ident(name) if name == "_" => {
                        self.advance();
                        ErrorCatchPattern::Wildcard(self.span_from(arm_start))
                    }
                    Ident(type_name) => {
                        let tname = type_name.clone();
                        self.advance();
                        let (var, _) = self
                            .expect_ident(&format!("variable name after error type '{}'", tname))?;
                        ErrorCatchPattern::Typed {
                            error_type: tname,
                            var,
                            span: self.span_from(arm_start),
                        }
                    }
                    t => {
                        let tok = self.peek();
                        return Err(Diagnostic::from_template(
                            DiagnosticTemplate::UnexpectedToken(UnexpectedToken {
                                expected: "error type or '_' in catch arm".to_string(),
                                found: format!("`{}`", t),
                            }),
                        )
                        .with_label(tok.span(), "unexpected token"));
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
            span: self.span_from(start),
        })
    }

    fn parse_match_pattern(&mut self) -> Result<MatchPattern, Diagnostic> {
        use TokenKind::*;
        let start = self.start_span();
        match &self.peek().kind {
            Minus => {
                // Negative numeric literal in match pattern
                self.advance();
                match &self.peek().kind {
                    Int(v) => {
                        let val = -*v;
                        self.advance();
                        let span = self.span_from(start);
                        Ok(MatchPattern::Literal(Box::new(Expr::Int(val, span)), span))
                    }
                    Float(v) => {
                        let val = -*v;
                        self.advance();
                        let span = self.span_from(start);
                        Ok(MatchPattern::Literal(
                            Box::new(Expr::Float(val, span)),
                            span,
                        ))
                    }
                    t => {
                        let tok = self.peek();
                        Err(
                            Diagnostic::from_template(DiagnosticTemplate::UnexpectedToken(
                                UnexpectedToken {
                                    expected: "number after '-' in match pattern".to_string(),
                                    found: format!("`{}`", t),
                                },
                            ))
                            .with_label(tok.span(), "expected numeric literal"),
                        )
                    }
                }
            }
            Int(v) => {
                let val = *v;
                self.advance();
                let span = self.span_from(start);
                Ok(MatchPattern::Literal(Box::new(Expr::Int(val, span)), span))
            }
            Float(v) => {
                let val = *v;
                self.advance();
                let span = self.span_from(start);
                Ok(MatchPattern::Literal(
                    Box::new(Expr::Float(val, span)),
                    span,
                ))
            }
            Str(s) => {
                let lit = s.clone();
                self.advance();
                let span = self.span_from(start);
                Ok(MatchPattern::Literal(Box::new(Expr::Str(lit, span)), span))
            }
            True => {
                self.advance();
                let span = self.span_from(start);
                Ok(MatchPattern::Literal(
                    Box::new(Expr::Bool(true, span)),
                    span,
                ))
            }
            False => {
                self.advance();
                let span = self.span_from(start);
                Ok(MatchPattern::Literal(
                    Box::new(Expr::Bool(false, span)),
                    span,
                ))
            }
            Nil => {
                self.advance();
                let span = self.span_from(start);
                Ok(MatchPattern::Literal(Box::new(Expr::Nil(span)), span))
            }
            Ident(n) => {
                let name = n.clone();
                self.advance();
                // Check for enum variant pattern: EnumName.Variant
                if self.at(&Dot) {
                    self.advance();
                    if let Ident(v) = &self.peek().kind {
                        let variant = v.clone();
                        self.advance();
                        // Optional field destructuring: Color.Circle(r) or Color.Circle(radius: r)
                        let bindings = if self.at(&LParen) {
                            self.advance();
                            let mut bs = Vec::new();
                            if !self.at(&RParen) {
                                loop {
                                    let (first_name, _) =
                                        self.expect_ident("binding name in enum pattern")?;
                                    // Check for named binding: field_name: binding_name
                                    if self.at(&Colon) {
                                        self.advance();
                                        let (bind_name, _) = self.expect_ident(
                                            "binding name after ':' in enum pattern",
                                        )?;
                                        bs.push((first_name, bind_name));
                                    } else {
                                        // Positional: binding name matches field name
                                        bs.push((first_name.clone(), first_name));
                                    }
                                    if self.at(&Comma) {
                                        self.advance();
                                    } else {
                                        break;
                                    }
                                }
                            }
                            self.expect(RParen)?;
                            bs
                        } else {
                            vec![]
                        };
                        let span = self.span_from(start);
                        return Ok(MatchPattern::EnumVariant {
                            enum_name: name,
                            variant,
                            bindings,
                            span,
                        });
                    } else {
                        let tok = self.peek();
                        return Err(Diagnostic::from_template(
                            DiagnosticTemplate::UnexpectedToken(UnexpectedToken {
                                expected: "variant name after '.' in enum pattern".to_string(),
                                found: format!("`{}`", self.peek().kind),
                            }),
                        )
                        .with_label(tok.span(), "expected variant name"));
                    }
                }
                let span = self.span_from(start);
                if name == "_" {
                    Ok(MatchPattern::Wildcard(span))
                } else {
                    Ok(MatchPattern::Ident(name, span))
                }
            }
            t => {
                let tok = self.peek();
                Err(
                    Diagnostic::from_template(DiagnosticTemplate::UnexpectedToken(
                        UnexpectedToken {
                            expected: "match pattern".to_string(),
                            found: format!("`{}`", t),
                        },
                    ))
                    .with_label(tok.span(), "not a valid pattern"),
                )
            }
        }
    }

    fn parse_primary(&mut self) -> Result<Expr, Diagnostic> {
        use TokenKind::*;
        let start = self.start_span();
        // Inline lambda: -> params: body  OR  -> : body
        if self.at(&Arrow) {
            self.advance();
            return self.parse_inline_lambda(start);
        }
        match &self.peek().kind {
            Ident(n) => {
                let name = n.clone();
                self.advance();
                Ok(Expr::Ident(name, self.span_from(start)))
            }
            Str(s) => {
                let lit = s.clone();
                self.advance();
                Ok(Expr::Str(lit, self.span_from(start)))
            }
            TokenKind::StringStart(_) => self.parse_string_interpolation(start),
            Int(v) => {
                let val = *v;
                self.advance();
                Ok(Expr::Int(val, self.span_from(start)))
            }
            Float(v) => {
                let val = *v;
                self.advance();
                Ok(Expr::Float(val, self.span_from(start)))
            }
            True => {
                self.advance();
                Ok(Expr::Bool(true, self.span_from(start)))
            }
            False => {
                self.advance();
                Ok(Expr::Bool(false, self.span_from(start)))
            }
            Nil => {
                self.advance();
                Ok(Expr::Nil(self.span_from(start)))
            }
            LParen => {
                if self.depth >= MAX_NESTING_DEPTH {
                    return Err(Diagnostic::from_template(
                        DiagnosticTemplate::NestingTooDeep(NestingTooDeep),
                    ));
                }
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(RParen)?;
                Ok(expr)
            }
            LBracket => {
                self.advance();
                self.parse_list_literal(start)
            }
            LBrace => {
                self.advance();
                self.parse_map_literal(start)
            }
            Match => {
                self.advance();
                self.parse_match_expr(start)
            }
            Resolve => {
                self.advance();
                let expr = self.parse_postfix_no_bang()?;
                Ok(Expr::Resolve {
                    expr: Box::new(expr),
                    span: self.span_from(start),
                })
            }
            Async => {
                self.advance();
                let Expr::Call { func, args, .. } = self.parse_keyword_call("async", start)? else {
                    unreachable!()
                };
                Ok(Expr::AsyncCall {
                    func,
                    args,
                    span: self.span_from(start),
                })
            }
            Blocking => {
                self.advance();
                let Expr::Call { func, args, .. } = self.parse_keyword_call("blocking", start)?
                else {
                    unreachable!()
                };
                Ok(Expr::BlockingCall {
                    func,
                    args,
                    span: self.span_from(start),
                })
            }
            Detached => {
                self.advance();
                if !self.at(&Async) {
                    let tok = self.peek();
                    return Err(
                        Diagnostic::from_template(DiagnosticTemplate::UnexpectedToken(
                            UnexpectedToken {
                                expected: "'async' after 'detached'".to_string(),
                                found: format!("`{}`", self.peek().kind),
                            },
                        ))
                        .with_label(tok.span(), "expected 'async' here"),
                    );
                }
                self.advance();
                let Expr::Call { func, args, .. } =
                    self.parse_keyword_call("detached async", start)?
                else {
                    unreachable!()
                };
                Ok(Expr::DetachedCall {
                    func,
                    args,
                    span: self.span_from(start),
                })
            }
            Throw => {
                self.advance();
                let expr = self.parse_expr()?;
                Ok(Expr::Throw(Box::new(expr), self.span_from(start)))
            }
            t => {
                let tok = self.peek();
                Err(
                    Diagnostic::from_template(DiagnosticTemplate::UnexpectedToken(
                        UnexpectedToken {
                            expected: "expression".to_string(),
                            found: format!("`{}`", t),
                        },
                    ))
                    .with_label(tok.span(), "not expected here"),
                )
            }
        }
    }

    fn parse_list_literal(&mut self, start: usize) -> Result<Expr, Diagnostic> {
        use TokenKind::*;
        self.consume_newlines();
        let mut elems = Vec::new();
        if !self.at(&RBracket) {
            loop {
                self.consume_newlines();
                if self.at(&RBracket) {
                    break; // trailing comma
                }
                elems.push(self.parse_expr()?);
                if elems.len() > MAX_COLLECTION_SIZE {
                    return Err(Diagnostic::from_template(
                        DiagnosticTemplate::UnexpectedToken(UnexpectedToken {
                            expected: format!(
                                "at most {} elements in list literal",
                                MAX_COLLECTION_SIZE
                            ),
                            found: "too many elements".to_string(),
                        }),
                    ));
                }
                self.consume_newlines();
                if self.at(&Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
        }
        self.consume_newlines();
        self.expect(RBracket)?;
        Ok(Expr::ListLiteral(elems, self.span_from(start)))
    }

    fn parse_map_literal(&mut self, start: usize) -> Result<Expr, Diagnostic> {
        use TokenKind::*;
        self.consume_newlines();
        let mut entries = Vec::new();
        if !self.at(&RBrace) {
            loop {
                self.consume_newlines();
                if self.at(&RBrace) {
                    break; // trailing comma
                }
                let key = self.parse_expr()?;
                self.expect(Colon)?;
                let value = self.parse_expr()?;
                entries.push((key, value));
                if entries.len() > MAX_COLLECTION_SIZE {
                    return Err(Diagnostic::from_template(
                        DiagnosticTemplate::UnexpectedToken(UnexpectedToken {
                            expected: format!(
                                "at most {} entries in map literal",
                                MAX_COLLECTION_SIZE
                            ),
                            found: "too many entries".to_string(),
                        }),
                    ));
                }
                self.consume_newlines();
                if self.at(&Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
        }
        self.consume_newlines();
        self.expect(RBrace)?;
        Ok(Expr::Map {
            entries,
            span: self.span_from(start),
        })
    }

    fn parse_match_expr(&mut self, start: usize) -> Result<Expr, Diagnostic> {
        use TokenKind::*;
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
            span: self.span_from(start),
        })
    }

    /// Parse a postfix expression and require it to be a function call.
    /// Used for `async`, `blocking`, and `detached async` keyword calls.
    /// Returns the `Expr::Call` on success (callers destructure it).
    fn parse_keyword_call(&mut self, keyword: &str, start: usize) -> Result<Expr, Diagnostic> {
        let func_expr = self.parse_postfix()?;
        if matches!(func_expr, Expr::Call { .. }) {
            Ok(func_expr)
        } else {
            Err(
                Diagnostic::from_template(DiagnosticTemplate::UnexpectedToken(UnexpectedToken {
                    expected: format!("function call after '{}'", keyword),
                    found: "non-call expression".to_string(),
                }))
                .with_label(self.span_from(start), "expected a call expression"),
            )
        }
    }

    /// Parse a string interpolation: StringStart expr StringMid expr ... StringEnd
    fn parse_string_interpolation(&mut self, start: usize) -> Result<Expr, Diagnostic> {
        let mut parts = Vec::new();

        // First token is StringStart
        if let TokenKind::StringStart(s) = &self.peek().kind {
            let lit = s.clone();
            self.advance();
            if !lit.is_empty() {
                parts.push(ast::StringPart::Literal(lit));
            }
        } else {
            return Err(Diagnostic::from_template(
                DiagnosticTemplate::UnexpectedToken(UnexpectedToken {
                    expected: "StringStart token".to_string(),
                    found: format!("`{}`", self.peek().kind),
                }),
            ));
        }

        loop {
            // Parse the interpolated expression
            let expr = self.parse_expr()?;
            parts.push(ast::StringPart::Expr(Box::new(expr)));

            // Next should be StringMid or StringEnd
            match &self.peek().kind {
                TokenKind::StringMid(s) => {
                    let lit = s.clone();
                    self.advance();
                    if !lit.is_empty() {
                        parts.push(ast::StringPart::Literal(lit));
                    }
                    // Continue loop — more interpolations follow
                }
                TokenKind::StringEnd(s) => {
                    let lit = s.clone();
                    self.advance();
                    if !lit.is_empty() {
                        parts.push(ast::StringPart::Literal(lit));
                    }
                    break;
                }
                t => {
                    let tok = self.peek();
                    return Err(
                        Diagnostic::from_template(DiagnosticTemplate::UnexpectedToken(
                            UnexpectedToken {
                                expected: "string continuation or end".to_string(),
                                found: format!("`{}`", t),
                            },
                        ))
                        .with_label(tok.span(), "unexpected token in string interpolation"),
                    );
                }
            }
        }

        Ok(Expr::StringInterpolation {
            parts,
            span: self.span_from(start),
        })
    }

    /// Parse an inline lambda after the `->` has been consumed.
    ///
    /// Forms:
    /// - `-> x: body`        — one param, inferred type
    /// - `-> a, b: body`     — multiple params, inferred types
    /// - `-> : body`         — zero params
    fn parse_inline_lambda(&mut self, start: usize) -> Result<Expr, Diagnostic> {
        use TokenKind::*;

        // -> : body  (zero-param lambda)
        if self.at(&Colon) {
            self.advance();
            let body_expr = self.parse_expr()?;
            let span = self.span_from(start);
            return Ok(Expr::Lambda {
                params: Vec::new(),
                ret_type: ast::Type::Inferred,
                body: vec![ast::Stmt::Expr(body_expr, span)],
                generic_params: None,
                throws: None,
                type_constraints: vec![],
                defaults: Box::new(vec![]),
                span,
            });
        }

        // Parse parameter names
        let mut params = Vec::new();
        loop {
            let (pname, _) = self.expect_ident("parameter name")?;
            params.push((pname, ast::Type::Inferred));
            if self.at(&Comma) {
                self.advance();
            } else {
                break;
            }
        }
        self.expect(Colon)?;
        let body_expr = self.parse_expr()?;
        let span = self.span_from(start);

        Ok(Expr::Lambda {
            params,
            ret_type: ast::Type::Inferred,
            body: vec![ast::Stmt::Expr(body_expr, span)],
            generic_params: None,
            throws: None,
            type_constraints: vec![],
            defaults: Box::new(vec![]),
            span,
        })
    }
}
