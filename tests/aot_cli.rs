mod common;

// --- Phase 4: AOT green thread tests ---

#[test]
fn build_green_spawn_and_resolve() {
    let dir = common::make_temp_dir("green-spawn-build");
    let src = dir.join("green_spawn.aster");
    std::fs::write(
        &src,
        "\
def work(n: Int) -> Int
  n * 2

def main() throws CancelledError -> Int
  let t: Task[Int] = async work(n: 21)
  resolve t!
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
fn build_green_many_tasks() {
    let dir = common::make_temp_dir("green-many-build");
    let src = dir.join("green_many.aster");
    std::fs::write(
        &src,
        "\
def double(n: Int) -> Int
  n * 2

def main() throws CancelledError -> Int
  let a: Task[Int] = async double(n: 1)
  let b: Task[Int] = async double(n: 2)
  let c: Task[Int] = async double(n: 3)
  let d: Task[Int] = async double(n: 4)
  let e: Task[Int] = async double(n: 5)
  let ra = resolve a!
  let rb = resolve b!
  let rc = resolve c!
  let rd = resolve d!
  let re = resolve e!
  ra + rb + rc + rd + re
",
    )
    .unwrap();

    let output = common::build_and_run(&src);
    assert_eq!(
        output.status.code(),
        Some(30),
        "{}",
        common::output_text(&output)
    );
}

#[test]
fn build_green_cancellation_at_safepoint() {
    let dir = common::make_temp_dir("green-cancel-build");
    let src = dir.join("green_cancel.aster");
    std::fs::write(
        &src,
        "\
def busy() -> Int
  let i: Int = 0
  while i < 50000000
    i = i + 1
  i

def main() throws CancelledError -> Int
  let t: Task[Int] = async busy()
  t.wait_cancel()
  resolve t!.catch
    _ -> 77
",
    )
    .unwrap();

    let output = common::build_and_run(&src);
    assert_eq!(
        output.status.code(),
        Some(77),
        "{}",
        common::output_text(&output)
    );
}

#[test]
fn build_green_resolve_all() {
    let dir = common::make_temp_dir("green-resolve-all-build");
    let src = dir.join("resolve_all.aster");
    std::fs::write(
        &src,
        "\
def double(n: Int) -> Int
  n * 2

def main() throws CancelledError -> Int
  let tasks: List[Task[Int]] = [async double(n: 10), async double(n: 11), async double(n: 12)]
  let results = resolve_all(tasks: tasks)!
  results[0] + results[1] + results[2]
",
    )
    .unwrap();

    let output = common::build_and_run(&src);
    assert_eq!(
        output.status.code(),
        Some(66),
        "{}",
        common::output_text(&output)
    );
}

#[test]
fn build_jit_and_aot_produce_same_output() {
    let dir = common::make_temp_dir("jit-aot-parity");
    let src = dir.join("parity.aster");
    std::fs::write(
        &src,
        "\
def work(n: Int) -> Int
  n * n + 1

def main() throws CancelledError -> Int
  let a: Task[Int] = async work(n: 5)
  let b: Task[Int] = async work(n: 3)
  let ra = resolve a!
  let rb = resolve b!
  ra + rb
",
    )
    .unwrap();

    // JIT
    let jit_output = common::cli(&["run", &src.to_string_lossy()]);

    // AOT
    let aot_output = common::build_and_run(&src);

    assert_eq!(
        jit_output.status.code(),
        aot_output.status.code(),
        "JIT exit={:?} AOT exit={:?}\nJIT: {}\nAOT: {}",
        jit_output.status.code(),
        aot_output.status.code(),
        common::output_text(&jit_output),
        common::output_text(&aot_output)
    );
}

// --- Original tests ---

#[test]
fn build_executes_hello_example() {
    let output = common::build_and_run("examples/executable/hello.aster");
    assert!(output.status.success(), "{}", common::output_text(&output));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "Hello\nYes\n");
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
