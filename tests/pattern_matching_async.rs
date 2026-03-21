mod common;

// ═══════════════════════════════════════════════════════════════════════
// Phase 6: Call-Site Async, Error Handling, Pattern Matching
// ═══════════════════════════════════════════════════════════════════════

// ─── 6A. Pattern Matching ────────────────────────────────────────────

#[test]
fn match_int_literal_patterns() {
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
fn match_bool_patterns() {
    common::check_ok(
        r#"let x = true
let y = match x
  true => 1
  false => 0
"#,
    );
}

#[test]
fn match_string_patterns() {
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
fn match_variable_binding() {
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
fn match_wildcard() {
    common::check_ok(
        r#"let x = 10
let y = match x
  _ => 0
"#,
    );
}

// -- Happy path: match as expression returns a value --

#[test]
fn match_expression_in_let() {
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
fn match_nested_in_function() {
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
fn match_single_arm() {
    common::check_ok(
        r#"let x = 1
let y = match x
  _ => 0
"#,
    );
}

#[test]
fn match_many_arms() {
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
fn match_arm_type_mismatch_error() {
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
fn match_pattern_type_mismatch_error() {
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
fn async_call_returns_task() {
    // BC-9: functions are plain def, async at call site returns Task[T]
    common::check_ok(
        r#"def fetch() -> Int
  42

def main() -> Task[Int]
  let t: Task[Int] = async fetch()
  t
"#,
    );
}

#[test]
fn blocking_call_returns_plain_value() {
    common::check_ok(
        r#"def fetch() -> Int
  async fetch_child()
  42

def fetch_child() -> Int
  42

def main() -> Int
  blocking fetch()
"#,
    );
}

#[test]
fn plain_call_to_suspendable_callee_is_compile_error() {
    let err = common::check_err(
        r#"def fetch() -> Int
  async fetch_child()
  42

def fetch_child() -> Int
  7

def main() -> Int
  fetch()
"#,
    );
    assert!(err.contains("blocking fetch()") || err.contains("async fetch()"));
}

#[test]
fn cross_module_suspendable_metadata_rejects_plain_call() {
    let mut files = std::collections::HashMap::new();
    files.insert(
        "worker".to_string(),
        r#"pub def fetch_child() -> Int
  7

pub def fetch() -> Int
  async fetch_child()
  42
"#
        .to_string(),
    );
    let err = common::check_err_with_files(
        r#"use worker { fetch }

def main() -> Int
  fetch()
"#,
        files,
    );
    assert!(err.contains("blocking fetch()") || err.contains("async fetch()"));
}

#[test]
fn cross_module_nested_suspendability_metadata_rejects_plain_call() {
    let mut files = std::collections::HashMap::new();
    files.insert(
        "worker".to_string(),
        r#"pub def fetch_child() -> Int
  7

pub def fetch() -> Int
  if true
    async fetch_child()
  42
"#
        .to_string(),
    );
    let err = common::check_err_with_files(
        r#"use worker { fetch }

def main() -> Int
  fetch()
"#,
        files,
    );
    assert!(err.contains("blocking fetch()") || err.contains("async fetch()"));
}

#[test]
fn detached_async_call() {
    common::check_ok(
        r#"def background_job() -> Void
  log(message: "working")

def main() -> Void
  detached async background_job()
"#,
    );
}

// -- async call works anywhere (no context restriction) --

#[test]
fn async_call_works_anywhere() {
    common::check_ok(
        r#"def fetch() -> Int
  42

def main() -> Task[Int]
  let t = async fetch()
  t
"#,
    );
}

// -- Error: detached without async --

#[test]
fn detached_without_async_error() {
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
fn task_type_annotation() {
    common::check_ok(
        r#"def fetch() -> Int
  42

def main() -> Task[Int]
  let t: Task[Int] = async fetch()
  t
"#,
    );
}

// -- resolve on Task[T] requires ! --

#[test]
fn resolve_without_bang_error() {
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
fn async_def_is_parse_error() {
    let err = common::check_parse_err(
        r#"async def fetch() -> Int
  42
"#,
    );
    assert!(err.contains("async def is not supported"));
}

// -- resolve on computed expressions (C1 fix) --

#[test]
fn resolve_member_access() {
    // resolve works on member access expressions
    common::check_ok(
        r#"class TaskHolder
  task: Task[Int]

def fetch() -> Int
  42

def main() -> Void
  let holder = TaskHolder(task: async fetch())
  let v = resolve holder.task!
"#,
    );
}

#[test]
fn resolve_index_access() {
    // resolve works on index expressions
    common::check_ok(
        r#"def fetch() -> Int
  42

def main() -> Void
  let tasks: List[Task[Int]] = [async fetch()]
  let v = resolve tasks[0]!
"#,
    );
}

// -- async + throws composition (C2 fix) --

#[test]
fn async_throwing_function() {
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
fn async_throwing_with_catch() {
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
fn async_throwing_with_or() {
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
fn detached_async_throwing() {
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

// -- CancelledError is implicit in task resolution --

#[test]
fn resolve_task_without_throws_cancelled_error() {
    // resolve task! should work without declaring throws CancelledError
    // CancelledError is a language-level concern, not a user concern
    common::check_ok(
        r#"def compute() -> Int
  42

def main() -> Int
  let t = async compute()
  resolve t!
"#,
    );
}

#[test]
fn resolve_task_without_any_throws_declaration() {
    // Function that only resolves tasks needs no throws at all
    common::check_ok(
        r#"def work_a() -> Int
  10

def work_b() -> Int
  20

def main() -> Int
  let a = async work_a()
  let b = async work_b()
  resolve a! + resolve b!
"#,
    );
}

#[test]
fn resolve_without_bang_still_errors() {
    // resolve without ! is still an error — ! is the propagation operator
    let err = common::check_err(
        r#"def compute() -> Int
  42

def main() -> Int
  let t = async compute()
  resolve t
"#,
    );
    assert!(err.contains("resolve") || err.contains("!"));
}

#[test]
fn explicit_throw_cancelled_error_requires_throws() {
    // If the user explicitly throws CancelledError, they must declare it
    let err = common::check_err(
        r#"def main() -> Void
  throw CancelledError(message: "abort")
"#,
    );
    assert!(err.contains("throws") || err.contains("E013"));
}

// ─── Soundness: S1 — Nullable match catch-all must not silently unwrap ───

#[test]
fn soundness_nullable_match_catchall_without_nil_arm_binds_nullable() {
    // Without a nil arm, catch-all should bind as T? (not T)
    // So v + 1 should fail because v is Int?, not Int
    let err = common::check_err(
        r#"let x: Int? = nil
let y = match x
  v => v + 1
"#,
    );
    assert!(
        err.contains("mismatch") || err.contains("Nullable") || err.contains("Int?"),
        "Expected type error because v should be Int?, got: {}",
        err
    );
}

#[test]
fn soundness_nullable_match_with_nil_arm_then_catchall_unwraps() {
    // With a nil arm before the catch-all, v is safely unwrapped to T
    common::check_ok(
        r#"let x: Int? = nil
let y = match x
  nil => 0
  v => v + 1
"#,
    );
}

// ═══════════════════════════════════════════════════════════════════════
// S5: Enum variant match patterns
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn enum_variant_match_pattern_basic() {
    common::check_ok(
        r#"enum Color
  Red
  Green
  Blue

let c = Color.Red
let name = match c
  Color.Red => "red"
  Color.Green => "green"
  Color.Blue => "blue"
"#,
    );
}

#[test]
fn enum_variant_match_pattern_with_wildcard() {
    common::check_ok(
        r#"enum Color
  Red
  Green
  Blue

let c = Color.Red
let name = match c
  Color.Red => "red"
  _ => "other"
"#,
    );
}

#[test]
fn enum_variant_match_wrong_enum_type() {
    let err = common::check_err(
        r#"enum Color
  Red
  Green
  Blue

enum Size
  Small
  Large

let c = Color.Red
let name = match c
  Size.Small => "small"
  _ => "other"
"#,
    );
    assert!(err.contains("mismatch") || err.contains("Color") || err.contains("Size"));
}

#[test]
fn enum_variant_match_unknown_variant() {
    let err = common::check_err(
        r#"enum Color
  Red
  Green
  Blue

let c = Color.Red
let name = match c
  Color.Purple => "purple"
  _ => "other"
"#,
    );
    assert!(err.contains("Purple") || err.contains("variant") || err.contains("unknown"));
}

// ═══════════════════════════════════════════════════════════════════════
// S1: Enum exhaustiveness checking
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn enum_exhaustive_match_no_wildcard_needed() {
    common::check_ok(
        r#"enum Direction
  North
  South
  East
  West

let d = Direction.North
let name = match d
  Direction.North => "n"
  Direction.South => "s"
  Direction.East => "e"
  Direction.West => "w"
"#,
    );
}

#[test]
fn enum_non_exhaustive_match_error() {
    let err = common::check_err(
        r#"enum Direction
  North
  South
  East
  West

let d = Direction.North
let name = match d
  Direction.North => "n"
  Direction.South => "s"
"#,
    );
    assert!(
        err.contains("exhaustive")
            || err.contains("East")
            || err.contains("West")
            || err.contains("missing")
    );
}

#[test]
fn enum_match_with_ordering_builtin() {
    common::check_ok(
        r#"class Point includes Ord
  x: Int

  def cmp(other: Point) -> Ordering
    match true
      true => Ordering.Equal
      false => Ordering.Less

let p1 = Point(x: 1)
let p2 = Point(x: 2)
let result = p1.cmp(other: p2)
let name = match result
  Ordering.Less => "less"
  Ordering.Equal => "equal"
  Ordering.Greater => "greater"
"#,
    );
}
