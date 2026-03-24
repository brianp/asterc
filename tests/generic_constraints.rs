mod common;

// ─── Generic Constraints: T extends Class, T includes Trait ──────────
//
// Syntax:
//   def clone(item: T extends Vehicle) -> T
//   def process(item: T includes Eq) -> T
//   def convert(item: T extends Animal includes Printable) -> T
//
// Constraints are validated at call sites when TypeVars are bound.

// ─── Basic extends constraint ────────────────────────────────────────

#[test]
fn extends_constraint_basic() {
    common::check_ok(
        r#"class Animal
  name: String

class Dog extends Animal
  breed: String

def clone_animal(item: T extends Animal) -> T
  item

let d = Dog(name: "Rex", breed: "Lab")
let d2 = clone_animal(item: d)
"#,
    );
}

#[test]
fn extends_constraint_exact_type() {
    // The base class itself satisfies extends
    common::check_ok(
        r#"class Animal
  name: String

def clone_animal(item: T extends Animal) -> T
  item

let a = Animal(name: "Rex")
let a2 = clone_animal(item: a)
"#,
    );
}

#[test]
fn extends_constraint_violation() {
    // String is not a subclass of Animal
    let err = common::check_err(
        r#"class Animal
  name: String

def clone_animal(item: T extends Animal) -> T
  item

let x = clone_animal(item: "hello")
"#,
    );
    assert!(
        err.contains("does not satisfy constraint") || err.contains("extends"),
        "Expected constraint violation error, got: {}",
        err
    );
}

#[test]
fn extends_constraint_unrelated_class() {
    // Vehicle is not a subclass of Animal
    let err = common::check_err(
        r#"class Animal
  name: String

class Vehicle
  speed: Int

def clone_animal(item: T extends Animal) -> T
  item

let v = Vehicle(speed: 100)
let x = clone_animal(item: v)
"#,
    );
    assert!(
        err.contains("does not satisfy constraint") || err.contains("extends"),
        "Expected constraint violation error, got: {}",
        err
    );
}

#[test]
fn extends_constraint_deep_hierarchy() {
    // Grandchild also satisfies extends
    common::check_ok(
        r#"class Animal
  name: String

class Dog extends Animal
  breed: String

class Puppy extends Dog
  is_tiny: Bool

def clone_animal(item: T extends Animal) -> T
  item

let p = Puppy(name: "Tiny", breed: "Chihuahua", is_tiny: true)
let p2 = clone_animal(item: p)
"#,
    );
}

// ─── Basic includes constraint ───────────────────────────────────────

#[test]
fn includes_constraint_basic() {
    common::check_ok(
        r#"class Point includes Eq
  x: Int
  y: Int

def are_equal(a: T includes Eq, b: T) -> Bool
  a == b

let p1 = Point(x: 1, y: 2)
let p2 = Point(x: 1, y: 2)
let eq = are_equal(a: p1, b: p2)
"#,
    );
}

#[test]
fn includes_constraint_violation() {
    // Point does not include Eq, so passing it violates the constraint
    let err = common::check_err(
        r#"class Point
  x: Int
  y: Int

def are_equal(a: T includes Eq, b: T) -> Bool
  true

let p1 = Point(x: 1, y: 2)
let p2 = Point(x: 1, y: 2)
let eq = are_equal(a: p1, b: p2)
"#,
    );
    assert!(
        err.contains("does not satisfy constraint") || err.contains("includes"),
        "Expected constraint violation error, got: {}",
        err
    );
}

#[test]
fn includes_constraint_with_primitives() {
    // Int includes Eq (primitives always satisfy Eq)
    common::check_ok(
        r#"def are_equal(a: T includes Eq, b: T) -> Bool
  true

let eq = are_equal(a: 1, b: 2)
"#,
    );
}

#[test]
fn includes_constraint_ord() {
    common::check_ok(
        r#"class Score includes Ord
  value: Int

def pick_max(a: T includes Ord, b: T) -> T
  if a > b
    a
  else
    b

let s1 = Score(value: 10)
let s2 = Score(value: 20)
let best = pick_max(a: s1, b: s2)
"#,
    );
}

#[test]
fn includes_constraint_printable() {
    common::check_ok(
        r#"class Name includes Printable
  value: String

def show(item: T includes Printable) -> String
  item.to_string()

let n = Name(value: "Alice")
let s = show(item: n)
"#,
    );
}

// ─── Combined extends + includes ─────────────────────────────────────

#[test]
fn extends_and_includes_combined() {
    common::check_ok(
        r#"class Animal
  name: String

class Dog extends Animal includes Eq
  breed: String

def process(item: T extends Animal includes Eq) -> T
  item

let d = Dog(name: "Rex", breed: "Lab")
let d2 = process(item: d)
"#,
    );
}

#[test]
fn combined_constraint_missing_includes() {
    // Dog extends Animal but does not include Eq
    let err = common::check_err(
        r#"class Animal
  name: String

class Dog extends Animal
  breed: String

def process(item: T extends Animal includes Eq) -> T
  item

let d = Dog(name: "Rex", breed: "Lab")
let d2 = process(item: d)
"#,
    );
    assert!(
        err.contains("does not satisfy constraint") || err.contains("includes"),
        "Expected constraint violation error, got: {}",
        err
    );
}

#[test]
fn combined_constraint_missing_extends() {
    // Cat includes Eq but does not extend Animal
    let err = common::check_err(
        r#"class Animal
  name: String

class Cat includes Eq
  name: String

def process(item: T extends Animal includes Eq) -> T
  item

let c = Cat(name: "Whiskers")
let c2 = process(item: c)
"#,
    );
    assert!(
        err.contains("does not satisfy constraint") || err.contains("extends"),
        "Expected constraint violation error, got: {}",
        err
    );
}

// ─── Multiple type parameters with different constraints ─────────────

#[test]
fn multiple_constrained_params() {
    common::check_ok(
        r#"class Animal
  name: String

class Dog extends Animal includes Eq
  breed: String

def compare(a: T extends Animal, b: U includes Eq) -> Bool
  true

let d = Dog(name: "Rex", breed: "Lab")
let p = Dog(name: "Fido", breed: "Poodle")
let result = compare(a: d, b: p)
"#,
    );
}

#[test]
fn mixed_constrained_and_unconstrained() {
    // T is constrained, U is unconstrained
    common::check_ok(
        r#"class Animal
  name: String

class Dog extends Animal
  breed: String

def pair(a: T extends Animal, b: U) -> Bool
  true

let d = Dog(name: "Rex", breed: "Lab")
let result = pair(a: d, b: 42)
"#,
    );
}

// ─── Constraint preserves type identity ──────────────────────────────

#[test]
fn constraint_preserves_return_type() {
    // Return type should be Dog, not Animal
    common::check_ok(
        r#"class Animal
  name: String

class Dog extends Animal
  breed: String
  def bark() -> String
    "woof"

def identity(item: T extends Animal) -> T
  item

let d = Dog(name: "Rex", breed: "Lab")
let d2 = identity(item: d)
let sound = d2.bark()
"#,
    );
}

// ─── Error cases ─────────────────────────────────────────────────────

#[test]
fn constraint_unknown_class() {
    // FooBar doesn't exist
    let err = common::check_err(
        r#"def process(item: T extends FooBar) -> T
  item
"#,
    );
    assert!(
        err.contains("Unknown") || err.contains("unknown") || err.contains("not found"),
        "Expected unknown type error, got: {}",
        err
    );
}

#[test]
fn constraint_unknown_trait() {
    let err = common::check_err(
        r#"def process(item: T includes FooBar) -> T
  item
"#,
    );
    assert!(
        err.contains("Unknown") || err.contains("unknown") || err.contains("not found"),
        "Expected unknown trait error, got: {}",
        err
    );
}

// ─── Constraint on second occurrence uses first's constraint ─────────

#[test]
fn second_use_of_constrained_typevar() {
    // T appears twice — constraint is on first occurrence only
    common::check_ok(
        r#"class Animal
  name: String

class Dog extends Animal
  breed: String

def swap(a: T extends Animal, b: T) -> T
  b

let d1 = Dog(name: "Rex", breed: "Lab")
let d2 = Dog(name: "Fido", breed: "Poodle")
let result = swap(a: d1, b: d2)
"#,
    );
}

// ─── Constraint with parametric traits ───────────────────────────────

#[test]
fn includes_constraint_parametric_trait() {
    common::check_ok(
        r#"trait From[T]
  def from(value: T) -> Self

class Celsius includes From[Float]
  temp: Float
  def from(value: Float) -> Self
    Celsius(temp: value)

def convert(item: T includes From[Float]) -> T
  item

let c = Celsius(temp: 100.0)
let c2 = convert(item: c)
"#,
    );
}

// ─── Existing unconstrained generics still work ──────────────────────

#[test]
fn unconstrained_generic_still_works() {
    common::check_ok("def identity(x: T) -> T\n  x\nlet y = identity(x: 42)\n");
}

#[test]
fn unconstrained_multi_param_still_works() {
    common::check_ok("def first(a: A, b: B) -> A\n  a\nlet y = first(a: 1, b: \"hello\")\n");
}

// ─── Nullable typevar substitution in generics ──────────────────────

#[test]
fn nullable_typevar_substitution_in_generic() {
    // A generic function returning T? should produce Int? when called with Int
    common::check_ok(
        r#"class Box[T]
  value: T

def maybe_first(xs: List[T]) -> T?
  nil

let items = [1, 2, 3]
let result: Int? = maybe_first(xs: items)
"#,
    );
}

// ─── Container invariance ───────────────────────────────────────────

#[test]
fn generic_container_invariance_rejected() {
    let err = common::check_err(
        r#"class Animal
  tag: String

class Dog extends Animal
  breed: String

class Box[T]
  value: T

def take_box(b: Box[Animal]) -> String
  b.value.tag

let dog_box = Box[Dog](value: Dog(tag: "a", breed: "lab"))
take_box(b: dog_box)
"#,
    );
    assert!(
        err.contains("E001") || err.contains("type mismatch") || err.contains("expected"),
        "Generic containers should be invariant, got: {err}"
    );
}

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
fn list_type_satisfies_eq_constraint() {
    // List[Int] should satisfy T includes Eq constraint since Int includes Eq
    common::check_ok(
        r#"def needs_eq(a: T includes Eq, b: T) -> Bool
  a == b

needs_eq(a: [1, 2], b: [1, 2])
"#,
    );
}
