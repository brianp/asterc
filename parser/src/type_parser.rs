use ast::{Diagnostic, Span, Type};
use lexer::TokenKind;

use crate::Parser;

impl Parser {
    pub(crate) fn parse_type(&mut self) -> Result<Type, Diagnostic> {
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
                param_names: (0..params.len()).map(|i| format!("_{}", i)).collect(),
                params,
                ret: Box::new(ret),
                throws: None,
                suspendable: false,
            });
        }

        let name_tok = self.advance();
        let name = match &name_tok.kind {
            TokenKind::Ident(n) => n.clone(),
            t => {
                let span = Span { start: name_tok.start, end: name_tok.end };
                return Err(
                    Diagnostic::error(format!("Expected type name, got `{}`", t))
                        .with_code("P001")
                        .with_label(span, "not a valid type name"),
                );
            }
        };

        // Catch lowercase built-in type names early with a clear error
        let correct = match name.as_str() {
            "int" => Some("Int"),
            "float" => Some("Float"),
            "bool" => Some("Bool"),
            "string" => Some("String"),
            "void" => Some("Void"),
            "nil" => Some("Nil"),
            "never" => Some("Never"),
            "list" => Some("List"),
            "map" => Some("Map"),
            "task" => Some("Task"),
            _ => None,
        };
        if let Some(correct) = correct {
            return Err(Diagnostic::error(format!(
                "Unknown type '{}'. Did you mean '{}'?",
                name, correct
            ))
            .with_code("P001")
            .with_label(
                self.span_from(self.pos - 1),
                format!("use '{}' instead", correct),
            ));
        }

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

        if self.type_params.get(&name).is_some_and(|c| *c > 0) {
            let base = Type::TypeVar(name, vec![]);
            return self.maybe_nullable(base);
        }

        // Generic type arguments for custom types: MyClass[T, U]
        if self.at(&TokenKind::LBracket) {
            self.advance();
            let mut type_args = Vec::new();
            type_args.push(self.parse_type()?);
            while self.at(&TokenKind::Comma) {
                self.advance();
                type_args.push(self.parse_type()?);
            }
            self.expect(TokenKind::RBracket)?;
            let base = Type::Custom(name, type_args);
            return self.maybe_nullable(base);
        }

        let base = Type::from_ident(&name);
        self.maybe_nullable(base)
    }

    fn maybe_nullable(&mut self, base: Type) -> Result<Type, Diagnostic> {
        if self.at(&TokenKind::Question) {
            self.advance();
            // No nested nullability: T?? is a compile error
            if self.at(&TokenKind::Question) {
                let tok = self.peek();
                let span = Span { start: tok.start, end: tok.end };
                return Err(
                    Diagnostic::error("Nested nullable types (T??) are not allowed")
                        .with_code("P001")
                        .with_label(span, "remove this second '?'"),
                );
            }
            // Don't allow Nullable(Nullable(...))
            if matches!(&base, Type::Nullable(_)) {
                let tok = self.peek();
                let span = Span { start: tok.start, end: tok.end };
                return Err(
                    Diagnostic::error("Nested nullable types are not allowed")
                        .with_code("P001")
                        .with_label(span, "type is already nullable"),
                );
            }
            Ok(Type::Nullable(Box::new(base)))
        } else {
            Ok(base)
        }
    }
}
