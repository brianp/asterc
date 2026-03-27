// ─── Mutex[T] type-checking ─────────────────────────────────────────

// Mutex constructor

#[test]
fn mutex_constructor() {
    crate::common::check_ok(
        "\
def main() -> Int
  let m = Mutex(value: 42)
  0
",
    );
}

#[test]
fn mutex_type_is_mutex_t() {
    // Mutex(value: Int) should produce Mutex[Int]
    crate::common::check_ok(
        "\
def take_mutex(m: Mutex[Int]) -> Int
  0

def main() -> Int
  let m = Mutex(value: 42)
  take_mutex(m: m)
",
    );
}

// Scoped lock

#[test]
fn mutex_lock_basic() {
    crate::common::check_ok(
        "\
def main() -> Int
  let m = Mutex(value: 42)
  blocking m.lock(block: -> v : say(message: v))
  0
",
    );
}

#[test]
fn mutex_lock_requires_block_arg() {
    let err = crate::common::check_err(
        "\
def main() -> Int
  let m = Mutex(value: 42)
  m.lock()
  0
",
    );
    assert!(
        err.contains("1 argument") || err.contains("block"),
        "expected argument error, got: {err}"
    );
}

#[test]
fn mutex_lock_block_param_type_matches_inner() {
    // Lambda param should be typed as Int (Mutex[Int] → lock block gets Int)
    crate::common::check_ok(
        "\
def use_int(n: Int) -> Int
  n

def main() -> Int
  let m = Mutex(value: 10)
  blocking m.lock(block: -> v : use_int(n: v))
  0
",
    );
}

// Escape analysis is enforced by the inline lambda being expression-only.
// The lock block's lambda parameter cannot be assigned to outer scope
// because inline lambdas (-> v : expr) can only contain expressions,
// not assignment statements. This is a structural guarantee.

#[test]
fn mutex_lock_codegen() {
    let dir = crate::common::make_temp_dir("mutex-lock-codegen");
    let src = dir.join("mutex_lock.aster");
    std::fs::write(
        &src,
        "\
def main() -> Int
  let m = Mutex(value: 21)
  blocking m.lock(block: -> v : say(message: v * 2))
  0
",
    )
    .unwrap();

    let output = crate::common::cli(&["run", &src.to_string_lossy()]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("42"),
        "lock block should have printed 42, got: {stdout}"
    );
}

// Manual acquire/release

#[test]
fn mutex_acquire_release() {
    crate::common::check_ok(
        "\
def main() throws Error -> Int
  let m = Mutex(value: 42)
  let value = blocking m.acquire()
  m.release(value: value + 1)
  0
",
    );
}

// Method type errors

#[test]
fn mutex_unknown_method_error() {
    let err = crate::common::check_err(
        "\
def main() -> Int
  let m = Mutex(value: 42)
  m.foo()
  0
",
    );
    assert!(
        err.contains("no method") || err.contains("Unknown field") || err.contains("foo"),
        "expected method error, got: {err}"
    );
}

// Constructor arg count

#[test]
fn mutex_constructor_no_args_error() {
    let err = crate::common::check_err(
        "\
def main() -> Int
  let m = Mutex()
  0
",
    );
    assert!(
        err.contains("1 argument") || err.contains("expected 1") || err.contains("parameter count"),
        "expected arg count error, got: {err}"
    );
}
