// ─── Match expression: literal patterns ─────────────────────────────

#[test]
fn match_int_literal_patterns() {
    crate::common::check_ok(
        r#"let x = 5
let y = match x
  1 => "one"
  2 => "two"
  _ => "other"
"#,
    );
}

#[test]
fn match_bool_patterns() {
    crate::common::check_ok(
        r#"let x = true
let y = match x
  true => 1
  false => 0
"#,
    );
}

#[test]
fn match_string_patterns() {
    crate::common::check_ok(
        r#"let x = "hello"
let y = match x
  "hello" => 1
  "world" => 2
  _ => 0
"#,
    );
}

// ─── Match expression: variable binding and wildcards ───────────────

#[test]
fn match_variable_binding() {
    // Wildcard identifier captures the value
    crate::common::check_ok(
        r#"let x = 42
let y = match x
  1 => "one"
  n => "other"
"#,
    );
}

#[test]
fn match_wildcard() {
    crate::common::check_ok(
        r#"let x = 10
let y = match x
  _ => 0
"#,
    );
}

// ─── Match as expression ────────────────────────────────────────────

#[test]
fn match_expression_in_let() {
    crate::common::check_ok(
        r#"let x = 5
let result = match x
  1 => 10
  2 => 20
  _ => 0
"#,
    );
}

#[test]
fn match_nested_in_function() {
    crate::common::check_ok(
        r#"def describe(n: Int) -> String
  match n
    0 => "zero"
    1 => "one"
    _ => "many"
"#,
    );
}

// ─── Match arm count: single and many ───────────────────────────────

#[test]
fn match_single_arm() {
    crate::common::check_ok(
        r#"let x = 1
let y = match x
  _ => 0
"#,
    );
}

#[test]
fn match_many_arms() {
    crate::common::check_ok(
        r#"let x = 5
let y = match x
  1 => "a"
  2 => "b"
  3 => "c"
  4 => "d"
  _ => "e"
"#,
    );
}

// ─── Match errors ───────────────────────────────────────────────────

#[test]
fn match_arm_type_mismatch_error() {
    let err = crate::common::check_err(
        r#"let x = 1
let y = match x
  1 => "hello"
  _ => 42
"#,
    );
    assert!(
        err.contains("match")
            || err.contains("arm")
            || err.contains("mismatch")
            || err.contains("Match")
    );
}

#[test]
fn match_pattern_type_mismatch_error() {
    let err = crate::common::check_err(
        r#"let x = 1
let y = match x
  "hello" => 1
  _ => 2
"#,
    );
    assert!(
        err.contains("pattern")
            || err.contains("mismatch")
            || err.contains("Pattern")
            || err.contains("match")
    );
}

// ─── Nullable match: soundness ──────────────────────────────────────

#[test]
fn nullable_match_catchall_without_nil_arm_binds_nullable() {
    // Without a nil arm, catch-all should bind as T? (not T)
    // So v + 1 should fail because v is Int?, not Int
    let err = crate::common::check_err(
        r#"let x: Int? = nil
let y = match x
  v => v + 1
"#,
    );
    assert!(
        err.contains("mismatch") || err.contains("Nullable") || err.contains("Int?"),
        "Expected type error because v should be Int?, got: {}",
        err
    );
}

#[test]
fn nullable_match_with_nil_arm_then_catchall_unwraps() {
    // With a nil arm before the catch-all, v is safely unwrapped to T
    crate::common::check_ok(
        r#"let x: Int? = nil
let y = match x
  nil => 0
  v => v + 1
"#,
    );
}

// ─── Enum variant match patterns ────────────────────────────────────

#[test]
fn enum_variant_match_pattern_basic() {
    crate::common::check_ok(
        r#"enum Color
  Red
  Green
  Blue

let c = Color.Red
let name = match c
  Color.Red => "red"
  Color.Green => "green"
  Color.Blue => "blue"
"#,
    );
}

#[test]
fn enum_variant_match_pattern_with_wildcard() {
    crate::common::check_ok(
        r#"enum Color
  Red
  Green
  Blue

let c = Color.Red
let name = match c
  Color.Red => "red"
  _ => "other"
"#,
    );
}

#[test]
fn enum_variant_match_wrong_enum_type() {
    let err = crate::common::check_err(
        r#"enum Color
  Red
  Green
  Blue

enum Size
  Small
  Large

let c = Color.Red
let name = match c
  Size.Small => "small"
  _ => "other"
"#,
    );
    assert!(err.contains("mismatch") || err.contains("Color") || err.contains("Size"));
}

#[test]
fn enum_variant_match_unknown_variant() {
    let err = crate::common::check_err(
        r#"enum Color
  Red
  Green
  Blue

let c = Color.Red
let name = match c
  Color.Purple => "purple"
  _ => "other"
"#,
    );
    assert!(err.contains("Purple") || err.contains("variant") || err.contains("unknown"));
}

// ─── Enum exhaustiveness checking ───────────────────────────────────

#[test]
fn enum_exhaustive_match_no_wildcard_needed() {
    crate::common::check_ok(
        r#"enum Direction
  North
  South
  East
  West

let d = Direction.North
let name = match d
  Direction.North => "n"
  Direction.South => "s"
  Direction.East => "e"
  Direction.West => "w"
"#,
    );
}

#[test]
fn enum_non_exhaustive_match_error() {
    let err = crate::common::check_err(
        r#"enum Direction
  North
  South
  East
  West

let d = Direction.North
let name = match d
  Direction.North => "n"
  Direction.South => "s"
"#,
    );
    assert!(
        err.contains("exhaustive")
            || err.contains("East")
            || err.contains("West")
            || err.contains("missing")
    );
}

#[test]
fn enum_match_with_ordering_builtin() {
    crate::common::check_ok(
        r#"class Point includes Ord
  x: Int

  def cmp(other: Point) -> Ordering
    match true
      true => Ordering.Equal
      false => Ordering.Less

let p1 = Point(x: 1)
let p2 = Point(x: 2)
let result = p1.cmp(other: p2)
let name = match result
  Ordering.Less => "less"
  Ordering.Equal => "equal"
  Ordering.Greater => "greater"
"#,
    );
}

// ─── Nullable match on scrutinee ────────────────────────────────────

#[test]
fn literal_match_on_nullable_scrutinee() {
    crate::common::check_ok(
        r#"def check(x: String?) -> Int
  match x
    "hello" => 1
    nil => 0
    _ => -1
"#,
    );
}

#[test]
fn match_as_statement() {
    crate::common::check_ok(
        r#"let x = match 1
  1 => "one"
  _ => "other"
"#,
    );
}

// ─── Match arm subtype unification ──────────────────────────────────

#[test]
fn match_arms_different_subtypes_unify() {
    // Match arms returning Dog and Animal should unify to Animal
    crate::common::check_ok(
        r#"class Animal
  name: String

class Dog extends Animal
  breed: String

let x: Animal = match true
  true => Animal(name: "Cat")
  false => Dog(name: "Rex", breed: "Lab")
"#,
    );
}

#[test]
fn match_arms_subtype_of_first_arm() {
    // Second arm (subtype) should be compatible with first arm (supertype)
    crate::common::check_ok(
        r#"class Animal
  name: String

class Dog extends Animal
  breed: String

let x: Animal = match true
  true => Dog(name: "Rex", breed: "Lab")
  false => Animal(name: "Cat")
"#,
    );
}

// ─── Nullable enum exhaustiveness ───────────────────────────────────

#[test]
fn nullable_enum_exhaustive_match() {
    // Matching on Color? with nil + all variants should be exhaustive
    crate::common::check_ok(
        r#"enum Color
  Red
  Blue

def test(c: Color?) -> Int
  match c
    nil => 0
    Color.Red => 1
    Color.Blue => 2
"#,
    );
}

#[test]
fn nullable_enum_missing_variant_rejected() {
    // Matching on Color? with nil but missing a variant should be rejected
    let err = crate::common::check_err(
        r#"enum Color
  Red
  Blue

def test(c: Color?) -> Int
  match c
    nil => 0
    Color.Red => 1
"#,
    );
    assert!(
        err.contains("exhaustive") || err.contains("missing") || err.contains("wildcard"),
        "Expected non-exhaustive error, got: {}",
        err
    );
}

#[test]
fn nullable_enum_missing_nil_rejected() {
    // Matching on Color? with all variants but no nil arm should be rejected
    let err = crate::common::check_err(
        r#"enum Color
  Red
  Blue

def test(c: Color?) -> Int
  match c
    Color.Red => 1
    Color.Blue => 2
"#,
    );
    assert!(
        err.contains("exhaustive") || err.contains("nil") || err.contains("wildcard"),
        "Expected non-exhaustive error (missing nil), got: {}",
        err
    );
}

// ─── Negative literal in match pattern ──────────────────────────────

#[test]
fn negative_int_match_pattern() {
    // Negative integer literals should work in match patterns
    crate::common::check_ok(
        r#"def test(x: Int) -> String
  match x
    -1 => "negative one"
    0 => "zero"
    _ => "other"
"#,
    );
}

// ─── Enum variant destructuring ─────────────────────────────────────

#[test]
fn enum_destructuring_single_field() {
    crate::common::check_ok(
        r#"enum Wrapper
  Val(x: Int)

def unwrap(w: Wrapper) -> Int
  match w
    Wrapper.Val(x) => x
"#,
    );
}

#[test]
fn enum_destructuring_multiple_fields() {
    crate::common::check_ok(
        r#"enum Shape
  Circle(radius: Float)
  Rect(w: Float, h: Float)

def area(s: Shape) -> Float
  match s
    Shape.Circle(radius) => radius * 3.14
    Shape.Rect(w, h) => w * h
"#,
    );
}

#[test]
fn enum_destructuring_named_bindings() {
    crate::common::check_ok(
        r#"enum Pair
  Two(a: Int, b: Int)

def sum(p: Pair) -> Int
  match p
    Pair.Two(a: x, b: y) => x + y
"#,
    );
}

#[test]
fn enum_destructuring_mixed_with_fieldless() {
    crate::common::check_ok(
        r#"enum Result
  Ok(value: Int)
  Err

def get_or_default(r: Result) -> Int
  match r
    Result.Ok(value) => value
    Result.Err => 0
"#,
    );
}

#[test]
fn enum_destructuring_wrong_field_name_error() {
    let err = crate::common::check_err(
        r#"enum Wrapper
  Val(x: Int)

def unwrap(w: Wrapper) -> Int
  match w
    Wrapper.Val(y) => y
"#,
    );
    // "y" is not a field name on Val, should error
    // (positional binding uses the name as field name too)
    assert!(
        err.contains("y") || err.contains("field") || err.contains("undefined"),
        "expected field error, got: {}",
        err
    );
}

#[test]
fn enum_destructuring_too_many_bindings_error() {
    let err = crate::common::check_err(
        r#"enum Wrapper
  Val(x: Int)

def unwrap(w: Wrapper) -> Int
  match w
    Wrapper.Val(x, extra) => x
"#,
    );
    assert!(
        err.contains("field") || err.contains("binding") || err.contains("1"),
        "expected too-many-bindings error, got: {}",
        err
    );
}

// ─── Additional enum destructuring tests ───────────────────────────────

#[test]
fn enum_destructuring_binding_used_in_arm_body() {
    // Verify the bound variable can be used in the arm expression
    crate::common::check_ok(
        r#"enum Wrapper
  Val(x: Int)

def double(w: Wrapper) -> Int
  match w
    Wrapper.Val(x) => x + x
"#,
    );
}

#[test]
fn enum_destructuring_partial_fields() {
    // Only destructure some fields of a multi-field variant
    crate::common::check_ok(
        r#"enum Record
  Entry(a: Int, b: Int, c: Int)

def first(r: Record) -> Int
  match r
    Record.Entry(a) => a
"#,
    );
}

#[test]
fn enum_destructuring_on_fieldless_variant_error() {
    // Trying to destructure a fieldless variant like Color.Red(x) should error
    let err = crate::common::check_err(
        r#"enum Color
  Red
  Blue

def test(c: Color) -> Int
  match c
    Color.Red(x) => x
    Color.Blue => 0
"#,
    );
    assert!(
        err.contains("field")
            || err.contains("binding")
            || err.contains("no fields")
            || err.contains("destructure"),
        "expected error for destructuring fieldless variant, got: {}",
        err
    );
}

#[test]
fn enum_destructuring_with_wildcard_arm() {
    // Mix destructured arm with wildcard fallback
    crate::common::check_ok(
        r#"enum Result
  Ok(value: Int)
  Err
  Unknown

def get_or_zero(r: Result) -> Int
  match r
    Result.Ok(value) => value
    _ => 0
"#,
    );
}

#[test]
fn enum_destructuring_type_inferred_from_field() {
    // Verify the bound variable gets the right type from the field declaration
    crate::common::check_ok(
        r#"enum Container
  Str(value: String)

def get_len(c: Container) -> Int
  match c
    Container.Str(value) => value.len()
"#,
    );
}
