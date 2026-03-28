// --- I/O Primitives ---

// File.read / File.write

#[test]
fn file_read_type_checks() {
    crate::common::check_ok(
        "\
def main() throws IOError -> Int
  let content = File.read(path: \"test.txt\")!
  0
",
    );
}

#[test]
fn file_write_type_checks() {
    crate::common::check_ok(
        "\
def main() throws IOError -> Int
  File.write(path: \"test.txt\", content: \"hello\")!
  0
",
    );
}

#[test]
fn file_append_type_checks() {
    crate::common::check_ok(
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
    crate::common::check_ok(
        "\
def main() throws IOError -> Int
  let content: String = File.read(path: \"test.txt\")!
  0
",
    );
}

#[test]
fn file_unknown_method_error() {
    let err = crate::common::check_err(
        "\
def main() -> Int
  File.foo()
  0
",
    );
    assert!(
        err.contains("no method")
            || err.contains("no member")
            || err.contains("Unknown field")
            || err.contains("foo"),
        "expected method error, got: {err}"
    );
}

// Codegen: File.write + File.read round-trip

#[test]
fn file_write_read_round_trip() {
    let dir = crate::common::make_temp_dir("io-file-roundtrip");
    let test_file = dir.join("data.txt");
    let src = dir.join("io_test.aster");
    std::fs::write(
        &src,
        format!(
            "\
def main() throws IOError -> Int
  File.write(path: \"{}\", content: \"hello world\")!
  let content = File.read(path: \"{}\")!
  say(message: content)
  0
",
            test_file.to_string_lossy().replace('\\', "\\\\"),
            test_file.to_string_lossy().replace('\\', "\\\\"),
        ),
    )
    .unwrap();

    let output = crate::common::cli(&["run", &src.to_string_lossy()]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("hello world"),
        "file round-trip should produce 'hello world', got: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn file_append_adds_to_file() {
    let dir = crate::common::make_temp_dir("io-file-append");
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
  say(message: content)
  0
",
            test_file.to_string_lossy().replace('\\', "\\\\"),
            test_file.to_string_lossy().replace('\\', "\\\\"),
            test_file.to_string_lossy().replace('\\', "\\\\"),
        ),
    )
    .unwrap();

    let output = crate::common::cli(&["run", &src.to_string_lossy()]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("hello world"),
        "append should produce 'hello world', got: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// TCP tests would go here but are deferred to avoid
// long-running network tests in the test suite.

// --- std/sys runtime ---

#[test]
fn std_sys_args_runtime() {
    let dir = crate::common::make_temp_dir("sys-args");
    let src = dir.join("test.aster");
    std::fs::write(
        &src,
        "\
use std/sys { args }

def main() -> Int
  let a = args()
  say(message: \"ok\")
  0
",
    )
    .unwrap();
    let output = crate::common::cli(&["run", &src.to_string_lossy()]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("ok"),
        "args() should work at runtime, got: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn std_sys_env_runtime() {
    let dir = crate::common::make_temp_dir("sys-env");
    let src = dir.join("test.aster");
    std::fs::write(
        &src,
        r#"use std/sys { env, set_env }

def main() -> Int
  set_env(key: "ASTER_TEST_VAR", value: "hello")
  let val = env(key: "ASTER_TEST_VAR")
  say(message: "done")
  0
"#,
    )
    .unwrap();
    let output = crate::common::cli(&["run", &src.to_string_lossy()]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("done"),
        "env/set_env should work at runtime, got: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// --- std/fs runtime ---

#[test]
fn std_fs_exists_runtime() {
    let dir = crate::common::make_temp_dir("fs-exists");
    let src = dir.join("test.aster");
    std::fs::write(
        &src,
        format!(
            "\
use std/fs {{ exists }}

def main() -> Int
  let e = exists(path: \"{}\")
  if e
    say(message: \"true\")
  0
",
            dir.to_string_lossy().replace('\\', "\\\\")
        ),
    )
    .unwrap();
    let output = crate::common::cli(&["run", &src.to_string_lossy()]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("true"),
        "exists() should return true for existing dir, got: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn std_fs_write_read_round_trip() {
    let dir = crate::common::make_temp_dir("fs-roundtrip");
    let test_file = dir.join("data.txt");
    let src = dir.join("test.aster");
    std::fs::write(
        &src,
        format!(
            "\
use std/fs {{ write_file, read_file }}

def main() throws IOError -> Int
  write_file(path: \"{}\", content: \"hello from std/fs\")!
  let content = read_file(path: \"{}\")!
  say(message: content)
  0
",
            test_file.to_string_lossy().replace('\\', "\\\\"),
            test_file.to_string_lossy().replace('\\', "\\\\"),
        ),
    )
    .unwrap();
    let output = crate::common::cli(&["run", &src.to_string_lossy()]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("hello from std/fs"),
        "write/read round-trip should work, got: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn std_fs_mkdir_list_dir_remove_runtime() {
    let dir = crate::common::make_temp_dir("fs-mkdir");
    let subdir = dir.join("testdir");
    let src = dir.join("test.aster");
    std::fs::write(
        &src,
        format!(
            "\
use std/fs {{ mkdir, list_dir, remove, exists }}

def main() throws IOError -> Int
  mkdir(path: \"{}\")!
  let e = exists(path: \"{}\")
  if e
    say(message: \"true\")
  remove(path: \"{}\")!
  let e2 = exists(path: \"{}\")
  if e2
    say(message: \"true\")
  else
    say(message: \"false\")
  0
",
            subdir.to_string_lossy().replace('\\', "\\\\"),
            subdir.to_string_lossy().replace('\\', "\\\\"),
            subdir.to_string_lossy().replace('\\', "\\\\"),
            subdir.to_string_lossy().replace('\\', "\\\\"),
        ),
    )
    .unwrap();
    let output = crate::common::cli(&["run", &src.to_string_lossy()]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("true") && stdout.contains("false"),
        "mkdir/remove should create then delete dir, got: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// --- std/process runtime ---

#[test]
fn std_process_run_echo_runtime() {
    let dir = crate::common::make_temp_dir("process-run");
    let src = dir.join("test.aster");
    std::fs::write(
        &src,
        "\
use std/process { run }

def main() throws ProcessError -> Int
  let result = run(cmd: \"echo\", args: [\"hello\"])!
  say(message: result.stdout)
  0
",
    )
    .unwrap();
    let output = crate::common::cli(&["run", &src.to_string_lossy()]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("hello"),
        "run('echo', ['hello']) should produce 'hello', got: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// --- std/crypto runtime ---

#[test]
fn std_crypto_sha256_runtime() {
    let dir = crate::common::make_temp_dir("crypto-sha256");
    let src = dir.join("test.aster");
    std::fs::write(
        &src,
        "\
use std/crypto { sha256 }

def main() -> Int
  let hash = sha256(data: \"hello\")
  say(message: hash)
  0
",
    )
    .unwrap();
    let output = crate::common::cli(&["run", &src.to_string_lossy()]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"),
        "sha256('hello') should return known digest, got: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// --- AOT parity ---

#[test]
fn std_crypto_sha256_aot() {
    let dir = crate::common::make_temp_dir("crypto-sha256-aot");
    let src = dir.join("test.aster");
    std::fs::write(
        &src,
        "\
use std/crypto { sha256 }

def main() -> Int
  let hash = sha256(data: \"hello\")
  say(message: hash)
  0
",
    )
    .unwrap();
    let output = crate::common::build_and_run(&src);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"),
        "AOT sha256('hello') should return known digest, got: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn std_fs_exists_aot() {
    let dir = crate::common::make_temp_dir("fs-exists-aot");
    let src = dir.join("test.aster");
    std::fs::write(
        &src,
        format!(
            "\
use std/fs {{ exists }}

def main() -> Int
  let e = exists(path: \"{}\")
  if e
    say(message: \"true\")
  0
",
            dir.to_string_lossy().replace('\\', "\\\\")
        ),
    )
    .unwrap();
    let output = crate::common::build_and_run(&src);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("true"),
        "AOT exists() should return true, got: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
