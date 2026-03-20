mod common;

// ─── random() Polymorphic Builtin ───────────────────────────────────
//
// Type-inferred from context:
//   let n: Int = random(max: 100)      → random int in [0, 100)
//   let f: Float = random(max: 1.0)    → random float in [0.0, 1.0)
//   let b: Bool = random()             → coin flip

// ─── Happy path ─────────────────────────────────────────────────────

#[test]
fn random_int_with_type_annotation() {
    common::check_ok(
        r#"let n: Int = random(max: 100)
"#,
    );
}

#[test]
fn random_float_with_type_annotation() {
    common::check_ok(
        r#"let f: Float = random(max: 1.0)
"#,
    );
}

#[test]
fn random_bool_with_type_annotation() {
    common::check_ok(
        r#"let b: Bool = random()
"#,
    );
}

// ─── Error cases ────────────────────────────────────────────────────

#[test]
fn random_without_type_context_errors() {
    let err = common::check_err(
        r#"let x = random(max: 100)
"#,
    );
    assert!(
        err.contains("type") || err.contains("infer") || err.contains("annotation"),
        "Expected type inference error, got: {}",
        err
    );
}

#[test]
fn random_bool_rejects_max_arg() {
    let err = common::check_err(
        r#"let b: Bool = random(max: 10)
"#,
    );
    assert!(
        err.contains("argument") || err.contains("Bool") || err.contains("max"),
        "Expected arg error for Bool random, got: {}",
        err
    );
}

#[test]
fn random_int_requires_max_arg() {
    let err = common::check_err(
        r#"let n: Int = random()
"#,
    );
    assert!(
        err.contains("max") || err.contains("argument"),
        "Expected missing max error, got: {}",
        err
    );
}

// ─── Random trait ───────────────────────────────────────────────────

#[test]
fn class_includes_random_trait() {
    common::check_ok(
        r#"class Dice includes Random
  face: Int

  def random() -> Dice
    Dice(face: 1)
"#,
    );
}

#[test]
fn random_trait_requires_random_method() {
    let err = common::check_err(
        r#"class Dice includes Random
  face: Int
"#,
    );
    assert!(
        err.contains("random") || err.contains("required"),
        "Expected missing random method error, got: {}",
        err
    );
}
