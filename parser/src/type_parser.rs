use ast::Type;
use lexer::TokenKind;

use crate::Parser;

impl Parser {
    pub(crate) fn parse_type(&mut self) -> Result<Type, String> {
        // Function type: (T, U) -> R
        if self.at(&TokenKind::LParen) {
            self.advance();
            let mut params = Vec::new();
            if !self.at(&TokenKind::RParen) {
                params.push(self.parse_type()?);
                while self.at(&TokenKind::Comma) {
                    self.advance();
                    params.push(self.parse_type()?);
                }
            }
            self.expect(TokenKind::RParen)?;
            self.expect(TokenKind::Arrow)?;
            let ret = self.parse_type()?;
            return Ok(Type::Function {
                params,
                ret: Box::new(ret),
                is_async: false,
                throws: None,
            });
        }

        let name = match &self.advance().kind {
            TokenKind::Ident(n) => n.clone(),
            t => return Err(format!("Expected type name, got {:?}", t)),
        };

        if name == "List" && self.at(&TokenKind::LBracket) {
            self.advance();
            let inner = self.parse_type()?;
            self.expect(TokenKind::RBracket)?;
            return self.maybe_nullable(Type::List(Box::new(inner)));
        }

        if name == "Map" && self.at(&TokenKind::LBracket) {
            self.advance();
            let key = self.parse_type()?;
            self.expect(TokenKind::Comma)?;
            let val = self.parse_type()?;
            self.expect(TokenKind::RBracket)?;
            return self.maybe_nullable(Type::Map(Box::new(key), Box::new(val)));
        }

        if name == "Task" && self.at(&TokenKind::LBracket) {
            self.advance();
            let inner = self.parse_type()?;
            self.expect(TokenKind::RBracket)?;
            return self.maybe_nullable(Type::Task(Box::new(inner)));
        }

        if self.type_params.get(&name).map_or(false, |c| *c > 0) {
            let base = Type::TypeVar(name);
            return self.maybe_nullable(base);
        }

        let base = Type::from_ident(&name);
        self.maybe_nullable(base)
    }

    fn maybe_nullable(&mut self, base: Type) -> Result<Type, String> {
        if self.at(&TokenKind::Question) {
            self.advance();
            // No nested nullability: T?? is a compile error
            if self.at(&TokenKind::Question) {
                return Err("Nested nullable types (T??) are not allowed".to_string());
            }
            // Don't allow Nullable(Nullable(...))
            if matches!(&base, Type::Nullable(_)) {
                return Err("Nested nullable types are not allowed".to_string());
            }
            Ok(Type::Nullable(Box::new(base)))
        } else {
            Ok(base)
        }
    }
}
