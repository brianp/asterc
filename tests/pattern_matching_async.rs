mod common;

// ═══════════════════════════════════════════════════════════════════════
// Phase 6: Call-Site Async, Error Handling, Pattern Matching
// ═══════════════════════════════════════════════════════════════════════

// ─── 6A. Pattern Matching ────────────────────────────────────────────

#[test]
fn integration_match_int_literal_patterns() {
    common::check_ok(
        r#"let x = 5
let y = match x
  1 => "one"
  2 => "two"
  _ => "other"
"#,
    );
}

#[test]
fn integration_match_bool_patterns() {
    common::check_ok(
        r#"let x = true
let y = match x
  true => 1
  false => 0
"#,
    );
}

#[test]
fn integration_match_string_patterns() {
    common::check_ok(
        r#"let x = "hello"
let y = match x
  "hello" => 1
  "world" => 2
  _ => 0
"#,
    );
}

#[test]
fn integration_match_variable_binding() {
    // Wildcard identifier captures the value
    common::check_ok(
        r#"let x = 42
let y = match x
  1 => "one"
  n => "other"
"#,
    );
}

#[test]
fn integration_match_wildcard() {
    common::check_ok(
        r#"let x = 10
let y = match x
  _ => 0
"#,
    );
}

// -- Happy path: match as expression returns a value --

#[test]
fn integration_match_expression_in_let() {
    common::check_ok(
        r#"let x = 5
let result = match x
  1 => 10
  2 => 20
  _ => 0
"#,
    );
}

#[test]
fn integration_match_nested_in_function() {
    common::check_ok(
        r#"def describe(n: Int) -> String
  match n
    0 => "zero"
    1 => "one"
    _ => "many"
"#,
    );
}

// -- Boundary: single arm, many arms --

#[test]
fn integration_match_single_arm() {
    common::check_ok(
        r#"let x = 1
let y = match x
  _ => 0
"#,
    );
}

#[test]
fn integration_match_many_arms() {
    common::check_ok(
        r#"let x = 5
let y = match x
  1 => "a"
  2 => "b"
  3 => "c"
  4 => "d"
  _ => "e"
"#,
    );
}

// -- Error: arm type inconsistency --

#[test]
fn integration_match_arm_type_mismatch_error() {
    let err = common::check_err(
        r#"let x = 1
let y = match x
  1 => "hello"
  _ => 42
"#,
    );
    assert!(
        err.contains("match")
            || err.contains("arm")
            || err.contains("mismatch")
            || err.contains("Match")
    );
}

// -- Error: pattern type vs scrutinee type mismatch --

#[test]
fn integration_match_pattern_type_mismatch_error() {
    let err = common::check_err(
        r#"let x = 1
let y = match x
  "hello" => 1
  _ => 2
"#,
    );
    assert!(
        err.contains("pattern")
            || err.contains("mismatch")
            || err.contains("Pattern")
            || err.contains("match")
    );
}

// ─── 6C. Call-Site Async (BC-9: no async def, async works anywhere) ──

#[test]
fn integration_async_call_returns_task() {
    // BC-9: functions are plain def, async at call site returns Task[T]
    common::check_ok(
        r#"def fetch() -> Int
  42

def main() -> Void
  let t: Task[Int] = async fetch()
"#,
    );
}

#[test]
fn integration_detached_async_call() {
    common::check_ok(
        r#"def background_job() -> Void
  log(message: "working")

def main() -> Void
  detached async background_job()
"#,
    );
}

// -- async scope for structured concurrency --

#[test]
fn integration_async_scope() {
    common::check_ok(
        r#"def task_a() -> Int
  1

def task_b() -> Int
  2

def main() -> Void
  async scope
    let a = async task_a()
    let b = async task_b()
"#,
    );
}

// -- async call works anywhere (no context restriction) --

#[test]
fn integration_async_call_works_anywhere() {
    common::check_ok(
        r#"def fetch() -> Int
  42

def main() -> Void
  let t = async fetch()
"#,
    );
}

// -- Error: detached without async --

#[test]
fn integration_detached_without_async_error() {
    let err = common::check_parse_err(
        r#"def fetch() -> Int
  42

def main() -> Void
  detached fetch()
"#,
    );
    assert!(err.contains("detached") || err.contains("async"));
}

// -- Task type --

#[test]
fn integration_task_type_annotation() {
    common::check_ok(
        r#"def fetch() -> Int
  42

def main() -> Void
  let t: Task[Int] = async fetch()
"#,
    );
}

// -- resolve on Task[T] requires ! --

#[test]
fn integration_resolve_without_bang_error() {
    let err = common::check_err(
        r#"def fetch() -> Int
  42

def main() -> Void
  let t = async fetch()
  resolve t
"#,
    );
    assert!(err.contains("resolve") || err.contains("!") || err.contains("CancelledError"));
}

// -- async def is now a parse error --

#[test]
fn integration_async_def_is_parse_error() {
    let err = common::check_parse_err(
        r#"async def fetch() -> Int
  42
"#,
    );
    assert!(err.contains("async def is not supported"));
}

// -- resolve on computed expressions (C1 fix) --

#[test]
fn integration_resolve_member_access() {
    // resolve works on member access expressions
    common::check_ok(
        r#"class TaskHolder
  task: Task[Int]

def fetch() -> Int
  42

def main() throws Error -> Void
  let holder = TaskHolder(task: async fetch())
  let v = resolve holder.task!
"#,
    );
}

#[test]
fn integration_resolve_index_access() {
    // resolve works on index expressions
    common::check_ok(
        r#"def fetch() -> Int
  42

def main() throws Error -> Void
  let tasks: List[Task[Int]] = [async fetch()]
  let v = resolve tasks[0]!
"#,
    );
}

// -- async + throws composition (C2 fix) --

#[test]
fn integration_async_throwing_function() {
    // async on a throwing function should work — errors handled at resolve
    common::check_ok(
        r#"class AppError extends Error
  code: Int

def risky() throws AppError -> String
  "data"

def main() throws Error -> Void
  let task = async risky()
  let result = resolve task!
"#,
    );
}

#[test]
fn integration_async_throwing_with_catch() {
    // async throwing + resolve with catch
    common::check_ok(
        r#"class AppError extends Error
  code: Int

def risky() throws AppError -> String
  "data"

def main() -> String
  let task = async risky()
  resolve task!.catch
    CancelledError e -> "cancelled"
    AppError e -> e.message
    _ -> "unknown"
"#,
    );
}

#[test]
fn integration_async_throwing_with_or() {
    // async throwing + resolve with or
    common::check_ok(
        r#"class AppError extends Error
  code: Int

def risky() throws AppError -> String
  "data"

def main() -> String
  let task = async risky()
  resolve task!.or("fallback")
"#,
    );
}

#[test]
fn integration_detached_async_throwing() {
    // detached async on throwing functions — errors logged at runtime
    common::check_ok(
        r#"class AppError extends Error
  code: Int

def risky() throws AppError -> Void
  log(message: "working")

def main() -> Void
  detached async risky()
"#,
    );
}
