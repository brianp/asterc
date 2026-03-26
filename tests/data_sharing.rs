mod common;

// ─── Async data sharing warnings ────────────────────────────────────

// Variable used after async pass → warning

#[test]
fn warn_use_after_async_pass() {
    let diags = common::check_all_errors(
        "\
class User
  name: String

def save_user(user: User) -> Int
  42

def main() -> Int
  let user = User(name: \"Alice\")
  let t = async save_user(user: user)
  user.name
  let r = resolve t!
  r
",
    );
    // check_all_errors returns errors only; warnings go to tc.diagnostics
    // We need a helper that captures warnings too. For now, verify no errors.
    assert!(diags.is_empty(), "expected no errors, got: {:?}", diags);
}

#[test]
fn warn_use_after_async_pass_has_warning() {
    let warnings = common::check_warnings(
        "\
class User
  name: String

def save_user(user: User) -> Int
  42

def main() -> Int
  let user = User(name: \"Alice\")
  let t = async save_user(user: user)
  user.name
  let r = resolve t!
  r
",
    );
    assert!(
        warnings.iter().any(|w| w.code() == Some("W002")),
        "expected W002 warning, got: {:?}",
        warnings
    );
}

// Variable not used after → no warning

#[test]
fn no_warn_when_not_used_after() {
    let warnings = common::check_warnings(
        "\
class User
  name: String

def save_user(user: User) -> Int
  42

def main() -> Int
  let user = User(name: \"Alice\")
  let t = async save_user(user: user)
  let r = resolve t!
  r
",
    );
    let data_sharing = warnings.iter().filter(|w| w.code() == Some("W002")).count();
    assert_eq!(
        data_sharing, 0,
        "expected no W002 warnings, got: {:?}",
        warnings
    );
}

// Reassignment after boundary → no warning

#[test]
fn no_warn_after_reassignment() {
    let warnings = common::check_warnings(
        "\
def save(x: Int) -> Int
  x

def main() -> Int
  let x = 42
  let t = async save(x: x)
  x = 99
  let r = resolve t!
  r
",
    );
    let data_sharing = warnings.iter().filter(|w| w.code() == Some("W002")).count();
    assert_eq!(
        data_sharing, 0,
        "expected no W002 after reassignment, got: {:?}",
        warnings
    );
}

// Multiple boundaries

#[test]
fn warn_multiple_boundaries() {
    let warnings = common::check_warnings(
        "\
def work(x: Int) -> Int
  x

def main() -> Int
  let x = 42
  let t1 = async work(x: x)
  let t2 = async work(x: x)
  x + 1
  let r1 = resolve t1!
  let r2 = resolve t2!
  r1 + r2
",
    );
    let data_sharing = warnings.iter().filter(|w| w.code() == Some("W002")).count();
    assert!(
        data_sharing >= 1,
        "expected W002 warning for use after boundary, got: {:?}",
        warnings
    );
}

// Primitive types also warned (shallow copy semantics)

#[test]
fn warn_primitive_use_after_async() {
    let warnings = common::check_warnings(
        "\
def double(n: Int) -> Int
  n * 2

def main() -> Int
  let n = 21
  let t = async double(n: n)
  n + 1
  let r = resolve t!
  r
",
    );
    let data_sharing = warnings.iter().filter(|w| w.code() == Some("W002")).count();
    assert!(
        data_sharing >= 1,
        "expected W002 warning for use of 'n' after async boundary, got: {:?}",
        warnings
    );
}
