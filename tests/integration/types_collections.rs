
// ─── Collections and type annotations ───────────────────────────────

// ─── Let with type annotation ───────────────────────────────────────

#[test]
fn let_with_type_annotation() {
    crate::common::check_ok("let x: Int = 5");
}

#[test]
fn let_type_annotation_mismatch() {
    let err = crate::common::check_err("let x: Int = \"hello\"");
    assert!(err.contains("annotation") || err.contains("mismatch"));
}

#[test]
fn let_type_annotation_string() {
    crate::common::check_ok("let name: String = \"alice\"");
}

#[test]
fn let_type_annotation_bool() {
    crate::common::check_ok("let flag: Bool = true");
}

// ─── List literals ──────────────────────────────────────────────────

#[test]
fn list_literal_ints() {
    crate::common::check_ok("let xs = [1, 2, 3]");
}

#[test]
fn list_literal_strings() {
    crate::common::check_ok("let xs = [\"a\", \"b\"]");
}

#[test]
fn empty_list_with_annotation() {
    crate::common::check_ok("let xs: List[Int] = []");
}

#[test]
fn list_mixed_types_error() {
    let err = crate::common::check_err("let xs = [1, \"two\"]");
    assert!(err.contains("element") || err.contains("mismatch") || err.contains("consistent"));
}

// ─── Indexing ───────────────────────────────────────────────────────

#[test]
fn index_list() {
    crate::common::check_ok("let xs = [1, 2, 3]\nlet y = xs[0]");
}

#[test]
fn index_non_int_error() {
    let err = crate::common::check_err("let xs = [1, 2]\nlet y = xs[\"bad\"]");
    assert!(err.contains("Int") || err.contains("index"));
}

// ─── For-in over List[T] ───────────────────────────────────────────

#[test]
fn for_over_list() {
    crate::common::check_ok("let xs = [1, 2, 3]\nfor x in xs\n  let y = x + 1\n");
}

// ─── Modules, imports, builtins ─────────────────────────────────────

#[test]
fn use_whole_module() {
    crate::common::check_ok("use io");
}

#[test]
fn use_with_path() {
    crate::common::check_ok("use std/http");
}

#[test]
fn use_selective() {
    crate::common::check_ok("use std/http { Request, Response }");
}

#[test]
fn pub_def() {
    crate::common::check_ok("pub def greet(name: String) -> String\n  name\n");
}

#[test]
fn pub_class() {
    crate::common::check_ok("pub class Point\n  x: Int\n  y: Int\n");
}

#[test]
fn pub_let() {
    crate::common::check_ok("pub let VERSION = 1");
}

#[test]
fn builtin_say() {
    crate::common::check_ok("say(message: \"hello\")");
}

#[test]
fn builtin_len_list() {
    crate::common::check_ok("let xs = [1, 2, 3]\nlet n = len(value: xs)");
}

#[test]
fn builtin_len_string() {
    crate::common::check_ok("let n = len(value: \"hello\")");
}

#[test]
fn builtin_to_string() {
    crate::common::check_ok("let s = to_string(value: 42)");
}

#[test]
fn use_deep_path() {
    crate::common::check_ok("use std/net/tcp");
}

#[test]
fn use_with_alias() {
    crate::common::check_ok("use std/http as h");
}

#[test]
fn use_selective_with_alias() {
    crate::common::check_ok("use std/http/Security { CSRF, BasicAuth } as hs");
}

#[test]
fn use_then_code() {
    crate::common::check_ok("use io\nlet x = 5\nlog(message: \"hello\")");
}

// ─── List type annotation ───────────────────────────────────────────

#[test]
fn let_list_type_annotation() {
    crate::common::check_ok("let xs: List[Int] = [1, 2, 3]");
}

#[test]
fn let_list_type_annotation_mismatch() {
    let err = crate::common::check_err("let xs: List[String] = [1, 2]");
    assert!(err.contains("annotation") || err.contains("mismatch"));
}

// ─── Nullable assignment ────────────────────────────────────────────

#[test]
fn nullable_assignment_accepts_inner_type() {
    crate::common::check_ok(
        r#"let x: String? = nil
x = "hello"
"#,
    );
}

#[test]
fn nullable_assignment_accepts_nil() {
    crate::common::check_ok(
        r#"let x: String? = "hello"
x = nil
"#,
    );
}

// ─── Never type recognition ────────────────────────────────────────

#[test]
fn never_type_recognized_by_from_ident() {
    use ast::Type;
    assert_eq!(Type::from_ident("Never"), Type::Never);
}

// ─── Nullable member access ─────────────────────────────────────────

#[test]
fn nullable_method_call_rejected() {
    let err = crate::common::check_err(
        r#"class Foo
  name: String

let x: Foo? = nil
let y = x.name
"#,
    );
    assert!(
        err.contains("nullable")
            || err.contains("Resolve")
            || err.contains("Cannot access")
            || err.contains("Cannot apply")
            || err.contains("Foo?"),
        "Expected nullable access error, got: {}",
        err
    );
}

// ─── List mutation methods (Issue #9) ────────────────────────────────

#[test]
fn list_insert_typecheck() {
    crate::common::check_ok("let xs = [1, 2, 3]\nxs.insert(at: 1, item: 99)\n");
}

#[test]
fn list_insert_wrong_type_error() {
    let err = crate::common::check_err("let xs = [1, 2]\nxs.insert(at: 0, item: \"bad\")\n");
    assert!(
        err.contains("mismatch") || err.contains("expected") || err.contains("Int"),
        "Expected type mismatch error, got: {}",
        err
    );
}

#[test]
fn list_insert_index_not_int_error() {
    let err = crate::common::check_err("let xs = [1, 2]\nxs.insert(at: \"zero\", item: 3)\n");
    assert!(
        err.contains("Int") || err.contains("mismatch") || err.contains("expected"),
        "Expected Int index error, got: {}",
        err
    );
}

#[test]
fn list_remove_typecheck() {
    crate::common::check_ok("let xs = [1, 2, 3]\nlet removed: Int = xs.remove(at: 0)\n");
}

#[test]
fn list_remove_returns_element_type() {
    let err = crate::common::check_err("let xs = [1, 2, 3]\nlet removed: String = xs.remove(at: 0)\n");
    assert!(
        err.contains("mismatch") || err.contains("String") || err.contains("Int"),
        "Expected type mismatch error, got: {}",
        err
    );
}

#[test]
fn list_pop_typecheck() {
    crate::common::check_ok("let xs = [1, 2, 3]\nlet last: Int = xs.pop()\n");
}

#[test]
fn list_pop_returns_element_type() {
    let err = crate::common::check_err("let xs = [1, 2, 3]\nlet last: String = xs.pop()\n");
    assert!(
        err.contains("mismatch") || err.contains("String") || err.contains("Int"),
        "Expected type mismatch error, got: {}",
        err
    );
}

#[test]
fn list_remove_first_typecheck() {
    crate::common::check_ok("let xs = [1, 2, 3]\nlet found = xs.remove_first(f: -> x : x == 2)\n");
}

#[test]
fn list_remove_first_returns_nullable() {
    crate::common::check_ok("let xs = [1, 2, 3]\nlet found: Int? = xs.remove_first(f: -> x : x > 1)\n");
}

#[test]
fn list_contains_item_typecheck() {
    crate::common::check_ok("let xs = [1, 2, 3]\nlet has_two: Bool = xs.contains(item: 2)\n");
}

#[test]
fn list_contains_item_wrong_type_error() {
    let err = crate::common::check_err("let xs = [1, 2]\nxs.contains(item: \"bad\")\n");
    assert!(
        err.contains("mismatch") || err.contains("expected") || err.contains("Int"),
        "Expected type mismatch error, got: {}",
        err
    );
}

#[test]
fn list_contains_predicate_typecheck() {
    crate::common::check_ok("let xs = [1, 2, 3]\nlet has_big: Bool = xs.contains(f: -> x : x > 2)\n");
}

#[test]
fn list_contains_string_typecheck() {
    crate::common::check_ok(
        r#"let xs = ["a", "b", "c"]
let has_b: Bool = xs.contains(item: "b")
"#,
    );
}
