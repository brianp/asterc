// ─── Channel[T] type-checking ───────────────────────────────────────

// Channel constructor

#[test]
fn channel_constructor_with_capacity() {
    crate::common::check_ok(
        "\
def main() -> Int
  let ch: Channel[Int] = Channel(capacity: 10)
  0
",
    );
}

#[test]
fn channel_constructor_default() {
    crate::common::check_ok(
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
    crate::common::check_ok(
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
    crate::common::check_ok(
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
    crate::common::check_ok(
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
    crate::common::check_ok(
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
    crate::common::check_ok(
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
    let err = crate::common::check_err(
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
    crate::common::check_ok(
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
    crate::common::check_ok(
        "\
def main() throws ChannelEmptyError -> Int
  let ch: Channel[Int] = Channel(capacity: 10)
  let v = ch.try_receive()!
  v
",
    );
}
