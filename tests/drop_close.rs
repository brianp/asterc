mod common;

// ─── Drop and Close trait registration ──────────────────────────────

// Drop and Close are registered in the virtual stdlib

#[test]
fn class_includes_drop_with_method() {
    common::check_ok(
        "\
class Resource includes Drop
  value: Int

  def drop()
    say(message: value)
",
    );
}

#[test]
fn class_includes_close_with_method() {
    common::check_ok(
        "\
class Handle includes Close
  value: Int

  def close() throws Error
    say(message: value)
",
    );
}

#[test]
fn class_includes_drop_and_close() {
    common::check_ok(
        "\
class Connection includes Drop, Close
  fd: Int

  def drop()
    say(message: fd)

  def close() throws Error
    say(message: fd)
",
    );
}

// Parser + typecheck validation

#[test]
fn drop_missing_method_is_error() {
    let err = common::check_err(
        "\
class BadDrop includes Drop
  value: Int
",
    );
    assert!(
        err.contains("must implement method 'drop'"),
        "expected drop method error, got: {err}"
    );
}

#[test]
fn close_missing_method_is_error() {
    let err = common::check_err(
        "\
class BadClose includes Close
  value: Int
",
    );
    assert!(
        err.contains("must implement method 'close'"),
        "expected close method error, got: {err}"
    );
}

// Codegen: drop() called on scope exit

#[test]
fn drop_called_on_scope_exit() {
    let dir = common::make_temp_dir("drop-scope-exit");
    let src = dir.join("drop_test.aster");
    std::fs::write(
        &src,
        "\
class Resource includes Drop
  value: Int

  def drop()
    say(message: 99)

def main() -> Int
  let r = Resource(value: 42)
  0
",
    )
    .unwrap();

    let output = common::cli(&["run", &src.to_string_lossy()]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("99"),
        "drop() should have printed 99, got: {stdout}"
    );
}

#[test]
fn drop_reverse_order() {
    let dir = common::make_temp_dir("drop-reverse-order");
    let src = dir.join("drop_order.aster");
    std::fs::write(
        &src,
        "\
class Resource includes Drop
  value: Int

  def drop()
    say(message: value)

def main() -> Int
  let a = Resource(value: 1)
  let b = Resource(value: 2)
  0
",
    )
    .unwrap();

    let output = common::cli(&["run", &src.to_string_lossy()]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(
        lines.len() >= 2 && lines[0] == "2" && lines[1] == "1",
        "expected '2\\n1\\n', got: {stdout}"
    );
}

#[test]
fn drop_called_on_explicit_return() {
    let dir = common::make_temp_dir("drop-explicit-return");
    let src = dir.join("drop_return.aster");
    std::fs::write(
        &src,
        "\
class Resource includes Drop
  value: Int

  def drop()
    say(message: value)

def work() -> Int
  let r = Resource(value: 77)
  return 0

def main() -> Int
  work()
",
    )
    .unwrap();

    let output = common::cli(&["run", &src.to_string_lossy()]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("77"),
        "drop() should have been called on explicit return, got: {stdout}"
    );
}

// Cleanup on break inside loop

#[test]
fn drop_called_on_break() {
    let dir = common::make_temp_dir("drop-break");
    let src = dir.join("drop_break.aster");
    std::fs::write(
        &src,
        "\
class Resource includes Drop
  value: Int

  def drop()
    say(message: value)

def main() -> Int
  while true
    let r = Resource(value: 55)
    break
  0
",
    )
    .unwrap();

    let output = common::cli(&["run", &src.to_string_lossy()]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("55"),
        "drop() should have been called on break, got: {stdout}"
    );
}

#[test]
fn drop_called_on_continue() {
    let dir = common::make_temp_dir("drop-continue");
    let src = dir.join("drop_continue.aster");
    std::fs::write(
        &src,
        "\
class Resource includes Drop
  value: Int

  def drop()
    say(message: value)

def main() -> Int
  let count = 0
  while count < 3
    let r = Resource(value: count)
    count = count + 1
    continue
  0
",
    )
    .unwrap();

    let output = common::cli(&["run", &src.to_string_lossy()]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(
        lines.len() >= 3,
        "drop() should have been called 3 times, got: {stdout}"
    );
}

#[test]
fn drop_called_on_void_function_end() {
    let dir = common::make_temp_dir("drop-void-end");
    let src = dir.join("drop_void.aster");
    std::fs::write(
        &src,
        "\
class Resource includes Drop
  value: Int

  def drop()
    say(message: value)

def work()
  let r = Resource(value: 33)

def main() -> Int
  work()
  0
",
    )
    .unwrap();

    let output = common::cli(&["run", &src.to_string_lossy()]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("33"),
        "drop() should have been called at void function end, got: {stdout}"
    );
}

#[test]
fn drop_on_break_only_cleans_loop_locals() {
    let dir = common::make_temp_dir("drop-break-scope");
    let src = dir.join("drop_break_scope.aster");
    std::fs::write(
        &src,
        "\
class Resource includes Drop
  value: Int

  def drop()
    say(message: value)

def main() -> Int
  let outer = Resource(value: 1)
  while true
    let inner = Resource(value: 2)
    break
  0
",
    )
    .unwrap();

    let output = common::cli(&["run", &src.to_string_lossy()]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    // inner (2) should be dropped at break, outer (1) at function end
    // Order: 2 then 1
    assert!(
        lines.len() >= 2 && lines[0] == "2" && lines[1] == "1",
        "expected inner drop then outer drop: '2\\n1', got: {stdout}"
    );
}
