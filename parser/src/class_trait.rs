use ast::{Expr, Stmt, Type};
use lexer::TokenKind;

use crate::Parser;

impl Parser {
    /// Parse a comma-separated list of identifiers inside brackets: `[A, B, C]`.
    /// The opening `[` must already be consumed. Returns the list of names.
    pub(crate) fn parse_bracketed_idents(&mut self, context: &str) -> Result<Vec<String>, String> {
        let mut names = Vec::new();
        loop {
            let name = match &self.advance().kind {
                TokenKind::Ident(n) => n.clone(),
                t => return Err(format!("Expected {} name, got {:?}", context, t)),
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

    /// Parse comma-separated identifiers (no brackets). Used for `includes Trait1, Trait2`.
    pub(crate) fn parse_ident_list(&mut self, context: &str) -> Result<Vec<String>, String> {
        let mut names = Vec::new();
        loop {
            let name = match &self.advance().kind {
                TokenKind::Ident(n) => n.clone(),
                t => return Err(format!("Expected {} name, got {:?}", context, t)),
            };
            names.push(name);
            if self.at(&TokenKind::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        Ok(names)
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

    pub(crate) fn parse_class(&mut self, is_public: bool) -> Result<Stmt, String> {
        use TokenKind::*;
        self.expect(Class)?;
        let name = match &self.advance().kind {
            Ident(n) => n.clone(),
            t => return Err(format!("class name, got {:?}", t)),
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
            let parent = match &self.advance().kind {
                Ident(n) => n.clone(),
                t => return Err(format!("Expected class name after 'extends', got {:?}", t)),
            };
            Some(parent)
        } else {
            None
        };

        // Optional includes: class User includes Printable, Serializable
        let includes = if self.at(&Includes) {
            self.advance();
            Some(self.parse_ident_list("trait")?)
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
                Def | Async => methods.push(self.parse_def_as_let(Some(name.clone()), false)?),
                Pub => {
                    self.advance();
                    match &self.peek().kind {
                        Def | Async => methods.push(self.parse_def_as_let(Some(name.clone()), true)?),
                        _ => return Err("Expected def or async after 'pub' in class".to_string()),
                    }
                }
                Class => {
                    methods.push(self.parse_class(false)?);
                }
                _ => {
                    let fname = match &self.advance().kind {
                        Ident(n) => n.clone(),
                        t => return Err(format!("field name, got {:?}", t)),
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
        })
    }

    pub(crate) fn parse_trait(&mut self, is_public: bool) -> Result<Stmt, String> {
        use TokenKind::*;
        self.expect(Trait)?;
        let name = match &self.advance().kind {
            Ident(n) => n.clone(),
            t => return Err(format!("Expected trait name, got {:?}", t)),
        };

        self.consume_newlines();
        self.expect(Indent)?;

        let mut methods = Vec::new();
        while !self.at(&Dedent) && !self.at(&EOF) {
            match &self.peek().kind {
                Def | Async => {
                    methods.push(self.parse_def_as_let(Some(name.clone()), false)?);
                }
                Pub => {
                    self.advance();
                    match &self.peek().kind {
                        Def | Async => methods.push(self.parse_def_as_let(Some(name.clone()), true)?),
                        _ => return Err("Expected def or async after 'pub' in trait".to_string()),
                    }
                }
                _ => return Err(format!("Expected method definition in trait, got {:?}", self.peek().kind)),
            }
            self.consume_newlines();
        }

        self.expect(Dedent)?;
        Ok(Stmt::Trait {
            name,
            methods,
            is_public,
        })
    }

    pub(crate) fn parse_def_as_let(&mut self, receiver: Option<String>, is_public: bool) -> Result<Stmt, String> {
        use TokenKind::*;
        let mut is_async = false;
        if self.at(&Async) {
            is_async = true;
            self.advance();
        }

        self.expect(Def)?;
        let name = match &self.advance().kind {
            Ident(n) => n.clone(),
            t => return Err(format!("fn name, got {:?}", t)),
        };

        // Optional generic parameters: def identity[T](x: T) -> T
        let generic_params = if self.at(&LBracket) {
            self.advance();
            Some(self.parse_bracketed_idents("type parameter")?)
        } else {
            None
        };

        // Push generic params into scope for type resolution
        if let Some(ref gp) = generic_params {
            self.push_type_params(gp);
        }

        let mut params: Vec<(String, Type)> = Vec::new();
        if self.at(&LParen) {
            self.advance();
            if !self.at(&RParen) {
                loop {
                    let pname = match &self.advance().kind {
                        Ident(n) => n.clone(),
                        t => return Err(format!("param name, got {:?}", t)),
                    };
                    self.expect(Colon)?;
                    let ptype = self.parse_type()?;
                    params.push((pname, ptype));
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

        let mut ret = Type::Void;
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

        // Pop generic params from scope
        if let Some(ref gp) = generic_params {
            self.pop_type_params(gp);
        }

        let lambda = Expr::Lambda {
            params,
            ret_type: ret,
            body,
            is_async,
            generic_params,
            throws,
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
        })
    }
}
