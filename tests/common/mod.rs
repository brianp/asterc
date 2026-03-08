#![allow(dead_code)]

use lexer::lex;
use parser::Parser;
use typecheck::typechecker::TypeChecker;

pub fn check_ok(src: &str) {
    let tokens = lex(src).expect("lex ok");
    let mut parser = Parser::new(tokens);
    let module = parser.parse_module("test").expect("parse ok");
    let mut tc = TypeChecker::new();
    tc.check_module(&module).expect("typecheck ok");
}

pub fn check_err(src: &str) -> String {
    let tokens = lex(src).expect("lex ok");
    let mut parser = Parser::new(tokens);
    let module = parser.parse_module("test").expect("parse ok");
    let mut tc = TypeChecker::new();
    tc.check_module(&module).unwrap_err()
}

pub fn check_parse_err(src: &str) -> String {
    let tokens = lex(src).expect("lex ok");
    let mut parser = Parser::new(tokens);
    parser.parse_module("test").unwrap_err()
}
