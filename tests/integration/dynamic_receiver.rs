// ─── DynamicReceiver Trait ──────────────────────────────────────────
//
// Tests for the DynamicReceiver trait: dynamic dispatch via method_missing,
// FunctionNotFound error type, bare call routing, and composition with
// traits, inheritance, and introspection.

// ═══════════════════════════════════════════════════════════════════════
// Contract tests: DynamicReceiver and FunctionNotFound exist as builtins
// ═══════════════════════════════════════════════════════════════════════

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

// ═══════════════════════════════════════════════════════════════════════
// Happy path: method_missing accepts any call (no match or non-throwing catch-all)
// ═══════════════════════════════════════════════════════════════════════

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

// ═══════════════════════════════════════════════════════════════════════
// Happy path: closed set (match with FunctionNotFound catch-all)
// ═══════════════════════════════════════════════════════════════════════

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

// ═══════════════════════════════════════════════════════════════════════
// Happy path: match with accepting catch-all (known names get matched,
// everything else still goes through)
// ═══════════════════════════════════════════════════════════════════════

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

// ═══════════════════════════════════════════════════════════════════════
// Bare calls inside DynamicReceiver class body (implicit self)
// ═══════════════════════════════════════════════════════════════════════

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

// ═══════════════════════════════════════════════════════════════════════
// Error tests: invalid method_missing signatures
// ═══════════════════════════════════════════════════════════════════════

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

// ═══════════════════════════════════════════════════════════════════════
// Rejection tests: arg type validation
// ═══════════════════════════════════════════════════════════════════════

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

// ═══════════════════════════════════════════════════════════════════════
// Rejection tests: closed set name conflicts with real methods
// ═══════════════════════════════════════════════════════════════════════

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

// ═══════════════════════════════════════════════════════════════════════
// Composition: trait methods take precedence over method_missing
// ═══════════════════════════════════════════════════════════════════════

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

// ═══════════════════════════════════════════════════════════════════════
// Composition: inheritance
// ═══════════════════════════════════════════════════════════════════════

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

// ═══════════════════════════════════════════════════════════════════════
// Composition: real methods always shadow dynamic dispatch
// ═══════════════════════════════════════════════════════════════════════

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

// ═══════════════════════════════════════════════════════════════════════
// Composition: non-DynamicReceiver classes are unaffected
// ═══════════════════════════════════════════════════════════════════════

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

// ═══════════════════════════════════════════════════════════════════════
// Boundary: method_missing with various arg counts
// ═══════════════════════════════════════════════════════════════════════

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
