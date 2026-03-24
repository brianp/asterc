mod common;

// ─── Map literal construction ───────────────────────────────────────

#[test]
fn map_literal_empty() {
    common::check_ok("let m: Map[String, Int] = {}\n");
}

#[test]
fn map_literal_string_keys() {
    common::check_ok("let m = {\"a\": 1, \"b\": 2, \"c\": 3}\n");
}

#[test]
fn map_literal_int_keys() {
    common::check_ok("let m = {1: \"one\", 2: \"two\"}\n");
}

#[test]
fn map_literal_type_annotation() {
    common::check_ok("let m: Map[String, Int] = {\"x\": 10}\n");
}

// ─── Map literal errors ─────────────────────────────────────────────

#[test]
fn map_literal_mixed_key_types_error() {
    let err = common::check_err("let m = {\"a\": 1, 2: 3}\n");
    assert!(
        err.contains("key type mismatch") || err.contains("E003"),
        "expected key type mismatch, got: {}",
        err
    );
}

#[test]
fn map_literal_mixed_value_types_error() {
    let err = common::check_err("let m = {\"a\": 1, \"b\": \"two\"}\n");
    assert!(
        err.contains("value type mismatch") || err.contains("E003"),
        "expected value type mismatch, got: {}",
        err
    );
}

// ─── Nullable map type ──────────────────────────────────────────────

#[test]
fn map_nullable_type_parses() {
    common::check_ok(
        r#"def get_map() -> Map[String, Int]?
  nil
"#,
    );
}
