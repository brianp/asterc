use std::env;
use std::fs;

use lexer::lex;
use parser::Parser;
use typecheck::typechecker::TypeChecker;

fn main() {
    // get args
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: compiler <file.ast>");
        std::process::exit(1);
    }

    let filename = &args[1];
    let source = match fs::read_to_string(filename) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Could not read file '{}': {}", filename, e);
            std::process::exit(1);
        }
    };

    // 1. Tokenize
    let tokens = match lex(&source) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Lexing failed: {e}");
            std::process::exit(1);
        }
    };

    // 2. Parse
    let mut parser = Parser::new(tokens);
    let module_ast = match parser.parse_module("Main") {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Parsing failed: {e}");
            std::process::exit(1);
        }
    };

    // 3. Typecheck
    let mut checker = TypeChecker::new();
    if let Err(e) = checker.check_module(&module_ast) {
        eprintln!("Type error: {e}");
        std::process::exit(1);
    }

    println!("✅ Type checking passed for {}", filename);
}
