mod common;

// --- Phase 7: Mutex[T] ---

// 7.2 — Mutex constructor

#[test]
fn mutex_constructor() {
    common::check_ok(
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
    common::check_ok(
        "\
def take_mutex(m: Mutex[Int]) -> Int
  0

def main() -> Int
  let m = Mutex(value: 42)
  take_mutex(m: m)
",
    );
}

// 7.2 — lock method not yet supported (needs escape analysis)

#[test]
fn mutex_lock_method_not_supported() {
    let err = common::check_err(
        "\
def main() -> Int
  let m = Mutex(value: 42)
  m.lock()
  0
",
    );
    assert!(
        err.contains("no method"),
        "expected method error, got: {err}"
    );
}

// 7.5 — Manual acquire/release

#[test]
fn mutex_acquire_release() {
    common::check_ok(
        "\
def main() throws Error -> Int
  let m = Mutex(value: 42)
  let value = blocking m.acquire()
  m.release(value: value + 1)
  0
",
    );
}

// 7.2 — Method type errors

#[test]
fn mutex_unknown_method_error() {
    let err = common::check_err(
        "\
def main() -> Int
  let m = Mutex(value: 42)
  m.foo()
  0
",
    );
    assert!(
        err.contains("no method"),
        "expected method error, got: {err}"
    );
}

// 7.1 — Constructor arg count

#[test]
fn mutex_constructor_no_args_error() {
    let err = common::check_err(
        "\
def main() -> Int
  let m = Mutex()
  0
",
    );
    assert!(
        err.contains("1 argument"),
        "expected arg count error, got: {err}"
    );
}
