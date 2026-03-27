
// ─── Set type annotation and construction ────────────────────────────

#[test]
fn set_type_annotation_empty() {
    crate::common::check_ok("let s: Set[Int] = Set[Int]()");
}

#[test]
fn set_type_annotation_string() {
    crate::common::check_ok("let s: Set[String] = Set[String]()");
}

#[test]
fn set_type_lowercase_error() {
    // Lowercase "set" is caught at parse time with a helpful message
    let err = crate::common::check_parse_err("let s: set[Int] = Set[Int]()");
    assert!(
        err.contains("Set") || err.contains("Did you mean"),
        "expected case correction, got: {}",
        err
    );
}

// ─── Set requires Eq on element type ─────────────────────────────────

#[test]
fn set_of_primitives_ok() {
    // All primitives include Eq, so Set[Int], Set[String], etc. should work
    crate::common::check_ok("let s: Set[Int] = Set[Int]()");
}

#[test]
fn set_of_custom_type_with_eq_ok() {
    crate::common::check_ok(
        "\
class Point includes Eq
  x: Int
  y: Int

let s: Set[Point] = Set[Point]()
",
    );
}

#[test]
fn set_of_custom_type_without_eq_error() {
    let err = crate::common::check_err(
        "\
class NoEq
  value: Int

let s: Set[NoEq] = Set[NoEq]()
",
    );
    assert!(
        err.contains("Eq") || err.contains("does not include"),
        "expected Eq constraint error, got: {}",
        err
    );
}

// ─── Set methods: push (silent no-op on duplicate) ───────────────────

#[test]
fn set_push_typecheck() {
    crate::common::check_ok(
        "\
let s = Set[Int]()
s.push(item: 1)
",
    );
}

#[test]
fn set_push_wrong_type_error() {
    let err = crate::common::check_err(
        "\
let s = Set[Int]()
s.push(item: \"bad\")
",
    );
    assert!(
        err.contains("mismatch") || err.contains("expected") || err.contains("Int"),
        "expected type mismatch, got: {}",
        err
    );
}

// ─── Set methods: pop ────────────────────────────────────────────────

#[test]
fn set_pop_typecheck() {
    crate::common::check_ok(
        "\
let s = Set[Int]()
s.push(item: 1)
let x: Int = s.pop()
",
    );
}

#[test]
fn set_pop_returns_element_type() {
    let err = crate::common::check_err(
        "\
let s = Set[Int]()
let x: String = s.pop()
",
    );
    assert!(
        err.contains("mismatch") || err.contains("String") || err.contains("Int"),
        "expected type mismatch, got: {}",
        err
    );
}

// ─── Set methods: remove ─────────────────────────────────────────────

#[test]
fn set_remove_typecheck() {
    crate::common::check_ok(
        "\
let s = Set[Int]()
s.push(item: 1)
s.remove(item: 1)
",
    );
}

#[test]
fn set_remove_wrong_type_error() {
    let err = crate::common::check_err(
        "\
let s = Set[Int]()
s.remove(item: \"bad\")
",
    );
    assert!(
        err.contains("mismatch") || err.contains("expected") || err.contains("Int"),
        "expected type mismatch, got: {}",
        err
    );
}

// ─── Set methods: contains ───────────────────────────────────────────

#[test]
fn set_contains_item_typecheck() {
    crate::common::check_ok(
        "\
let s = Set[Int]()
s.push(item: 1)
let has: Bool = s.contains(item: 1)
",
    );
}

#[test]
fn set_contains_item_wrong_type_error() {
    let err = crate::common::check_err(
        "\
let s = Set[Int]()
s.contains(item: \"bad\")
",
    );
    assert!(
        err.contains("mismatch") || err.contains("expected") || err.contains("Int"),
        "expected type mismatch, got: {}",
        err
    );
}

#[test]
fn set_contains_predicate_typecheck() {
    crate::common::check_ok(
        "\
let s = Set[Int]()
s.push(item: 1)
let has: Bool = s.contains(f: -> x : x > 0)
",
    );
}

// ─── Set methods: size/len ───────────────────────────────────────────

#[test]
fn set_len_typecheck() {
    crate::common::check_ok(
        "\
let s = Set[Int]()
let n: Int = s.len()
",
    );
}

// ─── Set methods: remove_first ───────────────────────────────────────

#[test]
fn set_remove_first_typecheck() {
    crate::common::check_ok(
        "\
let s = Set[Int]()
s.push(item: 1)
let found: Int? = s.remove_first(f: -> x : x == 1)
",
    );
}

// ─── Set methods: each (Iterable) ────────────────────────────────────

#[test]
fn set_each_typecheck() {
    crate::common::check_ok(
        "\
let s = Set[Int]()
s.push(item: 1)
s.each(f: -> x : log(message: to_string(value: x)))
",
    );
}

// ─── Set in for loop ─────────────────────────────────────────────────

#[test]
fn set_for_in_loop() {
    crate::common::check_ok(
        "\
let s = Set[Int]()
s.push(item: 1)
for x in s
  let y = x + 1
",
    );
}

// ─── Set type as function parameter ──────────────────────────────────

#[test]
fn set_as_function_param() {
    crate::common::check_ok(
        "\
def process(items: Set[String]) -> Int
  items.len()
",
    );
}

#[test]
fn set_as_return_type() {
    crate::common::check_ok(
        "\
def make_set() -> Set[Int]
  Set[Int]()
",
    );
}

// ─── Set nullable ────────────────────────────────────────────────────

#[test]
fn set_nullable_type() {
    crate::common::check_ok(
        "\
def get_set() -> Set[Int]?
  nil
",
    );
}

// ─── Map key Eq constraint ───────────────────────────────────────────

#[test]
fn map_key_custom_type_with_eq_ok() {
    crate::common::check_ok(
        "\
class Key includes Eq
  id: Int

let m: Map[Key, String] = {}
",
    );
}

#[test]
fn map_key_custom_type_without_eq_error() {
    let err = crate::common::check_err(
        "\
class NoEq
  id: Int

let m: Map[NoEq, String] = {}
",
    );
    assert!(
        err.contains("Eq") || err.contains("does not include"),
        "expected Eq constraint error on Map key, got: {}",
        err
    );
}

// ─── Composition: Set of Set not allowed (inner Set needs Eq) ────────

#[test]
fn set_of_list_without_eq_error() {
    let err = crate::common::check_err("let s: Set[List[Int]] = Set[List[Int]]()");
    assert!(
        err.contains("Eq") || err.contains("does not include"),
        "expected Eq constraint error, got: {}",
        err
    );
}

// ─── Iterable vocabulary on Set ──────────────────────────────────────

#[test]
fn set_map_method_typecheck() {
    crate::common::check_ok(
        "\
let s = Set[Int]()
s.push(item: 1)
let doubled = s.map(f: -> x : x * 2)
",
    );
}

#[test]
fn set_filter_method_typecheck() {
    crate::common::check_ok(
        "\
let s = Set[Int]()
s.push(item: 1)
let filtered = s.filter(f: -> x : x > 0)
",
    );
}
