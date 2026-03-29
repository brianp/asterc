use std::collections::HashMap;

// ─── Dynamic Dispatch & Unstable Features ──────────────────────────
//
// Tests for: DynamicReceiver trait, FunctionNotFound error type,
// std/unstable module gating, and FieldAccessible trait.
// Covers sections 5-7 of the introspection RFC.

// =====================================================================
//
//   PART 1: DynamicReceiver Trait
//
// =====================================================================

// ─── Contract tests: DynamicReceiver and FunctionNotFound exist as builtins ──

#[test]
fn function_not_found_is_builtin_error() {
    crate::common::check_ok(
        r#"
let e = FunctionNotFound(message: "oops", name: "foo")
let n: String = e.name
let m: String = e.message
"#,
    );
}

#[test]
fn function_not_found_extends_error() {
    crate::common::check_ok(
        r#"
def handle(err: Error) -> String
    err.message

let e = FunctionNotFound(message: "oops", name: "foo")
let result = handle(err: e)
"#,
    );
}

#[test]
fn dynamic_receiver_trait_exists() {
    crate::common::check_ok(
        r#"
class MyDSL includes DynamicReceiver
    log: List[String]

    def method_missing(fn_name: String, args: Map[String, String]) -> Void
        log.push(item: fn_name)
"#,
    );
}

// ─── Happy path: method_missing accepts any call ────────────────────

#[test]
fn accepts_any_call() {
    crate::common::check_ok(
        r#"
class SeedfileDSL includes DynamicReceiver
    deps: List[String]

    def method_missing(fn_name: String, args: Map[String, String]) -> Void
        deps.push(item: fn_name)

let dsl = SeedfileDSL(deps: [])
dsl.http(version: "1.2.0")
dsl.rails(version: "7.0")
dsl.anything_goes(version: "0.1")
"#,
    );
}

#[test]
fn accepts_empty_args() {
    crate::common::check_ok(
        r#"
class Logger includes DynamicReceiver
    entries: List[String]

    def method_missing(fn_name: String, args: Map[String, String]) -> Void
        entries.push(item: fn_name)

let log = Logger(entries: [])
log.start()
log.stop()
"#,
    );
}

#[test]
fn return_type_from_method_missing() {
    crate::common::check_ok(
        r#"
class Proxy includes DynamicReceiver
    target: String

    def method_missing(fn_name: String, args: Map[String, String]) -> String
        fn_name

let p = Proxy(target: "api")
let result: String = p.anything()
"#,
    );
}

// ─── Happy path: closed set (match with FunctionNotFound catch-all) ─

#[test]
fn closed_set_accepts_known_names() {
    crate::common::check_ok(
        r#"
class QueryBuilder includes DynamicReceiver
    queries: List[String]

    def method_missing(fn_name: String, args: Map[String, String]) throws Error -> Void
        match fn_name
            "find" => queries.push(item: "find")
            "find_by" => queries.push(item: "find_by")
            _ => throw FunctionNotFound(message: "unknown method", name: fn_name)

let qb = QueryBuilder(queries: [])
qb.find(id: "1")
qb.find_by(name: "Alice")
"#,
    );
}

#[test]
fn closed_set_rejects_unknown_names() {
    let err = crate::common::check_err(
        r#"
class QueryBuilder includes DynamicReceiver
    queries: List[String]

    def method_missing(fn_name: String, args: Map[String, String]) throws Error -> Void
        match fn_name
            "find" => queries.push(item: "find")
            "find_by" => queries.push(item: "find_by")
            _ => throw FunctionNotFound(message: "unknown method", name: fn_name)

let qb = QueryBuilder(queries: [])
qb.destroy_everything()
"#,
    );
    assert!(
        err.contains("destroy_everything") || err.contains("not a known dynamic method"),
        "expected unknown dynamic method error, got: {}",
        err
    );
}

// ─── Happy path: match with accepting catch-all ─────────────────────

#[test]
fn match_with_accepting_catchall() {
    crate::common::check_ok(
        r#"
class FlexDSL includes DynamicReceiver
    log: List[String]

    def method_missing(fn_name: String, args: Map[String, String]) -> Void
        match fn_name
            "special" => log.push(item: "special handler")
            _ => log.push(item: fn_name)

let f = FlexDSL(log: [])
f.special(key: "val")
f.anything_else(key: "val")
"#,
    );
}

// ─── Bare calls inside DynamicReceiver class body ───────────────────

#[test]
fn bare_call_routes_through_method_missing() {
    crate::common::check_ok(
        r#"
class SeedfileDSL includes DynamicReceiver
    deps: List[String]

    def method_missing(fn_name: String, args: Map[String, String]) -> Void
        deps.push(item: fn_name)

    def setup() -> Void
        http(version: "1.2.0")
        rails(version: "7.0")
"#,
    );
}

#[test]
fn bare_call_prefers_top_level_functions() {
    crate::common::check_ok(
        r#"
def helper(name: String) -> String
    name

class DSL includes DynamicReceiver
    log: List[String]

    def method_missing(fn_name: String, args: Map[String, String]) -> Void
        log.push(item: fn_name)

    def test() -> String
        helper(name: "test")
"#,
    );
}

#[test]
fn bare_call_prefers_local_vars() {
    crate::common::check_ok(
        r#"
def make_adder() -> Int
    42

class DSL includes DynamicReceiver
    log: List[String]

    def method_missing(fn_name: String, args: Map[String, String]) -> Void
        log.push(item: fn_name)

    def test() -> Int
        let result = make_adder()
        result
"#,
    );
}

// ─── Error tests: invalid method_missing signatures ─────────────────

#[test]
fn error_missing_method_missing() {
    let err = crate::common::check_err(
        r#"
class Bad includes DynamicReceiver
    x: Int
"#,
    );
    assert!(
        err.contains("method_missing") || err.contains("required method"),
        "expected missing method_missing error, got: {}",
        err
    );
}

#[test]
fn error_wrong_first_param_type() {
    let err = crate::common::check_err(
        r#"
class Bad includes DynamicReceiver
    x: Int

    def method_missing(fn_name: Int, args: Map[String, String]) -> Void
        x = fn_name
"#,
    );
    assert!(
        err.contains("String") || err.contains("method_missing"),
        "expected signature error, got: {}",
        err
    );
}

#[test]
fn error_wrong_second_param_type() {
    let err = crate::common::check_err(
        r#"
class Bad includes DynamicReceiver
    x: Int

    def method_missing(fn_name: String, args: List[String]) -> Void
        x = 1
"#,
    );
    assert!(
        err.contains("Map") || err.contains("method_missing"),
        "expected signature error, got: {}",
        err
    );
}

// ─── Rejection tests: arg type validation ───────────────────────────

#[test]
fn error_arg_type_mismatch() {
    let err = crate::common::check_err(
        r#"
class DSL includes DynamicReceiver
    log: List[String]

    def method_missing(fn_name: String, args: Map[String, String]) -> Void
        log.push(item: fn_name)

let d = DSL(log: [])
d.something(count: 42)
"#,
    );
    assert!(
        err.contains("type") || err.contains("String") || err.contains("Int"),
        "expected type mismatch error, got: {}",
        err
    );
}

// ─── Rejection tests: closed set name conflicts ─────────────────────

#[test]
fn error_dynamic_name_conflicts_with_real_method() {
    let diag = crate::common::check_err_diagnostic(
        r#"
class Bad includes DynamicReceiver
    log: List[String]

    def greet() -> String
        "hello"

    def method_missing(fn_name: String, args: Map[String, String]) throws Error -> Void
        match fn_name
            "greet" => log.push(item: "greet")
            _ => throw FunctionNotFound(message: "unknown", name: fn_name)
"#,
    );
    // Must use the distinct E032 DynamicMethodConflict template
    assert_eq!(diag.code(), Some("E032"));
    assert!(
        diag.message.contains("greet") && diag.message.contains("conflicts"),
        "expected conflict message mentioning 'greet', got: {}",
        diag.message
    );
    // Must have two labels: one for the match arm, one for the real method
    assert!(
        diag.labels.len() >= 2,
        "expected at least 2 labels (dynamic arm + real method), got {}: {:?}",
        diag.labels.len(),
        diag.labels
    );
}

// ─── Composition: trait methods take precedence ─────────────────────

#[test]
fn trait_methods_take_precedence() {
    crate::common::check_ok(
        r#"
class MyObj includes DynamicReceiver, Eq
    x: Int

    def method_missing(fn_name: String, args: Map[String, String]) -> Void
        x = 0

    def eq(other: MyObj) -> Bool
        true

let a = MyObj(x: 1)
let b = MyObj(x: 1)
let equal: Bool = a.eq(other: b)
"#,
    );
}

// ─── Composition: inheritance ───────────────────────────────────────

#[test]
fn child_inherits_dynamic_receiver() {
    crate::common::check_ok(
        r#"
class BaseDSL includes DynamicReceiver
    log: List[String]

    def method_missing(fn_name: String, args: Map[String, String]) -> Void
        log.push(item: fn_name)

class ChildDSL extends BaseDSL
    tag: String

let c = ChildDSL(log: [], tag: "child")
c.anything(key: "val")
"#,
    );
}

#[test]
fn child_overrides_method_missing() {
    crate::common::check_ok(
        r#"
class BaseDSL includes DynamicReceiver
    log: List[String]

    def method_missing(fn_name: String, args: Map[String, String]) -> Void
        log.push(item: fn_name)

class ChildDSL extends BaseDSL
    def method_missing(fn_name: String, args: Map[String, String]) -> Void
        log.push(item: "child:" + fn_name)

let c = ChildDSL(log: [])
c.anything(key: "val")
"#,
    );
}

// ─── Composition: real methods shadow dynamic dispatch ──────────────

#[test]
fn real_method_shadows_dynamic_dispatch() {
    crate::common::check_ok(
        r#"
class DSL includes DynamicReceiver
    log: List[String]

    def method_missing(fn_name: String, args: Map[String, String]) -> Void
        log.push(item: fn_name)

    def real_method() -> String
        "real"

let d = DSL(log: [])
let result: String = d.real_method()
"#,
    );
}

// ─── Composition: non-DynamicReceiver classes unaffected ────────────

#[test]
fn non_dynamic_class_not_affected() {
    let err = crate::common::check_err(
        r#"
class Normal
    x: Int

let n = Normal(x: 1)
n.unknown_method(key: "val")
"#,
    );
    assert!(
        err.contains("unknown") || err.contains("no member") || err.contains("not found"),
        "expected unknown method error, got: {}",
        err
    );
}

// ─── Boundary: method_missing with various arg counts ───────────────

#[test]
fn dynamic_call_multiple_args() {
    crate::common::check_ok(
        r#"
class DSL includes DynamicReceiver
    log: List[String]

    def method_missing(fn_name: String, args: Map[String, String]) -> Void
        log.push(item: fn_name)

let d = DSL(log: [])
d.configure(host: "localhost", port: "8080", protocol: "https")
"#,
    );
}

// =====================================================================
//
//   PART 2: std/unstable Module + --unstable Flag
//
// =====================================================================

// ─── Contract tests: the gate exists and works ──────────────────────

#[test]
fn use_std_unstable_rejected_without_flag() {
    let err = crate::common::check_err_with_files("use std/unstable\n", HashMap::new());
    assert!(
        err.contains("--unstable"),
        "Expected error mentioning --unstable flag, got: {}",
        err
    );
}

#[test]
fn use_std_unstable_selective_rejected_without_flag() {
    let err =
        crate::common::check_err_with_files("use std/unstable { SomeFeature }\n", HashMap::new());
    assert!(
        err.contains("--unstable"),
        "Expected error mentioning --unstable flag, got: {}",
        err
    );
}

#[test]
fn use_std_unstable_namespace_rejected_without_flag() {
    let err = crate::common::check_err_with_files("use std/unstable as u\n", HashMap::new());
    assert!(
        err.contains("--unstable"),
        "Expected error mentioning --unstable flag, got: {}",
        err
    );
}

// ─── Happy path: --unstable allows the import ───────────────────────

#[test]
fn use_std_unstable_allowed_with_flag() {
    crate::common::check_ok_with_files_unstable("use std/unstable\n", HashMap::new());
}

#[test]
fn use_std_unstable_selective_allowed_with_flag() {
    let err = crate::common::check_err_with_files_unstable(
        "use std/unstable { SomeFeature }\n",
        HashMap::new(),
    );
    assert!(
        !err.contains("--unstable"),
        "Should not mention --unstable when flag is enabled, got: {}",
        err
    );
    assert!(
        err.contains("SomeFeature") && err.contains("not"),
        "Expected 'not exported' error for unknown symbol, got: {}",
        err
    );
}

// ─── Error message quality ──────────────────────────────────────────

#[test]
fn unstable_error_has_m005_code() {
    let diag = crate::common::check_err_diagnostic_with_files("use std/unstable\n", HashMap::new());
    assert_eq!(
        diag.code(),
        Some("M005"),
        "Expected error code M005, got: {:?}",
        diag.code()
    );
}

#[test]
fn unstable_error_mentions_env_var() {
    let err = crate::common::check_err_with_files("use std/unstable\n", HashMap::new());
    assert!(
        err.contains("ASTER_UNSTABLE"),
        "Expected error to mention ASTER_UNSTABLE env var, got: {}",
        err
    );
}

// ─── Propagation: imported modules inherit the flag ─────────────────

#[test]
fn imported_module_can_use_unstable_when_root_has_flag() {
    let mut files = HashMap::new();
    files.insert(
        "experimental".to_string(),
        "use std/unstable\npub let FEATURE = 1\n".to_string(),
    );
    crate::common::check_ok_with_files_unstable(
        "use experimental { FEATURE }\nlet x = FEATURE\n",
        files,
    );
}

#[test]
fn imported_module_cannot_use_unstable_without_root_flag() {
    let mut files = HashMap::new();
    files.insert(
        "experimental".to_string(),
        "use std/unstable\npub let FEATURE = 1\n".to_string(),
    );
    let err = crate::common::check_err_with_files(
        "use experimental { FEATURE }\nlet x = FEATURE\n",
        files,
    );
    assert!(
        err.contains("--unstable"),
        "Expected --unstable error from imported module, got: {}",
        err
    );
}

// ─── Composition: stable std imports still work alongside ───────────

#[test]
fn stable_std_imports_unaffected_by_unstable_flag() {
    crate::common::check_ok_with_files_unstable(
        "use std/cmp { Eq }\n\nclass Point includes Eq\n  x: Int\n  y: Int\n",
        HashMap::new(),
    );
}

#[test]
fn stable_std_imports_work_without_unstable() {
    crate::common::check_ok_with_files(
        "use std/cmp { Eq }\n\nclass Point includes Eq\n  x: Int\n  y: Int\n",
        HashMap::new(),
    );
}

// ─── Rejection: unknown std submodules still rejected ───────────────

#[test]
fn unknown_std_submodule_still_rejected_with_unstable() {
    let err = crate::common::check_err_with_files_unstable("use std/nonexistent\n", HashMap::new());
    assert!(
        !err.contains("--unstable"),
        "Unknown submodule error should not mention --unstable, got: {}",
        err
    );
}

// ─── Non-std module named "unstable" is not gated ───────────────────

#[test]
fn user_module_named_unstable_not_gated() {
    let mut files = HashMap::new();
    files.insert("mylib/unstable".to_string(), "pub let X = 42\n".to_string());
    crate::common::check_ok_with_files("use mylib/unstable { X }\nlet y = X\n", files);
}

// ─── CLI integration: --unstable flag on check/run/build ────────────

#[test]
fn cli_check_unstable_flag_accepted() {
    let dir = crate::common::make_temp_dir("unstable-cli");
    let file = dir.join("test.aster");
    std::fs::write(&file, "use std/unstable\ndef main() -> Int\n  0\n").unwrap();

    let output = crate::common::cli(&["check", "--unstable", file.to_str().unwrap()]);
    let text = crate::common::output_text(&output);
    assert!(
        output.status.success(),
        "asterc check --unstable should succeed, got: {}",
        text
    );
}

#[test]
fn cli_check_without_unstable_flag_rejects() {
    let dir = crate::common::make_temp_dir("unstable-cli-reject");
    let file = dir.join("test.aster");
    std::fs::write(&file, "use std/unstable\ndef main() -> Int\n  0\n").unwrap();

    let output = crate::common::cli(&["check", file.to_str().unwrap()]);
    let text = crate::common::output_text(&output);
    assert!(
        !output.status.success(),
        "asterc check without --unstable should fail, got: {}",
        text
    );
    assert!(
        text.contains("--unstable"),
        "Error output should mention --unstable, got: {}",
        text
    );
}

#[test]
fn cli_env_var_enables_unstable() {
    let dir = crate::common::make_temp_dir("unstable-env");
    let file = dir.join("test.aster");
    std::fs::write(&file, "use std/unstable\ndef main() -> Int\n  0\n").unwrap();

    let output = std::process::Command::new(
        std::env::var_os("CARGO_BIN_EXE_asterc")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("target/debug/asterc")),
    )
    .args(["check", file.to_str().unwrap()])
    .env("ASTER_UNSTABLE", "1")
    .output()
    .expect("failed to run asterc");

    let text = crate::common::output_text(&output);
    assert!(
        output.status.success(),
        "ASTER_UNSTABLE=1 should enable unstable imports, got: {}",
        text
    );
}

// =====================================================================
//
//   PART 3: FieldAccessible Trait
//
// =====================================================================

// The auto-generated enum is named ClassNameFieldValue (flat, no dots)
// since the parser does not support dotted type paths in annotations.

// ─── Contract tests: FieldAccessible requires unstable import ───────

#[test]
fn field_accessible_requires_unstable_import() {
    let err = crate::common::check_err_with_files(
        "\
class Person includes FieldAccessible
  name: String
  age: Int
",
        HashMap::new(),
    );
    assert!(
        err.contains("FieldAccessible"),
        "Expected error mentioning FieldAccessible, got: {}",
        err
    );
}

#[test]
fn field_accessible_requires_unstable_flag() {
    let err = crate::common::check_err_with_files(
        "\
use std/unstable { FieldAccessible }

class Person includes FieldAccessible
  name: String
  age: Int
",
        HashMap::new(),
    );
    assert!(
        err.contains("--unstable"),
        "Expected --unstable error, got: {}",
        err
    );
}

#[test]
fn field_accessible_exported_from_std_unstable() {
    crate::common::check_ok_with_files_unstable(
        "\
use std/unstable { FieldAccessible }

class Person includes FieldAccessible
  name: String
  age: Int
",
        HashMap::new(),
    );
}

// ─── Happy path: field_value method is auto-generated ───────────────

#[test]
fn field_value_returns_nullable_for_known_field() {
    crate::common::check_ok_with_files_unstable(
        r#"use std/unstable { FieldAccessible }

class Person includes FieldAccessible
  name: String
  age: Int

let p = Person(name: "Alice", age: 30)
let v = p.field_value(name: "name")
"#,
        HashMap::new(),
    );
}

#[test]
fn field_value_returns_nil_for_unknown_field() {
    crate::common::check_ok_with_files_unstable(
        r#"use std/unstable { FieldAccessible }

class Person includes FieldAccessible
  name: String
  age: Int

let p = Person(name: "Alice", age: 30)
let v = p.field_value(name: "nonexistent")
"#,
        HashMap::new(),
    );
}

#[test]
fn field_value_return_type_is_nullable() {
    crate::common::check_ok_with_files_unstable(
        r#"use std/unstable { FieldAccessible }

class Person includes FieldAccessible
  name: String
  age: Int

let p = Person(name: "Alice", age: 30)
let v: PersonFieldValue? = p.field_value(name: "name")
"#,
        HashMap::new(),
    );
}

#[test]
fn field_value_works_with_multiple_field_types() {
    crate::common::check_ok_with_files_unstable(
        r#"use std/unstable { FieldAccessible }

class Record includes FieldAccessible
  label: String
  count: Int
  ratio: Float
  active: Bool

let r = Record(label: "test", count: 5, ratio: 3.14, active: true)
let a = r.field_value(name: "label")
let b = r.field_value(name: "count")
let c = r.field_value(name: "ratio")
let d = r.field_value(name: "active")
"#,
        HashMap::new(),
    );
}

// ─── Auto-generated FieldValue enum: matchable by user code ─────────

#[test]
fn field_value_enum_is_matchable() {
    crate::common::check_ok_with_files_unstable(
        r#"use std/unstable { FieldAccessible }

class Person includes FieldAccessible
  name: String
  age: Int

let p = Person(name: "Alice", age: 30)
let v = p.field_value(name: "name")
let result = match v
  nil => "none"
  _ => "found"
"#,
        HashMap::new(),
    );
}

#[test]
fn field_value_enum_variants_named_after_fields() {
    crate::common::check_ok_with_files_unstable(
        r#"use std/unstable { FieldAccessible }

class Person includes FieldAccessible
  name: String
  age: Int

let p = Person(name: "Alice", age: 30)
let v = p.field_value(name: "name")
match v
  PersonFieldValue.Name => "name"
  PersonFieldValue.Age => "age"
  nil => "nil"
"#,
        HashMap::new(),
    );
}

// ─── Boundary tests ─────────────────────────────────────────────────

#[test]
fn field_accessible_single_field_class() {
    crate::common::check_ok_with_files_unstable(
        r#"use std/unstable { FieldAccessible }

class Wrapper includes FieldAccessible
  value: Int

let w = Wrapper(value: 42)
let v = w.field_value(name: "value")
"#,
        HashMap::new(),
    );
}

#[test]
fn field_accessible_many_fields() {
    crate::common::check_ok_with_files_unstable(
        r#"use std/unstable { FieldAccessible }

class Config includes FieldAccessible
  host: String
  port: Int
  debug: Bool
  timeout: Float
  name: String

let c = Config(host: "localhost", port: 8080, debug: false, timeout: 30.0, name: "app")
let a = c.field_value(name: "host")
let b = c.field_value(name: "port")
let d = c.field_value(name: "debug")
let e = c.field_value(name: "timeout")
let f = c.field_value(name: "name")
"#,
        HashMap::new(),
    );
}

#[test]
fn field_value_empty_string_returns_nil() {
    crate::common::check_ok_with_files_unstable(
        r#"use std/unstable { FieldAccessible }

class Person includes FieldAccessible
  name: String
  age: Int

let p = Person(name: "Alice", age: 30)
let v = p.field_value(name: "")
"#,
        HashMap::new(),
    );
}

// ─── Error / rejection tests ────────────────────────────────────────

#[test]
fn field_accessible_rejected_on_generic_class() {
    let err = crate::common::check_err_with_files_unstable(
        r#"use std/unstable { FieldAccessible }

class Box[T] includes FieldAccessible
  value: T
"#,
        HashMap::new(),
    );
    assert!(
        err.to_lowercase().contains("generic") || err.contains("FieldAccessible"),
        "Expected error about generic classes, got: {}",
        err
    );
}

#[test]
fn field_accessible_rejects_user_defined_field_value() {
    let err = crate::common::check_err_with_files_unstable(
        r#"use std/unstable { FieldAccessible }

class Person includes FieldAccessible
  name: String
  age: Int

  def field_value(name: String) -> String
    "custom"
"#,
        HashMap::new(),
    );
    assert!(
        err.contains("field_value"),
        "Expected error about conflicting field_value definition, got: {}",
        err
    );
}

// ─── Composition: inheritance ───────────────────────────────────────

#[test]
fn field_accessible_includes_inherited_fields() {
    crate::common::check_ok_with_files_unstable(
        r#"use std/unstable { FieldAccessible }

class Base
  message: String

class AppError extends Base includes FieldAccessible
  code: Int

let e = AppError(message: "oops", code: 404)
let v = e.field_value(name: "message")
"#,
        HashMap::new(),
    );
}

#[test]
fn field_accessible_inherited_field_enum_has_all_variants() {
    crate::common::check_ok_with_files_unstable(
        r#"use std/unstable { FieldAccessible }

class Base
  message: String

class AppError extends Base includes FieldAccessible
  code: Int

let e = AppError(message: "oops", code: 404)
match e.field_value(name: "message")
  AppErrorFieldValue.Message => "message"
  AppErrorFieldValue.Code => "code"
  nil => "nil"
"#,
        HashMap::new(),
    );
}

#[test]
fn subclass_can_independently_include_field_accessible() {
    crate::common::check_ok_with_files_unstable(
        r#"use std/unstable { FieldAccessible }

class Parent includes FieldAccessible
  name: String

class Child extends Parent includes FieldAccessible
  age: Int

let p = Parent(name: "Alice")
let c = Child(name: "Bob", age: 10)
let pv = p.field_value(name: "name")
let cv = c.field_value(name: "name")
let cv2 = c.field_value(name: "age")
"#,
        HashMap::new(),
    );
}

// ─── Composition: with other traits ─────────────────────────────────

#[test]
fn field_accessible_with_eq() {
    crate::common::check_ok_with_files_unstable(
        r#"use std/unstable { FieldAccessible }
use std/cmp { Eq }

class Point includes Eq, FieldAccessible
  x: Int
  y: Int

let a = Point(x: 1, y: 2)
let b = Point(x: 1, y: 2)
let eq = a == b
let v = a.field_value(name: "x")
"#,
        HashMap::new(),
    );
}

#[test]
fn field_accessible_with_printable() {
    crate::common::check_ok_with_files_unstable(
        r#"use std/unstable { FieldAccessible }
use std/fmt { Printable }

class Item includes Printable, FieldAccessible
  label: String
  count: Int

let i = Item(label: "widget", count: 5)
let s = i.to_string()
let v = i.field_value(name: "label")
"#,
        HashMap::new(),
    );
}

#[test]
fn field_accessible_with_dynamic_receiver() {
    crate::common::check_ok_with_files_unstable(
        r#"use std/unstable { FieldAccessible }

class FlexObj includes DynamicReceiver, FieldAccessible
  data: String

  def method_missing(fn_name: String, args: Map[String, String]) -> Void
    data = fn_name

let obj = FlexObj(data: "init")
obj.unknown_method()
let v = obj.field_value(name: "data")
"#,
        HashMap::new(),
    );
}

// ─── Composition: field_value exposes private fields ────────────────

#[test]
fn field_accessible_exposes_private_fields() {
    crate::common::check_ok_with_files_unstable(
        r#"use std/unstable { FieldAccessible }

class Secret includes FieldAccessible
  pub label: String
  password: String

let s = Secret(label: "admin", password: "hunter2")
let v = s.field_value(name: "password")
"#,
        HashMap::new(),
    );
}

// ─── Integration: field_value usable in functions ───────────────────

#[test]
fn field_value_callable_from_function() {
    crate::common::check_ok_with_files_unstable(
        r#"use std/unstable { FieldAccessible }

class Person includes FieldAccessible
  name: String
  age: Int

def get_field(p: Person, field_name: String) -> PersonFieldValue?
  p.field_value(name: field_name)

let p = Person(name: "Alice", age: 30)
let v = get_field(p: p, field_name: "name")
"#,
        HashMap::new(),
    );
}

#[test]
fn field_value_type_usable_as_parameter() {
    crate::common::check_ok_with_files_unstable(
        r#"use std/unstable { FieldAccessible }

class Person includes FieldAccessible
  name: String
  age: Int

def describe(v: PersonFieldValue?) -> String
  match v
    nil => "nil"
    _ => "value"

let p = Person(name: "Alice", age: 30)
let result = describe(v: p.field_value(name: "name"))
"#,
        HashMap::new(),
    );
}

// =====================================================================
//
//   PART 4: DynamicReceiver Runtime (JIT + AOT)
//
// =====================================================================

// ─── method_missing executes through JIT ─────────────────────────────

#[test]
fn dynamic_receiver_method_missing_runtime() {
    let dir = crate::common::make_temp_dir("dr-method-missing");
    let src = dir.join("test.aster");
    std::fs::write(
        &src,
        "\
class Logger includes DynamicReceiver
  entries: List[String]

  def method_missing(fn_name: String, args: Map[String, String]) -> Void
    entries.push(item: fn_name)

def main() -> Int
  let log = Logger(entries: [])
  log.info()
  log.warn()
  log.error()
  say(message: log.entries.len())
  0
",
    )
    .unwrap();
    let output = crate::common::cli(&["run", &src.to_string_lossy()]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("3"),
        "method_missing should capture 3 calls, got: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// ─── field mutation through real methods + method_missing ────────────

#[test]
fn dynamic_receiver_field_mutation_runtime() {
    let dir = crate::common::make_temp_dir("dr-field-mutation");
    let src = dir.join("test.aster");
    std::fs::write(
        &src,
        "\
class Config includes DynamicReceiver
  project_name: String
  project_version: String
  deps: List[String]

  def package(n: String, v: String) -> Void
    project_name = n
    project_version = v

  def method_missing(fn_name: String, args: Map[String, String]) -> Void
    deps.push(item: fn_name)

def main() -> Int
  let c = Config(project_name: \"\", project_version: \"\", deps: [])
  c.package(n: \"my-app\", v: \"0.1.0\")
  c.http()
  c.json()
  c.crypto()
  say(message: c.project_name)
  say(message: c.project_version)
  say(message: c.deps.len())
  0
",
    )
    .unwrap();
    let output = crate::common::cli(&["run", &src.to_string_lossy()]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("my-app") && stdout.contains("0.1.0") && stdout.contains("3"),
        "Config should have name, version, and 3 deps, got: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// ─── method_missing with return values ──────────────────────────────

#[test]
fn dynamic_receiver_return_value_runtime() {
    let dir = crate::common::make_temp_dir("dr-return-value");
    let src = dir.join("test.aster");
    std::fs::write(
        &src,
        "\
class Proxy includes DynamicReceiver
  last: String

  def method_missing(fn_name: String, args: Map[String, String]) -> String
    last = fn_name
    fn_name

def main() -> Int
  let p = Proxy(last: \"\")
  let result = p.greet()
  say(message: result)
  say(message: p.last)
  0
",
    )
    .unwrap();
    let output = crate::common::cli(&["run", &src.to_string_lossy()]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("greet"),
        "method_missing should return the method name, got: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// ─── real methods take priority over method_missing ─────────────────

#[test]
fn dynamic_receiver_real_method_priority_runtime() {
    let dir = crate::common::make_temp_dir("dr-priority");
    let src = dir.join("test.aster");
    std::fs::write(
        &src,
        "\
class Handler includes DynamicReceiver
  log: List[String]

  def handle() -> String
    \"real\"

  def method_missing(fn_name: String, args: Map[String, String]) -> String
    log.push(item: fn_name)
    \"dynamic\"

def main() -> Int
  let h = Handler(log: [])
  let r1 = h.handle()
  let r2 = h.unknown()
  say(message: r1)
  say(message: r2)
  0
",
    )
    .unwrap();
    let output = crate::common::cli(&["run", &src.to_string_lossy()]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("real") && stdout.contains("dynamic"),
        "real method should win over method_missing, got: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// ─── AOT parity ─────────────────────────────────────────────────────

#[test]
fn dynamic_receiver_aot() {
    let dir = crate::common::make_temp_dir("dr-aot");
    let src = dir.join("test.aster");
    std::fs::write(
        &src,
        "\
class Config includes DynamicReceiver
  project_name: String
  deps: List[String]

  def package(n: String) -> Void
    project_name = n

  def method_missing(fn_name: String, args: Map[String, String]) -> Void
    deps.push(item: fn_name)

def main() -> Int
  let c = Config(project_name: \"\", deps: [])
  c.package(n: \"test\")
  c.http()
  c.json()
  say(message: c.project_name)
  say(message: c.deps.len())
  0
",
    )
    .unwrap();
    let output = crate::common::build_and_run(&src);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("test") && stdout.contains("2"),
        "AOT: Config should have name and 2 deps, got: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
