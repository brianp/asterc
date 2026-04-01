#[test]
fn run_executes_hello_example() {
    let output = crate::common::cli(&["run", "examples/executable/hello.aster"]);
    assert!(
        output.status.success(),
        "{}",
        crate::common::output_text(&output)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "Hello\nYes\n");
}

#[test]
fn run_returns_exit_code_from_main() {
    let output = crate::common::cli(&["run", "examples/executable/fibonacci.aster"]);
    assert_eq!(
        output.status.code(),
        Some(55),
        "{}",
        crate::common::output_text(&output)
    );
}

#[test]
fn run_reports_no_main_for_spec_example() {
    // Spec file with trait definitions at top level — compiles FIR but has no main()
    let output = crate::common::cli(&["run", "examples/spec/11_generics_and_traits.aster"]);
    // Traits are now supported in FIR lowering, so the error is "no main() found" (exit 1)
    // rather than the old "not executable yet" (exit 2).
    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        crate::common::output_text(&output)
    );

    let text = crate::common::output_text(&output);
    assert!(text.contains("no main()"), "{text}");
    assert!(!text.contains("Discriminant("), "{text}");
}

#[test]
fn run_top_level_control_flow() {
    // Top-level if/while/for should execute
    let dir = crate::common::make_temp_dir("tl-ctrl");
    let src = dir.join("top_level.aster");
    std::fs::write(
        &src,
        "\
let x = 0
let total = 0
while x < 5
  total = total + x
  x = x + 1

if total > 5
  total = total + 100

def main() -> Int
  total
",
    )
    .unwrap();
    let output = crate::common::cli(&["run", src.to_str().unwrap()]);
    assert_eq!(
        output.status.code(),
        Some(110),
        "{}",
        crate::common::output_text(&output)
    );
}

#[test]
fn build_and_run_top_level_for() {
    let dir = crate::common::make_temp_dir("tl-for");
    let src = dir.join("for_loop.aster");
    std::fs::write(
        &src,
        "\
let nums = [10, 20, 12]
let total = 0
for n in nums
  total = total + n

def main() -> Int
  total
",
    )
    .unwrap();
    let output = crate::common::build_and_run(&src);
    assert_eq!(
        output.status.code(),
        Some(42),
        "{}",
        crate::common::output_text(&output)
    );
}

#[test]
fn run_blocking_call_on_suspendable_callee_returns_plain_value() {
    let dir = crate::common::make_temp_dir("blocking-run");
    let src = dir.join("blocking_value.aster");
    std::fs::write(
        &src,
        "\
def child() -> Int
  41

def parent() -> Int
  let t: Task[Int] = async child()
  resolve t! + 1

def main() -> Int
  blocking parent()
",
    )
    .unwrap();

    let output = crate::common::cli(&["run", src.to_str().unwrap()]);
    assert_eq!(
        output.status.code(),
        Some(42),
        "{}",
        crate::common::output_text(&output)
    );
}

#[test]
fn run_wait_cancel_observes_cancelled_terminal_state() {
    let dir = crate::common::make_temp_dir("wait-cancel-run");
    let src = dir.join("wait_cancel.aster");
    std::fs::write(
        &src,
        "\
def slow() -> Int
  let i: Int = 0
  let total: Int = 0
  while i < 20000000
    total = total + i
    i = i + 1
  42

def main() -> Int
  let t: Task[Int] = async slow()
  t.wait_cancel()
  resolve t!.catch
    _ -> 99
",
    )
    .unwrap();

    let output = crate::common::cli(&["run", src.to_str().unwrap()]);
    assert_eq!(
        output.status.code(),
        Some(99),
        "{}",
        crate::common::output_text(&output)
    );
}

/// GH #3: .catch dispatches on the second arm (not just the first).
/// Throws ParseError, which should match the second arm and return exit code 2.
#[test]
fn run_catch_dispatches_second_arm() {
    let dir = crate::common::make_temp_dir("catch-dispatch-run");
    let src = dir.join("catch_dispatch.aster");
    std::fs::write(
        &src,
        "\
class NetworkError extends Error
  code: Int

class ParseError extends Error
  line: Int

def risky() throws Error -> Int
  throw ParseError(message: \"bad\", line: 42)

def main() -> Int
  risky()!.catch
    NetworkError e -> 1
    ParseError e -> 2
    _ -> 3
",
    )
    .unwrap();

    let output = crate::common::cli(&["run", src.to_str().unwrap()]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "Expected ParseError arm (exit 2), got: {}",
        crate::common::output_text(&output)
    );
}

/// GH #3: .catch binds error variable and accesses its field.
#[test]
fn run_catch_binds_error_variable_field() {
    let dir = crate::common::make_temp_dir("catch-bind-run");
    let src = dir.join("catch_bind.aster");
    std::fs::write(
        &src,
        "\
class AppError extends Error
  code: Int

def risky() throws AppError -> Int
  throw AppError(message: \"fail\", code: 77)

def main() -> Int
  risky()!.catch
    AppError e -> e.code
    _ -> 0
",
    )
    .unwrap();

    let output = crate::common::cli(&["run", src.to_str().unwrap()]);
    assert_eq!(
        output.status.code(),
        Some(77),
        "Expected AppError.code (77), got: {}",
        crate::common::output_text(&output)
    );
}

#[test]
fn run_resolve_first_returns_fastest_value() {
    let dir = crate::common::make_temp_dir("resolve-first-run");
    let src = dir.join("resolve_first.aster");
    std::fs::write(
        &src,
        "\
def slow() -> Int
  let i: Int = 0
  let total: Int = 0
  while i < 20000000
    total = total + i
    i = i + 1
  10

def fast() -> Int
  42

def main() -> Int
  let tasks: List[Task[Int]] = [async slow(), async fast()]
  resolve_first(tasks: tasks)!
",
    )
    .unwrap();

    let output = crate::common::cli(&["run", src.to_str().unwrap()]);
    assert_eq!(
        output.status.code(),
        Some(42),
        "{}",
        crate::common::output_text(&output)
    );
}

// ===========================================================================
// Iterable vocabulary methods — end-to-end execution
// ===========================================================================

#[test]
fn run_iterable_count() {
    let dir = crate::common::make_temp_dir("iter-count");
    let src = dir.join("count.aster");
    std::fs::write(
        &src,
        "\
let xs = [10, 20, 30]
def main() -> Int
  xs.count()
",
    )
    .unwrap();
    let output = crate::common::build_and_run(&src);
    assert_eq!(
        output.status.code(),
        Some(3),
        "{}",
        crate::common::output_text(&output)
    );
}

#[test]
fn run_iterable_reduce() {
    let dir = crate::common::make_temp_dir("iter-reduce");
    let src = dir.join("reduce.aster");
    std::fs::write(
        &src,
        "\
let xs = [1, 2, 3, 4]
let total = xs.reduce(init: 0, f: -> acc, x: acc + x)
def main() -> Int
  total
",
    )
    .unwrap();
    let output = crate::common::build_and_run(&src);
    assert_eq!(
        output.status.code(),
        Some(10),
        "{}",
        crate::common::output_text(&output)
    );
}

#[test]
fn run_iterable_map() {
    let dir = crate::common::make_temp_dir("iter-map");
    let src = dir.join("map.aster");
    std::fs::write(
        &src,
        "\
let xs = [1, 2, 3]
let ys = xs.map(f: -> x: x * 10)
def main() -> Int
  ys.reduce(init: 0, f: -> acc, x: acc + x)
",
    )
    .unwrap();
    let output = crate::common::build_and_run(&src);
    assert_eq!(
        output.status.code(),
        Some(60),
        "{}",
        crate::common::output_text(&output)
    );
}

#[test]
fn run_iterable_filter() {
    let dir = crate::common::make_temp_dir("iter-filter");
    let src = dir.join("filter.aster");
    std::fs::write(
        &src,
        "\
let xs = [1, 2, 3, 4, 5]
let evens = xs.filter(f: -> x: x % 2 == 0)
def main() -> Int
  evens.count()
",
    )
    .unwrap();
    let output = crate::common::build_and_run(&src);
    assert_eq!(
        output.status.code(),
        Some(2),
        "{}",
        crate::common::output_text(&output)
    );
}

#[test]
fn run_iterable_any_true() {
    let dir = crate::common::make_temp_dir("iter-any");
    let src = dir.join("any.aster");
    std::fs::write(
        &src,
        "\
let xs = [1, 2, 3]
let found = xs.any(f: -> x: x == 2)
def main() -> Int
  let result = 0
  if found
    result = 1
  result
",
    )
    .unwrap();
    let output = crate::common::build_and_run(&src);
    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        crate::common::output_text(&output)
    );
}

#[test]
fn run_iterable_all_true() {
    let dir = crate::common::make_temp_dir("iter-all");
    let src = dir.join("all.aster");
    std::fs::write(
        &src,
        "\
let xs = [1, 2, 3]
let ok = xs.all(f: -> x: x > 0)
def main() -> Int
  let result = 0
  if ok
    result = 1
  result
",
    )
    .unwrap();
    let output = crate::common::build_and_run(&src);
    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        crate::common::output_text(&output)
    );
}

#[test]
fn run_iterable_first_last() {
    let dir = crate::common::make_temp_dir("iter-first-last");
    let src = dir.join("first_last.aster");
    std::fs::write(
        &src,
        "\
let xs = [10, 20, 30]
let f = xs.first().or(default: 0)
let l = xs.last().or(default: 0)
def main() -> Int
  f + l
",
    )
    .unwrap();
    let output = crate::common::build_and_run(&src);
    assert_eq!(
        output.status.code(),
        Some(40),
        "{}",
        crate::common::output_text(&output)
    );
}

#[test]
fn run_iterable_to_list() {
    let dir = crate::common::make_temp_dir("iter-tolist");
    let src = dir.join("tolist.aster");
    std::fs::write(
        &src,
        "\
let xs = [1, 2, 3]
let ys = xs.to_list()
def main() -> Int
  ys.count()
",
    )
    .unwrap();
    let output = crate::common::build_and_run(&src);
    assert_eq!(
        output.status.code(),
        Some(3),
        "{}",
        crate::common::output_text(&output)
    );
}

#[test]
fn run_iterable_sort() {
    let dir = crate::common::make_temp_dir("iter-sort");
    let src = dir.join("sort.aster");
    std::fs::write(
        &src,
        "\
let xs = [3, 1, 2]
let sorted = xs.sort()
let first = sorted.first().or(default: 0)
let last = sorted.last().or(default: 0)
def main() -> Int
  first * 100 + last
",
    )
    .unwrap();
    let output = crate::common::build_and_run(&src);
    // sorted = [1,2,3], first=1, last=3, 1*100+3 = 103
    assert_eq!(
        output.status.code(),
        Some(103),
        "{}",
        crate::common::output_text(&output)
    );
}

#[test]
fn run_iterable_min_max() {
    let dir = crate::common::make_temp_dir("iter-minmax");
    let src = dir.join("minmax.aster");
    std::fs::write(
        &src,
        "\
let xs = [5, 2, 8, 1, 9]
let lo = xs.min().or(default: 0)
let hi = xs.max().or(default: 0)
def main() -> Int
  lo * 100 + hi
",
    )
    .unwrap();
    let output = crate::common::build_and_run(&src);
    // min=1, max=9, 1*100+9 = 109
    assert_eq!(
        output.status.code(),
        Some(109),
        "{}",
        crate::common::output_text(&output)
    );
}

#[test]
fn run_iterable_find() {
    let dir = crate::common::make_temp_dir("iter-find");
    let src = dir.join("find.aster");
    std::fs::write(
        &src,
        "\
let xs = [10, 20, 30]
let found = xs.find(f: -> x: x > 15)
def main() -> Int
  found.or(default: 0)
",
    )
    .unwrap();
    let output = crate::common::build_and_run(&src);
    assert_eq!(
        output.status.code(),
        Some(20),
        "{}",
        crate::common::output_text(&output)
    );
}

#[test]
fn run_int_min_div_neg_one_returns_zero() {
    let dir = crate::common::make_temp_dir("min-div");
    let src = dir.join("min_div.aster");
    std::fs::write(
        &src,
        "\
let min = -9223372036854775807 - 1
let result = min / -1
def main() -> Int
  result
",
    )
    .unwrap();
    let output = crate::common::cli(&["run", src.to_str().unwrap()]);
    assert!(
        output.status.success(),
        "i64::MIN / -1 should not SIGFPE: {}",
        crate::common::output_text(&output)
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "i64::MIN / -1 returns 0: {}",
        crate::common::output_text(&output)
    );
}

#[test]
fn run_int_min_mod_neg_one_returns_zero() {
    let dir = crate::common::make_temp_dir("min-mod");
    let src = dir.join("min_mod.aster");
    std::fs::write(
        &src,
        "\
let min = -9223372036854775807 - 1
let result = min % -1
def main() -> Int
  result
",
    )
    .unwrap();
    let output = crate::common::cli(&["run", src.to_str().unwrap()]);
    assert!(
        output.status.success(),
        "i64::MIN %% -1 should not SIGFPE: {}",
        crate::common::output_text(&output)
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "i64::MIN %% -1 returns 0: {}",
        crate::common::output_text(&output)
    );
}

#[test]
fn run_normal_division_still_works() {
    let dir = crate::common::make_temp_dir("normal-div");
    let src = dir.join("normal_div.aster");
    std::fs::write(
        &src,
        "\
def main() -> Int
  let a = 100 / 3
  let b = 100 % 3
  a + b
",
    )
    .unwrap();
    let output = crate::common::cli(&["run", src.to_str().unwrap()]);
    // 100/3 = 33, 100%3 = 1, sum = 34
    assert_eq!(
        output.status.code(),
        Some(34),
        "{}",
        crate::common::output_text(&output)
    );
}

#[test]
fn run_string_equality_compares_content() {
    let dir = crate::common::make_temp_dir("str-eq");
    let src = dir.join("str_eq.aster");
    std::fs::write(
        &src,
        r#"
let a = "hello"
let b = "hel" + "lo"
let result = 0
if a == b
  result = 1

def main() -> Int
  result
"#,
    )
    .unwrap();
    let output = crate::common::cli(&["run", src.to_str().unwrap()]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "string == should compare content, not pointers: {}",
        crate::common::output_text(&output)
    );
}

#[test]
fn run_string_inequality_compares_content() {
    let dir = crate::common::make_temp_dir("str-neq");
    let src = dir.join("str_neq.aster");
    std::fs::write(
        &src,
        r#"
let a = "hello"
let b = "world"
let result = 0
if a != b
  result = 1

def main() -> Int
  result
"#,
    )
    .unwrap();
    let output = crate::common::cli(&["run", src.to_str().unwrap()]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "string != should compare content: {}",
        crate::common::output_text(&output)
    );
}

#[test]
fn run_string_less_than() {
    let dir = crate::common::make_temp_dir("str-lt");
    let src = dir.join("str_lt.aster");
    std::fs::write(
        &src,
        r#"
let a = "apple"
let b = "banana"
let result = 0
if a < b
  result = 1

def main() -> Int
  result
"#,
    )
    .unwrap();
    let output = crate::common::cli(&["run", src.to_str().unwrap()]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "\"apple\" < \"banana\" should be true: {}",
        crate::common::output_text(&output)
    );
}

// ─── Iterable closures with captured variables (GH-1) ───────────────

#[test]
fn run_iterable_map_with_captured_int() {
    let dir = crate::common::make_temp_dir("iter-map-capture-int");
    let src = dir.join("map_capture.aster");
    std::fs::write(
        &src,
        "\
let items = [1, 2, 3]
let multiplier = 10
let result = items.map(f: -> x: x * multiplier)
def main() -> Int
  result.reduce(init: 0, f: -> acc, x: acc + x)
",
    )
    .unwrap();
    let output = crate::common::build_and_run(&src);
    assert_eq!(
        output.status.code(),
        Some(60),
        "map with captured int: {}",
        crate::common::output_text(&output)
    );
}

#[test]
fn run_iterable_filter_with_captured_int() {
    let dir = crate::common::make_temp_dir("iter-filter-capture-int");
    let src = dir.join("filter_capture.aster");
    std::fs::write(
        &src,
        "\
let items = [1, 2, 3, 4, 5]
let threshold = 3
let big = items.filter(f: -> x: x > threshold)
def main() -> Int
  big.count()
",
    )
    .unwrap();
    let output = crate::common::build_and_run(&src);
    assert_eq!(
        output.status.code(),
        Some(2),
        "filter with captured int: {}",
        crate::common::output_text(&output)
    );
}

#[test]
fn run_iterable_map_with_captured_string() {
    let dir = crate::common::make_temp_dir("iter-map-capture-str");
    let src = dir.join("map_capture_str.aster");
    std::fs::write(
        &src,
        r#"
let items = [1, 2, 3]
let tag = "v"
let strs = items.map(f: -> x: tag)
def main() -> Int
  strs.count()
"#,
    )
    .unwrap();
    let output = crate::common::build_and_run(&src);
    assert_eq!(
        output.status.code(),
        Some(3),
        "map with captured string: {}",
        crate::common::output_text(&output)
    );
}

#[test]
fn run_iterable_reduce_with_captured_int() {
    let dir = crate::common::make_temp_dir("iter-reduce-capture");
    let src = dir.join("reduce_capture.aster");
    std::fs::write(
        &src,
        "\
let items = [1, 2, 3, 4]
let bonus = 10
let total = items.reduce(init: bonus, f: -> acc, x: acc + x)
def main() -> Int
  total
",
    )
    .unwrap();
    let output = crate::common::build_and_run(&src);
    assert_eq!(
        output.status.code(),
        Some(20),
        "reduce with captured init: {}",
        crate::common::output_text(&output)
    );
}

#[test]
fn run_iterable_map_with_string_interpolation_capture() {
    // GH-1: This test exercises the critical path -- map callback produces Ptr
    // temporaries (via string interpolation with captured variable) that must be
    // rooted before passing to aster_list_push. Without the fix, GC pressure
    // during list_push can collect the unrooted temporary.
    let dir = crate::common::make_temp_dir("iter-map-strinterp");
    let src = dir.join("map_strinterp.aster");
    std::fs::write(
        &src,
        r#"
let items = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
let tag = "v"
let strs = items.map(f: -> x: "{tag}{x}")
def main() -> Int
  strs.count()
"#,
    )
    .unwrap();
    let output = crate::common::build_and_run(&src);
    assert_eq!(
        output.status.code(),
        Some(10),
        "map with string interpolation capture: {}",
        crate::common::output_text(&output)
    );
}

#[test]
fn run_iterable_map_gc_pressure_string_alloc() {
    // GC-pressure test: map over a large list producing Ptr results (strings)
    // each iteration. With enough allocations, aster_list_push will trigger GC
    // while an unrooted Ptr temporary is live. This test would segfault or
    // produce wrong results without the root_if_ptr fix.
    let dir = crate::common::make_temp_dir("iter-map-gc-pressure");
    let src = dir.join("gc_pressure.aster");
    std::fs::write(
        &src,
        r#"
def main() -> Int
  let items = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50]
  let tag = "item"
  let strs = items.map(f: -> x: "{tag}{x}")
  strs.count()
"#,
    )
    .unwrap();
    let output = crate::common::build_and_run(&src);
    assert_eq!(
        output.status.code(),
        Some(50),
        "map gc pressure (50 string allocs): {}",
        crate::common::output_text(&output)
    );
}

#[test]
fn run_iterable_map_chain_with_capture() {
    let dir = crate::common::make_temp_dir("iter-map-chain-capture");
    let src = dir.join("map_chain_capture.aster");
    std::fs::write(
        &src,
        "\
let items = [1, 2, 3, 4]
let scale = 2
let offset = 1
let result = items.map(f: -> x: x * scale).map(f: -> x: x + offset)
def main() -> Int
  result.reduce(init: 0, f: -> acc, x: acc + x)
",
    )
    .unwrap();
    let output = crate::common::build_and_run(&src);
    // items: [1,2,3,4] -> *2 -> [2,4,6,8] -> +1 -> [3,5,7,9] -> sum = 24
    assert_eq!(
        output.status.code(),
        Some(24),
        "chained map with captures: {}",
        crate::common::output_text(&output)
    );
}

// ─── .each(f:) lowering (GH-4) ──────────────────────────────────────

#[test]
fn run_iterable_each_executes_callback() {
    // .each should execute the callback for each element.
    // Verify via log output.
    let dir = crate::common::make_temp_dir("iter-each");
    let src = dir.join("each.aster");
    std::fs::write(
        &src,
        r#"
def main() -> Int
  let items = [1, 2, 3]
  items.each(f: -> x: log(message: "ok"))
  0
"#,
    )
    .unwrap();
    let output = crate::common::build_and_run(&src);
    assert!(
        output.status.success(),
        "each should not crash: {}",
        crate::common::output_text(&output)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let ok_count = stdout.matches("ok").count();
    assert_eq!(
        ok_count, 3,
        "each should call callback 3 times, got: {}",
        stdout
    );
}

#[test]
fn run_iterable_each_with_capture() {
    // .each callback captures an outer variable and uses it
    let dir = crate::common::make_temp_dir("iter-each-capture");
    let src = dir.join("each_capture.aster");
    std::fs::write(
        &src,
        r#"
def main() -> Int
  let items = [10, 20, 30]
  let tag = "val"
  items.each(f: -> x: log(message: tag))
  0
"#,
    )
    .unwrap();
    let output = crate::common::build_and_run(&src);
    assert!(
        output.status.success(),
        "each with capture should not crash: {}",
        crate::common::output_text(&output)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let tag_count = stdout.matches("val").count();
    assert_eq!(
        tag_count, 3,
        "each should call callback 3 times with capture, got: {}",
        stdout
    );
}

#[test]
fn run_iterable_each_empty_list() {
    // .each on an empty list should not execute the callback and not crash.
    let dir = crate::common::make_temp_dir("iter-each-empty");
    let src = dir.join("each_empty.aster");
    std::fs::write(
        &src,
        r#"
def main() -> Int
  let items: List[Int] = []
  items.each(f: -> x: log(message: "bad"))
  0
"#,
    )
    .unwrap();
    let output = crate::common::build_and_run(&src);
    assert!(
        output.status.success(),
        "each on empty list should not crash: {}",
        crate::common::output_text(&output)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout.matches("bad").count(),
        0,
        "each on empty list should not call callback, got: {}",
        stdout
    );
}
