// ─── Phase 2: DiagnosticTemplate specification as tests ─────────────
//
// These tests define the contract for the DiagnosticTemplate system
// introduced by GH issue #25. Each test asserts one behavior.

// ═══════════════════════════════════════════════════════════════════════
// Contract tests: types exist and have required methods
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn template_enum_exists_and_wraps_type_errors() {
    use ast::templates::DiagnosticTemplate;
    use ast::templates::type_errors::TypeMismatch;

    let t = DiagnosticTemplate::TypeMismatch(TypeMismatch {
        expected: ast::Type::Int,
        actual: ast::Type::String,
    });
    assert!(matches!(t, DiagnosticTemplate::TypeMismatch(_)));
}

#[test]
fn template_enum_wraps_parse_errors() {
    use ast::templates::DiagnosticTemplate;
    use ast::templates::parse_errors::UnexpectedToken;

    let t = DiagnosticTemplate::UnexpectedToken(UnexpectedToken {
        expected: "identifier".into(),
        found: ")".into(),
    });
    assert!(matches!(t, DiagnosticTemplate::UnexpectedToken(_)));
}

#[test]
fn template_enum_wraps_module_errors() {
    use ast::templates::DiagnosticTemplate;
    use ast::templates::module_errors::ModuleNotFound;

    let t = DiagnosticTemplate::ModuleNotFound(ModuleNotFound { name: "foo".into() });
    assert!(matches!(t, DiagnosticTemplate::ModuleNotFound(_)));
}

#[test]
fn template_enum_wraps_warnings() {
    use ast::templates::DiagnosticTemplate;
    use ast::templates::warnings::ShadowedVariable;

    let t = DiagnosticTemplate::ShadowedVariable(ShadowedVariable { name: "x".into() });
    assert!(matches!(t, DiagnosticTemplate::ShadowedVariable(_)));
}

#[test]
fn template_enum_wraps_lex_errors() {
    use ast::templates::DiagnosticTemplate;
    use ast::templates::lex_errors::UnterminatedString;

    let t = DiagnosticTemplate::UnterminatedString(UnterminatedString);
    assert!(matches!(t, DiagnosticTemplate::UnterminatedString(_)));
}

// ═══════════════════════════════════════════════════════════════════════
// Code tests: each template returns its stable error code
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn type_mismatch_code_is_e001() {
    use ast::templates::type_errors::TypeMismatch;
    let t = TypeMismatch {
        expected: ast::Type::Int,
        actual: ast::Type::String,
    };
    assert_eq!(t.code(), "E001");
}

#[test]
fn undefined_variable_code_is_e002() {
    use ast::templates::type_errors::UndefinedVariable;
    let t = UndefinedVariable { name: "foo".into() };
    assert_eq!(t.code(), "E002");
}

#[test]
fn unexpected_token_code_is_p001() {
    use ast::templates::parse_errors::UnexpectedToken;
    let t = UnexpectedToken {
        expected: "identifier".into(),
        found: ")".into(),
    };
    assert_eq!(t.code(), "P001");
}

#[test]
fn module_not_found_code_is_m001() {
    use ast::templates::module_errors::ModuleNotFound;
    let t = ModuleNotFound { name: "foo".into() };
    assert_eq!(t.code(), "M001");
}

#[test]
fn shadowed_variable_code_is_w003() {
    use ast::templates::warnings::ShadowedVariable;
    let t = ShadowedVariable { name: "x".into() };
    assert_eq!(t.code(), "W003");
}

#[test]
fn unterminated_string_code_is_l002() {
    use ast::templates::lex_errors::UnterminatedString;
    let t = UnterminatedString;
    assert_eq!(t.code(), "L002");
}

// ═══════════════════════════════════════════════════════════════════════
// Render tests: templates produce human-readable messages
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn type_mismatch_render_contains_both_types() {
    use ast::templates::type_errors::TypeMismatch;
    let t = TypeMismatch {
        expected: ast::Type::Int,
        actual: ast::Type::String,
    };
    let msg = t.render();
    assert!(
        msg.contains("Int"),
        "render should mention expected type: {}",
        msg
    );
    assert!(
        msg.contains("String"),
        "render should mention actual type: {}",
        msg
    );
}

#[test]
fn undefined_variable_render_contains_name() {
    use ast::templates::type_errors::UndefinedVariable;
    let t = UndefinedVariable {
        name: "my_var".into(),
    };
    let msg = t.render();
    assert!(
        msg.contains("my_var"),
        "render should mention the variable name: {}",
        msg
    );
}

#[test]
fn unexpected_token_render_contains_expected_and_found() {
    use ast::templates::parse_errors::UnexpectedToken;
    let t = UnexpectedToken {
        expected: "identifier".into(),
        found: ")".into(),
    };
    let msg = t.render();
    assert!(
        msg.contains("identifier"),
        "render should mention expected: {}",
        msg
    );
    assert!(msg.contains(")"), "render should mention found: {}", msg);
}

// ═══════════════════════════════════════════════════════════════════════
// Enum-level code() and render() delegation (macro-generated)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn template_enum_delegates_code() {
    use ast::templates::DiagnosticTemplate;
    use ast::templates::type_errors::TypeMismatch;

    let t = DiagnosticTemplate::TypeMismatch(TypeMismatch {
        expected: ast::Type::Int,
        actual: ast::Type::String,
    });
    assert_eq!(t.code(), "E001");
}

#[test]
fn template_enum_delegates_render() {
    use ast::templates::DiagnosticTemplate;
    use ast::templates::type_errors::TypeMismatch;

    let t = DiagnosticTemplate::TypeMismatch(TypeMismatch {
        expected: ast::Type::Int,
        actual: ast::Type::String,
    });
    let msg = t.render();
    assert!(msg.contains("Int") && msg.contains("String"));
}

// ═══════════════════════════════════════════════════════════════════════
// Diagnostic integration: from_template constructor
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn from_template_sets_template_field() {
    use ast::templates::DiagnosticTemplate;
    use ast::templates::type_errors::TypeMismatch;

    let diag = ast::Diagnostic::from_template(DiagnosticTemplate::TypeMismatch(TypeMismatch {
        expected: ast::Type::Int,
        actual: ast::Type::String,
    }));

    assert!(diag.template.is_some());
}

#[test]
fn from_template_populates_message_from_render() {
    use ast::templates::DiagnosticTemplate;
    use ast::templates::type_errors::TypeMismatch;

    let template = DiagnosticTemplate::TypeMismatch(TypeMismatch {
        expected: ast::Type::Int,
        actual: ast::Type::String,
    });
    let expected_message = template.render();

    let diag = ast::Diagnostic::from_template(template);
    assert_eq!(diag.message, expected_message);
}

#[test]
fn from_template_sets_severity_to_error() {
    use ast::templates::DiagnosticTemplate;
    use ast::templates::type_errors::TypeMismatch;

    let diag = ast::Diagnostic::from_template(DiagnosticTemplate::TypeMismatch(TypeMismatch {
        expected: ast::Type::Int,
        actual: ast::Type::String,
    }));
    assert_eq!(diag.severity, ast::Severity::Error);
}

#[test]
fn from_template_code_accessible_via_template() {
    use ast::templates::DiagnosticTemplate;
    use ast::templates::type_errors::TypeMismatch;

    let diag = ast::Diagnostic::from_template(DiagnosticTemplate::TypeMismatch(TypeMismatch {
        expected: ast::Type::Int,
        actual: ast::Type::String,
    }));

    assert_eq!(diag.template.as_ref().unwrap().code(), "E001");
}

#[test]
fn diagnostic_without_template_still_works() {
    let diag = ast::Diagnostic::error("manual error message");
    assert!(diag.template.is_none());
    assert_eq!(diag.message, "manual error message");
}

#[test]
fn from_template_still_chains_with_label() {
    use ast::templates::DiagnosticTemplate;
    use ast::templates::type_errors::TypeMismatch;

    let diag = ast::Diagnostic::from_template(DiagnosticTemplate::TypeMismatch(TypeMismatch {
        expected: ast::Type::Int,
        actual: ast::Type::String,
    }))
    .with_label(ast::Span::new(0, 5), "here");

    assert_eq!(diag.labels.len(), 1);
    assert!(diag.template.is_some());
}

// ═══════════════════════════════════════════════════════════════════════
// Serialization: templates serialize for agent consumption
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn template_serializes_to_json_with_variant_name() {
    use ast::templates::DiagnosticTemplate;
    use ast::templates::type_errors::TypeMismatch;

    let t = DiagnosticTemplate::TypeMismatch(TypeMismatch {
        expected: ast::Type::Int,
        actual: ast::Type::String,
    });
    let json = serde_json::to_string(&t).unwrap();
    assert!(
        json.contains("TypeMismatch"),
        "JSON should contain variant name: {}",
        json
    );
}

#[test]
fn template_deserializes_from_json() {
    use ast::templates::DiagnosticTemplate;
    use ast::templates::type_errors::TypeMismatch;

    let t = DiagnosticTemplate::TypeMismatch(TypeMismatch {
        expected: ast::Type::Int,
        actual: ast::Type::String,
    });
    let json = serde_json::to_string(&t).unwrap();
    let back: DiagnosticTemplate = serde_json::from_str(&json).unwrap();
    assert_eq!(back.code(), "E001");
}

#[test]
fn diagnostic_with_template_serializes_template_field() {
    use ast::templates::DiagnosticTemplate;
    use ast::templates::type_errors::TypeMismatch;

    let diag = ast::Diagnostic::from_template(DiagnosticTemplate::TypeMismatch(TypeMismatch {
        expected: ast::Type::Int,
        actual: ast::Type::String,
    }));
    let json = serde_json::to_string(&diag).unwrap();
    assert!(
        json.contains("TypeMismatch"),
        "serialized diagnostic should include template: {}",
        json
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Integration: compiler pipeline produces templates on real code
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn type_error_produces_template() {
    let src = "let x: Int = \"hello\"";
    let diag = crate::common::check_err_diagnostic(src);
    assert!(
        diag.template.is_some(),
        "type error diagnostic should have a template, got: {:?}",
        diag
    );
}

#[test]
fn type_error_template_has_correct_code() {
    let src = "let x: Int = \"hello\"";
    let diag = crate::common::check_err_diagnostic(src);
    let template = diag.template.as_ref().expect("should have template");
    assert_eq!(template.code(), "E001", "type mismatch should be E001");
}

#[test]
fn undefined_variable_produces_template() {
    let src = "let x: Int = unknown_var";
    let diag = crate::common::check_err_diagnostic(src);
    assert!(diag.template.is_some(), "should have template: {:?}", diag);
    assert_eq!(diag.template.as_ref().unwrap().code(), "E002");
}

#[test]
fn parse_error_produces_template() {
    let src = "let x = )";
    let diag = crate::common::check_parse_err_diagnostic(src);
    assert!(
        diag.template.is_some(),
        "parse error should have a template: {:?}",
        diag
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Completeness: every error code has a template variant
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn all_type_error_codes_have_variants() {
    use ast::templates::DiagnosticTemplate;
    use ast::templates::type_errors::*;
    // Compilation is the test: if a variant or struct is missing, this won't compile
    let _variants: Vec<DiagnosticTemplate> = vec![
        DiagnosticTemplate::TypeMismatch(TypeMismatch {
            expected: ast::Type::Int,
            actual: ast::Type::String,
        }),
        DiagnosticTemplate::UndefinedVariable(UndefinedVariable { name: "".into() }),
        DiagnosticTemplate::BinaryOpError(BinaryOpError {
            op: "".into(),
            left: ast::Type::Int,
            right: ast::Type::String,
        }),
        DiagnosticTemplate::ReturnTypeMismatch(ReturnTypeMismatch {
            function: "".into(),
            expected: ast::Type::Int,
            actual: ast::Type::String,
        }),
        DiagnosticTemplate::ArgumentTypeMismatch(ArgumentTypeMismatch {
            param: "".into(),
            expected: ast::Type::Int,
            actual: ast::Type::String,
        }),
        DiagnosticTemplate::ArgumentCountMismatch(ArgumentCountMismatch {
            expected: 0,
            actual: 0,
        }),
        DiagnosticTemplate::UnknownField(UnknownField {
            field: "".into(),
            type_name: "".into(),
        }),
    ];
}

#[test]
fn all_parse_error_codes_have_variants() {
    use ast::templates::DiagnosticTemplate;
    use ast::templates::parse_errors::*;
    let _variants: Vec<DiagnosticTemplate> = vec![
        DiagnosticTemplate::UnexpectedToken(UnexpectedToken {
            expected: "".into(),
            found: "".into(),
        }),
        DiagnosticTemplate::ExpectedIndentedBlock(ExpectedIndentedBlock),
        DiagnosticTemplate::NestingTooDeep(NestingTooDeep),
    ];
}

#[test]
fn all_module_error_codes_have_variants() {
    use ast::templates::DiagnosticTemplate;
    use ast::templates::module_errors::*;
    let _variants: Vec<DiagnosticTemplate> = vec![
        DiagnosticTemplate::ModuleNotFound(ModuleNotFound { name: "".into() }),
        DiagnosticTemplate::SymbolNotExported(SymbolNotExported {
            symbol: "".into(),
            module: "".into(),
        }),
        DiagnosticTemplate::CircularImport(CircularImport { module: "".into() }),
        DiagnosticTemplate::InvalidImportAlias(InvalidImportAlias),
    ];
}

// ═══════════════════════════════════════════════════════════════════════
// Uniqueness: no two templates share the same error code
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn all_template_codes_are_unique() {
    use ast::templates::DiagnosticTemplate;
    use ast::templates::lex_errors::*;
    use ast::templates::module_errors::*;
    use ast::templates::parse_errors::*;
    use ast::templates::type_errors::*;
    use ast::templates::warnings::*;
    use std::collections::HashSet;

    let templates: Vec<DiagnosticTemplate> = vec![
        DiagnosticTemplate::TypeMismatch(TypeMismatch {
            expected: ast::Type::Int,
            actual: ast::Type::Int,
        }),
        DiagnosticTemplate::UndefinedVariable(UndefinedVariable { name: "".into() }),
        DiagnosticTemplate::BinaryOpError(BinaryOpError {
            op: "".into(),
            left: ast::Type::Int,
            right: ast::Type::Int,
        }),
        DiagnosticTemplate::ReturnTypeMismatch(ReturnTypeMismatch {
            function: "".into(),
            expected: ast::Type::Int,
            actual: ast::Type::Int,
        }),
        DiagnosticTemplate::ArgumentTypeMismatch(ArgumentTypeMismatch {
            param: "".into(),
            expected: ast::Type::Int,
            actual: ast::Type::Int,
        }),
        DiagnosticTemplate::ArgumentCountMismatch(ArgumentCountMismatch {
            expected: 0,
            actual: 0,
        }),
        DiagnosticTemplate::MissingIterable(MissingIterable {
            type_name: "".into(),
        }),
        DiagnosticTemplate::InvalidAssignment(InvalidAssignment),
        DiagnosticTemplate::UndeclaredAssignment(UndeclaredAssignment { name: "".into() }),
        DiagnosticTemplate::UnknownField(UnknownField {
            field: "".into(),
            type_name: "".into(),
        }),
        DiagnosticTemplate::MatchError(MatchError { message: "".into() }),
        DiagnosticTemplate::TaskAlreadyConsumed(TaskAlreadyConsumed { name: "".into() }),
        DiagnosticTemplate::ErrorPropagation(ErrorPropagation { message: "".into() }),
        DiagnosticTemplate::TraitError(TraitError { message: "".into() }),
        DiagnosticTemplate::ConditionTypeError(ConditionTypeError {
            actual: ast::Type::Int,
        }),
        DiagnosticTemplate::IndexTypeError(IndexTypeError {
            actual: ast::Type::Int,
        }),
        DiagnosticTemplate::InconsistentListType(InconsistentListType {
            expected: ast::Type::Int,
            actual: ast::Type::Int,
        }),
        DiagnosticTemplate::UnaryOpError(UnaryOpError {
            op: "".into(),
            actual: ast::Type::Int,
        }),
        DiagnosticTemplate::ComparisonError(ComparisonError { message: "".into() }),
        DiagnosticTemplate::LogicalOpError(LogicalOpError),
        DiagnosticTemplate::ConstraintError(ConstraintError { message: "".into() }),
        DiagnosticTemplate::PrintableError(PrintableError),
        DiagnosticTemplate::TypeConstraintError(TypeConstraintError { message: "".into() }),
        DiagnosticTemplate::CollectionConstraintError(CollectionConstraintError {
            message: "".into(),
        }),
        DiagnosticTemplate::ConstReassignment(ConstReassignment { name: "".into() }),
        DiagnosticTemplate::TaskNotResolved(TaskNotResolved { name: "".into() }),
        DiagnosticTemplate::NotCompilable(NotCompilable { message: "".into() }),
        DiagnosticTemplate::UnexpectedToken(UnexpectedToken {
            expected: "".into(),
            found: "".into(),
        }),
        DiagnosticTemplate::ExpectedIndentedBlock(ExpectedIndentedBlock),
        DiagnosticTemplate::NestingTooDeep(NestingTooDeep),
        DiagnosticTemplate::ModuleNotFound(ModuleNotFound { name: "".into() }),
        DiagnosticTemplate::SymbolNotExported(SymbolNotExported {
            symbol: "".into(),
            module: "".into(),
        }),
        DiagnosticTemplate::CircularImport(CircularImport { module: "".into() }),
        DiagnosticTemplate::InvalidImportAlias(InvalidImportAlias),
        DiagnosticTemplate::RedundantTypeAnnotation(RedundantTypeAnnotation {
            type_name: "".into(),
        }),
        DiagnosticTemplate::UnusedDefaultParam(UnusedDefaultParam { name: "".into() }),
        DiagnosticTemplate::UseAfterMove(UseAfterMove { name: "".into() }),
        DiagnosticTemplate::ShadowedVariable(ShadowedVariable { name: "".into() }),
        DiagnosticTemplate::InterpolationError(InterpolationError),
        DiagnosticTemplate::UnterminatedString(UnterminatedString),
        DiagnosticTemplate::TabIndentation(TabIndentation),
        DiagnosticTemplate::InvalidEscape(InvalidEscape {
            sequence: "".into(),
        }),
        DiagnosticTemplate::StringTooLong(StringTooLong),
        DiagnosticTemplate::BadFloatLiteral(BadFloatLiteral { line: 0 }),
        DiagnosticTemplate::IntegerOverflow(IntegerOverflow),
        DiagnosticTemplate::MissingNewline(MissingNewline),
    ];

    let mut seen = HashSet::new();
    for t in &templates {
        let code = t.code();
        assert!(
            seen.insert(code),
            "Duplicate error code '{}' found in DiagnosticTemplate",
            code
        );
    }
}
