mod class_trait;
mod expr;
mod type_parser;

#[cfg(test)]
mod tests;

use std::collections::HashMap;

use ast::{Expr, Stmt};
use lexer::{Token, TokenKind};

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    pub(crate) type_params: HashMap<String, usize>,
    pub(crate) depth: usize,
}

const MAX_NESTING_DEPTH: usize = 50;
const MAX_COLLECTION_SIZE: usize = 10_000;

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Parser {
            tokens,
            pos: 0,
            depth: 0,
            type_params: HashMap::new(),
        }
    }

    pub(crate) fn peek(&self) -> &Token {
        static EOF_TOKEN: Token = Token {
            kind: TokenKind::EOF,
            line: 0,
            col: 0,
        };
        self.tokens.get(self.pos).unwrap_or(&EOF_TOKEN)
    }

    pub(crate) fn at(&self, kind: &TokenKind) -> bool {
        &self.peek().kind == kind
    }

    pub(crate) fn advance(&mut self) -> Token {
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
        self.tokens.get(self.pos - 1).cloned().unwrap_or(Token {
            kind: TokenKind::EOF,
            line: 0,
            col: 0,
        })
    }

    pub(crate) fn expect(&mut self, kind: TokenKind) -> Result<(), String> {
        let token = self.advance();
        if token.kind == kind {
            Ok(())
        } else {
            Err(format!("Expected {:?}, found {:?}", kind, token.kind))
        }
    }

    pub(crate) fn consume_newlines(&mut self) {
        while self.at(&TokenKind::Newline) {
            self.advance();
        }
    }

    pub(crate) fn parse_block(&mut self) -> Result<Vec<Stmt>, String> {
        self.depth += 1;
        if self.depth > MAX_NESTING_DEPTH {
            return Err(format!(
                "Nesting depth exceeds maximum of {}",
                MAX_NESTING_DEPTH
            ));
        }
        let result = self.parse_block_inner();
        self.depth -= 1;
        result
    }

    fn parse_block_inner(&mut self) -> Result<Vec<Stmt>, String> {
        self.consume_newlines();
        self.expect(TokenKind::Indent)?;
        let mut body = Vec::new();
        while !self.at(&TokenKind::Dedent) && !self.at(&TokenKind::EOF) {
            body.push(self.parse_stmt()?);
            self.consume_newlines();
        }
        self.expect(TokenKind::Dedent)?;
        Ok(body)
    }

    // ─── Module & statements ────────────────────────────────────────

    pub fn parse_module(&mut self, name: &str) -> Result<ast::Module, String> {
        let mut body = Vec::new();
        while !self.at(&TokenKind::EOF) {
            body.push(self.parse_stmt()?);
            self.consume_newlines();
        }
        Ok(ast::Module {
            name: name.to_string(),
            body,
        })
    }

    fn parse_stmt(&mut self) -> Result<Stmt, String> {
        self.consume_newlines();
        match &self.peek().kind {
            TokenKind::Use => self.parse_use(),
            TokenKind::Pub => self.parse_pub(),
            TokenKind::Trait => self.parse_trait(false),
            TokenKind::Class => self.parse_class(false),
            TokenKind::Def => self.parse_def_as_let(None, false),
            TokenKind::Async => {
                // Could be `async def`, `async scope`, or `async expr()` at statement level
                // Peek ahead to decide
                if self.tokens.get(self.pos + 1).map(|t| &t.kind) == Some(&TokenKind::Def) {
                    self.parse_def_as_let(None, false)
                } else if self.tokens.get(self.pos + 1).map(|t| &t.kind) == Some(&TokenKind::Scope)
                {
                    // async scope block
                    self.advance(); // consume async
                    self.advance(); // consume scope
                    let body = self.parse_block()?;
                    Ok(Stmt::Expr(Expr::AsyncScope { body }))
                } else {
                    // async as call-site modifier — parse as expression
                    let e = self.parse_expr()?;
                    if self.at(&TokenKind::Equals) {
                        self.advance();
                        let value = self.parse_expr()?;
                        return Ok(Stmt::Assignment { target: e, value });
                    }
                    Ok(Stmt::Expr(e))
                }
            }
            TokenKind::If => self.parse_if(),
            TokenKind::While => self.parse_while(),
            TokenKind::For => self.parse_for(),
            TokenKind::Let => self.parse_let(false),
            TokenKind::Return => {
                self.advance();
                let expr = self.parse_expr()?;
                Ok(Stmt::Return(expr))
            }
            TokenKind::Break => {
                self.advance();
                Ok(Stmt::Break)
            }
            TokenKind::Continue => {
                self.advance();
                Ok(Stmt::Continue)
            }
            // Throw, Match, Resolve, Detached all start expressions — fall through
            _ => {
                let e = self.parse_expr()?;
                // Check for assignment: expr = value
                if self.at(&TokenKind::Equals) {
                    self.advance();
                    let value = self.parse_expr()?;
                    return Ok(Stmt::Assignment { target: e, value });
                }
                Ok(Stmt::Expr(e))
            }
        }
    }

    fn parse_use(&mut self) -> Result<Stmt, String> {
        self.expect(TokenKind::Use)?;
        let mut path = Vec::new();

        // First segment
        let name = match &self.advance().kind {
            TokenKind::Ident(n) => n.clone(),
            t => return Err(format!("Expected module name after 'use', got {:?}", t)),
        };
        path.push(name);

        // Additional path segments: use std/http/thing
        while self.at(&TokenKind::Slash) {
            self.advance();
            let seg = match &self.advance().kind {
                TokenKind::Ident(n) => n.clone(),
                t => return Err(format!("Expected module name after '/', got {:?}", t)),
            };
            path.push(seg);
        }

        // Optional selective imports: { Name1, Name2 }
        let names = if self.at(&TokenKind::LBrace) {
            self.advance();
            let mut names = Vec::new();
            if !self.at(&TokenKind::RBrace) {
                loop {
                    let n = match &self.advance().kind {
                        TokenKind::Ident(n) => n.clone(),
                        t => return Err(format!("Expected identifier in use list, got {:?}", t)),
                    };
                    names.push(n);
                    if self.at(&TokenKind::Comma) {
                        self.advance();
                    } else {
                        break;
                    }
                }
            }
            self.expect(TokenKind::RBrace)?;
            Some(names)
        } else {
            None
        };

        // Optional alias: as hs
        let alias = if self.at(&TokenKind::As) {
            self.advance();
            let a = match &self.advance().kind {
                TokenKind::Ident(n) => n.clone(),
                t => return Err(format!("Expected alias name after 'as', got {:?}", t)),
            };
            Some(a)
        } else {
            None
        };

        Ok(Stmt::Use { path, names, alias })
    }

    fn parse_pub(&mut self) -> Result<Stmt, String> {
        self.expect(TokenKind::Pub)?;
        match &self.peek().kind {
            TokenKind::Def | TokenKind::Async => self.parse_def_as_let(None, true),
            TokenKind::Class => self.parse_class(true),
            TokenKind::Trait => self.parse_trait(true),
            TokenKind::Let => self.parse_let(true),
            t => Err(format!(
                "Expected def, class, trait, or let after 'pub', got {:?}",
                t
            )),
        }
    }

    fn parse_if(&mut self) -> Result<Stmt, String> {
        use TokenKind::*;
        self.expect(If)?;
        let cond = self.parse_expr()?;
        let then_body = self.parse_block()?;

        let mut elif_branches = Vec::new();
        while self.at(&Elif) {
            self.advance();
            let elif_cond = self.parse_expr()?;
            let elif_body = self.parse_block()?;
            elif_branches.push((elif_cond, elif_body));
        }

        let mut else_body = Vec::new();
        if self.at(&Else) {
            self.advance();
            else_body = self.parse_block()?;
        }

        Ok(Stmt::If {
            cond,
            then_body,
            elif_branches,
            else_body,
        })
    }

    fn parse_while(&mut self) -> Result<Stmt, String> {
        self.expect(TokenKind::While)?;
        let cond = self.parse_expr()?;
        let body = self.parse_block()?;
        Ok(Stmt::While { cond, body })
    }

    fn parse_for(&mut self) -> Result<Stmt, String> {
        use TokenKind::*;
        self.expect(For)?;
        let var = match &self.advance().kind {
            Ident(n) => n.clone(),
            t => return Err(format!("Expected variable name after 'for', got {:?}", t)),
        };
        self.expect(In)?;
        let iter = self.parse_expr()?;
        let body = self.parse_block()?;
        Ok(Stmt::For { var, iter, body })
    }

    fn parse_let(&mut self, is_public: bool) -> Result<Stmt, String> {
        self.expect(TokenKind::Let)?;

        let name_tok = self.advance();
        let name = if let TokenKind::Ident(s) = name_tok.kind {
            s
        } else {
            return Err(format!(
                "Expected identifier after let at line {}",
                name_tok.line
            ));
        };

        let type_ann = if self.at(&TokenKind::Colon) {
            self.advance();
            Some(self.parse_type()?)
        } else {
            None
        };

        self.expect(TokenKind::Equals)?;
        let value = self.parse_expr()?;

        Ok(Stmt::Let {
            name,
            type_ann,
            value,
            is_public,
        })
    }
}
