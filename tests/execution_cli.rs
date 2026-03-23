mod common;

#[test]
fn run_executes_hello_example() {
    let output = common::cli(&["run", "examples/executable/hello.aster"]);
    assert!(output.status.success(), "{}", common::output_text(&output));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "Hello\nYes\n");
}

#[test]
fn run_returns_exit_code_from_main() {
    let output = common::cli(&["run", "examples/executable/fibonacci.aster"]);
    assert_eq!(
        output.status.code(),
        Some(55),
        "{}",
        common::output_text(&output)
    );
}

#[test]
fn run_reports_not_executable_yet_for_spec_example() {
    // Spec file with trait definitions at top level — still not executable
    let output = common::cli(&["run", "examples/spec/11_generics_and_traits.aster"]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "{}",
        common::output_text(&output)
    );

    let text = common::output_text(&output);
    assert!(text.contains("not executable yet"), "{text}");
    assert!(text.contains("top-level `trait`"), "{text}");
    assert!(!text.contains("Discriminant("), "{text}");
    // E028 errors must point at a source location (span), not just a message
    assert!(
        text.contains(":16:"),
        "E028 should include a source location: {text}"
    );
}

#[test]
fn run_top_level_control_flow() {
    // Top-level if/while/for should execute
    let dir = common::make_temp_dir("tl-ctrl");
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
    let output = common::cli(&["run", src.to_str().unwrap()]);
    assert_eq!(
        output.status.code(),
        Some(110),
        "{}",
        common::output_text(&output)
    );
}

#[test]
fn build_and_run_top_level_for() {
    let dir = common::make_temp_dir("tl-for");
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
    let output = common::build_and_run(&src);
    assert_eq!(
        output.status.code(),
        Some(42),
        "{}",
        common::output_text(&output)
    );
}

#[test]
fn run_blocking_call_on_suspendable_callee_returns_plain_value() {
    let dir = common::make_temp_dir("blocking-run");
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

    let output = common::cli(&["run", src.to_str().unwrap()]);
    assert_eq!(
        output.status.code(),
        Some(42),
        "{}",
        common::output_text(&output)
    );
}

#[test]
fn run_wait_cancel_observes_cancelled_terminal_state() {
    let dir = common::make_temp_dir("wait-cancel-run");
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

    let output = common::cli(&["run", src.to_str().unwrap()]);
    assert_eq!(
        output.status.code(),
        Some(99),
        "{}",
        common::output_text(&output)
    );
}

#[test]
fn run_resolve_first_returns_fastest_value() {
    let dir = common::make_temp_dir("resolve-first-run");
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

    let output = common::cli(&["run", src.to_str().unwrap()]);
    assert_eq!(
        output.status.code(),
        Some(42),
        "{}",
        common::output_text(&output)
    );
}

// ===========================================================================
// Iterable vocabulary methods — end-to-end execution
// ===========================================================================

#[test]
fn run_iterable_count() {
    let dir = common::make_temp_dir("iter-count");
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
    let output = common::build_and_run(&src);
    assert_eq!(
        output.status.code(),
        Some(3),
        "{}",
        common::output_text(&output)
    );
}

#[test]
fn run_iterable_reduce() {
    let dir = common::make_temp_dir("iter-reduce");
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
    let output = common::build_and_run(&src);
    assert_eq!(
        output.status.code(),
        Some(10),
        "{}",
        common::output_text(&output)
    );
}

#[test]
fn run_iterable_map() {
    let dir = common::make_temp_dir("iter-map");
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
    let output = common::build_and_run(&src);
    assert_eq!(
        output.status.code(),
        Some(60),
        "{}",
        common::output_text(&output)
    );
}

#[test]
fn run_iterable_filter() {
    let dir = common::make_temp_dir("iter-filter");
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
    let output = common::build_and_run(&src);
    assert_eq!(
        output.status.code(),
        Some(2),
        "{}",
        common::output_text(&output)
    );
}

#[test]
fn run_iterable_any_true() {
    let dir = common::make_temp_dir("iter-any");
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
    let output = common::build_and_run(&src);
    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        common::output_text(&output)
    );
}

#[test]
fn run_iterable_all_true() {
    let dir = common::make_temp_dir("iter-all");
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
    let output = common::build_and_run(&src);
    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        common::output_text(&output)
    );
}

#[test]
fn run_iterable_first_last() {
    let dir = common::make_temp_dir("iter-first-last");
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
    let output = common::build_and_run(&src);
    assert_eq!(
        output.status.code(),
        Some(40),
        "{}",
        common::output_text(&output)
    );
}

#[test]
fn run_iterable_to_list() {
    let dir = common::make_temp_dir("iter-tolist");
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
    let output = common::build_and_run(&src);
    assert_eq!(
        output.status.code(),
        Some(3),
        "{}",
        common::output_text(&output)
    );
}

#[test]
fn run_iterable_sort() {
    let dir = common::make_temp_dir("iter-sort");
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
    let output = common::build_and_run(&src);
    // sorted = [1,2,3], first=1, last=3, 1*100+3 = 103
    assert_eq!(
        output.status.code(),
        Some(103),
        "{}",
        common::output_text(&output)
    );
}

#[test]
fn run_iterable_min_max() {
    let dir = common::make_temp_dir("iter-minmax");
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
    let output = common::build_and_run(&src);
    // min=1, max=9, 1*100+9 = 109
    assert_eq!(
        output.status.code(),
        Some(109),
        "{}",
        common::output_text(&output)
    );
}

#[test]
fn run_iterable_find() {
    let dir = common::make_temp_dir("iter-find");
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
    let output = common::build_and_run(&src);
    assert_eq!(
        output.status.code(),
        Some(20),
        "{}",
        common::output_text(&output)
    );
}

#[test]
fn run_int_min_div_neg_one_returns_zero() {
    let dir = common::make_temp_dir("min-div");
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
    let output = common::cli(&["run", src.to_str().unwrap()]);
    assert!(
        output.status.success(),
        "i64::MIN / -1 should not SIGFPE: {}",
        common::output_text(&output)
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "i64::MIN / -1 returns 0: {}",
        common::output_text(&output)
    );
}

#[test]
fn run_int_min_mod_neg_one_returns_zero() {
    let dir = common::make_temp_dir("min-mod");
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
    let output = common::cli(&["run", src.to_str().unwrap()]);
    assert!(
        output.status.success(),
        "i64::MIN %% -1 should not SIGFPE: {}",
        common::output_text(&output)
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "i64::MIN %% -1 returns 0: {}",
        common::output_text(&output)
    );
}

#[test]
fn run_normal_division_still_works() {
    let dir = common::make_temp_dir("normal-div");
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
    let output = common::cli(&["run", src.to_str().unwrap()]);
    // 100/3 = 33, 100%3 = 1, sum = 34
    assert_eq!(
        output.status.code(),
        Some(34),
        "{}",
        common::output_text(&output)
    );
}
