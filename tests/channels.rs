mod common;

// ─── Channel[T] type-checking ───────────────────────────────────────

// Channel constructor

#[test]
fn channel_constructor_with_capacity() {
    common::check_ok(
        "\
def main() -> Int
  let ch: Channel[Int] = Channel(capacity: 10)
  0
",
    );
}

#[test]
fn channel_constructor_default() {
    common::check_ok(
        "\
def main() -> Int
  let ch: Channel[Int] = Channel()
  0
",
    );
}

// Send methods

#[test]
fn channel_send() {
    common::check_ok(
        "\
def main() -> Int
  let ch: Channel[Int] = Channel(capacity: 10)
  ch.send(value: 42)
  0
",
    );
}

#[test]
fn channel_wait_send() {
    common::check_ok(
        "\
def main() -> Int
  let ch: Channel[Int] = Channel(capacity: 10)
  blocking ch.wait_send(value: 42)
  0
",
    );
}

// Receive methods

#[test]
fn channel_receive_nullable() {
    common::check_ok(
        "\
def main() -> Int
  let ch: Channel[Int] = Channel(capacity: 10)
  ch.send(value: 42)
  let v: Int? = ch.receive()
  0
",
    );
}

#[test]
fn channel_wait_receive() {
    common::check_ok(
        "\
def main() -> Int
  let ch: Channel[Int] = Channel(capacity: 10)
  ch.send(value: 42)
  let v = blocking ch.wait_receive()
  v
",
    );
}

// Close

#[test]
fn channel_close() {
    common::check_ok(
        "\
def main() -> Int
  let ch: Channel[Int] = Channel(capacity: 10)
  ch.close()
  0
",
    );
}

// Type errors

#[test]
fn channel_unknown_method_error() {
    let err = common::check_err(
        "\
def main() -> Int
  let ch: Channel[Int] = Channel(capacity: 10)
  ch.foo()
  0
",
    );
    assert!(
        err.contains("no method") || err.contains("Unknown field") || err.contains("foo"),
        "expected method error, got: {err}"
    );
}

// try_send throws

#[test]
fn channel_try_send_throws() {
    common::check_ok(
        "\
def main() throws ChannelFullError -> Int
  let ch: Channel[Int] = Channel(capacity: 1)
  ch.try_send(value: 42)!
  0
",
    );
}

// try_receive throws

#[test]
fn channel_try_receive_throws() {
    common::check_ok(
        "\
def main() throws ChannelEmptyError -> Int
  let ch: Channel[Int] = Channel(capacity: 10)
  let v = ch.try_receive()!
  v
",
    );
}
