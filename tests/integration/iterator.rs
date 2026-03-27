
// ─── Iterator[T] Protocol ──────────────────────────────────────────
//
// Parametric trait: `trait Iterator[T]` with required `next() -> T?`
// Classes include Iterator[ConcreteType] and implement next().
// For-loop can iterate over Iterator types (calls next() until nil).

// ─── Basic: includes Iterator[Int] with correct next() ─────────────

#[test]
fn includes_iterator_int() {
    crate::common::check_ok(
        "\
class Counter includes Iterator[Int]
  current: Int
  max: Int

  def next() -> Int?
    if current >= max
      return nil
    return nil
",
    );
}

// ─── Missing next() method ──────────────────────────────────────────

#[test]
fn iterator_missing_next_method() {
    let err = crate::common::check_err(
        "\
class BadIter includes Iterator[Int]
  x: Int
",
    );
    assert!(
        err.contains("next"),
        "Expected error about missing 'next' method, got: {}",
        err
    );
}

// ─── Wrong return type on next() ────────────────────────────────────

#[test]
fn iterator_wrong_return_type() {
    let err = crate::common::check_err(
        "\
class BadIter includes Iterator[Int]
  x: Int

  def next() -> Int
    return 1
",
    );
    assert!(
        err.contains("next") || err.contains("signature"),
        "Expected error about wrong return type, got: {}",
        err
    );
}

// ─── Iterator with String type parameter ────────────────────────────

#[test]
fn includes_iterator_string() {
    crate::common::check_ok(
        "\
class StringIter includes Iterator[String]
  items: List[String]
  index: Int

  def next() -> String?
    if index >= 0
      return nil
    return nil
",
    );
}

// ─── Iterator with custom class type parameter ──────────────────────

#[test]
fn includes_iterator_custom_type() {
    crate::common::check_ok(
        "\
class User
  name: String

class UserIter includes Iterator[User]
  users: List[User]
  index: Int

  def next() -> User?
    if index >= 0
      return nil
    return nil
",
    );
}

// ─── For-loop with Iterator ─────────────────────────────────────────

#[test]
fn for_loop_with_iterator() {
    crate::common::check_ok(
        "\
class Range includes Iterator[Int]
  current: Int
  max: Int

  def next() -> Int?
    if current >= max
      return nil
    return nil

let r = Range(current: 0, max: 10)
for x in r
  log(message: \"hi\")
",
    );
}

// ─── For-loop element type inferred from Iterator type param ────────

#[test]
fn for_loop_iterator_element_type() {
    // The element in for-loop should be the Iterator's T, not T?
    let err = crate::common::check_err(
        "\
class Range includes Iterator[Int]
  current: Int
  max: Int

  def next() -> Int?
    if current >= max
      return nil
    return nil

let r = Range(current: 0, max: 10)
for x in r
  let s: String = x
",
    );
    assert!(
        err.contains("Int") || err.contains("String"),
        "Expected type mismatch error, got: {}",
        err
    );
}

// ─── Iterator without type parameter should error ───────────────────

#[test]
fn iterator_requires_type_parameter() {
    let err = crate::common::check_err(
        "\
class BadIter includes Iterator
  x: Int

  def next() -> Int?
    return nil
",
    );
    assert!(
        err.contains("type parameter") || err.contains("Iterator"),
        "Expected error about missing type parameter, got: {}",
        err
    );
}

// ─── Implicit Hash: Eq includes implies hashable ────────────────────

#[test]
fn eq_class_is_implicitly_hashable() {
    // This just validates that Eq classes work — Hash is invisible
    crate::common::check_ok(
        "\
class Point includes Eq
  x: Int
  y: Int

  def eq(other: Point) -> Bool
    true
",
    );
}
