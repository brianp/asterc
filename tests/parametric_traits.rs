mod common;

// ============================================================
// Phase 1: Parametric Trait Parsing
// ============================================================

#[test]
fn parse_parametric_trait_single_param() {
    // trait From[T] with a single type parameter should parse
    common::check_ok(
        "\
trait MyFrom[T]
  def from(value: T) -> Self",
    );
}

#[test]
fn parse_parametric_trait_multiple_params() {
    // trait Convert[A, B] with multiple type parameters should parse
    common::check_ok(
        "\
trait Convert[A, B]
  def convert(value: A) -> B",
    );
}

#[test]
fn parse_includes_parametric_trait_with_type_args() {
    // class includes From[Int] — parametric trait with concrete type arg
    common::check_ok(
        "\
class Wrapper includes From[Int]
  value: Int
  def from(value: Int) -> Self
    Wrapper(value: value)",
    );
}

// ============================================================
// Phase 2: Parametric Trait Satisfaction
// ============================================================

#[test]
fn parametric_trait_satisfied_with_correct_method() {
    // Class implements the method with T substituted to the concrete type
    common::check_ok(
        "\
class Celsius includes From[Float]
  temp: Float
  def from(value: Float) -> Self
    Celsius(temp: value)",
    );
}

#[test]
fn parametric_trait_wrong_method_type_error() {
    // Method signature doesn't match trait with T=Int → error
    let err = common::check_err(
        "\
class Wrapper includes From[Int]
  value: Int
  def from(value: String) -> Self
    Wrapper(value: 0)",
    );
    assert!(
        err.contains("signature") || err.contains("mismatch") || err.contains("E014"),
        "got: {}",
        err
    );
}

#[test]
fn parametric_trait_missing_required_method() {
    // Class includes From[Int] but doesn't implement from() → error
    let err = common::check_err(
        "\
class Wrapper includes From[Int]
  value: Int",
    );
    assert!(
        err.contains("must implement") || err.contains("from") || err.contains("E014"),
        "got: {}",
        err
    );
}

#[test]
fn parametric_trait_with_self_substitution() {
    // Self is substituted to the class type in method signatures
    common::check_ok(
        "\
class Celsius includes Into[Float]
  temp: Float
  def into() -> Float
    0.0",
    );
}

#[test]
fn parametric_trait_with_generic_return_type() {
    // Trait method returns T, checked against concrete type arg
    common::check_ok(
        "\
trait Wrap[T]
  def wrap() -> List[T]
class IntWrapper includes Wrap[Int]
  value: Int
  def wrap() -> List[Int]
    [0]",
    );
}

// ============================================================
// Phase 3: Non-parametric traits still work (regression)
// ============================================================

#[test]
fn eq_still_works_after_parametric_trait_changes() {
    common::check_ok(
        "\
class Point includes Eq
  x: Int
  y: Int
  def eq(other: Point) -> Bool
    true
let a = Point(x: 1, y: 2)
let b = Point(x: 1, y: 2)
let same = a == b",
    );
}

#[test]
fn ord_still_works_after_parametric_trait_changes() {
    common::check_ok(
        "\
class Score includes Ord
  value: Int
  def cmp(other: Score) -> Ordering
    Ordering.Less",
    );
}

#[test]
fn printable_still_works_after_parametric_trait_changes() {
    common::check_ok(
        "\
class Name includes Printable
  value: String
  def to_string() -> String
    \"hello\"",
    );
}

#[test]
fn auto_derive_eq_still_works() {
    common::check_ok(
        "\
class Point includes Eq
  x: Int
  y: Int",
    );
}

// ============================================================
// Phase 4: Built-in From[T] Protocol
// ============================================================

#[test]
fn from_trait_registered_builtin() {
    // From[T] is registered as a built-in parametric trait
    common::check_ok(
        "\
class Celsius includes From[Float]
  temp: Float
  def from(value: Float) -> Self
    Celsius(temp: value)",
    );
}

#[test]
fn type_dot_from_intrinsic_call() {
    // Type.from(value: x) — compiler intrinsic for From[T]
    common::check_ok(
        "\
class Celsius includes From[Float]
  temp: Float
  def from(value: Float) -> Self
    Celsius(temp: value)
let c = Celsius.from(value: 100.0)",
    );
}

#[test]
fn type_dot_from_returns_correct_type() {
    // Celsius.from(value: 100.0) should return Celsius
    common::check_ok(
        "\
class Celsius includes From[Float]
  temp: Float
  def from(value: Float) -> Self
    Celsius(temp: value)
let c = Celsius.from(value: 100.0)
let t: Float = c.temp",
    );
}

#[test]
fn type_dot_from_wrong_arg_type_error() {
    // Celsius.from(value: "hello") should fail — From[Float] expects Float
    let err = common::check_err(
        "\
class Celsius includes From[Float]
  temp: Float
  def from(value: Float) -> Self
    Celsius(temp: value)
let c = Celsius.from(value: \"hello\")",
    );
    assert!(
        err.contains("mismatch") || err.contains("expected") || err.contains("Float"),
        "got: {}",
        err
    );
}

#[test]
fn type_dot_from_no_from_trait_error() {
    // Type.from() on a class that doesn't include From → error
    let err = common::check_err(
        "\
class Celsius
  temp: Float
let c = Celsius.from(value: 100.0)",
    );
    assert!(
        err.contains("From") || err.contains("from") || err.contains("no field or method"),
        "got: {}",
        err
    );
}

// ============================================================
// Phase 5: Built-in Into[T] Protocol
// ============================================================

#[test]
fn into_trait_registered_builtin() {
    // Into[T] is registered as a built-in parametric trait
    common::check_ok(
        "\
class Celsius includes Into[Float]
  temp: Float
  def into() -> Float
    0.0",
    );
}

#[test]
fn into_method_call() {
    // instance.into() returns the target type
    common::check_ok(
        "\
class Celsius includes Into[Float]
  temp: Float
  def into() -> Float
    0.0
let c = Celsius(temp: 100.0)
let f: Float = c.into()",
    );
}

#[test]
fn into_wrong_return_type_error() {
    // into() returns wrong type → error
    let err = common::check_err(
        "\
class Celsius includes Into[Float]
  temp: Float
  def into() -> String
    \"wrong\"",
    );
    assert!(
        err.contains("signature") || err.contains("mismatch") || err.contains("E014"),
        "got: {}",
        err
    );
}

#[test]
fn into_missing_method_error() {
    // Class includes Into[Float] but doesn't define into() → error
    let err = common::check_err(
        "\
class Celsius includes Into[Float]
  temp: Float",
    );
    assert!(
        err.contains("must implement") || err.contains("into") || err.contains("E014"),
        "got: {}",
        err
    );
}

// ============================================================
// Phase 6: From + Into together
// ============================================================

#[test]
fn class_with_both_from_and_into() {
    common::check_ok(
        "\
class Celsius includes From[Float], Into[Float]
  temp: Float
  def from(value: Float) -> Self
    Celsius(temp: value)
  def into() -> Float
    0.0",
    );
}

#[test]
fn from_and_into_with_different_types() {
    common::check_ok(
        "\
class UserId includes From[Int], Into[String]
  id: Int
  def from(value: Int) -> Self
    UserId(id: value)
  def into() -> String
    \"user\"",
    );
}

// ============================================================
// Phase 7: Parametric trait + non-parametric trait combined
// ============================================================

#[test]
fn class_includes_eq_and_from() {
    common::check_ok(
        "\
class UserId includes Eq, From[Int]
  id: Int
  def eq(other: UserId) -> Bool
    true
  def from(value: Int) -> Self
    UserId(id: value)",
    );
}

#[test]
fn class_includes_eq_auto_derive_and_from() {
    // Eq auto-derived + From[Int] manual
    common::check_ok(
        "\
class UserId includes Eq, From[Int]
  id: Int
  def from(value: Int) -> Self
    UserId(id: value)",
    );
}

// ============================================================
// Phase 8: From[T] fallible conversion (throws)
// ============================================================

#[test]
fn from_with_throws() {
    common::check_ok(
        "\
class ParseError extends Error
  code: Int
class Port includes From[Int]
  value: Int
  def from(value: Int) throws ParseError -> Self
    Port(value: value)",
    );
}

// ============================================================
// Phase 9: Edge cases
// ============================================================

#[test]
fn unknown_parametric_trait_error() {
    let err = common::check_err(
        "\
class Foo includes Unknown[Int]
  x: Int",
    );
    assert!(
        err.contains("Unknown") || err.contains("E014"),
        "got: {}",
        err
    );
}

#[test]
fn parametric_trait_wrong_arity_error() {
    // Trait has 1 type param, includes provides 2 → error
    let err = common::check_err(
        "\
trait MyTrait[T]
  def do_it(value: T) -> Self
class Foo includes MyTrait[Int, String]
  x: Int
  def do_it(value: Int) -> Self
    Foo(x: value)",
    );
    assert!(
        err.contains("expects") || err.contains("type parameter"),
        "got: {}",
        err
    );
}

#[test]
fn user_defined_parametric_trait_with_map() {
    // User defines their own parametric trait
    common::check_ok(
        "\
trait Mapper[T]
  def map_to() -> T
class IntToString includes Mapper[String]
  value: Int
  def map_to() -> String
    \"mapped\"",
    );
}

// ============================================================
// Phase 10: Multiple parametric trait inclusions
// ============================================================

#[test]
fn multiple_into_inclusions_different_types() {
    // A class can include Into[T] multiple times with different T
    common::check_ok(
        "\
class Fahrenheit
  value: Float
class Kelvin
  value: Float
class Celsius includes Into[Fahrenheit], Into[Kelvin]
  value: Float
  def into() -> Fahrenheit
    Fahrenheit(value: value * 9.0 / 5.0 + 32.0)
  def into() -> Kelvin
    Kelvin(value: value + 273.15)",
    );
}

#[test]
fn multiple_from_inclusions_different_types() {
    // A class can include From[T] multiple times with different T
    common::check_ok(
        "\
class UserId includes From[Int], From[String]
  id: Int
  def from(value: Int) -> Self
    UserId(id: value)
  def from(value: String) -> Self
    UserId(id: 0)",
    );
}

// ============================================================
// Phase 11: Expected-type resolution for .into()
// ============================================================

#[test]
fn into_resolved_by_let_annotation() {
    // let x: Fahrenheit = celsius.into() — compiler picks Into[Fahrenheit]
    common::check_ok(
        "\
class Fahrenheit
  value: Float
class Kelvin
  value: Float
class Celsius includes Into[Fahrenheit], Into[Kelvin]
  value: Float
  def into() -> Fahrenheit
    Fahrenheit(value: value * 9.0 / 5.0 + 32.0)
  def into() -> Kelvin
    Kelvin(value: value + 273.15)
let c = Celsius(value: 100.0)
let f: Fahrenheit = c.into()",
    );
}

#[test]
fn into_resolved_by_function_arg_type() {
    // take_fahrenheit(temp: celsius.into()) — compiler picks Into[Fahrenheit]
    common::check_ok(
        "\
class Fahrenheit
  value: Float
class Kelvin
  value: Float
class Celsius includes Into[Fahrenheit], Into[Kelvin]
  value: Float
  def into() -> Fahrenheit
    Fahrenheit(value: value * 9.0 / 5.0 + 32.0)
  def into() -> Kelvin
    Kelvin(value: value + 273.15)
def take_fahrenheit(temp: Fahrenheit) -> Bool
  true
let c = Celsius(value: 100.0)
let result = take_fahrenheit(temp: c.into())",
    );
}

#[test]
fn into_ambiguous_error_no_expected_type() {
    // c.into() with no type context — ambiguous, should error
    let err = common::check_err(
        "\
class Fahrenheit
  value: Float
class Kelvin
  value: Float
class Celsius includes Into[Fahrenheit], Into[Kelvin]
  value: Float
  def into() -> Fahrenheit
    Fahrenheit(value: value * 9.0 / 5.0 + 32.0)
  def into() -> Kelvin
    Kelvin(value: value + 273.15)
let c = Celsius(value: 100.0)
let x = c.into()",
    );
    assert!(
        err.contains("ambiguous") || err.contains("type annotation"),
        "expected ambiguity error, got: {}",
        err
    );
}

#[test]
fn into_single_inclusion_no_annotation_needed() {
    // Single Into[T] — no annotation needed, unambiguous
    common::check_ok(
        "\
class Fahrenheit
  value: Float
class Celsius includes Into[Fahrenheit]
  value: Float
  def into() -> Fahrenheit
    Fahrenheit(value: value * 9.0 / 5.0 + 32.0)
let c = Celsius(value: 100.0)
let f = c.into()",
    );
}

// ============================================================
// Phase 12: From/Into auto-reverse
// ============================================================

#[test]
fn from_implies_into_with_expected_type() {
    // User includes From[PgRow], so pg_row.into() works where User is expected
    common::check_ok(
        "\
class PgRow
  data: String
class User includes From[PgRow]
  name: String
  def from(value: PgRow) -> Self
    User(name: value.data)
let row = PgRow(data: \"alice\")
let user: User = row.into()",
    );
}

#[test]
fn from_implies_into_in_function_arg() {
    // From[PgRow] auto-reverse: pg_row.into() resolves in function arg context
    common::check_ok(
        "\
class PgRow
  data: String
class User includes From[PgRow]
  name: String
  def from(value: PgRow) -> Self
    User(name: value.data)
def greet(user: User) -> String
  \"hello\"
let row = PgRow(data: \"alice\")
let msg = greet(user: row.into())",
    );
}

#[test]
fn from_implies_into_multiple_from() {
    // Multiple From[T] on target: into() resolves via expected type
    common::check_ok(
        "\
class PgRow
  data: String
class CsvRow
  data: String
class User includes From[PgRow], From[CsvRow]
  name: String
  def from(value: PgRow) -> Self
    User(name: value.data)
  def from(value: CsvRow) -> Self
    User(name: value.data)
let row = PgRow(data: \"alice\")
let user: User = row.into()",
    );
}

// ============================================================
// Phase 13: Expected-type resolution for Type.from() with multiple From
// ============================================================

#[test]
fn multiple_from_type_dot_from_selects_by_arg_type() {
    // User.from(value: pg_row) — selects From[PgRow] by argument type
    common::check_ok(
        "\
class PgRow
  data: String
class CsvRow
  data: String
class User includes From[PgRow], From[CsvRow]
  name: String
  def from(value: PgRow) -> Self
    User(name: value.data)
  def from(value: CsvRow) -> Self
    User(name: value.data)
let row = PgRow(data: \"alice\")
let user = User.from(value: row)",
    );
}
