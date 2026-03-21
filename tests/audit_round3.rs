mod common;

// ============================================================
// Audit Round 3: Language Soundness Fixes
// ============================================================

// --- C2: Return type should accept subtypes ---

#[test]
fn return_subtype_accepted() {
    // Returning a Dog from a function declared -> Animal should work
    common::check_ok(
        r#"class Animal
  name: String

class Dog extends Animal
  breed: String

def test() -> Animal
  return Dog(name: "Rex", breed: "Lab")
"#,
    );
}

#[test]
fn implicit_return_subtype_accepted() {
    // Implicit return (last expression) should also accept subtypes
    common::check_ok(
        r#"class Animal
  name: String

class Dog extends Animal
  breed: String

def test() -> Animal
  Dog(name: "Rex", breed: "Lab")
"#,
    );
}

#[test]
fn return_unrelated_type_rejected() {
    // Returning an unrelated type should still be an error
    let err = common::check_err(
        r#"class Animal
  name: String

class Car
  model: String

def test() -> Animal
  return Car(model: "Tesla")
"#,
    );
    assert!(
        err.contains("mismatch") || err.contains("expected"),
        "Expected type mismatch error, got: {}",
        err
    );
}

// --- C3: Match arm types should accept subtypes ---

#[test]
fn match_arms_different_subtypes_unify() {
    // Match arms returning Dog and Animal should unify to Animal
    common::check_ok(
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
    common::check_ok(
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

// --- H1: Nullable enum exhaustiveness ---

#[test]
fn nullable_enum_exhaustive_match() {
    // Matching on Color? with nil + all variants should be exhaustive
    common::check_ok(
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
    let err = common::check_err(
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
    let err = common::check_err(
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

// --- D1: type_includes_trait should handle List types ---

#[test]
fn list_type_satisfies_eq_constraint() {
    // List[Int] should satisfy T includes Eq constraint since Int includes Eq
    common::check_ok(
        r#"def needs_eq(a: T includes Eq, b: T) -> Bool
  a == b

needs_eq(a: [1, 2], b: [1, 2])
"#,
    );
}

// --- M1-Rust: FsResolver path sanitization ---
// (This is tested via the module loader, not direct file access)

#[test]
fn path_traversal_in_module_rejected() {
    // A module path with ".." should be rejected at the parser level.
    // The parser only accepts identifiers as path segments, so dots are rejected.
    let err = common::check_parse_err(
        r#"use ..secret
"#,
    );
    assert!(
        !err.is_empty(),
        "Path traversal should be rejected at parse time"
    );
}

// --- C1: List invariance ---

#[test]
fn list_invariant_rejects_subtype() {
    // List[Dog] should NOT be accepted where List[Animal] is expected
    let err = common::check_err(
        r#"class Animal
  name: String

class Dog extends Animal
  breed: String

def add_animal(animals: List[Animal]) -> Void
  let x = 1

let dogs = [Dog(name: "Rex", breed: "Lab")]
add_animal(animals: dogs)
"#,
    );
    assert!(
        err.contains("mismatch") || err.contains("expects"),
        "List[Dog] should not match List[Animal], got: {}",
        err
    );
}

#[test]
fn list_exact_type_accepted() {
    // List[Animal] should match List[Animal]
    common::check_ok(
        r#"class Animal
  name: String

def count_animals(animals: List[Animal]) -> Int
  animals.count()

let animals = [Animal(name: "Cat"), Animal(name: "Dog")]
count_animals(animals: animals)
"#,
    );
}

#[test]
fn generic_extends_constraint_accepts_subtype() {
    // Passing Dog directly (not in List) to a constrained generic still works
    common::check_ok(
        r#"class Animal
  name: String

class Dog extends Animal
  breed: String

def get_name(a: T extends Animal) -> String
  a.name

get_name(a: Dog(name: "Rex", breed: "Lab"))
"#,
    );
}

#[test]
fn direct_subtype_passing_accepted() {
    // Direct subtype passing should still work (not in a container)
    common::check_ok(
        r#"class Animal
  name: String

class Dog extends Animal
  breed: String

def greet(a: Animal) -> String
  a.name

greet(a: Dog(name: "Rex", breed: "Lab"))
"#,
    );
}

// --- Negative match patterns ---

#[test]
fn negative_int_match_pattern() {
    // Negative integer literals should work in match patterns
    common::check_ok(
        r#"def test(x: Int) -> String
  match x
    -1 => "negative one"
    0 => "zero"
    _ => "other"
"#,
    );
}
