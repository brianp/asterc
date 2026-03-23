use ast::{Diagnostic, EnumVariant, Expr, Span, Stmt, Type, TypeConstraint};
use lexer::TokenKind;

use crate::Parser;

impl Parser {
    /// Parse a comma-separated list of identifiers inside brackets: `[A, B, C]`.
    /// The opening `[` must already be consumed. Returns the list of names.
    pub(crate) fn parse_bracketed_idents(
        &mut self,
        context: &str,
    ) -> Result<Vec<String>, Diagnostic> {
        let mut names = Vec::new();
        loop {
            let tok = self.advance();
            let name = match &tok.kind {
                TokenKind::Ident(n) => n.clone(),
                t => {
                    let span = Span {
                        start: tok.start,
                        end: tok.end,
                    };
                    return Err(Diagnostic::error(format!(
                        "Expected {} name, got `{}`",
                        context, t
                    ))
                    .with_code("P001")
                    .with_label(span, "expected identifier"));
                }
            };
            names.push(name);
            if self.at(&TokenKind::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        self.expect(TokenKind::RBracket)?;
        Ok(names)
    }

    /// Parse comma-separated trait references with optional type arguments.
    /// Used for `includes Eq, From[Int], Into[String]`.
    pub(crate) fn parse_trait_ref_list(
        &mut self,
    ) -> Result<Vec<(String, Vec<ast::Type>)>, Diagnostic> {
        let mut refs = Vec::new();
        loop {
            let tok = self.advance();
            let name = match &tok.kind {
                TokenKind::Ident(n) => n.clone(),
                t => {
                    let span = Span {
                        start: tok.start,
                        end: tok.end,
                    };
                    return Err(
                        Diagnostic::error(format!("Expected trait name, got `{}`", t))
                            .with_code("P001")
                            .with_label(span, "expected trait name"),
                    );
                }
            };
            // Optional type arguments: From[Int] or Convert[A, B]
            let type_args = if self.at(&TokenKind::LBracket) {
                self.advance();
                let mut args = Vec::new();
                loop {
                    let ty = self.parse_type()?;
                    args.push(ty);
                    if self.at(&TokenKind::Comma) {
                        self.advance();
                    } else {
                        break;
                    }
                }
                self.expect(TokenKind::RBracket)?;
                args
            } else {
                Vec::new()
            };
            refs.push((name, type_args));
            if self.at(&TokenKind::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        Ok(refs)
    }

    pub(crate) fn push_type_params(&mut self, params: &[String]) {
        for p in params {
            *self.type_params.entry(p.clone()).or_insert(0) += 1;
        }
    }

    pub(crate) fn pop_type_params(&mut self, params: &[String]) {
        for p in params {
            if let Some(count) = self.type_params.get_mut(p) {
                *count -= 1;
                if *count == 0 {
                    self.type_params.remove(p);
                }
            }
        }
    }

    pub(crate) fn parse_enum(&mut self, is_public: bool) -> Result<Stmt, Diagnostic> {
        use TokenKind::*;
        let start = self.start_span();
        self.expect(Enum)?;
        let name_tok = self.advance();
        let name = match &name_tok.kind {
            Ident(n) => n.clone(),
            t => {
                let span = Span {
                    start: name_tok.start,
                    end: name_tok.end,
                };
                return Err(
                    Diagnostic::error(format!("Expected enum name, got `{}`", t))
                        .with_code("P001")
                        .with_label(span, "expected enum name"),
                );
            }
        };

        // Optional includes: enum Color includes Eq
        let includes = if self.at(&Includes) {
            self.advance();
            self.parse_trait_ref_list()?
        } else {
            Vec::new()
        };

        self.consume_newlines();
        self.expect(Indent)?;

        let mut variants = Vec::new();
        let mut methods = Vec::new();
        while !self.at(&Dedent) && !self.at(&EOF) {
            match &self.peek().kind {
                Def => methods.push(self.parse_def_as_let(Some(name.clone()), false)?),
                Pub => {
                    self.advance();
                    let pub_next = self.peek();
                    match &pub_next.kind {
                        Def => methods.push(self.parse_def_as_let(Some(name.clone()), true)?),
                        _ => {
                            let span = Span {
                                start: pub_next.start,
                                end: pub_next.end,
                            };
                            return Err(Diagnostic::error("Expected def after 'pub' in enum")
                                .with_code("P001")
                                .with_label(span, "expected 'def'"));
                        }
                    }
                }
                Ident(_) => {
                    let vstart = self.start_span();
                    let vname = match &self.advance().kind {
                        Ident(n) => n.clone(),
                        _ => unreachable!(),
                    };
                    // Optional fields: Circle(radius: Float)
                    let fields = if self.at(&LParen) {
                        self.advance();
                        let mut fs = Vec::new();
                        if !self.at(&RParen) {
                            loop {
                                let fname_tok = self.advance();
                                let fname = match &fname_tok.kind {
                                    Ident(n) => n.clone(),
                                    t => {
                                        let span = Span {
                                            start: fname_tok.start,
                                            end: fname_tok.end,
                                        };
                                        return Err(Diagnostic::error(format!(
                                            "Expected field name in enum variant, got `{}`",
                                            t
                                        ))
                                        .with_code("P001")
                                        .with_label(span, "expected field name"));
                                    }
                                };
                                self.expect(Colon)?;
                                let ftype = self.parse_type()?;
                                fs.push((fname, ftype));
                                if self.at(&Comma) {
                                    self.advance();
                                } else {
                                    break;
                                }
                            }
                        }
                        self.expect(RParen)?;
                        fs
                    } else {
                        Vec::new()
                    };
                    variants.push(EnumVariant {
                        name: vname,
                        fields,
                        span: self.span_from(vstart),
                    });
                }
                _ => {
                    let tok = self.peek();
                    let span = Span {
                        start: tok.start,
                        end: tok.end,
                    };
                    return Err(Diagnostic::error(format!(
                        "Expected variant name or method in enum, got `{}`",
                        tok.kind
                    ))
                    .with_code("P001")
                    .with_label(span, "unexpected token in enum body"));
                }
            }
            self.consume_newlines();
        }

        self.expect(Dedent)?;

        Ok(Stmt::Enum {
            name,
            variants,
            methods,
            includes,
            is_public,
            span: self.span_from(start),
        })
    }

    pub(crate) fn parse_class(&mut self, is_public: bool) -> Result<Stmt, Diagnostic> {
        use TokenKind::*;
        let start = self.start_span();
        self.expect(Class)?;
        let name_tok = self.advance();
        let name = match &name_tok.kind {
            Ident(n) => n.clone(),
            t => {
                let span = Span {
                    start: name_tok.start,
                    end: name_tok.end,
                };
                return Err(
                    Diagnostic::error(format!("Expected class name, got `{}`", t))
                        .with_code("P001")
                        .with_label(span, "expected class name"),
                );
            }
        };

        // Optional generic parameters: class Stack[T] or class Pair[A, B]
        let generic_params = if self.at(&LBracket) {
            self.advance();
            Some(self.parse_bracketed_idents("type parameter")?)
        } else {
            None
        };

        // Optional extends: class NetworkError extends Error
        let extends = if self.at(&TokenKind::Extends) {
            self.advance();
            let parent_tok = self.advance();
            let parent = match &parent_tok.kind {
                Ident(n) => n.clone(),
                t => {
                    let span = Span {
                        start: parent_tok.start,
                        end: parent_tok.end,
                    };
                    return Err(Diagnostic::error(format!(
                        "Expected class name after 'extends', got `{}`",
                        t
                    ))
                    .with_code("P001")
                    .with_label(span, "expected class name"));
                }
            };
            Some(parent)
        } else {
            None
        };

        // Optional includes: class User includes Printable, From[Int]
        let includes = if self.at(&Includes) {
            self.advance();
            Some(self.parse_trait_ref_list()?)
        } else {
            None
        };

        // Push generic params into scope for field/method type resolution
        if let Some(ref gp) = generic_params {
            self.push_type_params(gp);
        }

        self.consume_newlines();
        self.expect(Indent)?;

        let mut fields = Vec::new();
        let mut methods = Vec::new();
        while !self.at(&Dedent) && !self.at(&EOF) {
            match &self.peek().kind {
                Def => methods.push(self.parse_def_as_let(Some(name.clone()), false)?),
                Pub => {
                    self.advance();
                    let pub_next = self.peek();
                    match &pub_next.kind {
                        Def => methods.push(self.parse_def_as_let(Some(name.clone()), true)?),
                        _ => {
                            let span = Span {
                                start: pub_next.start,
                                end: pub_next.end,
                            };
                            return Err(Diagnostic::error("Expected def after 'pub' in class")
                                .with_code("P001")
                                .with_label(span, "expected 'def'"));
                        }
                    }
                }
                Class => {
                    methods.push(self.parse_class(false)?);
                }
                _ => {
                    let fname_tok = self.advance();
                    let fname = match &fname_tok.kind {
                        Ident(n) => n.clone(),
                        t => {
                            let span = Span {
                                start: fname_tok.start,
                                end: fname_tok.end,
                            };
                            return Err(Diagnostic::error(format!(
                                "Expected field name, got `{}`",
                                t
                            ))
                            .with_code("P001")
                            .with_label(span, "expected field name"));
                        }
                    };
                    self.expect(Colon)?;
                    let ftype = self.parse_type()?;
                    fields.push((fname, ftype));
                }
            }
            self.consume_newlines();
        }

        self.expect(Dedent)?;

        // Pop generic params from scope
        if let Some(ref gp) = generic_params {
            self.pop_type_params(gp);
        }

        Ok(Stmt::Class {
            name,
            fields,
            methods,
            is_public,
            generic_params,
            extends,
            includes,
            span: self.span_from(start),
        })
    }

    pub(crate) fn parse_trait(&mut self, is_public: bool) -> Result<Stmt, Diagnostic> {
        use TokenKind::*;
        let start = self.start_span();
        self.expect(Trait)?;
        let name_tok = self.advance();
        let name = match &name_tok.kind {
            Ident(n) => n.clone(),
            t => {
                let span = Span {
                    start: name_tok.start,
                    end: name_tok.end,
                };
                return Err(
                    Diagnostic::error(format!("Expected trait name, got `{}`", t))
                        .with_code("P001")
                        .with_label(span, "expected trait name"),
                );
            }
        };

        // Optional generic parameters: trait From[T] or trait Convert[A, B]
        let generic_params = if self.at(&LBracket) {
            self.advance();
            Some(self.parse_bracketed_idents("type parameter")?)
        } else {
            None
        };

        // Push generic params into scope for method type resolution
        if let Some(ref gp) = generic_params {
            self.push_type_params(gp);
        }

        self.consume_newlines();
        self.expect(Indent)?;

        let mut methods = Vec::new();
        while !self.at(&Dedent) && !self.at(&EOF) {
            match &self.peek().kind {
                Def => {
                    methods.push(self.parse_def_as_let(Some(name.clone()), false)?);
                }
                Pub => {
                    self.advance();
                    let pub_next = self.peek();
                    match &pub_next.kind {
                        Def => methods.push(self.parse_def_as_let(Some(name.clone()), true)?),
                        _ => {
                            let span = Span {
                                start: pub_next.start,
                                end: pub_next.end,
                            };
                            return Err(Diagnostic::error("Expected def after 'pub' in trait")
                                .with_code("P001")
                                .with_label(span, "expected 'def'"));
                        }
                    }
                }
                _ => {
                    let tok = self.peek();
                    let span = Span {
                        start: tok.start,
                        end: tok.end,
                    };
                    return Err(Diagnostic::error(format!(
                        "Expected method definition in trait, got `{}`",
                        tok.kind
                    ))
                    .with_code("P001")
                    .with_label(span, "unexpected token in trait body"));
                }
            }
            self.consume_newlines();
        }

        self.expect(Dedent)?;

        // Pop generic params from scope
        if let Some(ref gp) = generic_params {
            self.pop_type_params(gp);
        }

        Ok(Stmt::Trait {
            name,
            methods,
            is_public,
            generic_params,
            span: self.span_from(start),
        })
    }

    pub(crate) fn parse_def_as_let(
        &mut self,
        receiver: Option<String>,
        is_public: bool,
    ) -> Result<Stmt, Diagnostic> {
        use TokenKind::*;
        let start = self.start_span();
        if self.at(&Async) {
            return Err(Diagnostic::error(
                "async def is not supported. Functions are plain def — use async f() at the call site"
            ).with_code("P001")
            .with_label(self.span_from(start), "remove 'async' keyword"));
        }

        self.expect(Def)?;
        let name_tok = self.advance();
        let name = match &name_tok.kind {
            Ident(n) => n.clone(),
            t => {
                let span = Span {
                    start: name_tok.start,
                    end: name_tok.end,
                };
                return Err(
                    Diagnostic::error(format!("Expected function name, got `{}`", t))
                        .with_code("P001")
                        .with_label(span, "expected function name"),
                );
            }
        };

        // Generic type parameters are inferred inline from param types (BC-5).
        // Bracket syntax [T] on functions is no longer supported (use class Box[T] for classes).
        if self.at(&LBracket) {
            return Err(Diagnostic::error(
                "Bracket generic syntax [T] is not supported on functions. Type parameters are inferred inline from parameter types: def f(x: T) -> T"
            ).with_code("P001"));
        }
        let generic_params: Option<Vec<String>> = None;

        let mut params: Vec<(String, Type)> = Vec::new();
        let mut defaults: Vec<Option<Expr>> = Vec::new();
        let mut type_constraints: Vec<(String, Vec<TypeConstraint>)> = Vec::new();
        let mut seen_default = false;
        if self.at(&LParen) {
            self.advance();
            if !self.at(&RParen) {
                loop {
                    let pname_tok = self.advance();
                    let pname = match &pname_tok.kind {
                        Ident(n) => n.clone(),
                        t => {
                            let span = Span {
                                start: pname_tok.start,
                                end: pname_tok.end,
                            };
                            return Err(Diagnostic::error(format!(
                                "Expected parameter name, got `{}`",
                                t
                            ))
                            .with_code("P001")
                            .with_label(span, "expected parameter name"));
                        }
                    };
                    self.expect(Colon)?;
                    let ptype = self.parse_type()?;

                    // Parse optional generic constraints: T extends Foo includes Bar
                    let mut constraints = Vec::new();
                    if self.at(&Extends) {
                        self.advance();
                        let class_tok = self.advance();
                        let class_name = match &class_tok.kind {
                            Ident(n) => n.clone(),
                            t => {
                                let span = Span {
                                    start: class_tok.start,
                                    end: class_tok.end,
                                };
                                return Err(Diagnostic::error(format!(
                                    "Expected class name after 'extends', got `{}`",
                                    t
                                ))
                                .with_code("P001")
                                .with_label(span, "expected class name"));
                            }
                        };
                        constraints.push(TypeConstraint::Extends(class_name));
                    }
                    if self.at(&Includes) {
                        self.advance();
                        let trait_tok = self.advance();
                        let trait_name = match &trait_tok.kind {
                            Ident(n) => n.clone(),
                            t => {
                                let span = Span {
                                    start: trait_tok.start,
                                    end: trait_tok.end,
                                };
                                return Err(Diagnostic::error(format!(
                                    "Expected trait name after 'includes', got `{}`",
                                    t
                                ))
                                .with_code("P001")
                                .with_label(span, "expected trait name"));
                            }
                        };
                        // Optional type args: includes From[Float]
                        let trait_args = if self.at(&LBracket) {
                            self.advance();
                            let mut args = Vec::new();
                            args.push(self.parse_type()?);
                            while self.at(&Comma) {
                                self.advance();
                                args.push(self.parse_type()?);
                            }
                            self.expect(RBracket)?;
                            args
                        } else {
                            vec![]
                        };
                        constraints.push(TypeConstraint::Includes(trait_name, trait_args));
                    }
                    if !constraints.is_empty() {
                        // Extract the type param name from the Custom type
                        let type_param_name = match &ptype {
                            Type::Custom(name, args) if args.is_empty() => name.clone(),
                            _ => {
                                return Err(Diagnostic::error(
                                    "Generic constraints can only be applied to type parameters (e.g. T extends Foo)",
                                )
                                .with_code("P001"));
                            }
                        };
                        type_constraints.push((type_param_name, constraints));
                    }

                    // Parse optional default value: = expr
                    let default_val = if self.at(&Equals) {
                        self.advance();
                        seen_default = true;
                        Some(self.parse_expr()?)
                    } else {
                        if seen_default {
                            return Err(Diagnostic::error(format!(
                                "Parameter '{}' without default follows parameter with default",
                                pname
                            ))
                            .with_code("P001"));
                        }
                        None
                    };

                    params.push((pname, ptype));
                    defaults.push(default_val);
                    if self.at(&Comma) {
                        self.advance();
                    } else {
                        break;
                    }
                }
            }
            self.expect(RParen)?;
        }

        // Optional throws: def fetch(url: String) throws NetworkError -> String
        let throws = if self.at(&TokenKind::Throws) {
            self.advance();
            Some(self.parse_type()?)
        } else {
            None
        };

        let mut ret = Type::Inferred;
        if self.at(&Arrow) {
            self.advance();
            ret = self.parse_type()?;
        }

        // Body is optional for trait method signatures (no body = abstract)
        self.consume_newlines();
        let body = if self.at(&TokenKind::Indent) {
            self.parse_block()?
        } else {
            vec![]
        };

        let lambda_span = self.span_from(start);
        let lambda = Expr::Lambda {
            params,
            ret_type: ret,
            body,
            generic_params,
            throws: throws.map(Box::new),
            type_constraints,
            defaults: Box::new(defaults),
            span: lambda_span,
        };

        let bind = if let Some(ref recv) = receiver {
            format!("{}.{}", recv, name)
        } else {
            name
        };

        Ok(Stmt::Let {
            name: bind,
            type_ann: None,
            value: lambda,
            is_public,
            span: self.span_from(start),
        })
    }
}
