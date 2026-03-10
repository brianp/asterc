#![allow(dead_code)]

use ast::{Diagnostic, ParseResult};
use lexer::lex;
use parser::Parser;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use typecheck::module_loader::{ModuleLoader, VirtualResolver};
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
    tc.check_module(&module).unwrap_err().to_string()
}

pub fn check_err_diagnostic(src: &str) -> Diagnostic {
    let tokens = lex(src).expect("lex ok");
    let mut parser = Parser::new(tokens);
    let module = parser.parse_module("test").expect("parse ok");
    let mut tc = TypeChecker::new();
    tc.check_module(&module).unwrap_err()
}

pub fn check_all_errors(src: &str) -> Vec<Diagnostic> {
    let tokens = lex(src).expect("lex ok");
    let mut parser = Parser::new(tokens);
    let module = parser.parse_module("test").expect("parse ok");
    let mut tc = TypeChecker::new();
    tc.check_module_all(&module)
}

pub fn check_parse_err(src: &str) -> String {
    let tokens = lex(src).expect("lex ok");
    let mut parser = Parser::new(tokens);
    parser.parse_module("test").unwrap_err().to_string()
}

pub fn check_parse_err_diagnostic(src: &str) -> Diagnostic {
    let tokens = lex(src).expect("lex ok");
    let mut parser = Parser::new(tokens);
    parser.parse_module("test").unwrap_err()
}

pub fn check_lex_err_diagnostic(src: &str) -> Diagnostic {
    lex(src).unwrap_err()
}

pub fn parse_with_recovery(src: &str) -> ParseResult {
    let tokens = lex(src).expect("lex ok");
    let mut parser = Parser::new(tokens);
    parser.parse_module_recovering("test")
}

pub fn check_ok_with_files(src: &str, files: HashMap<String, String>) {
    let tokens = lex(src).expect("lex ok");
    let mut parser = Parser::new(tokens);
    let module = parser.parse_module("test").expect("parse ok");
    let resolver = VirtualResolver { files };
    let loader = Rc::new(RefCell::new(ModuleLoader::new(Box::new(resolver))));
    let mut tc = TypeChecker::with_loader(loader);
    tc.check_module(&module).expect("typecheck ok");
}

pub fn check_err_with_files(src: &str, files: HashMap<String, String>) -> String {
    let tokens = lex(src).expect("lex ok");
    let mut parser = Parser::new(tokens);
    let module = parser.parse_module("test").expect("parse ok");
    let resolver = VirtualResolver { files };
    let loader = Rc::new(RefCell::new(ModuleLoader::new(Box::new(resolver))));
    let mut tc = TypeChecker::with_loader(loader);
    tc.check_module(&module).unwrap_err().to_string()
}

pub fn compile_file(path: &str) {
    let source =
        std::fs::read_to_string(path).unwrap_or_else(|_| panic!("Could not read {}", path));
    let tokens = lex(&source).unwrap_or_else(|e| panic!("Lex error in {}: {}", path, e));
    let mut parser = Parser::new(tokens);
    let module = parser
        .parse_module("test")
        .unwrap_or_else(|e| panic!("Parse error in {}: {}", path, e));
    let mut tc = TypeChecker::new();
    tc.check_module(&module)
        .unwrap_or_else(|e| panic!("Type error in {}: {}", path, e));
}
