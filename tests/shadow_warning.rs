mod common;

use ast::Severity;

/// Return only W003 (variable shadowing) warnings from the given source.
fn shadow_warnings(src: &str) -> Vec<ast::Diagnostic> {
    common::check_warnings(src)
        .into_iter()
        .filter(|d| d.severity == Severity::Warning && d.code() == Some("W003"))
        .collect()
}

// =============================================================================
// Cases that SHOULD warn
// =============================================================================

#[test]
fn warn_nested_scope_shadows_outer() {
    let src = "\
let x = 1
if true
    let x = 2
";
    let warnings = shadow_warnings(src);
    assert_eq!(
        warnings.len(),
        1,
        "expected 1 shadow warning, got: {:?}",
        warnings
    );
    assert!(
        warnings[0].message.contains("x"),
        "should mention 'x': {}",
        warnings[0].message
    );
}

#[test]
fn warn_for_loop_var_shadows_outer() {
    let src = "\
let i = 99
for i in [1, 2, 3]
    let unused = i
";
    let warnings = shadow_warnings(src);
    assert_eq!(
        warnings.len(),
        1,
        "expected 1 shadow warning, got: {:?}",
        warnings
    );
    assert!(
        warnings[0].message.contains("i"),
        "should mention 'i': {}",
        warnings[0].message
    );
}

#[test]
fn warn_function_param_shadows_outer() {
    let src = "\
let x = 10
def foo(x: Int) -> Int
    return x + 1
";
    let warnings = shadow_warnings(src);
    assert_eq!(
        warnings.len(),
        1,
        "expected 1 shadow warning, got: {:?}",
        warnings
    );
}

#[test]
fn warn_match_binding_shadows_outer() {
    let src = "\
let x = 42
let val = 10
let result = match val
    x => x + 1
";
    let warnings = shadow_warnings(src);
    assert!(
        warnings.iter().any(|w| w.message.contains("x")),
        "expected shadow warning for 'x', got: {:?}",
        warnings
    );
}

#[test]
fn warn_catch_var_shadows_outer() {
    let src = r#"class AppError
  message: String

let e = 1
def risky() throws AppError -> String
  "data"

def safe() -> String
  risky()!.catch
    AppError e -> e.message
    _ -> "unknown"
"#;
    let warnings = shadow_warnings(src);
    assert!(
        warnings.iter().any(|w| w.message.contains("e")),
        "expected shadow warning for 'e', got: {:?}",
        warnings
    );
}

#[test]
fn warn_multiple_shadows() {
    let src = "\
let x = 1
let y = 2
if true
    let x = 10
    let y = 20
";
    let warnings = shadow_warnings(src);
    assert_eq!(
        warnings.len(),
        2,
        "expected 2 shadow warnings, got: {:?}",
        warnings
    );
}

#[test]
fn warn_deeply_nested_shadow() {
    let src = "\
let x = 1
if true
    if true
        let x = 2
";
    let warnings = shadow_warnings(src);
    assert_eq!(
        warnings.len(),
        1,
        "expected 1 shadow warning, got: {:?}",
        warnings
    );
}

// =============================================================================
// Cases that should NOT warn
// =============================================================================

#[test]
fn no_warn_different_names() {
    let src = "\
let x = 1
let y = 2
";
    let warnings = shadow_warnings(src);
    assert!(
        warnings.is_empty(),
        "expected no shadow warnings, got: {:?}",
        warnings
    );
}

#[test]
fn no_warn_single_binding() {
    let src = "let x = 42";
    let warnings = shadow_warnings(src);
    assert!(
        warnings.is_empty(),
        "expected no shadow warnings, got: {:?}",
        warnings
    );
}

#[test]
fn no_warn_separate_function_scopes() {
    let src = "\
def foo(x: Int) -> Int
    return x + 1
def bar(x: Int) -> Int
    return x + 2
";
    let warnings = shadow_warnings(src);
    assert!(
        warnings.is_empty(),
        "expected no shadow warnings, got: {:?}",
        warnings
    );
}

#[test]
fn no_warn_separate_if_branches() {
    let src = "\
if true
    let x = 1
else
    let x = 2
";
    let warnings = shadow_warnings(src);
    assert!(
        warnings.is_empty(),
        "expected no shadow warnings, got: {:?}",
        warnings
    );
}

// =============================================================================
// Warning structure
// =============================================================================

#[test]
fn warning_has_correct_code() {
    let src = "\
let x = 1
if true
    let x = 2
";
    let warnings = shadow_warnings(src);
    assert!(!warnings.is_empty(), "expected a shadow warning");
    assert_eq!(warnings[0].code(), Some("W003"));
}

#[test]
fn warning_has_correct_severity() {
    let src = "\
let x = 1
if true
    let x = 2
";
    let warnings = shadow_warnings(src);
    assert!(!warnings.is_empty(), "expected a shadow warning");
    assert_eq!(warnings[0].severity, Severity::Warning);
}

#[test]
fn warning_message_contains_shadows() {
    let src = "\
let x = 1
if true
    let x = 2
";
    let warnings = shadow_warnings(src);
    assert!(!warnings.is_empty(), "expected a shadow warning");
    assert!(
        warnings[0].message.contains("shadows"),
        "message should contain 'shadows': {}",
        warnings[0].message
    );
}

// =============================================================================
// Warnings survive fixpoint iteration for inferred return types
// =============================================================================

#[test]
fn warning_survives_fixpoint_iteration() {
    // Function A has a shadow warning.
    // Function B has an inferred return type that requires fixpoint iteration.
    // The shadow warning from A must not be lost.
    let src = "\
let x = 1
def has_warning(n: Int) -> Int
    let x = n + 1
    x

def inferred_ret(n: Int)
    n + 1

let a = has_warning(n: 5)
let b = inferred_ret(n: 3)
";
    let warnings = shadow_warnings(src);
    assert!(
        !warnings.is_empty(),
        "shadow warning from has_warning() should survive fixpoint iteration"
    );
    assert!(
        warnings.iter().any(|w| w.message.contains("x")),
        "expected shadow warning for 'x', got: {:?}",
        warnings
    );
}
