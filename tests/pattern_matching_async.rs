mod common;

// ═══════════════════════════════════════════════════════════════════════
// Phase 6: Call-Site Async, Error Handling, Pattern Matching
// ═══════════════════════════════════════════════════════════════════════

// ─── 6A. Pattern Matching ────────────────────────────────────────────

#[test]
fn integration_match_int_literal_patterns() {
    common::check_ok(r#"let x = 5
let y = match x
  1 => "one"
  2 => "two"
  _ => "other"
"#);
}

#[test]
fn integration_match_bool_patterns() {
    common::check_ok(r#"let x = true
let y = match x
  true => 1
  false => 0
"#);
}

#[test]
fn integration_match_string_patterns() {
    common::check_ok(r#"let x = "hello"
let y = match x
  "hello" => 1
  "world" => 2
  _ => 0
"#);
}

#[test]
fn integration_match_variable_binding() {
    // Wildcard identifier captures the value
    common::check_ok(r#"let x = 42
let y = match x
  1 => "one"
  n => "other"
"#);
}

#[test]
fn integration_match_wildcard() {
    common::check_ok(r#"let x = 10
let y = match x
  _ => 0
"#);
}

// -- Happy path: match as expression returns a value --

#[test]
fn integration_match_expression_in_let() {
    common::check_ok(r#"let x = 5
let result = match x
  1 => 10
  2 => 20
  _ => 0
"#);
}

#[test]
fn integration_match_nested_in_function() {
    common::check_ok(r#"def describe(n: Int) -> String
  match n
    0 => "zero"
    1 => "one"
    _ => "many"
"#);
}

// -- Boundary: single arm, many arms --

#[test]
fn integration_match_single_arm() {
    common::check_ok(r#"let x = 1
let y = match x
  _ => 0
"#);
}

#[test]
fn integration_match_many_arms() {
    common::check_ok(r#"let x = 5
let y = match x
  1 => "a"
  2 => "b"
  3 => "c"
  4 => "d"
  _ => "e"
"#);
}

// -- Error: arm type inconsistency --

#[test]
fn integration_match_arm_type_mismatch_error() {
    let err = common::check_err(r#"let x = 1
let y = match x
  1 => "hello"
  _ => 42
"#);
    assert!(err.contains("match") || err.contains("arm") || err.contains("mismatch") || err.contains("Match"));
}

// -- Error: pattern type vs scrutinee type mismatch --

#[test]
fn integration_match_pattern_type_mismatch_error() {
    let err = common::check_err(r#"let x = 1
let y = match x
  "hello" => 1
  _ => 2
"#);
    assert!(err.contains("pattern") || err.contains("mismatch") || err.contains("Pattern") || err.contains("match"));
}

// ─── 6C. Call-Site Async ─────────────────────────────────────────────

#[test]
fn integration_resolve_call() {
    common::check_ok(r#"async def fetch_data() -> String
  "data"

def main() -> String
  resolve fetch_data()
"#);
}

#[test]
fn integration_async_call_returns_task() {
    common::check_ok(r#"async def fetch() -> Int
  42

async def main() -> Void
  let t: Task[Int] = async fetch()
"#);
}

#[test]
fn integration_detached_async_call() {
    common::check_ok(r#"async def background_job() -> Void
  log("working")

async def main() -> Void
  detached async background_job()
"#);
}

// -- resolve resolves suspension: parent stays sync --

#[test]
fn integration_resolve_makes_parent_sync() {
    common::check_ok(r#"async def slow_op() -> Int
  42

def sync_caller() -> Int
  resolve slow_op()
"#);
}

// -- async scope for structured concurrency --

#[test]
fn integration_async_scope() {
    common::check_ok(r#"async def task_a() -> Int
  1

async def task_b() -> Int
  2

async def main() -> Void
  async scope
    let a = async task_a()
    let b = async task_b()
"#);
}

// -- Error: calling async function synchronously from sync context --

#[test]
fn integration_sync_call_to_async_error() {
    let err = common::check_err(r#"async def fetch() -> Int
  42

def main() -> Int
  fetch()
"#);
    assert!(err.contains("async") || err.contains("suspend") || err.contains("resolve"));
}

// -- Error: async call outside async context --

#[test]
fn integration_async_call_in_sync_context_error() {
    let err = common::check_err(r#"async def fetch() -> Int
  42

def main() -> Int
  let t = async fetch()
  0
"#);
    assert!(err.contains("async") || err.contains("context") || err.contains("scope"));
}

// -- Error: detached without async --

#[test]
fn integration_detached_without_async_error() {
    let err = common::check_parse_err(r#"async def fetch() -> Int
  42

async def main() -> Void
  detached fetch()
"#);
    assert!(err.contains("detached") || err.contains("async"));
}

// -- Task type --

#[test]
fn integration_task_type_annotation() {
    common::check_ok(r#"async def fetch() -> Int
  42

async def main() -> Void
  let t: Task[Int] = async fetch()
"#);
}
