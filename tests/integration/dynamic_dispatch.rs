// ─── DynamicReceiver Trait ───────────────────────────────────────────
//
// Tests for: DynamicReceiver trait, FunctionNotFound error type,
// method_missing dispatch, and runtime execution.

// =====================================================================
//
//   DynamicReceiver Trait (type-checking)
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
//   DynamicReceiver Runtime (JIT + AOT)
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
