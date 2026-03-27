// ─── Map literal construction ───────────────────────────────────────

#[test]
fn map_literal_empty() {
    crate::common::check_ok("let m: Map[String, Int] = {}\n");
}

#[test]
fn map_literal_string_keys() {
    crate::common::check_ok("let m = {\"a\": 1, \"b\": 2, \"c\": 3}\n");
}

#[test]
fn map_literal_int_keys() {
    crate::common::check_ok("let m = {1: \"one\", 2: \"two\"}\n");
}

#[test]
fn map_literal_type_annotation() {
    crate::common::check_ok("let m: Map[String, Int] = {\"x\": 10}\n");
}

// ─── Map literal errors ─────────────────────────────────────────────

#[test]
fn map_literal_mixed_key_types_error() {
    let err = crate::common::check_err("let m = {\"a\": 1, 2: 3}\n");
    assert!(
        err.contains("key type mismatch")
            || err.contains("E003")
            || err.contains("incompatible types")
            || err.contains("map key"),
        "expected key type mismatch, got: {}",
        err
    );
}

#[test]
fn map_literal_mixed_value_types_error() {
    let err = crate::common::check_err("let m = {\"a\": 1, \"b\": \"two\"}\n");
    assert!(
        err.contains("value type mismatch")
            || err.contains("E003")
            || err.contains("incompatible types")
            || err.contains("map value"),
        "expected value type mismatch, got: {}",
        err
    );
}

// ─── Nullable map type ──────────────────────────────────────────────

#[test]
fn map_nullable_type_parses() {
    crate::common::check_ok(
        r#"def get_map() -> Map[String, Int]?
  nil
"#,
    );
}

// ─── Map index returns nullable ─────────────────────────────────────

#[test]
fn map_index_returns_nullable() {
    // Map index should return V?, requiring nil check to use the value
    let err = crate::common::check_err(
        r#"let m = {"a": 1, "b": 2}
let x: Int = m["a"]
"#,
    );
    assert!(
        err.contains("mismatch") || err.contains("Nullable") || err.contains("Int?"),
        "expected type mismatch (Int? vs Int), got: {}",
        err
    );
}

#[test]
fn map_index_nullable_match_unwrap() {
    // Should be able to match on the nullable result
    crate::common::check_ok(
        r#"let m = {"a": 1, "b": 2}
let x = match m["a"]
  nil => 0
  v => v
"#,
    );
}

#[test]
fn map_index_nullable_string_values() {
    // String map values: index returns String?
    let err = crate::common::check_err(
        r#"let m = {"a": "hello"}
let x: String = m["a"]
"#,
    );
    assert!(
        err.contains("mismatch") || err.contains("Nullable") || err.contains("String?"),
        "expected type mismatch (String? vs String), got: {}",
        err
    );
}
