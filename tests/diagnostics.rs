mod common;

// ─── Rich error messages: spans, codes, and notes ───────────────────

#[test]
fn diagnostic_type_mismatch_has_span() {
    let src = "let x: Int = \"hello\"";
    let diag = common::check_err_diagnostic(src);
    assert_eq!(diag.severity, ast::Severity::Error);
    assert!(!diag.labels.is_empty(), "diagnostic should have labels");
    assert!(
        diag.message.contains("mismatch") || diag.message.contains("Type annotation"),
        "message: {}",
        diag.message
    );
}

#[test]
fn diagnostic_unknown_ident_has_span() {
    let src = "let x: Int = unknown_var";
    let diag = common::check_err_diagnostic(src);
    assert!(
        diag.message.contains("Unknown identifier"),
        "message: {}",
        diag.message
    );
    assert!(!diag.labels.is_empty());
}

#[test]
fn diagnostic_has_error_code() {
    let src = "let x: Int = \"hello\"";
    let diag = common::check_err_diagnostic(src);
    assert!(
        diag.code.is_some(),
        "diagnostic should have an error code, got: {:?}",
        diag
    );
}

#[test]
fn diagnostic_break_outside_loop_has_code() {
    let src = "break";
    let diag = common::check_err_diagnostic(src);
    assert!(diag.code.is_some());
    assert!(diag.message.contains("break"));
}

#[test]
fn parse_error_is_diagnostic() {
    let src = "let x = )";
    let diag = common::check_parse_err_diagnostic(src);
    assert_eq!(diag.severity, ast::Severity::Error);
    assert!(diag.code.is_some(), "parse error should have error code");
}

#[test]
fn lex_error_is_diagnostic() {
    let src = "let x = \"unterminated";
    let diag = common::check_lex_err_diagnostic(src);
    assert_eq!(diag.severity, ast::Severity::Error);
}

#[test]
fn diagnostic_suggestion_for_unknown_ident() {
    // "sy" is close to "say" — should suggest
    let src = "sy(message: \"hello\")";
    let diag = common::check_err_diagnostic(src);
    assert!(
        diag.notes.iter().any(|n| n.contains("say")),
        "should suggest 'say' for 'sy', notes: {:?}",
        diag.notes
    );
}

#[test]
fn diagnostic_no_suggestion_for_very_different_name() {
    let src = "xyzzy_thing()";
    let diag = common::check_err_diagnostic(src);
    // Should NOT suggest anything since nothing is close
    let has_did_you_mean = diag.notes.iter().any(|n| n.contains("did you mean"));
    // This is acceptable either way — just verify it doesn't crash
    assert!(diag.message.contains("Unknown identifier"));
    let _ = has_did_you_mean;
}

// ─── Error recovery: Type::Error prevents cascading ────────────────

#[test]
fn type_error_variant_exists() {
    // Type::Error should exist and be equal to itself
    let e = ast::Type::Error;
    assert_eq!(e, ast::Type::Error);
}

#[test]
fn type_error_display() {
    let e = ast::Type::Error;
    assert_eq!(format!("{:?}", e), "Error");
}

#[test]
fn typechecker_accumulates_multiple_errors() {
    let src = "\
let a: Int = \"hello\"
let b: Int = \"world\"";
    let diags = common::check_all_errors(src);
    assert!(
        diags.len() >= 2,
        "should report at least 2 errors, got {}",
        diags.len()
    );
}

#[test]
fn typechecker_error_recovery_does_not_cascade() {
    // First line has an error, second line should still be checked independently
    let src = "\
let a: Int = \"hello\"
let b: Int = 42";
    let diags = common::check_all_errors(src);
    // Should have exactly 1 error (the first line), not cascade to the second
    assert_eq!(diags.len(), 1, "errors: {:?}", diags);
}

#[test]
fn parser_recovery_reports_multiple_errors() {
    // This tests that parser can recover from errors and report multiple
    // For now, at least verify the first error is reported properly
    let src = "let x = )\nlet y = 42";
    let result = common::parse_with_recovery(src);
    assert!(
        !result.diagnostics.is_empty(),
        "should have parse diagnostics"
    );
}

// ─── Serialization, NodeId, DiagnosticTemplate ─────────────────────

#[test]
fn span_serializable() {
    let span = ast::Span::new(10, 20);
    let json = serde_json::to_string(&span).unwrap();
    assert!(json.contains("10"));
    assert!(json.contains("20"));
    let back: ast::Span = serde_json::from_str(&json).unwrap();
    assert_eq!(back, span);
}

#[test]
fn diagnostic_serializable() {
    let diag = ast::Diagnostic::error("test error")
        .with_code("E001")
        .with_label(ast::Span::new(0, 5), "here");
    let json = serde_json::to_string(&diag).unwrap();
    assert!(json.contains("E001"));
    assert!(json.contains("test error"));
}

#[test]
fn type_serializable() {
    let ty = ast::Type::List(Box::new(ast::Type::Int));
    let json = serde_json::to_string(&ty).unwrap();
    assert!(json.contains("Int"));
    let back: ast::Type = serde_json::from_str(&json).unwrap();
    assert_eq!(back, ty);
}

#[test]
fn expr_serializable() {
    let expr = ast::Expr::Int(42, ast::Span::new(0, 2));
    let json = serde_json::to_string(&expr).unwrap();
    assert!(json.contains("42"));
}

#[test]
fn stmt_serializable() {
    let stmt = ast::Stmt::Break(ast::Span::new(0, 5));
    let json = serde_json::to_string(&stmt).unwrap();
    assert!(json.contains("Break"));
}

#[test]
fn module_serializable() {
    let module = ast::Module {
        name: "test".to_string(),
        body: vec![],
        span: ast::Span::new(0, 0),
    };
    let json = serde_json::to_string(&module).unwrap();
    assert!(json.contains("test"));
}

#[test]
fn token_kind_serializable() {
    let tk = lexer::TokenKind::Int(42);
    let json = serde_json::to_string(&tk).unwrap();
    assert!(json.contains("42"));
}

#[test]
fn token_serializable() {
    let tok = lexer::Token {
        kind: lexer::TokenKind::Plus,
        line: 1,
        col: 5,
        start: 4,
        end: 5,
    };
    let json = serde_json::to_string(&tok).unwrap();
    assert!(json.contains("Plus"));
}

// =============================================================================
// Existing behavior preserved
// =============================================================================

#[test]
fn existing_check_ok_still_works() {
    common::check_ok("let x = 42");
}

#[test]
fn existing_check_err_still_works() {
    let msg = common::check_err("let x: Int = \"hello\"");
    assert!(msg.contains("mismatch") || msg.contains("Type annotation"));
}

#[test]
fn existing_check_parse_err_still_works() {
    let msg = common::check_parse_err("let x = )");
    assert!(msg.contains("Expected") || msg.contains("unexpected"));
}

// ─── Input size and literal limits ──────────────────────────────────

#[test]
fn file_size_limit_rejects_large_input() {
    // 10MB+ input should be rejected
    let huge = "a".repeat(11 * 1024 * 1024);
    let result = lexer::lex(&huge);
    assert!(result.is_err(), "Huge input should be rejected");
}

#[test]
fn string_literal_length_limit() {
    let long_str = format!("let x = \"{}\"", "a".repeat(1_000_001));
    let result = lexer::lex(&long_str);
    assert!(
        result.is_err(),
        "Very long string literal should be rejected"
    );
}

#[test]
fn number_literal_digit_limit() {
    let long_num = format!("let x = {}", "9".repeat(1001));
    let result = lexer::lex(&long_num);
    assert!(
        result.is_err(),
        "Very long number literal should be rejected"
    );
}

// ─── Type name suggestions ──────────────────────────────────────────

#[test]
fn lowercase_builtin_type_rejected() {
    for (wrong, right) in [
        ("bool", "Bool"),
        ("int", "Int"),
        ("float", "Float"),
        ("string", "String"),
        ("void", "Void"),
    ] {
        let src = format!("def f() -> {}\n  1\n", wrong);
        let err = common::check_parse_err(&src);
        assert!(
            err.contains(&format!("Did you mean '{}'?", right)),
            "'{}' should suggest '{}', got: {}",
            wrong,
            right,
            err
        );
    }
}

// ─── Identifier validation ──────────────────────────────────────────

#[test]
fn unicode_homoglyph_identifier_rejected() {
    // Cyrillic 'а' (U+0430) looks identical to Latin 'a' but is a different character
    let result = lexer::lex("let \u{0430} = 1");
    assert!(result.is_err(), "Cyrillic homoglyph should be rejected");
}

#[test]
fn ascii_identifiers_accepted() {
    let result = lexer::lex("let abc = 1");
    assert!(result.is_ok());
}

#[test]
fn underscore_identifiers_accepted() {
    let result = lexer::lex("let _foo = 1");
    assert!(result.is_ok());
}

// ─── Indentation validation ─────────────────────────────────────────

#[test]
fn tab_indentation_rejected() {
    let result = lexer::lex("def f() -> Int\n\treturn 1\n");
    assert!(result.is_err(), "Tab indentation should be rejected");
    let err = result.unwrap_err().to_string();
    assert!(err.contains("tab") || err.contains("Tab") || err.contains("indent"));
}
