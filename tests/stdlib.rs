mod common;

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
    common::check_ok_with_files(
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
    common::check_ok_with_files(
        r#"use std/cmp { Ord }

class Score includes Ord
  value: Int
"#,
        HashMap::new(),
    );
}

#[test]
fn std_cmp_import_ordering() {
    common::check_ok_with_files(
        r#"use std/cmp { Ordering }

let x = Ordering.Less
"#,
        HashMap::new(),
    );
}

#[test]
fn std_cmp_import_multiple() {
    common::check_ok_with_files(
        r#"use std/cmp { Eq, Ord, Ordering }

class Score includes Eq, Ord
  value: Int
"#,
        HashMap::new(),
    );
}

#[test]
fn std_fmt_import_printable() {
    common::check_ok_with_files(
        r#"use std/fmt { Printable }

class User includes Printable
  name: String
"#,
        HashMap::new(),
    );
}

#[test]
fn std_collections_import_iterable() {
    common::check_ok_with_files(
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
    common::check_ok_with_files(
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
    common::check_ok_with_files(
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
    let err = common::check_err("use std\ndef main() -> Int\n  0\n");
    assert!(
        err.contains("Import from a submodule"),
        "expected submodule suggestion, got: {err}"
    );
}

#[test]
fn std_bare_selective_import_rejected() {
    // `use std { Eq }` should suggest the right submodule
    let err = common::check_err("use std { Eq }\ndef main() -> Int\n  0\n");
    assert!(
        err.contains("std/cmp"),
        "expected suggestion for std/cmp, got: {err}"
    );
}

// ─── Wildcard import of submodule ───────────────────────────────────

#[test]
fn std_cmp_wildcard_import() {
    // `use std/cmp` imports all of Eq, Ord, Ordering
    common::check_ok_with_files(
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
    common::check_ok_with_files(
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
    common::check_ok_with_files(
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
    common::check_ok_with_files(
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

    common::check_ok_with_files(
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

    common::check_ok_with_files(
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

    common::check_ok_with_files(
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

    common::check_ok_with_files(
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

    common::check_ok_with_files(
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
    let err = common::check_err_with_files(
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
    let err = common::check_err_with_files(
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
    let err = common::check_err_with_files(
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
    let err = common::check_err_with_files(
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
    common::check_ok(
        r#"class Point includes Eq
  x: Int
  y: Int

let a = Point(x: 1, y: 2)
let b = Point(x: 3, y: 4)
let same = a == b
"#,
    );
}
