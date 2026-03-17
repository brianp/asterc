mod common;

// --- I/O Primitives ---

// File.read / File.write

#[test]
fn file_read_type_checks() {
    common::check_ok(
        "\
def main() throws IOError -> Int
  let content = File.read(path: \"test.txt\")!
  0
",
    );
}

#[test]
fn file_write_type_checks() {
    common::check_ok(
        "\
def main() throws IOError -> Int
  File.write(path: \"test.txt\", content: \"hello\")!
  0
",
    );
}

#[test]
fn file_append_type_checks() {
    common::check_ok(
        "\
def main() throws IOError -> Int
  File.append(path: \"test.txt\", content: \"world\")!
  0
",
    );
}

#[test]
fn file_read_returns_string() {
    // File.read returns String, should be assignable to String variable
    common::check_ok(
        "\
def main() throws IOError -> Int
  let content: String = File.read(path: \"test.txt\")!
  0
",
    );
}

#[test]
fn file_unknown_method_error() {
    let err = common::check_err(
        "\
def main() -> Int
  File.foo()
  0
",
    );
    assert!(
        err.contains("no method") || err.contains("no member"),
        "expected method error, got: {err}"
    );
}

// Codegen: File.write + File.read round-trip

#[test]
fn file_write_read_round_trip() {
    let dir = common::make_temp_dir("io-file-roundtrip");
    let test_file = dir.join("data.txt");
    let src = dir.join("io_test.aster");
    std::fs::write(
        &src,
        format!(
            "\
def main() throws IOError -> Int
  File.write(path: \"{}\", content: \"hello world\")!
  let content = File.read(path: \"{}\")!
  print(value: content)
  0
",
            test_file.to_string_lossy().replace('\\', "\\\\"),
            test_file.to_string_lossy().replace('\\', "\\\\"),
        ),
    )
    .unwrap();

    let output = common::cli(&["run", &src.to_string_lossy()]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("hello world"),
        "file round-trip should produce 'hello world', got: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn file_append_adds_to_file() {
    let dir = common::make_temp_dir("io-file-append");
    let test_file = dir.join("append.txt");
    let src = dir.join("append_test.aster");
    std::fs::write(
        &src,
        format!(
            "\
def main() throws IOError -> Int
  File.write(path: \"{}\", content: \"hello\")!
  File.append(path: \"{}\", content: \" world\")!
  let content = File.read(path: \"{}\")!
  print(value: content)
  0
",
            test_file.to_string_lossy().replace('\\', "\\\\"),
            test_file.to_string_lossy().replace('\\', "\\\\"),
            test_file.to_string_lossy().replace('\\', "\\\\"),
        ),
    )
    .unwrap();

    let output = common::cli(&["run", &src.to_string_lossy()]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("hello world"),
        "append should produce 'hello world', got: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// TCP tests would go here but are deferred to avoid
// long-running network tests in the test suite.
