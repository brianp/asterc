#![allow(dead_code)]

use ast::{Diagnostic, ParseResult};
use lexer::lex;
use parser::Parser;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
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

pub fn check_warnings(src: &str) -> Vec<Diagnostic> {
    let tokens = lex(src).expect("lex ok");
    let mut parser = Parser::new(tokens);
    let module = parser.parse_module("test").expect("parse ok");
    let mut tc = TypeChecker::new();
    let errors = tc.check_module_all(&module);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
    tc.reg.diagnostics
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

fn binary_path() -> PathBuf {
    std::env::var_os("CARGO_BIN_EXE_asterc")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/debug/asterc"))
}

pub fn cli(args: &[&str]) -> Output {
    Command::new(binary_path())
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("failed to run asterc {:?}: {}", args, e))
}

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn temp_path(prefix: &str, ext: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_nanos();
    let pid = std::process::id();
    let seq = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let suffix = if ext.is_empty() {
        OsString::from(format!("{prefix}-{pid}-{nanos}-{seq}"))
    } else {
        OsString::from(format!("{prefix}-{pid}-{nanos}-{seq}.{ext}"))
    };
    std::env::temp_dir().join(suffix)
}

pub fn make_temp_dir(prefix: &str) -> PathBuf {
    let dir = temp_path(prefix, "");
    std::fs::create_dir_all(&dir)
        .unwrap_or_else(|e| panic!("failed to create temp dir {}: {}", dir.display(), e));
    dir
}

pub fn output_text(output: &Output) -> String {
    let mut text = String::new();
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    text
}

pub fn build_and_run<P: AsRef<Path>>(source: P) -> Output {
    let output_path = temp_path("asterc-bin", "out");
    let source_arg = source.as_ref().to_string_lossy().into_owned();
    let output_arg = output_path.to_string_lossy().into_owned();
    let build = cli(&["build", &source_arg, "-o", &output_arg]);
    assert!(
        build.status.success(),
        "build failed for {}:\n{}",
        source_arg,
        output_text(&build)
    );

    Command::new(&output_path)
        .output()
        .unwrap_or_else(|e| panic!("failed to run {}: {}", output_path.display(), e))
}
