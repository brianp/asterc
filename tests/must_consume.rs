mod common;

// --- Phase 9: Must-Consume Task[T] Enforcement ---

// 9.1 — Consumed via resolve — no error

#[test]
fn task_consumed_via_resolve() {
    common::check_ok(
        "\
def work() -> Int
  42

def main() throws Error -> Int
  let t = async work()
  let result = resolve t!
  result
",
    );
}

// 9.2 — Unconsumed task — compile error E027

#[test]
fn unconsumed_task_is_error() {
    let diag = common::check_err_diagnostic(
        "\
def work() -> Int
  42

def main() throws Error -> Int
  let t = async work()
  0
",
    );
    assert_eq!(diag.code.as_deref(), Some("E027"));
}

#[test]
fn unconsumed_task_error_message() {
    let err = common::check_err(
        "\
def work() -> Int
  42

def main() throws Error -> Int
  let t = async work()
  0
",
    );
    assert!(
        err.contains("never consumed"),
        "expected 'never consumed' message, got: {err}"
    );
}

// 9.3 — Returned from function — no error (caller's responsibility)

#[test]
fn task_returned_from_function_is_consumed() {
    common::check_ok(
        "\
def work() -> Int
  42

def spawn_work() -> Task[Int]
  let t = async work()
  t
",
    );
}

// 9.4 — Detached async — no task to consume, no error

#[test]
fn detached_async_no_consume_needed() {
    common::check_ok(
        "\
def work() -> Int
  42

def main() -> Int
  detached async work()
  0
",
    );
}

// 9.5 — Task consumed via resolve in function body — no error

#[test]
fn task_consumed_in_function_body() {
    common::check_ok(
        "\
def work() -> Int
  42

def main() -> Int
  let t = async work()
  resolve t!
",
    );
}

// 9.6 — Multiple tasks, one unconsumed

#[test]
fn multiple_tasks_one_unconsumed() {
    let diag = common::check_err_diagnostic(
        "\
def work() -> Int
  42

def main() throws Error -> Int
  let a = async work()
  let b = async work()
  let ra = resolve a!
  ra
",
    );
    assert_eq!(diag.code.as_deref(), Some("E027"));
    assert!(
        diag.message.contains("'b'"),
        "expected error about 'b', got: {}",
        diag.message
    );
}

// 9.7 — Task consumed via error recovery (resolve t!.or(default))

#[test]
fn task_consumed_via_error_or() {
    common::check_ok(
        "\
def work() -> Int
  42

def main() -> Int
  let t = async work()
  let result = resolve t!.or(0)
  result
",
    );
}

// 9.8 — Task consumed via error catch

#[test]
fn task_consumed_via_error_catch() {
    common::check_ok(
        "\
def work() -> Int
  42

def main() -> Int
  let t = async work()
  let result = resolve t!.catch
    CancelledError e -> 0
  result
",
    );
}

// 9.9 — Task passed as argument to function (consumed)

#[test]
fn task_passed_as_argument_is_consumed() {
    common::check_ok(
        "\
def work() -> Int
  42

def wait_for(t: Task[Int]) throws Error -> Int
  resolve t!

def main() throws Error -> Int
  let t = async work()
  blocking wait_for(t: t)
",
    );
}

// 9.10 — Task consumed inside if branch

#[test]
fn task_consumed_inside_if_branch() {
    common::check_ok(
        "\
def work() -> Int
  42

def main(flag: Bool) throws Error -> Int
  let t = async work()
  if flag
    let r = resolve t!
    return r
  resolve t!
",
    );
}

// 9.11 — Task created inside if branch is still tracked

#[test]
fn task_created_inside_if_branch_unconsumed() {
    let diag = common::check_err_diagnostic(
        "\
def work() -> Int
  42

def main(flag: Bool) throws Error -> Int
  if flag
    let t = async work()
  0
",
    );
    assert_eq!(diag.code.as_deref(), Some("E027"));
}

// 9.12 — Task returned from inside if branch is consumed

#[test]
fn task_returned_from_if_branch() {
    common::check_ok(
        "\
def work() -> Int
  42

def spawn_maybe(flag: Bool) -> Task[Int]
  let t = async work()
  if flag
    return t
  t
",
    );
}

// 9.13 — Non-task let bindings are not affected

#[test]
fn non_task_let_not_affected() {
    common::check_ok(
        "\
def main() -> Int
  let x = 42
  x
",
    );
}
