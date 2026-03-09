use std::env;
use std::fs;

use ariadne::{Color, Label, Report, ReportKind, Source};

use ast::{Diagnostic, Severity};
use lexer::lex;
use parser::Parser;
use typecheck::typechecker::TypeChecker;

fn render_diagnostic(source: &str, filename: &str, diag: &Diagnostic) {
    let kind = match diag.severity {
        Severity::Error => ReportKind::Error,
        Severity::Warning => ReportKind::Warning,
        Severity::Hint => ReportKind::Advice,
    };

    let offset = diag.labels.first().map(|l| l.span.start).unwrap_or(0);

    let mut report = Report::build(kind, filename, offset);

    if let Some(ref code) = diag.code {
        report = report.with_code(code);
    }

    report = report.with_message(&diag.message);

    for (i, label) in diag.labels.iter().enumerate() {
        let color = if i == 0 { Color::Red } else { Color::Blue };
        report = report.with_label(
            Label::new((filename, label.span.start..label.span.end))
                .with_message(&label.message)
                .with_color(color),
        );
    }

    for note in &diag.notes {
        report = report.with_note(note);
    }

    report
        .finish()
        .eprint((filename, Source::from(source)))
        .unwrap();
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: asterc <file.aster>");
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
        Err(diag) => {
            render_diagnostic(&source, filename, &diag);
            std::process::exit(1);
        }
    };

    // 2. Parse
    let mut parser = Parser::new(tokens);
    let module_ast = match parser.parse_module(filename) {
        Ok(m) => m,
        Err(diag) => {
            render_diagnostic(&source, filename, &diag);
            std::process::exit(1);
        }
    };

    // 3. Typecheck (accumulate all errors)
    let mut checker = TypeChecker::new();
    let diagnostics = checker.check_module_all(&module_ast);

    if !diagnostics.is_empty() {
        for diag in &diagnostics {
            render_diagnostic(&source, filename, diag);
        }
        let error_count = diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count();
        eprintln!(
            "\n{} error{} emitted",
            error_count,
            if error_count == 1 { "" } else { "s" }
        );
        std::process::exit(1);
    }

    println!("Type checking passed for {}", filename);
}
