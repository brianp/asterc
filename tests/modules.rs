mod common;

use std::collections::HashMap;

// ===========================================================
// Phase M1: Module system tests — TDD specification
// ===========================================================
// These tests define the contract for the module system.
// All must FAIL before implementation, then PASS after.

// --- Contract tests: basic import mechanics ---

#[test]
fn import_pub_class_from_module() {
    let mut files = HashMap::new();
    files.insert(
        "models/user".to_string(),
        "pub class User\n  name: String\n  age: Int\n".to_string(),
    );
    common::check_ok_with_files(
        "use models/user { User }\nlet u = User(name: \"Jo\", age: 1)\n",
        files,
    );
}

#[test]
fn import_pub_function_from_module() {
    let mut files = HashMap::new();
    files.insert(
        "utils".to_string(),
        "pub def double(n: Int) -> Int\n  n * 2\n".to_string(),
    );
    common::check_ok_with_files("use utils { double }\nlet x = double(n: 4)\n", files);
}

#[test]
fn import_pub_trait_from_module() {
    let mut files = HashMap::new();
    files.insert(
        "traits/greetable".to_string(),
        "pub trait Greetable\n  def greet() -> String\n".to_string(),
    );
    common::check_ok_with_files(
        "use traits/greetable { Greetable }\n\
         class Hello includes Greetable\n  def greet() -> String\n    \"hi\"\n",
        files,
    );
}

#[test]
fn import_pub_enum_from_module() {
    let mut files = HashMap::new();
    files.insert(
        "types/color".to_string(),
        "pub enum Color\n  Red\n  Green\n  Blue\n".to_string(),
    );
    common::check_ok_with_files("use types/color { Color }\nlet c = Color.Red\n", files);
}

#[test]
fn import_pub_let_binding_from_module() {
    let mut files = HashMap::new();
    files.insert("config".to_string(), "pub let VERSION = 42\n".to_string());
    common::check_ok_with_files("use config { VERSION }\nlet v: Int = VERSION\n", files);
}

// --- Selective import tests ---

#[test]
fn selective_import_only_imports_named_items() {
    let mut files = HashMap::new();
    files.insert(
        "stuff".to_string(),
        "pub class Foo\n  x: Int\npub class Bar\n  y: Int\n".to_string(),
    );
    // Import only Foo, Bar should NOT be visible
    let err = common::check_err_with_files("use stuff { Foo }\nlet b = Bar(y: 1)\n", files);
    assert!(err.contains("Bar"), "should report Bar as unknown: {}", err);
}

#[test]
fn wildcard_import_imports_all_pub_names() {
    let mut files = HashMap::new();
    files.insert(
        "stuff".to_string(),
        "pub class Foo\n  x: Int\npub class Bar\n  y: Int\n".to_string(),
    );
    // use without { } imports everything public
    common::check_ok_with_files("use stuff\nlet f = Foo(x: 1)\nlet b = Bar(y: 2)\n", files);
}

// --- Visibility enforcement ---

#[test]
fn private_class_not_importable() {
    let mut files = HashMap::new();
    files.insert(
        "secret".to_string(),
        "class Hidden\n  x: Int\npub class Visible\n  y: Int\n".to_string(),
    );
    let err = common::check_err_with_files("use secret { Hidden }\n", files);
    assert!(
        err.contains("M002") || err.contains("not exported"),
        "should be not-exported error: {}",
        err
    );
}

#[test]
fn private_function_not_importable() {
    let mut files = HashMap::new();
    files.insert(
        "secret".to_string(),
        "def internal() -> Int\n  42\npub def public_fn() -> Int\n  1\n".to_string(),
    );
    let err = common::check_err_with_files("use secret { internal }\n", files);
    assert!(
        err.contains("M002") || err.contains("not exported"),
        "should be not-exported error: {}",
        err
    );
}

#[test]
fn wildcard_import_skips_private_items() {
    let mut files = HashMap::new();
    files.insert(
        "mixed".to_string(),
        "class Private\n  x: Int\npub class Public\n  y: Int\n".to_string(),
    );
    // Wildcard should import Public but not Private
    let err = common::check_err_with_files("use mixed\nlet p = Private(x: 1)\n", files);
    assert!(
        err.contains("Private"),
        "Private should not be visible: {}",
        err
    );
}

// --- Error cases ---

#[test]
fn module_not_found() {
    let files = HashMap::new();
    let err = common::check_err_with_files("use nonexistent/module { Foo }\n", files);
    assert!(
        err.contains("M001") || err.contains("not found"),
        "should be module-not-found error: {}",
        err
    );
}

#[test]
fn name_not_in_module() {
    let mut files = HashMap::new();
    files.insert("stuff".to_string(), "pub class Foo\n  x: Int\n".to_string());
    let err = common::check_err_with_files("use stuff { DoesNotExist }\n", files);
    assert!(
        err.contains("M002") || err.contains("not exported"),
        "should report name not found: {}",
        err
    );
}

#[test]
fn circular_import_detected() {
    let mut files = HashMap::new();
    // Module a imports b, module b imports a
    files.insert(
        "a".to_string(),
        "use b { Y }\npub class X\n  val: Int\n".to_string(),
    );
    files.insert(
        "b".to_string(),
        "use a { X }\npub class Y\n  val: Int\n".to_string(),
    );
    let err = common::check_err_with_files("use a { X }\nlet x = X(val: 1)\n", files);
    assert!(
        err.contains("M003") || err.contains("ircular"),
        "should detect circular import: {}",
        err
    );
}

// --- Transitive / diamond imports ---

#[test]
fn transitive_import_works() {
    let mut files = HashMap::new();
    files.insert(
        "base".to_string(),
        "pub class Base\n  id: Int\n".to_string(),
    );
    files.insert(
        "mid".to_string(),
        "use base { Base }\npub class Mid\n  b: Base\n".to_string(),
    );
    // main imports mid (which internally imports base).
    // Main can use Mid but NOT Base directly (no re-export).
    common::check_ok_with_files(
        "use mid { Mid }\nuse base { Base }\nlet m = Mid(b: Base(id: 1))\n",
        files,
    );
}

#[test]
fn diamond_import_compiles_module_once() {
    let mut files = HashMap::new();
    files.insert(
        "shared".to_string(),
        "pub class Shared\n  val: Int\n".to_string(),
    );
    files.insert(
        "left".to_string(),
        "use shared { Shared }\npub class Left\n  s: Shared\n".to_string(),
    );
    files.insert(
        "right".to_string(),
        "use shared { Shared }\npub class Right\n  s: Shared\n".to_string(),
    );
    // Both left and right import shared — should compile fine
    common::check_ok_with_files(
        "use left { Left }\nuse right { Right }\nuse shared { Shared }\n\
         let s = Shared(val: 1)\nlet l = Left(s: s)\nlet r = Right(s: s)\n",
        files,
    );
}

// --- Inline generics with imported types ---

#[test]
fn inline_generics_see_imported_types() {
    let mut files = HashMap::new();
    files.insert(
        "types/point".to_string(),
        "pub class Point\n  x: Int\n  y: Int\n".to_string(),
    );
    // The inline generics heuristic must recognize Point as a known type
    // so T is inferred as a generic, but Point is NOT treated as generic
    common::check_ok_with_files(
        "use types/point { Point }\n\
         def identity(p: Point) -> Point\n  p\n\
         let pt = Point(x: 1, y: 2)\nlet pt2 = identity(p: pt)\n",
        files,
    );
}

// --- Protocol metadata transfers ---

#[test]
fn imported_class_with_eq_supports_equality() {
    let mut files = HashMap::new();
    files.insert(
        "types/token".to_string(),
        "use std/cmp { Eq }\npub class Token includes Eq\n  val: Int\n".to_string(),
    );
    common::check_ok_with_files(
        "use types/token { Token }\n\
         let a = Token(val: 1)\nlet b = Token(val: 2)\nlet eq = a == b\n",
        files,
    );
}

// --- Multiple imports from same module ---

#[test]
fn multiple_selective_imports_from_same_module() {
    let mut files = HashMap::new();
    files.insert(
        "shapes".to_string(),
        "pub class Circle\n  r: Int\npub class Square\n  s: Int\npub class Triangle\n  b: Int\n"
            .to_string(),
    );
    common::check_ok_with_files(
        "use shapes { Circle, Square }\nlet c = Circle(r: 5)\nlet s = Square(s: 3)\n",
        files,
    );
}

// --- Import a throwing function ---

#[test]
fn import_throwing_function() {
    let mut files = HashMap::new();
    files.insert(
        "risky".to_string(),
        "\
pub class MyError extends Error
  code: Int

pub def risky_op(x: Int) throws MyError -> Int
  if x < 0
    throw MyError(message: \"negative\", code: 1)
  x
"
        .to_string(),
    );
    common::check_ok_with_files(
        "use risky { MyError, risky_op }\n\
         def wrapper(x: Int) -> Int\n  risky_op(x: x)!.or(0)\n",
        files,
    );
}

// ===========================================================
// Phase M2: Namespace imports — TDD specification
// ===========================================================

// --- Namespace import: class access via alias ---

#[test]
fn namespace_import_construct_class() {
    let mut files = HashMap::new();
    files.insert(
        "models/user".to_string(),
        "pub class User\n  name: String\n  age: Int\n".to_string(),
    );
    common::check_ok_with_files(
        "use models/user as u\nlet person = u.User(name: \"Jo\", age: 25)\n",
        files,
    );
}

#[test]
fn namespace_import_call_function() {
    let mut files = HashMap::new();
    files.insert(
        "utils".to_string(),
        "pub def double(n: Int) -> Int\n  n * 2\n".to_string(),
    );
    common::check_ok_with_files("use utils as u\nlet x = u.double(n: 5)\n", files);
}

#[test]
fn namespace_import_access_variable() {
    let mut files = HashMap::new();
    files.insert("config".to_string(), "pub let VERSION = 42\n".to_string());
    common::check_ok_with_files("use config as cfg\nlet v: Int = cfg.VERSION\n", files);
}

#[test]
fn namespace_import_class_field_access() {
    let mut files = HashMap::new();
    files.insert(
        "models/counter".to_string(),
        "pub class Counter\n  value: Int\n".to_string(),
    );
    common::check_ok_with_files(
        "use models/counter as m\n\
         let c = m.Counter(value: 42)\nlet v: Int = c.value\n",
        files,
    );
}

#[test]
fn namespace_import_multiple_members() {
    let mut files = HashMap::new();
    files.insert(
        "shapes".to_string(),
        "pub class Circle\n  r: Int\npub def area(r: Int) -> Int\n  r * r\n".to_string(),
    );
    common::check_ok_with_files(
        "use shapes as s\nlet c = s.Circle(r: 5)\nlet a = s.area(r: 3)\n",
        files,
    );
}

// --- Namespace import: default alias from last path segment ---

#[test]
fn namespace_import_without_alias_uses_last_segment() {
    let mut files = HashMap::new();
    files.insert(
        "models/user".to_string(),
        "pub class User\n  name: String\n".to_string(),
    );
    // `use models/user as user` — alias is explicitly "user"
    common::check_ok_with_files(
        "use models/user as user\nlet u = user.User(name: \"Jo\")\n",
        files,
    );
}

// --- Namespace import: error cases ---

#[test]
fn namespace_member_not_exported() {
    let mut files = HashMap::new();
    files.insert("stuff".to_string(), "pub class Foo\n  x: Int\n".to_string());
    let err = common::check_err_with_files("use stuff as s\nlet b = s.Bar(x: 1)\n", files);
    assert!(
        err.contains("M004") || err.contains("not found in namespace") || err.contains("Bar"),
        "should report member not found in namespace: {}",
        err
    );
}

#[test]
fn namespace_private_member_not_accessible() {
    let mut files = HashMap::new();
    files.insert(
        "mixed".to_string(),
        "class Hidden\n  x: Int\npub class Visible\n  y: Int\n".to_string(),
    );
    let err = common::check_err_with_files("use mixed as m\nlet h = m.Hidden(x: 1)\n", files);
    assert!(
        err.contains("M004") || err.contains("not found in namespace") || err.contains("Hidden"),
        "private items should not be accessible via namespace: {}",
        err
    );
}

#[test]
fn selective_with_alias_is_error() {
    let mut files = HashMap::new();
    files.insert("stuff".to_string(), "pub class Foo\n  x: Int\n".to_string());
    let err = common::check_err_with_files("use stuff { Foo } as s\n", files);
    assert!(
        err.contains("Cannot combine") || err.contains("P001") || err.contains("alias"),
        "selective + alias should error: {}",
        err
    );
}

// --- Namespace import: doesn't pollute global scope ---

#[test]
fn namespace_import_does_not_pollute_scope() {
    let mut files = HashMap::new();
    files.insert("stuff".to_string(), "pub class Foo\n  x: Int\n".to_string());
    // Foo should NOT be directly accessible — only via s.Foo
    let err = common::check_err_with_files("use stuff as s\nlet f = Foo(x: 1)\n", files);
    assert!(
        err.contains("Foo"),
        "Foo should not be in global scope: {}",
        err
    );
}

// --- Namespace import with protocol metadata ---

#[test]
fn namespace_class_with_eq_supports_equality() {
    let mut files = HashMap::new();
    files.insert(
        "types/token".to_string(),
        "use std/cmp { Eq }\n\npub class Token includes Eq\n  val: Int\n".to_string(),
    );
    common::check_ok_with_files(
        "use types/token as t\n\
         let a = t.Token(val: 1)\nlet b = t.Token(val: 2)\nlet eq = a == b\n",
        files,
    );
}

// ===========================================================
// Phase M3: Re-exports (pub use) — TDD specification
// ===========================================================

// --- Basic re-export: selective ---

#[test]
fn pub_use_reexports_class() {
    let mut files = HashMap::new();
    files.insert(
        "internal/user".to_string(),
        "pub class User\n  name: String\n".to_string(),
    );
    files.insert(
        "models".to_string(),
        "pub use internal/user { User }\n".to_string(),
    );
    // Consumer imports from models, which re-exports User from internal/user
    common::check_ok_with_files("use models { User }\nlet u = User(name: \"Jo\")\n", files);
}

#[test]
fn pub_use_reexports_function() {
    let mut files = HashMap::new();
    files.insert(
        "internal/math".to_string(),
        "pub def double(n: Int) -> Int\n  n * 2\n".to_string(),
    );
    files.insert(
        "math".to_string(),
        "pub use internal/math { double }\n".to_string(),
    );
    common::check_ok_with_files("use math { double }\nlet x = double(n: 5)\n", files);
}

#[test]
fn pub_use_reexports_trait() {
    let mut files = HashMap::new();
    files.insert(
        "internal/traits".to_string(),
        "pub trait Greetable\n  def greet() -> String\n".to_string(),
    );
    files.insert(
        "traits".to_string(),
        "pub use internal/traits { Greetable }\n".to_string(),
    );
    common::check_ok_with_files(
        "use traits { Greetable }\n\
         class Hello includes Greetable\n  def greet() -> String\n    \"hi\"\n",
        files,
    );
}

#[test]
fn pub_use_reexports_enum() {
    let mut files = HashMap::new();
    files.insert(
        "internal/types".to_string(),
        "pub enum Color\n  Red\n  Green\n  Blue\n".to_string(),
    );
    files.insert(
        "types".to_string(),
        "pub use internal/types { Color }\n".to_string(),
    );
    common::check_ok_with_files("use types { Color }\nlet c = Color.Red\n", files);
}

#[test]
fn pub_use_reexports_variable() {
    let mut files = HashMap::new();
    files.insert(
        "internal/config".to_string(),
        "pub let VERSION = 42\n".to_string(),
    );
    files.insert(
        "config".to_string(),
        "pub use internal/config { VERSION }\n".to_string(),
    );
    common::check_ok_with_files("use config { VERSION }\nlet v: Int = VERSION\n", files);
}

// --- Wildcard re-export ---

#[test]
fn pub_use_wildcard_reexports_all() {
    let mut files = HashMap::new();
    files.insert(
        "internal/stuff".to_string(),
        "pub class Foo\n  x: Int\npub class Bar\n  y: Int\n".to_string(),
    );
    files.insert("stuff".to_string(), "pub use internal/stuff\n".to_string());
    common::check_ok_with_files("use stuff\nlet f = Foo(x: 1)\nlet b = Bar(y: 2)\n", files);
}

// --- Re-export doesn't expose private items from source ---

#[test]
fn pub_use_only_reexports_pub_items_from_source() {
    let mut files = HashMap::new();
    files.insert(
        "internal/mixed".to_string(),
        "class Private\n  x: Int\npub class Public\n  y: Int\n".to_string(),
    );
    files.insert("facade".to_string(), "pub use internal/mixed\n".to_string());
    // Private should not be re-exported through facade
    let err = common::check_err_with_files("use facade { Private }\n", files);
    assert!(
        err.contains("M002") || err.contains("not exported"),
        "Private should not be re-exportable: {}",
        err
    );
}

// --- Non-pub use does NOT re-export ---

#[test]
fn non_pub_use_does_not_reexport() {
    let mut files = HashMap::new();
    files.insert(
        "internal/user".to_string(),
        "pub class User\n  name: String\n".to_string(),
    );
    // Regular use (not pub use) — imports for local use only
    files.insert(
        "facade".to_string(),
        "use internal/user { User }\npub def make_user(name: String) -> User\n  User(name: name)\n"
            .to_string(),
    );
    // User should NOT be accessible through facade (it was imported, not re-exported)
    let err = common::check_err_with_files("use facade { User }\n", files);
    assert!(
        err.contains("M002") || err.contains("not exported"),
        "Non-pub use should not re-export: {}",
        err
    );
}

// --- Re-export with own definitions ---

#[test]
fn module_with_own_defs_and_reexports() {
    let mut files = HashMap::new();
    files.insert(
        "internal/user".to_string(),
        "pub class User\n  name: String\n".to_string(),
    );
    files.insert(
        "api".to_string(),
        "\
pub use internal/user { User }
pub def greet(u: User) -> String
  u.name
"
        .to_string(),
    );
    common::check_ok_with_files(
        "use api { User, greet }\nlet u = User(name: \"Jo\")\nlet g = greet(u: u)\n",
        files,
    );
}

// --- Chained re-exports ---

#[test]
fn chained_reexports() {
    let mut files = HashMap::new();
    files.insert(
        "deep/core".to_string(),
        "pub class Widget\n  id: Int\n".to_string(),
    );
    files.insert(
        "mid".to_string(),
        "pub use deep/core { Widget }\n".to_string(),
    );
    files.insert("top".to_string(), "pub use mid { Widget }\n".to_string());
    // Widget re-exported through two layers
    common::check_ok_with_files("use top { Widget }\nlet w = Widget(id: 42)\n", files);
}

// --- Re-export with protocol metadata ---

#[test]
fn reexported_class_preserves_eq() {
    let mut files = HashMap::new();
    files.insert(
        "internal/token".to_string(),
        "use std/cmp { Eq }\n\npub class Token includes Eq\n  val: Int\n".to_string(),
    );
    files.insert(
        "tokens".to_string(),
        "pub use internal/token { Token }\n".to_string(),
    );
    common::check_ok_with_files(
        "use tokens { Token }\n\
         let a = Token(val: 1)\nlet b = Token(val: 2)\nlet eq = a == b\n",
        files,
    );
}

// --- Re-export via namespace ---

#[test]
fn pub_use_accessible_via_namespace() {
    let mut files = HashMap::new();
    files.insert(
        "internal/user".to_string(),
        "pub class User\n  name: String\n".to_string(),
    );
    files.insert(
        "models".to_string(),
        "pub use internal/user { User }\n".to_string(),
    );
    common::check_ok_with_files("use models as m\nlet u = m.User(name: \"Jo\")\n", files);
}

// --- Multiple use from same module ---

#[test]
fn multiple_selective_imports_same_module() {
    let mut files = std::collections::HashMap::new();
    files.insert(
        "utils".to_string(),
        "\
pub def foo() -> Int
  1

pub def bar() -> Int
  2
"
        .to_string(),
    );
    common::check_ok_with_files(
        "\
use utils { foo }
use utils { bar }

def main() -> Int
  foo() + bar()
",
        files,
    );
}

// --- Import class with inheritance ---

#[test]
fn import_class_hierarchy() {
    let mut files = HashMap::new();
    files.insert(
        "animals".to_string(),
        "pub class Animal\n  name: String\npub class Dog extends Animal\n  breed: String\n"
            .to_string(),
    );
    common::check_ok_with_files(
        "use animals { Animal, Dog }\nlet d = Dog(name: \"Rex\", breed: \"Lab\")\n",
        files,
    );
}
