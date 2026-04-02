use std::collections::HashMap;

// ─── Virtual std module ─────────────────────────────────────────────
//
// Protocol traits and supporting types live in hierarchical std submodules:
//   std/cmp         — Eq, Ord, Ordering
//   std/fmt         — Printable
//   std/collections — Iterable
//   std/convert     — From, Into
//
// Users import via `use std/cmp { Eq }` etc.
// `use std` wildcard imports everything from all submodules.
//
// Operators (+, ==, <) on primitives work without imports.
// Trait metadata travels with classes — consumers don't need to
// import the trait, only producers do.

// ─── Selective imports from submodules ───────────────────────────────

#[test]
fn std_cmp_import_eq() {
    crate::common::check_ok_with_files(
        r#"use std/cmp { Eq }

class Point includes Eq
  x: Int
  y: Int
"#,
        HashMap::new(),
    );
}

#[test]
fn std_cmp_import_ord() {
    crate::common::check_ok_with_files(
        r#"use std/cmp { Ord }

class Score includes Ord
  value: Int
"#,
        HashMap::new(),
    );
}

#[test]
fn std_cmp_import_ordering() {
    crate::common::check_ok_with_files(
        r#"use std/cmp { Ordering }

let x = Ordering.Less
"#,
        HashMap::new(),
    );
}

#[test]
fn std_cmp_import_multiple() {
    crate::common::check_ok_with_files(
        r#"use std/cmp { Eq, Ord, Ordering }

class Score includes Eq, Ord
  value: Int
"#,
        HashMap::new(),
    );
}

#[test]
fn std_fmt_import_printable() {
    crate::common::check_ok_with_files(
        r#"use std/fmt { Printable }

class User includes Printable
  name: String
"#,
        HashMap::new(),
    );
}

#[test]
fn std_collections_import_iterable() {
    crate::common::check_ok_with_files(
        r#"use std/collections { Iterable }

class Numbers includes Iterable
  items: List[Int]

  def each(f: Fn(Int) -> Void) -> Void
    items.each(f: f)

let nums = Numbers(items: [1, 2, 3])
let doubled = nums.map(f: -> x: x * 2)
"#,
        HashMap::new(),
    );
}

#[test]
fn std_convert_import_from() {
    crate::common::check_ok_with_files(
        r#"use std/convert { From }

class Celsius includes From[Int]
  value: Int

  def from(value: Int) -> Celsius
    Celsius(value: value)
"#,
        HashMap::new(),
    );
}

// ─── Multiple imports from different submodules ─────────────────────

#[test]
fn std_import_from_multiple_submodules() {
    crate::common::check_ok_with_files(
        r#"use std/cmp { Eq, Ord }
use std/fmt { Printable }

class Score includes Eq, Ord, Printable
  value: Int
"#,
        HashMap::new(),
    );
}

// ─── Bare `use std` is rejected ─────────────────────────────────────

#[test]
fn std_bare_import_rejected() {
    // `use std` without a submodule is an error
    let err = crate::common::check_err("use std\ndef main() -> Int\n  0\n");
    assert!(
        err.contains("Import from a submodule"),
        "expected submodule suggestion, got: {err}"
    );
}

#[test]
fn std_bare_selective_import_rejected() {
    // `use std { Eq }` should suggest the right submodule
    let err = crate::common::check_err("use std { Eq }\ndef main() -> Int\n  0\n");
    assert!(
        err.contains("std/cmp"),
        "expected suggestion for std/cmp, got: {err}"
    );
}

// ─── Wildcard import of submodule ───────────────────────────────────

#[test]
fn std_cmp_wildcard_import() {
    // `use std/cmp` imports all of Eq, Ord, Ordering
    crate::common::check_ok_with_files(
        r#"use std/cmp

class Point includes Eq, Ord
  x: Int
  y: Int
"#,
        HashMap::new(),
    );
}

// ─── Namespace import ───────────────────────────────────────────────

#[test]
fn std_cmp_namespace_import() {
    // `use std/cmp as c` — access via c.Eq etc (for traits used in includes,
    // the selective import is more natural, but namespace should work)
    crate::common::check_ok_with_files(
        r#"use std/cmp { Ordering }
use std/cmp as c

let x = Ordering.Less
"#,
        HashMap::new(),
    );
}

// ─── Operators work without imports ─────────────────────────────────

#[test]
fn operators_without_std_import() {
    crate::common::check_ok_with_files(
        r#"let a = 1 == 2
let b = 1 < 2
let c = "hello" + " world"
let d = 3 > 1
let e = true and false
"#,
        HashMap::new(),
    );
}

#[test]
fn list_operations_without_std_import() {
    // List has built-in Iterable vocabulary — no import needed to USE it
    crate::common::check_ok_with_files(
        r#"let nums = [1, 2, 3]
let doubled = nums.map(f: -> x: x * 2)
let evens = nums.filter(f: -> x: x % 2 == 0)
let total = nums.reduce(init: 0, f: -> acc, x: acc + x)
"#,
        HashMap::new(),
    );
}

// ─── Cross-module: trait metadata travels with class ────────────────

#[test]
fn cross_module_eq_operator() {
    // Module A defines class with Eq. Module B uses == without importing Eq.
    let mut files = HashMap::new();
    files.insert(
        "models".into(),
        r#"use std/cmp { Eq }

pub class User includes Eq
  name: String
"#
        .into(),
    );

    crate::common::check_ok_with_files(
        r#"use models { User }

let a = User(name: "Alice")
let b = User(name: "Bob")
let same = a == b
"#,
        files,
    );
}

#[test]
fn cross_module_eq_method() {
    // .eq() method travels with the class — no Eq import needed by consumer
    let mut files = HashMap::new();
    files.insert(
        "models".into(),
        r#"use std/cmp { Eq }

pub class User includes Eq
  name: String
"#
        .into(),
    );

    crate::common::check_ok_with_files(
        r#"use models { User }

let a = User(name: "Alice")
let b = User(name: "Bob")
let same = a.eq(other: b)
"#,
        files,
    );
}

#[test]
fn cross_module_iterable_vocabulary() {
    // Iterable vocabulary methods travel with the class
    let mut files = HashMap::new();
    files.insert(
        "collections".into(),
        r#"use std/collections { Iterable }

pub class Numbers includes Iterable
  items: List[Int]

  def each(f: Fn(Int) -> Void) -> Void
    items.each(f: f)
"#
        .into(),
    );

    crate::common::check_ok_with_files(
        r#"use collections { Numbers }

let nums = Numbers(items: [1, 2, 3])
let doubled = nums.map(f: -> x: x * 2)
let total = nums.reduce(init: 0, f: -> acc, x: acc + x)
"#,
        files,
    );
}

#[test]
fn cross_module_ord_operators() {
    // Ordering operators work on imported class without Ord import
    let mut files = HashMap::new();
    files.insert(
        "models".into(),
        r#"use std/cmp { Ord }

pub class Score includes Ord
  value: Int
"#
        .into(),
    );

    crate::common::check_ok_with_files(
        r#"use models { Score }

let a = Score(value: 10)
let b = Score(value: 20)
let less = a < b
"#,
        files,
    );
}

#[test]
fn cross_module_printable_method() {
    // .to_string() method travels with the class
    let mut files = HashMap::new();
    files.insert(
        "models".into(),
        r#"use std/fmt { Printable }

pub class Tag includes Printable
  label: String
"#
        .into(),
    );

    crate::common::check_ok_with_files(
        r#"use models { Tag }

let t = Tag(label: "v1")
let s = t.to_string()
"#,
        files,
    );
}

// ─── Error: unknown trait without import ─────────────────────────────

#[test]
fn error_includes_eq_without_import() {
    let err = crate::common::check_err_with_files(
        r#"class Point includes Eq
  x: Int
"#,
        HashMap::new(),
    );
    assert!(
        err.contains("Eq") && err.contains("use std/cmp"),
        "Expected error suggesting `use std/cmp {{ Eq }}`, got: {}",
        err
    );
}

#[test]
fn error_includes_iterable_without_import() {
    let err = crate::common::check_err_with_files(
        r#"class Numbers includes Iterable
  items: List[Int]

  def each(f: Fn(Int) -> Void) -> Void
    items.each(f: f)
"#,
        HashMap::new(),
    );
    assert!(
        err.contains("Iterable") && err.contains("use std/collections"),
        "Expected error suggesting `use std/collections {{ Iterable }}`, got: {}",
        err
    );
}

#[test]
fn error_includes_ord_without_import() {
    let err = crate::common::check_err_with_files(
        r#"class Score includes Ord
  value: Int
"#,
        HashMap::new(),
    );
    assert!(
        err.contains("Ord") && err.contains("use std/cmp"),
        "Expected error suggesting `use std/cmp {{ Ord }}`, got: {}",
        err
    );
}

#[test]
fn error_includes_printable_without_import() {
    let err = crate::common::check_err_with_files(
        r#"class Tag includes Printable
  label: String
"#,
        HashMap::new(),
    );
    assert!(
        err.contains("Printable") && err.contains("use std/fmt"),
        "Expected error suggesting `use std/fmt {{ Printable }}`, got: {}",
        err
    );
}

// ─── Prelude mode: no module loader ─────────────────────────────────

#[test]
fn prelude_mode_without_loader() {
    // Tests without a module loader still get built-in traits (prelude mode)
    crate::common::check_ok(
        r#"class Point includes Eq
  x: Int
  y: Int

let a = Point(x: 1, y: 2)
let b = Point(x: 3, y: 4)
let same = a == b
"#,
    );
}

// ─── std/sys — system primitives ────────────────────────────────────

#[test]
fn std_sys_import_args() {
    crate::common::check_ok_with_files("use std/sys { args }\n\nlet a = args()\n", HashMap::new());
}

#[test]
fn std_sys_import_env() {
    crate::common::check_ok_with_files(
        "use std/sys { env }\n\nlet val = env(key: \"HOME\")\n",
        HashMap::new(),
    );
}

#[test]
fn std_sys_import_set_env() {
    crate::common::check_ok_with_files(
        "use std/sys { set_env }\n\nset_env(key: \"FOO\", value: \"bar\")\n",
        HashMap::new(),
    );
}

#[test]
fn std_sys_import_exit() {
    crate::common::check_ok_with_files(
        "use std/sys { exit }\n\ndef main() -> Int\n  exit(code: 0)\n  0\n",
        HashMap::new(),
    );
}

#[test]
fn std_sys_import_multiple() {
    crate::common::check_ok_with_files(
        "use std/sys { args, env, set_env, exit }\n\nlet a = args()\nlet v = env(key: \"PATH\")\n",
        HashMap::new(),
    );
}

#[test]
fn std_sys_import_nonexistent_fails() {
    let err = crate::common::check_err_with_files("use std/sys { nonexistent }\n", HashMap::new());
    assert!(
        err.contains("not found") || err.contains("not exported"),
        "expected not found error, got: {err}"
    );
}

#[test]
fn std_sys_args_returns_list_string() {
    crate::common::check_ok_with_files(
        "use std/sys { args }\n\nlet a: List[String] = args()\n",
        HashMap::new(),
    );
}

#[test]
fn std_sys_env_returns_nullable_string() {
    crate::common::check_ok_with_files(
        r#"use std/sys { env }

let val = env(key: "HOME")
"#,
        HashMap::new(),
    );
}

#[test]
fn std_sys_wildcard_import() {
    crate::common::check_ok_with_files("use std/sys\n\nlet a = args()\n", HashMap::new());
}

// ─── std/fs — filesystem primitives ─────────────────────────────────

#[test]
fn std_fs_import_read_file() {
    crate::common::check_ok_with_files(
        "use std/fs { read_file }\n\ndef main() throws IOError -> Int\n  let c = read_file(path: \"test.txt\")!\n  0\n",
        HashMap::new(),
    );
}

#[test]
fn std_fs_import_write_file() {
    crate::common::check_ok_with_files(
        "use std/fs { write_file }\n\ndef main() throws IOError -> Int\n  write_file(path: \"test.txt\", content: \"hello\")!\n  0\n",
        HashMap::new(),
    );
}

#[test]
fn std_fs_import_exists() {
    crate::common::check_ok_with_files(
        "use std/fs { exists }\n\nlet e: Bool = exists(path: \"/tmp\")\n",
        HashMap::new(),
    );
}

#[test]
fn std_fs_import_is_dir() {
    crate::common::check_ok_with_files(
        "use std/fs { is_dir }\n\nlet d: Bool = is_dir(path: \"/tmp\")\n",
        HashMap::new(),
    );
}

#[test]
fn std_fs_import_mkdir() {
    crate::common::check_ok_with_files(
        "use std/fs { mkdir }\n\ndef main() throws IOError -> Int\n  mkdir(path: \"/tmp/test\")!\n  0\n",
        HashMap::new(),
    );
}

#[test]
fn std_fs_import_list_dir() {
    crate::common::check_ok_with_files(
        "use std/fs { list_dir }\n\ndef main() throws IOError -> Int\n  let entries: List[String] = list_dir(path: \"/tmp\")!\n  0\n",
        HashMap::new(),
    );
}

#[test]
fn std_fs_import_multiple() {
    crate::common::check_ok_with_files(
        "use std/fs { read_file, write_file, exists, mkdir, list_dir, remove, copy, rename }\n",
        HashMap::new(),
    );
}

#[test]
fn std_fs_import_nonexistent_fails() {
    let err = crate::common::check_err_with_files("use std/fs { nonexistent }\n", HashMap::new());
    assert!(
        err.contains("not found") || err.contains("not exported"),
        "expected not found error, got: {err}"
    );
}

#[test]
fn std_fs_wildcard_import() {
    crate::common::check_ok_with_files(
        "use std/fs\n\nlet e = exists(path: \"/tmp\")\n",
        HashMap::new(),
    );
}

// ─── std/process — process spawning ─────────────────────────────────

#[test]
fn std_process_import_run() {
    crate::common::check_ok_with_files(
        "\
use std/process { run }

def main() throws ProcessError -> Int
  let result = run(cmd: \"echo\", args: [\"hello\"])!
  0
",
        HashMap::new(),
    );
}

#[test]
fn std_process_result_fields() {
    crate::common::check_ok_with_files(
        "\
use std/process { run }

def main() throws ProcessError -> Int
  let result = run(cmd: \"echo\", args: [\"hello\"])!
  let code: Int = result.exit_code
  let out: String = result.stdout
  let err: String = result.stderr
  0
",
        HashMap::new(),
    );
}

#[test]
fn std_process_import_nonexistent_fails() {
    let err =
        crate::common::check_err_with_files("use std/process { nonexistent }\n", HashMap::new());
    assert!(
        err.contains("not found") || err.contains("not exported"),
        "expected not found error, got: {err}"
    );
}

// ─── std/runtime — JIT evaluation ───────────────────────────────────

#[test]
fn std_runtime_import_jit_run() {
    crate::common::check_ok_with_files_jit(
        "use std/runtime { jit_run }\n\ndef main() -> Int\n  jit_run(code: \"def main() -> Int\\n  0\")\n",
        HashMap::new(),
    );
}

#[test]
fn std_runtime_jit_run_returns_int() {
    crate::common::check_ok_with_files_jit(
        "use std/runtime { jit_run }\n\ndef main() -> Int\n  let result: Int = jit_run(code: \"def main() -> Int\\n  42\")\n  result\n",
        HashMap::new(),
    );
}

#[test]
fn std_runtime_import_nonexistent_fails() {
    let err =
        crate::common::check_err_with_files("use std/runtime { nonexistent }\n", HashMap::new());
    assert!(
        err.contains("not found") || err.contains("not exported"),
        "expected not found error, got: {err}"
    );
}

#[test]
fn std_runtime_wildcard_import() {
    crate::common::check_ok_with_files_jit(
        "use std/runtime\n\ndef main() -> Int\n  jit_run(code: \"def main() -> Int\\n  0\")\n",
        HashMap::new(),
    );
}

// ─── std/crypto — hashing ───────────────────────────────────────────

#[test]
fn std_crypto_import_sha256() {
    crate::common::check_ok_with_files(
        "use std/crypto { sha256 }\n\nlet hash: String = sha256(data: \"hello\")\n",
        HashMap::new(),
    );
}

#[test]
fn std_crypto_import_nonexistent_fails() {
    let err =
        crate::common::check_err_with_files("use std/crypto { nonexistent }\n", HashMap::new());
    assert!(
        err.contains("not found") || err.contains("not exported"),
        "expected not found error, got: {err}"
    );
}

#[test]
fn std_crypto_wildcard_import() {
    crate::common::check_ok_with_files(
        "use std/crypto\n\nlet h = sha256(data: \"hello\")\n",
        HashMap::new(),
    );
}
