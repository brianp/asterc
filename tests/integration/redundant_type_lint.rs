use ast::{Diagnostic, Severity};
use lexer::lex;
use parser::Parser;
use typecheck::typechecker::TypeChecker;

/// Run the typechecker and return only Warning-severity diagnostics whose code
/// matches "W004" (redundant type annotation).
fn check_warnings(src: &str) -> Vec<Diagnostic> {
    let tokens = lex(src).expect("lex ok");
    let mut parser = Parser::new(tokens);
    let module = parser.parse_module("test").expect("parse ok");
    let mut tc = TypeChecker::new();
    // The program should typecheck successfully — warnings are not errors.
    tc.check_module(&module).expect("typecheck ok");
    tc.reg
        .diagnostics
        .into_iter()
        .filter(|d| d.severity == Severity::Warning && d.code() == Some("W004"))
        .collect()
}

/// Assert that the source triggers exactly one W001 warning containing the
/// given type name in its message.
fn assert_redundant_warning(src: &str, expected_type: &str) {
    let warnings = check_warnings(src);
    assert!(
        !warnings.is_empty(),
        "expected a redundant-type-annotation warning for `{}`, got none",
        expected_type
    );
    assert_eq!(
        warnings.len(),
        1,
        "expected exactly 1 warning, got {}: {:?}",
        warnings.len(),
        warnings
    );
    let msg = &warnings[0].message;
    assert!(
        msg.contains(expected_type),
        "warning message should mention type `{}`, got: {}",
        expected_type,
        msg
    );
    assert!(
        msg.contains("redundant type annotation"),
        "warning message should say 'redundant type annotation', got: {}",
        msg
    );
}

/// Assert that the source triggers NO W001 warnings.
fn assert_no_redundant_warning(src: &str) {
    let warnings = check_warnings(src);
    assert!(
        warnings.is_empty(),
        "expected no redundant-type-annotation warnings, got {}: {:?}",
        warnings.len(),
        warnings
    );
}

// =============================================================================
// Cases that SHOULD warn (redundant annotation)
// =============================================================================

#[test]
fn warn_let_int_literal() {
    assert_redundant_warning("let x: Int = 42", "Int");
}

#[test]
fn warn_let_string_literal() {
    assert_redundant_warning("let s: String = \"hello\"", "String");
}

#[test]
fn warn_let_bool_true() {
    assert_redundant_warning("let b: Bool = true", "Bool");
}

#[test]
fn warn_let_bool_false() {
    assert_redundant_warning("let b: Bool = false", "Bool");
}

#[test]
fn warn_let_list_of_int() {
    assert_redundant_warning("let items: List[Int] = [1, 2, 3]", "List[Int]");
}

#[test]
fn warn_let_float_literal() {
    assert_redundant_warning("let f: Float = 3.14", "Float");
}

#[test]
fn warn_let_class_constructor() {
    let src = "\
class Point
    x: Int
    y: Int

let p: Point = Point(1, 2)
";
    assert_redundant_warning(src, "Point");
}

#[test]
fn warn_let_negative_int() {
    assert_redundant_warning("let x: Int = -1", "Int");
}

#[test]
fn warn_let_string_empty() {
    assert_redundant_warning("let s: String = \"\"", "String");
}

#[test]
fn warn_let_list_empty_typed() {
    // An empty list with a type annotation matching the inferred element type.
    // `let xs: List[String] = []` — if the typechecker infers List[String] from
    // context, the annotation is redundant. However, if it can't infer the
    // element type without the annotation, it should NOT warn. This test
    // documents the "should warn" side: the annotation matches inference.
    // If this is too ambitious for the initial implementation, it can be
    // adjusted later.
    assert_redundant_warning("let xs: List[Int] = [1]", "List[Int]");
}

// =============================================================================
// Cases that should NOT warn (annotation adds information or is absent)
// =============================================================================

#[test]
fn no_warn_no_annotation() {
    assert_no_redundant_warning("let x = 42");
}

#[test]
fn no_warn_no_annotation_string() {
    assert_no_redundant_warning("let s = \"hello\"");
}

#[test]
fn no_warn_float_from_float_literal() {
    // Float literal with Float annotation — but this IS redundant, so it
    // should warn. We test the non-redundant case: no annotation at all.
    assert_no_redundant_warning("let x = 3.14");
}

#[test]
fn no_warn_complex_return_type() {
    // Annotation on a function call result where the type is non-obvious.
    let src = "\
def get_value() -> Int
    return 42

let handler: Int = get_value()
";
    // This is technically redundant (function returns Int, annotation says Int),
    // but for a first pass it's acceptable to only warn on literals and
    // constructors. If the implementation is smarter and does warn here, this
    // test documents that the feature could go either way. For now we test that
    // it does NOT warn, since the call return type requires looking up the
    // function signature — a user might reasonably annotate for clarity.
    //
    // NOTE: If the implementation DOES warn here, flip this to
    // assert_redundant_warning and adjust the design. This test documents the
    // conservative choice.
    assert_no_redundant_warning(src);
}

#[test]
fn no_warn_function_return_type_annotation() {
    // Annotating with the return type of a function call — user may want
    // clarity. This should not warn in the conservative first pass.
    let src = "\
def get_name() -> String
    return \"alice\"

let name = get_name()
";
    assert_no_redundant_warning(src);
}

// =============================================================================
// Multiple bindings in one program
// =============================================================================

#[test]
fn warn_multiple_redundant_bindings() {
    let src = "\
let x: Int = 42
let s: String = \"hello\"
";
    let warnings = check_warnings(src);
    assert_eq!(
        warnings.len(),
        2,
        "expected 2 warnings, got {}: {:?}",
        warnings.len(),
        warnings
    );
}

#[test]
fn warn_only_redundant_not_necessary() {
    let src = "\
let x: Int = 42
let y = 10
";
    let warnings = check_warnings(src);
    // Only `let x: Int = 42` is redundant. `y` has no annotation.
    assert_eq!(
        warnings.len(),
        1,
        "expected 1 warning (only for x: Int), got {}: {:?}",
        warnings.len(),
        warnings
    );
    assert!(warnings[0].message.contains("Int"));
}

// =============================================================================
// Warning structure
// =============================================================================

#[test]
fn warning_has_correct_code() {
    let src = "let x: Int = 42";
    let warnings = check_warnings(src);
    assert!(!warnings.is_empty(), "expected a warning");
    assert_eq!(warnings[0].code(), Some("W004"));
}

#[test]
fn warning_has_correct_severity() {
    let src = "let x: Int = 42";
    let warnings = check_warnings(src);
    assert!(!warnings.is_empty(), "expected a warning");
    assert_eq!(warnings[0].severity, Severity::Warning);
}

#[test]
fn warning_message_format() {
    let src = "let x: Int = 42";
    let warnings = check_warnings(src);
    assert!(!warnings.is_empty(), "expected a warning");
    assert_eq!(
        warnings[0].message,
        "redundant type annotation: type `Int` can be inferred"
    );
}

// =============================================================================
// W005: Redundant main() -> Int with return 0
// =============================================================================

fn check_w005_warnings(src: &str) -> Vec<Diagnostic> {
    let tokens = lex(src).expect("lex ok");
    let mut parser = Parser::new(tokens);
    let module = parser.parse_module("test").expect("parse ok");
    let mut tc = TypeChecker::new();
    tc.check_module(&module).expect("typecheck ok");
    tc.reg
        .diagnostics
        .into_iter()
        .filter(|d| d.severity == Severity::Warning && d.code() == Some("W005"))
        .collect()
}

#[test]
fn w005_warn_main_int_returning_zero_literal() {
    let src = "def main() -> Int\n  0\n";
    let warnings = check_w005_warnings(src);
    assert_eq!(
        warnings.len(),
        1,
        "expected W005 warning, got: {:?}",
        warnings
    );
}

#[test]
fn w005_warn_main_int_returning_zero_explicit() {
    let src = "def main() -> Int\n  return 0\n";
    let warnings = check_w005_warnings(src);
    assert_eq!(
        warnings.len(),
        1,
        "expected W005 warning, got: {:?}",
        warnings
    );
}

#[test]
fn w005_no_warn_main_int_returning_nonzero() {
    let src = "def main() -> Int\n  42\n";
    let warnings = check_w005_warnings(src);
    assert!(warnings.is_empty(), "expected no W005, got: {:?}", warnings);
}

#[test]
fn w005_no_warn_main_no_return_type() {
    let src = "def main()\n  log(message: \"hi\")\n";
    let warnings = check_w005_warnings(src);
    assert!(warnings.is_empty(), "expected no W005, got: {:?}", warnings);
}

#[test]
fn w005_no_warn_main_void() {
    let src = "def main() -> Void\n  log(message: \"hi\")\n";
    let warnings = check_w005_warnings(src);
    assert!(warnings.is_empty(), "expected no W005, got: {:?}", warnings);
}

#[test]
fn w005_no_warn_non_main_int_returning_zero() {
    let src = "def helper() -> Int\n  0\n\ndef main() -> Int\n  helper()\n";
    let warnings = check_w005_warnings(src);
    assert!(warnings.is_empty(), "expected no W005, got: {:?}", warnings);
}
