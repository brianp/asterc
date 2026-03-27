// ─── BC-5: Inline generic syntax for functions ──────────────────────
//
// New syntax: def identity(x: T) -> T
// Old syntax: def identity[T](x: T) -> T  (no longer valid)
//
// Classes/traits still use bracket syntax: class Box[T], trait Foo[T]

// ─── Basic inline generics ──────────────────────────────────────────

#[test]
fn inline_generic_identity() {
    crate::common::check_ok("def identity(x: T) -> T\n  x\n");
}

#[test]
fn inline_generic_identity_call_int() {
    crate::common::check_ok("def identity(x: T) -> T\n  x\nlet y = identity(x: 42)\n");
}

#[test]
fn inline_generic_identity_call_string() {
    crate::common::check_ok("def identity(x: T) -> T\n  x\nlet y = identity(x: \"hello\")\n");
}

#[test]
fn inline_generic_multi_params() {
    crate::common::check_ok("def first(a: A, b: B) -> A\n  a\nlet y = first(a: 1, b: \"hello\")\n");
}

#[test]
fn inline_generic_second_param() {
    crate::common::check_ok(
        "def second(a: A, b: B) -> B\n  b\nlet y = second(a: 1, b: \"hello\")\n",
    );
}

// ─── Generics with container types ──────────────────────────────────

#[test]
fn inline_generic_list_param() {
    // Just check that List[T] with inline T parses and typechecks the signature
    crate::common::check_ok("def wrap(x: T) -> List[T]\n  [x]\n");
}

// ─── Function with known class param (not a type variable) ──────────

#[test]
fn function_with_class_param() {
    crate::common::check_ok(
        r#"class User
  name: String

def greet(u: User) -> String
  "hello"
"#,
    );
}

#[test]
fn function_with_class_param_call() {
    crate::common::check_ok(
        r#"class User
  name: String

def greet(u: User) -> String
  "hello"

let u = User(name: "Alice")
let g = greet(u: u)
"#,
    );
}

// ─── Old bracket syntax on functions is now a parse error ───────────

#[test]
fn bracket_generic_on_function_is_error() {
    crate::common::check_parse_err("def identity[T](x: T) -> T\n  x\n");
}

// ─── Classes still use bracket syntax ───────────────────────────────

#[test]
fn class_still_uses_brackets() {
    crate::common::check_ok("class Box[T]\n  value: Int\n");
}

#[test]
fn class_method_with_class_type_param() {
    crate::common::check_ok(
        r#"class Box[T]
  value: Int
  def get() -> Int
    42
"#,
    );
}

// ─── Mixed: class type params + method inline generics ──────────────
// Note: This test is aspirational — method-level type params on generic
// classes are complex. Deferring if needed.
