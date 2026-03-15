mod common;

#[test]
fn build_executes_hello_example() {
    let output = common::build_and_run("examples/executable/hello.aster");
    assert!(output.status.success(), "{}", common::output_text(&output));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "Hello\nYes\n");
}

#[test]
fn build_async_scope_exit_cancels_unresolved_tasks() {
    let dir = common::make_temp_dir("async-scope-build");
    let src = dir.join("scope_cancel.aster");
    std::fs::write(
        &src,
        "\
def fast() -> Int
  0

def slow() -> Int
  let i: Int = 0
  let total: Int = 0
  while i < 20000000
    total = total + i
    i = i + 1
  42

def main() throws CancelledError -> Int
  let t: Task[Int] = async fast()
  async scope
    t = async slow()
  let blocker: Task[Int] = async slow()
  let waited = resolve blocker!
  resolve t!.catch
    _ -> 99
",
    )
    .unwrap();

    let output = common::build_and_run(&src);
    assert_eq!(
        output.status.code(),
        Some(99),
        "{}",
        common::output_text(&output)
    );
}

#[test]
fn build_blocking_call_on_suspendable_callee_returns_plain_value() {
    let dir = common::make_temp_dir("blocking-build");
    let src = dir.join("blocking_value.aster");
    std::fs::write(
        &src,
        "\
def child() -> Int
  41

def parent() throws CancelledError -> Int
  let t: Task[Int] = async child()
  resolve t! + 1

def main() throws CancelledError -> Int
  blocking parent()
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
fn build_wait_cancel_observes_cancelled_terminal_state() {
    let dir = common::make_temp_dir("wait-cancel-build");
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

def main() throws CancelledError -> Int
  let t: Task[Int] = async slow()
  t.wait_cancel()
  resolve t!.catch
    _ -> 99
",
    )
    .unwrap();

    let output = common::build_and_run(&src);
    assert_eq!(
        output.status.code(),
        Some(99),
        "{}",
        common::output_text(&output)
    );
}

#[test]
fn build_resolve_first_returns_fastest_value() {
    let dir = common::make_temp_dir("resolve-first-build");
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

def main() throws CancelledError -> Int
  let tasks: List[Task[Int]] = [async slow(), async fast()]
  resolve_first(tasks: tasks)!
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
