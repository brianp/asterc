mod common;

// ─── Phase 3: Collections and Type Annotations ─────────────────────

// ─── Let with type annotation ───────────────────────────────────────

#[test]
fn let_with_type_annotation() {
    common::check_ok("let x: Int = 5");
}

#[test]
fn let_type_annotation_mismatch() {
    let err = common::check_err("let x: Int = \"hello\"");
    assert!(err.contains("annotation") || err.contains("mismatch"));
}

#[test]
fn let_type_annotation_string() {
    common::check_ok("let name: String = \"alice\"");
}

#[test]
fn let_type_annotation_bool() {
    common::check_ok("let flag: Bool = true");
}

// ─── List literals ──────────────────────────────────────────────────

#[test]
fn list_literal_ints() {
    common::check_ok("let xs = [1, 2, 3]");
}

#[test]
fn list_literal_strings() {
    common::check_ok("let xs = [\"a\", \"b\"]");
}

#[test]
fn empty_list_with_annotation() {
    common::check_ok("let xs: List[Int] = []");
}

#[test]
fn list_mixed_types_error() {
    let err = common::check_err("let xs = [1, \"two\"]");
    assert!(err.contains("element") || err.contains("mismatch") || err.contains("consistent"));
}

// ─── Indexing ───────────────────────────────────────────────────────

#[test]
fn index_list() {
    common::check_ok("let xs = [1, 2, 3]\nlet y = xs[0]");
}

#[test]
fn index_non_int_error() {
    let err = common::check_err("let xs = [1, 2]\nlet y = xs[\"bad\"]");
    assert!(err.contains("Int") || err.contains("index"));
}

// ─── For-in over List[T] ───────────────────────────────────────────

#[test]
fn for_over_list() {
    common::check_ok("let xs = [1, 2, 3]\nfor x in xs\n  let y = x + 1\n");
}

// ─── Phase 4: Modules, Imports, Builtins ────────────────────────────

#[test]
fn use_whole_module() {
    common::check_ok("use io");
}

#[test]
fn use_with_path() {
    common::check_ok("use std/http");
}

#[test]
fn use_selective() {
    common::check_ok("use std/http { Request, Response }");
}

#[test]
fn pub_def() {
    common::check_ok("pub def greet(name: String) -> String\n  name\n");
}

#[test]
fn pub_class() {
    common::check_ok("pub class Point\n  x: Int\n  y: Int\n");
}

#[test]
fn pub_let() {
    common::check_ok("pub let VERSION = 1");
}

#[test]
fn builtin_print() {
    common::check_ok("print(message: \"hello\")");
}

#[test]
fn builtin_len_list() {
    common::check_ok("let xs = [1, 2, 3]\nlet n = len(value: xs)");
}

#[test]
fn builtin_len_string() {
    common::check_ok("let n = len(value: \"hello\")");
}

#[test]
fn builtin_to_string() {
    common::check_ok("let s = to_string(value: 42)");
}

#[test]
fn use_deep_path() {
    common::check_ok("use std/net/tcp");
}

#[test]
fn use_with_alias() {
    common::check_ok("use std/http as h");
}

#[test]
fn use_selective_with_alias() {
    common::check_ok("use std/http/Security { CSRF, BasicAuth } as hs");
}

#[test]
fn use_then_code() {
    common::check_ok("use io\nlet x = 5\nlog(message: \"hello\")");
}

// ─── List type annotation ───────────────────────────────────────────

#[test]
fn let_list_type_annotation() {
    common::check_ok("let xs: List[Int] = [1, 2, 3]");
}

#[test]
fn let_list_type_annotation_mismatch() {
    let err = common::check_err("let xs: List[String] = [1, 2]");
    assert!(err.contains("annotation") || err.contains("mismatch"));
}
