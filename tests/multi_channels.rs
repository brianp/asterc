mod common;

// --- MultiSend / MultiReceive ---

// Type-checking tests

#[test]
fn multi_send_constructor() {
    common::check_ok(
        "\
def main() -> Int
  let ch: MultiSend[Int] = MultiSend(capacity: 10)
  0
",
    );
}

#[test]
fn multi_receive_constructor() {
    common::check_ok(
        "\
def main() -> Int
  let ch: MultiReceive[Int] = MultiReceive(capacity: 10)
  0
",
    );
}

#[test]
fn multi_send_methods() {
    common::check_ok(
        "\
def main() -> Int
  let ch: MultiSend[Int] = MultiSend(capacity: 10)
  ch.send(value: 42)
  ch.close()
  0
",
    );
}

#[test]
fn multi_send_clone_sender() {
    common::check_ok(
        "\
def main() -> Int
  let ch: MultiSend[Int] = MultiSend(capacity: 10)
  let ch2: MultiSend[Int] = ch.clone_sender()
  ch2.send(value: 99)
  ch.close()
  0
",
    );
}

#[test]
fn multi_receive_clone_receiver() {
    common::check_ok(
        "\
def main() -> Int
  let ch: MultiReceive[Int] = MultiReceive(capacity: 10)
  let ch2: MultiReceive[Int] = ch.clone_receiver()
  ch.close()
  0
",
    );
}

#[test]
fn multi_send_cannot_receive() {
    let err = common::check_err(
        "\
def main() -> Int
  let ch: MultiSend[Int] = MultiSend(capacity: 10)
  ch.receive()
  0
",
    );
    assert!(
        err.contains("no method") || err.contains("Unknown field") || err.contains("receive") || err.contains("send") || err.contains("foo"),
        "expected no method error for receive on MultiSend, got: {err}"
    );
}

#[test]
fn multi_receive_cannot_send() {
    let err = common::check_err(
        "\
def main() -> Int
  let ch: MultiReceive[Int] = MultiReceive(capacity: 10)
  ch.send(value: 42)
  0
",
    );
    assert!(
        err.contains("no method") || err.contains("Unknown field") || err.contains("receive") || err.contains("send") || err.contains("foo"),
        "expected no method error for send on MultiReceive, got: {err}"
    );
}

#[test]
fn multi_send_unknown_method() {
    let err = common::check_err(
        "\
def main() -> Int
  let ch: MultiSend[Int] = MultiSend(capacity: 10)
  ch.foo()
  0
",
    );
    assert!(
        err.contains("no method") || err.contains("Unknown field") || err.contains("receive") || err.contains("send") || err.contains("foo"),
        "expected method error, got: {err}"
    );
}
