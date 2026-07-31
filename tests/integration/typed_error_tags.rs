// ─── Typed runtime error tags ──────────────────────────────────────
//
// Runtime builtins that declare `throws T` must set a T-typed error on
// failure so a typed `.catch(T e)` arm dispatches (instead of falling
// through to the wildcard or re-raising), and `e.message` / `e.command`
// must carry the real underlying failure text.
//
// Every program runs through both JIT (`aster run`) and AOT (`aster
// build` + execute) so the two backends stay in lockstep.

use std::path::Path;

struct RunResult {
    jit_stdout: String,
    jit_exit: i32,
    aot_stdout: String,
    aot_exit: i32,
}

/// Run an Aster program through JIT and AOT, returning both stdout+stderr
/// blobs and exit codes.
fn run_both(name: &str, source: &str) -> RunResult {
    let dir = crate::common::make_temp_dir(name);
    let src = dir.join(format!("{name}.aster"));
    std::fs::write(&src, source).unwrap();

    let jit = crate::common::cli(&["run", src.to_str().unwrap()]);
    let jit_stdout = crate::common::output_text(&jit);
    let jit_exit = jit.status.code().unwrap_or(-1);

    let aot = crate::common::build_and_run(&src);
    let aot_stdout = crate::common::output_text(&aot);
    let aot_exit = aot.status.code().unwrap_or(-1);

    RunResult {
        jit_stdout,
        jit_exit,
        aot_stdout,
        aot_exit,
    }
}

/// Assert both backends produced stdout containing every `needle`.
fn assert_both_contain(r: &RunResult, needles: &[&str]) {
    for needle in needles {
        assert!(
            r.jit_stdout.contains(needle),
            "JIT stdout missing {needle:?}:\n{}",
            r.jit_stdout
        );
        assert!(
            r.aot_stdout.contains(needle),
            "AOT stdout missing {needle:?}:\n{}",
            r.aot_stdout
        );
    }
}

/// Assert neither backend produced `needle` in stdout.
fn assert_neither_contains(r: &RunResult, needle: &str) {
    assert!(
        !r.jit_stdout.contains(needle),
        "JIT stdout unexpectedly contains {needle:?}:\n{}",
        r.jit_stdout
    );
    assert!(
        !r.aot_stdout.contains(needle),
        "AOT stdout unexpectedly contains {needle:?}:\n{}",
        r.aot_stdout
    );
}

fn assert_both_exit(r: &RunResult, code: i32) {
    assert_eq!(
        r.jit_exit, code,
        "JIT exit {} != {code}:\n{}",
        r.jit_exit, r.jit_stdout
    );
    assert_eq!(
        r.aot_exit, code,
        "AOT exit {} != {code}:\n{}",
        r.aot_exit, r.aot_stdout
    );
}

fn esc(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "\\\\")
}

// ─── IOError: File.read on a missing path ───────────────────────────

// Criteria 1 + 2: the IOError arm runs (not the wildcard, no re-raise),
// and e.message carries the real std::io text.
#[test]
fn io_read_missing_path_dispatches_ioerror_arm_with_real_message() {
    let r = run_both(
        "typed-io-read",
        "\
def main() -> Int
  let s = File.read(path: \"/no/such/aster/typed/missing.txt\")!.catch
    IOError e -> \"IO-OK:\" + e.message
    _ -> \"IO-WILD\"
  say(message: s)
  0
",
    );
    assert_both_exit(&r, 0);
    assert_both_contain(&r, &["IO-OK:", "No such file or directory"]);
    assert_neither_contains(&r, "IO-WILD");
}

// ─── IOError: File.write / File.append into a missing directory ─────

// Criterion 3: write failure is caught with a populated message.
#[test]
fn io_write_into_missing_directory_caught_by_ioerror() {
    let r = run_both(
        "typed-io-write",
        "\
def main() -> Int
  File.write(path: \"/no/such/aster/typed/dir/out.txt\", content: \"x\")!.catch
    IOError e -> say(message: \"WRITE-OK:\" + e.message)
    _ -> say(message: \"WRITE-WILD\")
  0
",
    );
    assert_both_exit(&r, 0);
    assert_both_contain(&r, &["WRITE-OK:", "No such file or directory"]);
    assert_neither_contains(&r, "WRITE-WILD");
}

// Criterion 3: append failure is caught with a populated message.
#[test]
fn io_append_into_missing_directory_caught_by_ioerror() {
    let r = run_both(
        "typed-io-append",
        "\
def main() -> Int
  File.append(path: \"/no/such/aster/typed/dir/out.txt\", content: \"x\")!.catch
    IOError e -> say(message: \"APPEND-OK:\" + e.message)
    _ -> say(message: \"APPEND-WILD\")
  0
",
    );
    assert_both_exit(&r, 0);
    assert_both_contain(&r, &["APPEND-OK:", "No such file or directory"]);
    assert_neither_contains(&r, "APPEND-WILD");
}

// ─── IOError: the std/fs family (mkdir, remove, copy, rename, list_dir) ─

// Criterion 4: every fs builtin that declares `throws IOError` sets a
// typed IOError matched by `.catch(IOError e)`.
#[test]
fn fs_builtins_set_io_error_on_failure() {
    let dir = crate::common::make_temp_dir("typed-fs-family");
    // A regular file used as a bogus parent so mkdir(create_dir_all) fails.
    let a_file = dir.join("a_file");
    std::fs::write(&a_file, "x").unwrap();
    let missing = dir.join("missing");
    let dst = dir.join("dst");

    let source = format!(
        "\
use std/fs {{ mkdir, remove, copy, rename, list_dir }}

def keep(xs: List[String]) -> List[String]
  say(message: \"LIST-OK\")
  xs

def keep_bad(xs: List[String]) -> List[String]
  say(message: \"LIST-WILD\")
  xs

def main() -> Int
  mkdir(path: \"{afile}/child\")!.catch
    IOError e -> say(message: \"MKDIR-OK\")
    _ -> say(message: \"MKDIR-WILD\")
  remove(path: \"{missing}\")!.catch
    IOError e -> say(message: \"REMOVE-OK\")
    _ -> say(message: \"REMOVE-WILD\")
  copy(src: \"{missing}\", dst: \"{dst}\")!.catch
    IOError e -> say(message: \"COPY-OK\")
    _ -> say(message: \"COPY-WILD\")
  rename(src: \"{missing}\", dst: \"{dst}\")!.catch
    IOError e -> say(message: \"RENAME-OK\")
    _ -> say(message: \"RENAME-WILD\")
  let entries: List[String] = list_dir(path: \"{missing}\")!.catch
    IOError e -> keep(xs: [])
    _ -> keep_bad(xs: [])
  0
",
        afile = esc(&a_file),
        missing = esc(&missing),
        dst = esc(&dst),
    );

    let r = run_both("typed-fs-family", &source);
    assert_both_exit(&r, 0);
    assert_both_contain(
        &r,
        &["MKDIR-OK", "REMOVE-OK", "COPY-OK", "RENAME-OK", "LIST-OK"],
    );
    for bad in [
        "MKDIR-WILD",
        "REMOVE-WILD",
        "COPY-WILD",
        "RENAME-WILD",
        "LIST-WILD",
    ] {
        assert_neither_contains(&r, bad);
    }
}

// ─── ProcessError: spawn failure carries message + command ──────────

// Criterion 5: process.run spawn failure is caught by ProcessError with
// e.message (underlying text) and e.command (the failing command).
#[test]
fn process_run_spawn_failure_caught_by_process_error() {
    let r = run_both(
        "typed-process",
        "\
use std/process { run }

def fallback() -> ProcessResult
  let ok: List[String] = []
  run(cmd: \"true\", args: ok)!.catch
    _ -> fallback()

def report(m: String, c: String) -> ProcessResult
  say(message: \"PROC-MSG:\" + m)
  say(message: \"PROC-CMD:\" + c)
  fallback()

def main() -> Int
  let bad: List[String] = []
  let r: ProcessResult = run(cmd: \"no_such_cmd_aster_xyzzy\", args: bad)!.catch
    ProcessError e -> report(m: e.message, c: e.command)
    _ -> fallback()
  say(message: \"PROC-DONE\")
  0
",
    );
    assert_both_exit(&r, 0);
    assert_both_contain(
        &r,
        &[
            "PROC-MSG:",
            "No such file or directory",
            "PROC-CMD:no_such_cmd_aster_xyzzy",
            "PROC-DONE",
        ],
    );
}

// ─── IntParseError: unparseable string ──────────────────────────────

// Criterion 6: to_int on a non-numeric string is caught by IntParseError
// with a populated message.
#[test]
fn to_int_unparseable_caught_by_int_parse_error() {
    let r = run_both(
        "typed-intparse",
        "\
def report(m: String) -> Int
  say(message: \"PARSE-MSG:\" + m)
  0

def print_wild() -> Int
  say(message: \"PARSE-WILD\")
  7

def main() -> Int
  to_int(text: \"not_a_number\")!.catch
    IntParseError e -> report(m: e.message)
    _ -> print_wild()
",
    );
    assert_both_exit(&r, 0);
    assert_both_contain(&r, &["PARSE-MSG:", "invalid digit"]);
    assert_neither_contains(&r, "PARSE-WILD");
}

// ─── Channel typed errors (concurrency primitive) ───────────────────

// Criterion 7 + 14: try_receive on an empty channel -> ChannelEmptyError.
#[test]
fn channel_try_receive_empty_caught_by_channel_empty_error() {
    let r = run_both(
        "typed-chan-empty",
        "\
def report(m: String) -> Int
  say(message: \"CHAN-EMPTY-OK\")
  0

def main() -> Int
  let ch: Channel[Int] = Channel(capacity: 10)
  ch.try_receive()!.catch
    ChannelEmptyError e -> report(m: e.message)
    _ -> print_wild()

def print_wild() -> Int
  say(message: \"CHAN-EMPTY-WILD\")
  7
",
    );
    assert_both_exit(&r, 0);
    assert_both_contain(&r, &["CHAN-EMPTY-OK"]);
    assert_neither_contains(&r, "CHAN-EMPTY-WILD");
}

// Criterion 7: try_send on a full channel -> ChannelFullError.
#[test]
fn channel_try_send_full_caught_by_channel_full_error() {
    let r = run_both(
        "typed-chan-full",
        "\
def main() -> Int
  let ch: Channel[Int] = Channel(capacity: 1)
  ch.send(value: 1)
  ch.try_send(value: 2)!.catch
    ChannelFullError e -> say(message: \"CHAN-FULL-OK\")
    _ -> say(message: \"CHAN-FULL-WILD\")
  0
",
    );
    assert_both_exit(&r, 0);
    assert_both_contain(&r, &["CHAN-FULL-OK"]);
    assert_neither_contains(&r, "CHAN-FULL-WILD");
}

// ─── CancelledError: resolving a cancelled task ─────────────────────

// Criterion 9: resolving a cancelled task is caught by CancelledError.
#[test]
fn resolve_cancelled_task_caught_by_cancelled_error() {
    let r = run_both(
        "typed-cancelled",
        "\
def slow() -> Int
  let i: Int = 0
  let total: Int = 0
  while i < 20000000
    total = total + i
    i = i + 1
  42

def main() -> Int
  let t: Task[Int] = async slow()
  t.wait_cancel()
  resolve t!.catch
    CancelledError e -> 77
    _ -> 88
",
    );
    assert_both_exit(&r, 77);
}

// ─── ClassId non-collision ──────────────────────────────────────────

// Criterion 13: user-defined classes coexist with builtin IOError; both
// dispatch correctly with no ClassId collision.
#[test]
fn user_class_and_builtin_io_error_no_collision() {
    let r = run_both(
        "typed-no-collision",
        "\
class MyThing extends Error
  code: Int

def risky() throws Error -> Int
  throw MyThing(message: \"boom\", code: 5)

def main() -> Int
  File.write(path: \"/no/such/aster/typed/dir/f.txt\", content: \"x\")!.catch
    IOError e -> say(message: \"IO-OK\")
    _ -> say(message: \"IO-BAD\")
  let b = risky()!.catch
    MyThing e -> e.code
    _ -> 30
  b
",
    );
    // b == 5 proves the user class dispatched; IO-OK proves the builtin
    // sentinel dispatched alongside it.
    assert_both_exit(&r, 5);
    assert_both_contain(&r, &["IO-OK"]);
    assert_neither_contains(&r, "IO-BAD");
}

// ─── Wildcard catch and .or() still work (criterion 12) ─────────────

#[test]
fn wildcard_catch_and_or_still_work_on_builtins() {
    let r = run_both(
        "typed-wildcard-or",
        "\
def main() -> Int
  let s = File.read(path: \"/no/such/aster/typed/missing2.txt\")!.catch
    _ -> \"WILD-CAUGHT\"
  say(message: s)
  let n = to_int(text: \"nope\")!.or(-3)
  say(message: \"OR-DONE\")
  0
",
    );
    assert_both_exit(&r, 0);
    assert_both_contain(&r, &["WILD-CAUGHT", "OR-DONE"]);
}
