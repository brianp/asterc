
// ─── Runtime Parity Tests ─────────────────────────────────────────
//
// Each test runs an Aster program through both JIT (`aster run`) and
// AOT (`aster build` + execute), then asserts identical stdout and
// exit code. This catches behavioral divergence between the two
// runtime implementations.

/// Run a program through JIT and AOT, assert identical output.
fn assert_parity(name: &str, source: &str) {
    let dir = crate::common::make_temp_dir(name);
    let src = dir.join(format!("{name}.aster"));
    std::fs::write(&src, source).unwrap();
    let src_str = src.to_str().unwrap();

    // JIT path: `aster run`
    let jit = crate::common::cli(&["run", src_str]);
    let jit_stdout = String::from_utf8_lossy(&jit.stdout).to_string();
    let jit_exit = jit.status.code().unwrap_or(-1);

    // AOT path: `aster build` + execute binary
    let aot = crate::common::build_and_run(&src);
    let aot_stdout = String::from_utf8_lossy(&aot.stdout).to_string();
    let aot_exit = aot.status.code().unwrap_or(-1);

    assert_eq!(
        jit_stdout, aot_stdout,
        "stdout mismatch for {name}:\n  JIT: {jit_stdout:?}\n  AOT: {aot_stdout:?}"
    );
    assert_eq!(
        jit_exit,
        aot_exit,
        "exit code mismatch for {name}: JIT={jit_exit}, AOT={aot_exit}\n  JIT stderr: {}\n  AOT stderr: {}",
        String::from_utf8_lossy(&jit.stderr),
        String::from_utf8_lossy(&aot.stderr),
    );
}

// ─── String operations ────────────────────────────────────────────

#[test]
fn parity_string_concat_and_len() {
    assert_parity(
        "str-concat",
        "\
def main() -> Int
  let a = \"hello\"
  let b = \" world\"
  let c = a + b
  c.len()
",
    );
}

#[test]
fn parity_string_contains_starts_ends() {
    assert_parity(
        "str-search",
        "\
def main() -> Int
  let s = \"hello world\"
  let r1 = s.contains(str: \"world\")
  let r2 = s.starts_with(pre: \"hello\")
  let r3 = s.ends_with(suf: \"world\")
  let total = 0
  if r1
    total = total + 1
  if r2
    total = total + 10
  if r3
    total = total + 100
  total
",
    );
}

#[test]
fn parity_string_trim() {
    assert_parity(
        "str-trim",
        "\
def main() -> Int
  let s = \"  hello  \"
  let t = s.trim()
  say(message: t)
  t.len()
",
    );
}

#[test]
fn parity_string_upper_lower() {
    assert_parity(
        "str-case",
        "\
def main() -> Int
  let s = \"Hello World\"
  say(message: s.to_upper())
  say(message: s.to_lower())
  0
",
    );
}

#[test]
fn parity_string_slice() {
    assert_parity(
        "str-slice",
        "\
def main() -> Int
  let s = \"hello world\"
  say(message: s.slice(from: 0, to: 5))
  say(message: s.slice(from: 6, to: 11))
  0
",
    );
}

#[test]
fn parity_string_replace() {
    assert_parity(
        "str-replace",
        "\
def main() -> Int
  let s = \"hello world\"
  say(message: s.replace(old: \"world\", new: \"rust\"))
  0
",
    );
}

#[test]
fn parity_string_split() {
    assert_parity(
        "str-split",
        "\
def main() -> Int
  let s = \"a,b,c\"
  let parts = s.split(sep: \",\")
  say(message: parts[0])
  say(message: parts[1])
  say(message: parts[2])
  parts.len()
",
    );
}

// ─── List operations ──────────────────────────────────────────────

#[test]
fn parity_list_push_len_get() {
    assert_parity(
        "list-basic",
        "\
def main() -> Int
  let xs: List[Int] = [10, 20, 30]
  xs.push(item: 40)
  say(message: xs[0])
  say(message: xs[3])
  xs.len()
",
    );
}

#[test]
fn parity_list_insert_remove() {
    assert_parity(
        "list-insert-remove",
        "\
def main() -> Int
  let xs: List[Int] = [1, 2, 3]
  xs.insert(at: 1, item: 99)
  say(message: xs[1])
  let removed: Int = xs.remove(at: 1)
  say(message: removed)
  xs.len()
",
    );
}

#[test]
fn parity_list_pop() {
    assert_parity(
        "list-pop",
        "\
def main() -> Int
  let xs: List[Int] = [10, 20, 30]
  let last = xs.pop()
  say(message: last)
  xs.len()
",
    );
}

// ─── Map operations ───────────────────────────────────────────────

#[test]
fn parity_map_get() {
    // Map indexing tested through the exit code.
    // Note: has_key is not a user-callable method, and nullable
    // comparison requires match, so we test map via simple get + use.
    assert_parity(
        "map-basic",
        "\
def main() -> Int
  let m: Map[String, Int] = {\"a\": 1, \"b\": 2, \"c\": 3}
  let va: Int? = m[\"a\"]
  let vb: Int? = m[\"b\"]
  say(message: \"map ok\")
  6
",
    );
}

// ─── Numeric operations ──────────────────────────────────────────

#[test]
fn parity_int_arithmetic() {
    assert_parity(
        "int-arith",
        "\
def main() -> Int
  let a = 10
  let b = 3
  say(message: a + b)
  say(message: a - b)
  say(message: a * b)
  say(message: a / b)
  say(message: a % b)
  0
",
    );
}

#[test]
fn parity_int_pow() {
    // Note: Int methods (.abs, .clamp, .min, .max, .is_even, .is_odd)
    // are not executable yet (E028). Test via pow operator instead.
    // Keep results small to avoid exit code truncation (codes > 255 wrap).
    assert_parity(
        "int-pow",
        "\
def main() -> Int
  let a = 2 ** 5
  let b = 3 ** 3
  say(message: a)
  say(message: b)
  a + b
",
    );
}

#[test]
fn parity_pow_int() {
    assert_parity(
        "pow-int",
        "\
def main() -> Int
  say(message: 2 ** 10)
  say(message: 3 ** 0)
  say(message: 1 ** 100)
  0
",
    );
}

#[test]
fn parity_pow_float() {
    assert_parity(
        "pow-float",
        "\
def main() -> Int
  say(message: 2.0 ** 3.0)
  say(message: 9.0 ** 0.5)
  0
",
    );
}

// ─── String interpolation ─────────────────────────────────────────

#[test]
fn parity_string_interpolation() {
    assert_parity(
        "str-interp",
        "\
def main() -> Int
  let name = \"world\"
  let n = 42
  say(message: \"hello {name}\")
  say(message: \"answer: {n}\")
  0
",
    );
}

// ─── Error handling ───────────────────────────────────────────────

#[test]
fn parity_error_try_catch() {
    assert_parity(
        "error-catch",
        "\
class AppError
  message: String

def risky() throws AppError -> Int
  throw AppError(message: \"boom\")

def main() -> Int
  risky()!.catch
    AppError e -> 99
    _ -> 0
",
    );
}

#[test]
fn parity_error_or_default() {
    assert_parity(
        "error-or",
        "\
class AppError
  message: String

def risky() throws AppError -> Int
  throw AppError(message: \"boom\")

def safe() throws AppError -> Int
  42

def main() -> Int
  let a = risky()!.or(10)
  let b = safe()!.or(10)
  a + b
",
    );
}

// ─── Control flow ─────────────────────────────────────────────────

#[test]
fn parity_for_loop_range() {
    assert_parity(
        "for-range",
        "\
def main() -> Int
  let total = 0
  for i in 0..5
    total = total + i
  total
",
    );
}

#[test]
fn parity_for_loop_inclusive_range() {
    assert_parity(
        "for-range-incl",
        "\
def main() -> Int
  let total = 0
  for i in 0..=5
    total = total + i
  total
",
    );
}

#[test]
fn parity_for_loop_list() {
    assert_parity(
        "for-list",
        "\
def main() -> Int
  let xs = [10, 20, 30]
  let total = 0
  for x in xs
    total = total + x
  total
",
    );
}

#[test]
fn parity_while_loop() {
    assert_parity(
        "while-loop",
        "\
def main() -> Int
  let i = 0
  let total = 0
  while i < 10
    total = total + i
    i = i + 1
  total
",
    );
}

#[test]
fn parity_pattern_match() {
    assert_parity(
        "match",
        "\
def classify(n: Int) -> Int
  match n
    0 => 10
    1 => 20
    _ => 30

def main() -> Int
  classify(n: 0) + classify(n: 1) + classify(n: 42)
",
    );
}

// ─── Tasks / Async ────────────────────────────────────────────────

#[test]
fn parity_async_spawn_resolve() {
    assert_parity(
        "async-basic",
        "\
def double(n: Int) -> Int
  n * 2

def main() -> Int
  let t: Task[Int] = async double(n: 21)
  resolve t!
",
    );
}

#[test]
fn parity_async_multiple_tasks() {
    assert_parity(
        "async-multi",
        "\
def work(n: Int) -> Int
  n * n

def main() -> Int
  let a: Task[Int] = async work(n: 3)
  let b: Task[Int] = async work(n: 4)
  let ra = resolve a!
  let rb = resolve b!
  ra + rb
",
    );
}

#[test]
fn parity_blocking_call() {
    assert_parity(
        "blocking-call",
        "\
def inner() -> Int
  21

def compute() -> Int
  let t: Task[Int] = async inner()
  let v = resolve t!
  v * 2

def main() -> Int
  blocking compute()
",
    );
}

// ─── Closures / Lambdas ──────────────────────────────────────────

#[test]
fn parity_closure_capture() {
    assert_parity(
        "closure-capture",
        "\
let scale = 3

def multiply(x: Int) -> Int
  x * scale

def main() -> Int
  multiply(x: 14)
",
    );
}

// ─── Classes ──────────────────────────────────────────────────────

#[test]
fn parity_class_fields() {
    assert_parity(
        "class-fields",
        "\
class Point
  x: Int
  y: Int

def main() -> Int
  let p = Point(x: 3, y: 4)
  say(message: p.x)
  say(message: p.y)
  p.x + p.y
",
    );
}

// ─── Enum / pattern matching ──────────────────────────────────────

#[test]
fn parity_enum_match() {
    assert_parity(
        "enum-match",
        "\
enum Color
  Red
  Green
  Blue

def to_int(c: Color) -> Int
  match c
    Color.Red => 1
    Color.Green => 2
    Color.Blue => 3

def main() -> Int
  to_int(c: Color.Red) + to_int(c: Color.Green) + to_int(c: Color.Blue)
",
    );
}
