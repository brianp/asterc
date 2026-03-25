mod common;

// ─── Iterable Protocol ──────────────────────────────────────────────
//
// Implement each(), get the full vocabulary for free.
// Element type T inferred from each()'s callback parameter.
// No type param needed on `includes Iterable`.
//
// Vocabulary: map, filter, reduce, find, any, all, count,
//             first, last, min, max, sort, to_list
// Conditional: min/max/sort require T includes Ord

// ─── Basic: includes Iterable, element type inferred ─────────────────

#[test]
fn includes_iterable_basic() {
    common::check_ok(
        r#"class Numbers includes Iterable
  items: List[Int]

  def each(f: Fn(Int) -> Void) -> Void
    items.each(f: f)
"#,
    );
}

#[test]
fn includes_iterable_custom_element_type() {
    common::check_ok(
        r#"class User
  name: String

class Users includes Iterable
  items: List[User]

  def each(f: Fn(User) -> Void) -> Void
    items.each(f: f)
"#,
    );
}

#[test]
fn includes_iterable_requires_each() {
    // Missing each() should error
    let err = common::check_err(
        r#"class Numbers includes Iterable
  items: List[Int]
"#,
    );
    assert!(
        err.contains("each") || err.contains("required"),
        "Expected missing each error, got: {}",
        err
    );
}

// ─── Vocabulary: map ─────────────────────────────────────────────────

#[test]
fn iterable_map() {
    common::check_ok(
        r#"class Numbers includes Iterable
  items: List[Int]

  def each(f: Fn(Int) -> Void) -> Void
    items.each(f: f)

let nums = Numbers(items: [1, 2, 3])
let doubled = nums.map(f: -> x: x * 2)
"#,
    );
}

#[test]
fn iterable_map_transforms_type() {
    common::check_ok(
        r#"class Numbers includes Iterable
  items: List[Int]

  def each(f: Fn(Int) -> Void) -> Void
    items.each(f: f)

let nums = Numbers(items: [1, 2, 3])
let strs = nums.map(f: -> x: "hello")
"#,
    );
}

// ─── Vocabulary: filter ──────────────────────────────────────────────

#[test]
fn iterable_filter() {
    common::check_ok(
        r#"class Numbers includes Iterable
  items: List[Int]

  def each(f: Fn(Int) -> Void) -> Void
    items.each(f: f)

let nums = Numbers(items: [1, 2, 3])
let evens = nums.filter(f: -> x: x % 2 == 0)
"#,
    );
}

// ─── Vocabulary: reduce ──────────────────────────────────────────────

#[test]
fn iterable_reduce() {
    common::check_ok(
        r#"class Numbers includes Iterable
  items: List[Int]

  def each(f: Fn(Int) -> Void) -> Void
    items.each(f: f)

let nums = Numbers(items: [1, 2, 3])
let total = nums.reduce(init: 0, f: -> acc, x: acc + x)
"#,
    );
}

// ─── Vocabulary: find ────────────────────────────────────────────────

#[test]
fn iterable_find() {
    common::check_ok(
        r#"class Numbers includes Iterable
  items: List[Int]

  def each(f: Fn(Int) -> Void) -> Void
    items.each(f: f)

let nums = Numbers(items: [1, 2, 3])
let found = nums.find(f: -> x: x > 2)
"#,
    );
}

// ─── Vocabulary: any / all ───────────────────────────────────────────

#[test]
fn iterable_any() {
    common::check_ok(
        r#"class Numbers includes Iterable
  items: List[Int]

  def each(f: Fn(Int) -> Void) -> Void
    items.each(f: f)

let nums = Numbers(items: [1, 2, 3])
let has_big = nums.any(f: -> x: x > 2)
"#,
    );
}

#[test]
fn iterable_all() {
    common::check_ok(
        r#"class Numbers includes Iterable
  items: List[Int]

  def each(f: Fn(Int) -> Void) -> Void
    items.each(f: f)

let nums = Numbers(items: [1, 2, 3])
let all_pos = nums.all(f: -> x: x > 0)
"#,
    );
}

// ─── Vocabulary: count ───────────────────────────────────────────────

#[test]
fn iterable_count() {
    common::check_ok(
        r#"class Numbers includes Iterable
  items: List[Int]

  def each(f: Fn(Int) -> Void) -> Void
    items.each(f: f)

let nums = Numbers(items: [1, 2, 3])
let n = nums.count()
"#,
    );
}

// ─── Vocabulary: first / last ────────────────────────────────────────

#[test]
fn iterable_first() {
    common::check_ok(
        r#"class Numbers includes Iterable
  items: List[Int]

  def each(f: Fn(Int) -> Void) -> Void
    items.each(f: f)

let nums = Numbers(items: [1, 2, 3])
let f = nums.first()
"#,
    );
}

#[test]
fn iterable_last() {
    common::check_ok(
        r#"class Numbers includes Iterable
  items: List[Int]

  def each(f: Fn(Int) -> Void) -> Void
    items.each(f: f)

let nums = Numbers(items: [1, 2, 3])
let l = nums.last()
"#,
    );
}

// ─── Vocabulary: to_list ─────────────────────────────────────────────

#[test]
fn iterable_to_list() {
    common::check_ok(
        r#"class Numbers includes Iterable
  items: List[Int]

  def each(f: Fn(Int) -> Void) -> Void
    items.each(f: f)

let nums = Numbers(items: [1, 2, 3])
let list = nums.to_list()
"#,
    );
}

// ─── Conditional: min / max / sort require Ord ───────────────────────

#[test]
fn iterable_min_with_ord() {
    common::check_ok(
        r#"class Numbers includes Iterable
  items: List[Int]

  def each(f: Fn(Int) -> Void) -> Void
    items.each(f: f)

let nums = Numbers(items: [3, 1, 2])
let smallest = nums.min()
"#,
    );
}

#[test]
fn iterable_max_with_ord() {
    common::check_ok(
        r#"class Numbers includes Iterable
  items: List[Int]

  def each(f: Fn(Int) -> Void) -> Void
    items.each(f: f)

let nums = Numbers(items: [3, 1, 2])
let biggest = nums.max()
"#,
    );
}

#[test]
fn iterable_sort_with_ord() {
    common::check_ok(
        r#"class Numbers includes Iterable
  items: List[Int]

  def each(f: Fn(Int) -> Void) -> Void
    items.each(f: f)

let nums = Numbers(items: [3, 1, 2])
let sorted = nums.sort()
"#,
    );
}

#[test]
fn iterable_min_without_ord_error() {
    let err = common::check_err(
        r#"class Thing
  label: String

class Things includes Iterable
  items: List[Thing]

  def each(f: Fn(Thing) -> Void) -> Void
    items.each(f: f)

let things = Things(items: [])
let smallest = things.min()
"#,
    );
    assert!(
        err.contains("Ord") || err.contains("min"),
        "Expected Ord requirement error, got: {}",
        err
    );
}

#[test]
fn iterable_max_without_ord_error() {
    let err = common::check_err(
        r#"class Thing
  label: String

class Things includes Iterable
  items: List[Thing]

  def each(f: Fn(Thing) -> Void) -> Void
    items.each(f: f)

let things = Things(items: [])
let biggest = things.max()
"#,
    );
    assert!(
        err.contains("Ord") || err.contains("max"),
        "Expected Ord requirement error, got: {}",
        err
    );
}

#[test]
fn iterable_sort_without_ord_error() {
    let err = common::check_err(
        r#"class Thing
  label: String

class Things includes Iterable
  items: List[Thing]

  def each(f: Fn(Thing) -> Void) -> Void
    items.each(f: f)

let things = Things(items: [])
let sorted = things.sort()
"#,
    );
    assert!(
        err.contains("Ord") || err.contains("sort"),
        "Expected Ord requirement error, got: {}",
        err
    );
}

// ─── Conditional: min/max/sort with custom Ord type ──────────────────

#[test]
fn iterable_min_custom_ord_type() {
    common::check_ok(
        r#"class Score includes Ord
  value: Int

class Scores includes Iterable
  items: List[Score]

  def each(f: Fn(Score) -> Void) -> Void
    items.each(f: f)

let scores = Scores(items: [])
let lowest = scores.min()
"#,
    );
}

// ─── Captured variables in iterable closures ────────────────────────

#[test]
fn iterable_map_with_captured_variable() {
    common::check_ok(
        r#"let items = [1, 2, 3, 4, 5]
let multiplier = 10
let result = items.map(f: -> x: x * multiplier)
"#,
    );
}

#[test]
fn iterable_filter_with_captured_variable() {
    common::check_ok(
        r#"let items = [1, 2, 3, 4, 5]
let threshold = 3
let result = items.filter(f: -> x: x > threshold)
"#,
    );
}

#[test]
fn iterable_map_with_captured_string() {
    common::check_ok(
        r#"let items = [1, 2, 3]
let prefix = "item"
let result = items.map(f: -> x: prefix)
"#,
    );
}

// ─── Chaining ────────────────────────────────────────────────────────

#[test]
fn iterable_chain_filter_map() {
    common::check_ok(
        r#"class Numbers includes Iterable
  items: List[Int]

  def each(f: Fn(Int) -> Void) -> Void
    items.each(f: f)

let nums = Numbers(items: [1, 2, 3, 4])
let result = nums.filter(f: -> x: x > 2).map(f: -> x: x * 10)
"#,
    );
}

// ─── Vocabulary: each ───────────────────────────────────────────────

#[test]
fn iterable_each_on_list() {
    common::check_ok(
        r#"let nums = [1, 2, 3]
nums.each(f: -> x: log(message: "got one"))
"#,
    );
}

#[test]
fn iterable_each_with_capture() {
    common::check_ok(
        r#"let nums = [1, 2, 3]
let tag = "item"
nums.each(f: -> x: log(message: tag))
"#,
    );
}

#[test]
fn iterable_each_on_empty_list() {
    common::check_ok(
        r#"let nums: List[Int] = []
nums.each(f: -> x: log(message: "nope"))
"#,
    );
}

// ─── each() on List should work (List includes Iterable) ─────────────

#[test]
fn list_includes_iterable() {
    common::check_ok(
        r#"let nums = [1, 2, 3]
let doubled = nums.map(f: -> x: x * 2)
"#,
    );
}

#[test]
fn list_filter() {
    common::check_ok(
        r#"let nums = [1, 2, 3, 4]
let evens = nums.filter(f: -> x: x % 2 == 0)
"#,
    );
}

#[test]
fn list_reduce() {
    common::check_ok(
        r#"let nums = [1, 2, 3]
let total = nums.reduce(init: 0, f: -> acc, x: acc + x)
"#,
    );
}

#[test]
fn list_min() {
    common::check_ok(
        r#"let nums = [3, 1, 2]
let smallest = nums.min()
"#,
    );
}

// ─── Override vocabulary method ──────────────────────────────────────

#[test]
fn override_vocabulary_method() {
    // Class can override a vocabulary method for performance
    common::check_ok(
        r#"class Numbers includes Iterable
  items: List[Int]

  def each(f: Fn(Int) -> Void) -> Void
    items.each(f: f)

  def count() -> Int
    items.len()

let nums = Numbers(items: [1, 2, 3])
let n = nums.count()
"#,
    );
}

// ─── Iterable with explicit type arg is error ────────────────────────

// ─── For-loop over custom Iterable ──────────────────────────────────

#[test]
fn for_loop_over_custom_iterable() {
    common::check_ok(
        r#"class NumberRange includes Iterable
  start_val: Int
  end_val: Int

  def each(f: Fn(Int) -> Void) -> Void
    log(message: "iterating")

for x in NumberRange(start_val: 0, end_val: 10)
  log(message: "got value")
"#,
    );
}

#[test]
fn for_loop_over_non_iterable_class_error() {
    let err = common::check_err(
        r#"class NotIterable
  x: Int

for item in NotIterable(x: 1)
  log(message: "nope")
"#,
    );
    assert!(
        err.contains("iterate") || err.contains("Iterable") || err.contains("expected"),
        "Expected iteration error, got: {}",
        err
    );
}

#[test]
fn iterable_explicit_type_arg_error() {
    let err = common::check_err(
        r#"class Numbers includes Iterable[Int]
  items: List[Int]

  def each(f: Fn(Int) -> Void) -> Void
    items.each(f: f)
"#,
    );
    assert!(
        err.contains("inferred") || err.contains("type param") || err.contains("Iterable"),
        "Expected error about Iterable not taking type args, got: {}",
        err
    );
}
