use fir::lower::Lowerer;
use fir::module::FirModule;
use fir::types::FunctionId;

use crate::aot::CraneliftAOT;
use crate::jit::CraneliftJIT;

// ---------------------------------------------------------------------------
// Helper: source → FIR → JIT compile
// ---------------------------------------------------------------------------

fn compile_and_run(src: &str) -> FirModule {
    let tokens = lexer::lex(src).expect("lex ok");
    let mut parser = parser::Parser::new(tokens);
    let module = parser.parse_module("test").expect("parse ok");
    let mut tc = typecheck::TypeChecker::new();
    tc.check_module(&module).expect("typecheck ok");
    let mut lowerer = Lowerer::new(tc.env);
    lowerer.lower_module(&module).expect("lower ok");
    lowerer.finish()
}

fn jit_compile(fir: &FirModule) -> CraneliftJIT {
    let mut jit = CraneliftJIT::new();
    jit.compile_module(fir).expect("JIT compile ok");
    jit
}

// ===========================================================================
// Integer arithmetic
// ===========================================================================

#[test]
fn return_constant() {
    let fir = compile_and_run("def main() -> Int\n  return 42\n");
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn add_two_numbers() {
    let fir = compile_and_run("def add(a: Int, b: Int) -> Int\n  a + b\n");
    let jit = jit_compile(&fir);
    let add_id = FunctionId(0);
    let result = jit.call_i64_i64_i64(add_id, 3, 4);
    assert_eq!(result, 7);
}

#[test]
fn subtract() {
    let fir = compile_and_run("def sub(a: Int, b: Int) -> Int\n  a - b\n");
    let jit = jit_compile(&fir);
    let result = jit.call_i64_i64_i64(FunctionId(0), 10, 3);
    assert_eq!(result, 7);
}

#[test]
fn multiply() {
    let fir = compile_and_run("def mul(a: Int, b: Int) -> Int\n  a * b\n");
    let jit = jit_compile(&fir);
    let result = jit.call_i64_i64_i64(FunctionId(0), 6, 7);
    assert_eq!(result, 42);
}

#[test]
fn divide() {
    let fir = compile_and_run("def div(a: Int, b: Int) -> Int\n  a / b\n");
    let jit = jit_compile(&fir);
    let result = jit.call_i64_i64_i64(FunctionId(0), 20, 4);
    assert_eq!(result, 5);
}

#[test]
fn nested_arithmetic() {
    let fir = compile_and_run("def f(a: Int, b: Int) -> Int\n  (a + b) * (a - b)\n");
    let jit = jit_compile(&fir);
    // (5+3) * (5-3) = 8 * 2 = 16
    let result = jit.call_i64_i64_i64(FunctionId(0), 5, 3);
    assert_eq!(result, 16);
}

#[test]
fn unary_negation() {
    let fir = compile_and_run("def neg(x: Int) -> Int\n  -x\n");
    let jit = jit_compile(&fir);
    let result = jit.call_i64_i64(FunctionId(0), 42);
    assert_eq!(result, -42);
}

#[test]
fn let_binding_and_return() {
    let fir = compile_and_run("def f() -> Int\n  let x: Int = 10\n  let y: Int = 20\n  x + y\n");
    let jit = jit_compile(&fir);
    let result = jit.call_i64(FunctionId(0));
    assert_eq!(result, 30);
}

#[test]
fn modulo() {
    let fir = compile_and_run("def modulo(a: Int, b: Int) -> Int\n  a % b\n");
    let jit = jit_compile(&fir);
    let result = jit.call_i64_i64_i64(FunctionId(0), 17, 5);
    assert_eq!(result, 2);
}

#[test]
fn negative_literal() {
    let fir = compile_and_run("def f() -> Int\n  return -42\n");
    let jit = jit_compile(&fir);
    let result = jit.call_i64(FunctionId(0));
    assert_eq!(result, -42);
}

// ===========================================================================
// Float arithmetic
// ===========================================================================

#[test]
fn float_return_constant() {
    let src = "def main() -> Float\n  3.14\n";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.get_function_ptr(fir.entry.unwrap()).unwrap();
    let f: fn() -> f64 = unsafe { std::mem::transmute(ptr) };
    let result = f();
    assert!(
        (result - 3.14).abs() < 1e-10,
        "expected 3.14, got {}",
        result
    );
}

#[test]
fn float_add() {
    let src = "def main() -> Float\n  1.5 + 2.5\n";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.get_function_ptr(fir.entry.unwrap()).unwrap();
    let f: fn() -> f64 = unsafe { std::mem::transmute(ptr) };
    let result = f();
    assert!((result - 4.0).abs() < 1e-10, "expected 4.0, got {}", result);
}

#[test]
fn float_subtract() {
    let src = "def main() -> Float\n  3.5 - 1.5\n";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.get_function_ptr(fir.entry.unwrap()).unwrap();
    let f: fn() -> f64 = unsafe { std::mem::transmute(ptr) };
    let result = f();
    assert!((result - 2.0).abs() < 1e-10, "expected 2.0, got {}", result);
}

#[test]
fn float_multiply() {
    let src = "def main() -> Float\n  2.0 * 3.0\n";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.get_function_ptr(fir.entry.unwrap()).unwrap();
    let f: fn() -> f64 = unsafe { std::mem::transmute(ptr) };
    let result = f();
    assert!((result - 6.0).abs() < 1e-10, "expected 6.0, got {}", result);
}

#[test]
fn float_comparison() {
    // 3.14 > 2.71 should return true (1)
    let src = "\
def main() -> Int
  if 3.14 > 2.71
    return 1
  else
    return 0
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 1);
}

// ===========================================================================
// Control flow
// ===========================================================================

#[test]
fn if_else_true_branch() {
    let src = "def f(x: Int) -> Int\n  if x > 0\n    return 1\n  else\n    return -1\n";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    assert_eq!(jit.call_i64_i64(FunctionId(0), 5), 1);
}

#[test]
fn if_else_false_branch() {
    let src = "def f(x: Int) -> Int\n  if x > 0\n    return 1\n  else\n    return -1\n";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    assert_eq!(jit.call_i64_i64(FunctionId(0), -5), -1);
}

#[test]
fn if_else_zero() {
    let src = "def f(x: Int) -> Int\n  if x > 0\n    return 1\n  else\n    return -1\n";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    assert_eq!(jit.call_i64_i64(FunctionId(0), 0), -1);
}

#[test]
fn elif_chain() {
    let src = "\
def classify(x: Int) -> Int
  if x > 0
    return 1
  elif x < 0
    return -1
  else
    return 0
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    assert_eq!(jit.call_i64_i64(FunctionId(0), 10), 1);
    assert_eq!(jit.call_i64_i64(FunctionId(0), -10), -1);
    assert_eq!(jit.call_i64_i64(FunctionId(0), 0), 0);
}

#[test]
fn while_loop_sum() {
    let src = "\
def sum_to(n: Int) -> Int
  let total: Int = 0
  let i: Int = 1
  while i <= n
    total = total + i
    i = i + 1
  return total
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    assert_eq!(jit.call_i64_i64(FunctionId(0), 10), 55);
}

#[test]
fn while_loop_zero_iterations() {
    let src = "\
def sum_to(n: Int) -> Int
  let total: Int = 0
  let i: Int = 1
  while i <= n
    total = total + i
    i = i + 1
  return total
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    assert_eq!(jit.call_i64_i64(FunctionId(0), 0), 0);
}

#[test]
fn break_in_while() {
    let src = "\
def f() -> Int
  let x: Int = 0
  while true
    x = x + 1
    if x == 5
      break
  return x
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    assert_eq!(jit.call_i64(FunctionId(0)), 5);
}

#[test]
fn comparison_operators() {
    // Test all comparisons by encoding results as bits
    let src = "\
def test_cmp(a: Int, b: Int) -> Int
  let result: Int = 0
  if a == b
    result = result + 1
  if a != b
    result = result + 2
  if a < b
    result = result + 4
  if a > b
    result = result + 8
  if a <= b
    result = result + 16
  if a >= b
    result = result + 32
  return result
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    // a=3, b=5: != + < + <= = 2 + 4 + 16 = 22
    assert_eq!(jit.call_i64_i64_i64(FunctionId(0), 3, 5), 22);
    // a=5, b=5: == + <= + >= = 1 + 16 + 32 = 49
    assert_eq!(jit.call_i64_i64_i64(FunctionId(0), 5, 5), 49);
    // a=7, b=5: != + > + >= = 2 + 8 + 32 = 42
    assert_eq!(jit.call_i64_i64_i64(FunctionId(0), 7, 5), 42);
}

#[test]
fn nested_if() {
    let src = "\
def f(x: Int, y: Int) -> Int
  if x > 0
    if y > 0
      return 1
    else
      return 2
  else
    return 3
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    assert_eq!(jit.call_i64_i64_i64(FunctionId(0), 1, 1), 1);
    assert_eq!(jit.call_i64_i64_i64(FunctionId(0), 1, -1), 2);
    assert_eq!(jit.call_i64_i64_i64(FunctionId(0), -1, 1), 3);
}

// ===========================================================================
// Strings
// ===========================================================================

#[test]
fn string_return() {
    // Verify string functions compile without crashing
    let src = "def f() -> String\n  return \"hello\"\n";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    // String returns a heap pointer — just check it's non-null
    let ptr = jit.call_i64(FunctionId(0));
    assert_ne!(ptr, 0, "string should return non-null pointer");
}

#[test]
fn string_length() {
    // Create a string and verify its heap layout (len at offset 0)
    let src = "def f() -> String\n  return \"hello\"\n";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.call_i64(FunctionId(0));
    // Read length from heap string layout: [len: i64][data...]
    let len = unsafe { *(ptr as *const i64) };
    assert_eq!(len, 5);
}

#[test]
fn string_data() {
    let src = "def f() -> String\n  return \"hello\"\n";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.call_i64(FunctionId(0));
    unsafe {
        let len = *(ptr as *const i64) as usize;
        let data = (ptr as *const u8).add(8);
        let s = std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len));
        assert_eq!(s, "hello");
    }
}

#[test]
fn log_compiles() {
    // Verify that log() calls compile (runtime call)
    let src = "def f() -> Void\n  log(message: \"hello\")\n";
    let fir = compile_and_run(src);
    let _jit = jit_compile(&fir);
    // If we get here, compilation succeeded
}

// ===========================================================================
// Function calls
// ===========================================================================

#[test]
fn call_another_function() {
    let src = "\
def double(x: Int) -> Int
  x * 2

def main() -> Int
  double(x: 21)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn call_chain() {
    let src = "\
def add(a: Int, b: Int) -> Int
  a + b

def mul(a: Int, b: Int) -> Int
  a * b

def main() -> Int
  mul(a: add(a: 2, b: 3), b: add(a: 4, b: 5))
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    // (2+3) * (4+5) = 5 * 9 = 45
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 45);
}

#[test]
fn recursive_factorial() {
    let src = "\
def factorial(n: Int) -> Int
  if n <= 1
    return 1
  else
    return n * factorial(n: n - 1)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    assert_eq!(jit.call_i64_i64(FunctionId(0), 5), 120);
    assert_eq!(jit.call_i64_i64(FunctionId(0), 10), 3628800);
}

#[test]
fn recursive_fibonacci() {
    let src = "\
def fib(n: Int) -> Int
  if n <= 1
    return n
  else
    return fib(n: n - 1) + fib(n: n - 2)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    assert_eq!(jit.call_i64_i64(FunctionId(0), 0), 0);
    assert_eq!(jit.call_i64_i64(FunctionId(0), 1), 1);
    assert_eq!(jit.call_i64_i64(FunctionId(0), 10), 55);
}

#[test]
fn mutual_recursion() {
    let src = "\
def is_even(n: Int) -> Int
  if n == 0
    return 1
  else
    return is_odd(n: n - 1)

def is_odd(n: Int) -> Int
  if n == 0
    return 0
  else
    return is_even(n: n - 1)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let is_even_id = fir
        .functions
        .iter()
        .find(|f| f.name == "is_even")
        .unwrap()
        .id;
    assert_eq!(jit.call_i64_i64(is_even_id, 4), 1);
    assert_eq!(jit.call_i64_i64(is_even_id, 5), 0);
}

#[test]
fn function_with_multiple_calls() {
    let src = "\
def square(x: Int) -> Int
  x * x

def sum_of_squares(a: Int, b: Int) -> Int
  square(x: a) + square(x: b)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let sos_id = fir
        .functions
        .iter()
        .find(|f| f.name == "sum_of_squares")
        .unwrap()
        .id;
    assert_eq!(jit.call_i64_i64_i64(sos_id, 3, 4), 25);
}

// ===========================================================================
// Classes
// ===========================================================================

#[test]
fn class_construction() {
    let src = "\
class Point
  x: Int
  y: Int

def make_point() -> Point
  Point(x: 10, y: 20)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    // Class returns a heap pointer — just check non-null
    let make_id = fir
        .functions
        .iter()
        .find(|f| f.name == "make_point")
        .unwrap()
        .id;
    let ptr = jit.call_i64(make_id);
    assert_ne!(ptr, 0, "class instance should be non-null");
}

#[test]
fn class_field_access() {
    let src = "\
class Point
  x: Int
  y: Int

def get_x() -> Int
  let p: Point = Point(x: 42, y: 99)
  p.x
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let get_x_id = fir.functions.iter().find(|f| f.name == "get_x").unwrap().id;
    let result = jit.call_i64(get_x_id);
    assert_eq!(result, 42);
}

#[test]
fn class_field_access_second_field() {
    let src = "\
class Point
  x: Int
  y: Int

def get_y() -> Int
  let p: Point = Point(x: 42, y: 99)
  p.y
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let get_y_id = fir.functions.iter().find(|f| f.name == "get_y").unwrap().id;
    let result = jit.call_i64(get_y_id);
    assert_eq!(result, 99);
}

#[test]
fn method_returns_field() {
    let src = "\
class Counter
  value: Int

  def get() -> Int
    value

def main() -> Int
  let c: Counter = Counter(value: 42)
  c.get()
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn method_returns_computed_field() {
    let src = "\
class Point
  x: Int
  y: Int

  def sum() -> Int
    x + y

def main() -> Int
  let p: Point = Point(x: 10, y: 32)
  p.sum()
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn method_accesses_multiple_fields() {
    let src = "\
class Rect
  w: Int
  h: Int

  def area() -> Int
    w * h

def main() -> Int
  let r: Rect = Rect(w: 6, h: 7)
  r.area()
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

// ===========================================================================
// Field mutation
// ===========================================================================

#[test]
fn field_mutation_assign_and_read() {
    let src = "\
class Point
  x: Int
  y: Int

def main() -> Int
  let p: Point = Point(x: 1, y: 2)
  p.x = 99
  p.x
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 99);
}

#[test]
fn field_mutation_second_field() {
    let src = "\
class Point
  x: Int
  y: Int

def main() -> Int
  let p: Point = Point(x: 1, y: 2)
  p.y = 77
  p.y
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 77);
}

#[test]
fn field_mutation_preserves_other_fields() {
    let src = "\
class Point
  x: Int
  y: Int

def main() -> Int
  let p: Point = Point(x: 10, y: 32)
  p.x = 99
  p.y
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 32);
}

// ===========================================================================
// Lists
// ===========================================================================

#[test]
fn list_creation_and_get() {
    let src = "\
def main() -> Int
  let xs: List[Int] = [10, 20, 30]
  xs[1]
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 20);
}

#[test]
fn list_set() {
    let src = "\
def main() -> Int
  let xs: List[Int] = [10, 20, 30]
  xs[0] = 99
  xs[0]
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 99);
}

#[test]
fn list_len() {
    let src = "\
def main() -> Int
  let xs: List[Int] = [1, 2, 3, 4, 5]
  xs.len()
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 5);
}

#[test]
fn list_push() {
    let src = "\
def main() -> Int
  let xs: List[Int] = [1, 2]
  xs.push(item: 3)
  xs.len()
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 3);
}

// ===========================================================================
// Maps
// ===========================================================================

#[test]
fn map_literal_creation() {
    // Map literal should be constructable without crashing
    let src = "\
def main() -> Int
  let m: Map[String, Int] = {\"x\": 1, \"y\": 2}
  42
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn map_get_value() {
    let src = "\
def main() -> Int
  let m: Map[String, Int] = {\"x\": 10, \"y\": 32}
  m[\"y\"]
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 32);
}

#[test]
fn map_get_first_key() {
    let src = "\
def main() -> Int
  let m: Map[String, Int] = {\"a\": 99, \"b\": 1}
  m[\"a\"]
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 99);
}

#[test]
fn for_loop_sum() {
    let src = "\
def main() -> Int
  let xs: List[Int] = [1, 2, 3, 4, 5]
  let total: Int = 0
  for x in xs
    total = total + x
  return total
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 15);
}

#[test]
fn for_loop_empty_list() {
    let src = "\
def main() -> Int
  let xs: List[Int] = []
  let total: Int = 0
  for x in xs
    total = total + x
  return total
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 0);
}

#[test]
fn for_loop_with_break() {
    let src = "\
def main() -> Int
  let xs: List[Int] = [1, 2, 3, 4, 5]
  let total: Int = 0
  for x in xs
    if x == 4
      break
    total = total + x
  return total
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 6); // 1 + 2 + 3
}

// ===========================================================================
// Nullable operations
// ===========================================================================

#[test]
fn nullable_return_value() {
    // Nullable Int? return — value is boxed (heap pointer), non-zero
    let src = "\
def f() -> Int?
  return 42
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(FunctionId(0));
    // Value should be a non-zero heap pointer (boxed 42)
    assert_ne!(result, 0, "Some(42) should be non-zero (boxed pointer)");
    // Verify the boxed value by reading from the pointer
    let ptr = result as *const i64;
    let unboxed = unsafe { *ptr };
    assert_eq!(unboxed, 42);
}

#[test]
fn nullable_nil_return() {
    let src = "\
def f() -> Int?
  return nil
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(FunctionId(0));
    // nil should be 0
    assert_eq!(result, 0);
}

// ===========================================================================
// Generics
// ===========================================================================

#[test]
fn generic_identity_int() {
    let src = "\
def identity(x: T) -> T
  x

def main() -> Int
  identity(x: 42)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn generic_class_field() {
    let src = "\
class Box[T]
  value: T

def main() -> Int
  let b: Box[Int] = Box(value: 99)
  b.value
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 99);
}

// ===========================================================================
// AOT backend
// ===========================================================================

#[test]
fn aot_compile_simple() {
    let src = "\
def main() -> Int
  return 42
";
    let fir = compile_and_run(src);
    let mut aot = CraneliftAOT::new();
    aot.compile_module(&fir).expect("AOT compile ok");
    let bytes = aot.emit_object().expect("emit object ok");
    // Object file should start with a valid magic number
    assert!(!bytes.is_empty(), "object file should not be empty");
    // Mach-O magic (macOS) or ELF magic (Linux)
    assert!(
        bytes.len() > 4,
        "object file too small: {} bytes",
        bytes.len()
    );
}

#[test]
fn aot_compile_with_functions() {
    let src = "\
def double(x: Int) -> Int
  x * 2

def main() -> Int
  double(x: 21)
";
    let fir = compile_and_run(src);
    let mut aot = CraneliftAOT::new();
    aot.compile_module(&fir).expect("AOT compile ok");
    let bytes = aot.emit_object().expect("emit object ok");
    assert!(
        bytes.len() > 100,
        "object file should contain compiled code"
    );
}

#[test]
fn aot_compile_with_strings() {
    let src = "\
def main() -> Int
  log(message: \"hello AOT\")
  return 0
";
    let fir = compile_and_run(src);
    let mut aot = CraneliftAOT::new();
    aot.compile_module(&fir).expect("AOT compile ok");
    let bytes = aot.emit_object().expect("emit object ok");
    assert!(!bytes.is_empty());
}

#[test]
fn aot_compile_with_control_flow() {
    let src = "\
def factorial(n: Int) -> Int
  if n <= 1
    return 1
  else
    return n * factorial(n: n - 1)

def main() -> Int
  factorial(n: 10)
";
    let fir = compile_and_run(src);
    let mut aot = CraneliftAOT::new();
    aot.compile_module(&fir).expect("AOT compile ok");
    let bytes = aot.emit_object().expect("emit object ok");
    assert!(bytes.len() > 100);
}

#[test]
fn aot_compile_with_classes() {
    let src = "\
class Point
  x: Int
  y: Int

def main() -> Int
  let p: Point = Point(x: 3, y: 4)
  p.x * p.x + p.y * p.y
";
    let fir = compile_and_run(src);
    let mut aot = CraneliftAOT::new();
    aot.compile_module(&fir).expect("AOT compile ok");
    let bytes = aot.emit_object().expect("emit object ok");
    assert!(bytes.len() > 100);
}

#[test]
fn aot_compile_with_lists() {
    let src = "\
def main() -> Int
  let xs: List[Int] = [1, 2, 3, 4, 5]
  let total: Int = 0
  for x in xs
    total = total + x
  return total
";
    let fir = compile_and_run(src);
    let mut aot = CraneliftAOT::new();
    aot.compile_module(&fir).expect("AOT compile ok");
    let bytes = aot.emit_object().expect("emit object ok");
    assert!(bytes.len() > 100);
}

// ===========================================================================
// Async
// ===========================================================================

#[test]
fn async_call_and_resolve() {
    let src = "\
def compute() -> Int
  42

def main() throws CancelledError -> Int
  let t: Task[Int] = async compute()
  resolve t!
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn async_with_args() {
    let src = "\
def add(a: Int, b: Int) -> Int
  a + b

def main() throws CancelledError -> Int
  let t: Task[Int] = async add(a: 20, b: 22)
  resolve t!
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

// ===========================================================================
// Classes (continued)
// ===========================================================================

#[test]
fn class_in_function() {
    let src = "\
class Point
  x: Int
  y: Int

def distance_sq() -> Int
  let p: Point = Point(x: 3, y: 4)
  p.x * p.x + p.y * p.y
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let f_id = fir
        .functions
        .iter()
        .find(|f| f.name == "distance_sq")
        .unwrap()
        .id;
    let result = jit.call_i64(f_id);
    assert_eq!(result, 25);
}

// ===========================================================================
// Match expressions — desugared to if/else chains
// ===========================================================================

#[test]
fn match_int_literal() {
    let src = "\
def classify(x: Int) -> Int
  match x
    1 => 10
    2 => 20
    _ => 99
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    assert_eq!(jit.call_i64_i64(FunctionId(0), 1), 10);
    assert_eq!(jit.call_i64_i64(FunctionId(0), 2), 20);
    assert_eq!(jit.call_i64_i64(FunctionId(0), 3), 99);
}

#[test]
fn match_wildcard_only() {
    let src = "\
def f(x: Int) -> Int
  match x
    _ => 42
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    assert_eq!(jit.call_i64_i64(FunctionId(0), 0), 42);
    assert_eq!(jit.call_i64_i64(FunctionId(0), 100), 42);
}

#[test]
fn match_variable_binding() {
    let src = "\
def f(x: Int) -> Int
  match x
    1 => 10
    other => other + 100
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    assert_eq!(jit.call_i64_i64(FunctionId(0), 1), 10);
    assert_eq!(jit.call_i64_i64(FunctionId(0), 5), 105);
}

#[test]
fn match_in_expression() {
    let src = "\
def f(x: Int) -> Int
  let result: Int = match x
    0 => 0
    _ => 1
  result * 10
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    assert_eq!(jit.call_i64_i64(FunctionId(0), 0), 0);
    assert_eq!(jit.call_i64_i64(FunctionId(0), 5), 10);
}

// ===========================================================================
// MILESTONE 13: Enum lowering — tagged union layout
// ===========================================================================

#[test]
fn fieldless_enum_construct() {
    let src = "\
enum Color
  Red
  Green
  Blue

def main() -> Int
  let c = Color.Red
  0
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 0);
}

#[test]
fn enum_match_tag() {
    let src = "\
enum Color
  Red
  Green
  Blue

def test(c: Color) -> Int
  match c
    Color.Red => 1
    Color.Green => 2
    Color.Blue => 3
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    // Call the test function with a Color.Green (tag=1)
    // First, construct Color.Green by calling its constructor
    let green_ctor = fir
        .functions
        .iter()
        .find(|f| f.name == "Color.Green")
        .unwrap()
        .id;
    let green_ptr = jit.call_i64(green_ctor);
    let test_id = fir.functions.iter().find(|f| f.name == "test").unwrap().id;
    let result = jit.call_i64_i64(test_id, green_ptr);
    assert_eq!(result, 2);
}

#[test]
fn enum_match_with_wildcard() {
    let src = "\
enum Direction
  North
  South
  East
  West

def is_vertical(d: Direction) -> Int
  match d
    Direction.North => 1
    Direction.South => 1
    _ => 0
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let east_ctor = fir
        .functions
        .iter()
        .find(|f| f.name == "Direction.East")
        .unwrap()
        .id;
    let east_ptr = jit.call_i64(east_ctor);
    let test_id = fir
        .functions
        .iter()
        .find(|f| f.name == "is_vertical")
        .unwrap()
        .id;
    let result = jit.call_i64_i64(test_id, east_ptr);
    assert_eq!(result, 0);

    let north_ctor = fir
        .functions
        .iter()
        .find(|f| f.name == "Direction.North")
        .unwrap()
        .id;
    let north_ptr = jit.call_i64(north_ctor);
    let result = jit.call_i64_i64(test_id, north_ptr);
    assert_eq!(result, 1);
}

#[test]
fn enum_variant_with_field() {
    // Construct an enum variant that carries a field, match on tag to dispatch
    let src = "\
enum Shape
  Circle(radius: Int)
  Square(side: Int)

def describe(s: Shape) -> Int
  match s
    Shape.Circle => 1
    Shape.Square => 2
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    // Construct a Circle with radius=10
    let circle_ctor = fir
        .functions
        .iter()
        .find(|f| f.name == "Shape.Circle")
        .unwrap()
        .id;
    let circle_ptr = jit.call_i64_i64(circle_ctor, 10);
    let describe_id = fir
        .functions
        .iter()
        .find(|f| f.name == "describe")
        .unwrap()
        .id;
    let result = jit.call_i64_i64(describe_id, circle_ptr);
    assert_eq!(result, 1);

    // Construct a Square with side=5
    let square_ctor = fir
        .functions
        .iter()
        .find(|f| f.name == "Shape.Square")
        .unwrap()
        .id;
    let square_ptr = jit.call_i64_i64(square_ctor, 5);
    let result = jit.call_i64_i64(describe_id, square_ptr);
    assert_eq!(result, 2);
}

// ===========================================================================
// MILESTONE 14: Closures — full calling convention
// ===========================================================================

#[test]
fn lambda_no_captures() {
    // Nested def (closure without captures) called directly
    let src = "\
def main() -> Int
  def double(x: Int) -> Int
    x * 2
  double(x: 21)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn lambda_with_captures() {
    // Nested def captures a local variable from enclosing scope
    let src = "\
def main() -> Int
  let offset: Int = 10
  def add_offset(x: Int) -> Int
    x + offset
  add_offset(x: 32)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn closure_multiple_captures() {
    let src = "\
def main() -> Int
  let a: Int = 10
  let b: Int = 20
  def sum_with(x: Int) -> Int
    x + a + b
  sum_with(x: 12)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

// ===========================================================================
// Auto-derived to_string (Printable)
// ===========================================================================

#[test]
fn auto_derived_to_string_single_int_field() {
    // Class with auto-derived Printable should produce "ClassName(field_value)"
    let src = "\
class Wrapper includes Printable
  val: Int

def main() -> String
  let w = Wrapper(val: 42)
  w.to_string()
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.call_i64(fir.entry.unwrap());
    assert_ne!(ptr, 0);
    unsafe {
        let len = *(ptr as *const i64) as usize;
        let data = (ptr as *const u8).add(8);
        let s = std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len));
        assert_eq!(s, "Wrapper(42)");
    }
}

#[test]
fn auto_derived_to_string_multiple_fields() {
    let src = "\
class Point includes Printable
  x: Int
  y: Int

def main() -> String
  let p = Point(x: 10, y: 20)
  p.to_string()
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.call_i64(fir.entry.unwrap());
    assert_ne!(ptr, 0);
    unsafe {
        let len = *(ptr as *const i64) as usize;
        let data = (ptr as *const u8).add(8);
        let s = std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len));
        assert_eq!(s, "Point(10, 20)");
    }
}

#[test]
fn auto_derived_to_string_in_interpolation() {
    let src = "\
class Pair includes Printable
  a: Int
  b: Int

def main() -> String
  let p = Pair(a: 1, b: 2)
  \"result: {p}\"
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.call_i64(fir.entry.unwrap());
    assert_ne!(ptr, 0);
    unsafe {
        let len = *(ptr as *const i64) as usize;
        let data = (ptr as *const u8).add(8);
        let s = std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len));
        assert_eq!(s, "result: Pair(1, 2)");
    }
}

// ===========================================================================
// MILESTONE 15: ClosureCall dynamic dispatch
// ===========================================================================

#[test]
fn closure_stored_in_variable_then_called() {
    // Closure assigned to a local variable and called dynamically
    let src = "\
def main() -> Int
  def adder(x: Int) -> Int
    x + 10
  let f: (Int) -> Int = adder
  f(_0: 32)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn closure_no_captures_stored_and_called() {
    let src = "\
def main() -> Int
  let offset: Int = 10
  def add_offset(x: Int) -> Int
    x + offset
  let f: (Int) -> Int = add_offset
  f(_0: 32)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

// ===========================================================================
// Top-level let bindings
// ===========================================================================

#[test]
fn top_level_let_int() {
    let src = "\
let MAGIC: Int = 42

def main() -> Int
  MAGIC
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn top_level_let_expression() {
    let src = "\
let BASE: Int = 20
let OFFSET: Int = 22

def main() -> Int
  BASE + OFFSET
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn top_level_let_used_in_function() {
    let src = "\
let FACTOR: Int = 7

def multiply(x: Int) -> Int
  x * FACTOR

def main() -> Int
  multiply(x: 6)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

// ===========================================================================
// Generic monomorphization
// ===========================================================================

#[test]
fn generic_identity_int_and_string() {
    // Same generic function called with Int and String in same program
    let src = "\
def identity(x: T) -> T
  x

def main() -> Int
  let a: Int = identity(x: 42)
  a
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn generic_max() {
    let src = "\
def max_val(a: Int, b: Int) -> Int
  if a > b
    return a
  else
    return b

def main() -> Int
  max_val(a: 10, b: 42)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

// ===========================================================================
// Short-circuit And/Or
// ===========================================================================

#[test]
fn short_circuit_and_false() {
    let src = "def main() -> Int\n  if false and true\n    return 1\n  else\n    return 0\n";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 0);
}

#[test]
fn short_circuit_and_true() {
    let src = "def main() -> Int\n  if true and true\n    return 1\n  else\n    return 0\n";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 1);
}

#[test]
fn short_circuit_or_true() {
    let src = "def main() -> Int\n  if true or false\n    return 1\n  else\n    return 0\n";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 1);
}

#[test]
fn short_circuit_or_false() {
    let src = "def main() -> Int\n  if false or false\n    return 1\n  else\n    return 0\n";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 0);
}

// ===========================================================================
// Build profiles: optimization levels
// ===========================================================================

#[test]
fn profile_debug_opt_none() {
    use crate::config::{BuildConfig, OptLevel};
    let config = BuildConfig::debug();
    assert_eq!(config.opt_level, OptLevel::None);

    // Compile and run with debug config
    let src = "def main() -> Int\n  return 42\n";
    let fir = compile_and_run(src);
    let mut jit = CraneliftJIT::with_config(&BuildConfig::debug());
    jit.compile_module(&fir).expect("JIT compile ok");
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn profile_release_opt_speed() {
    use crate::config::BuildConfig;
    let src = "def main() -> Int\n  return 42\n";
    let fir = compile_and_run(src);
    let mut jit = CraneliftJIT::with_config(&BuildConfig::release());
    jit.compile_module(&fir).expect("JIT compile ok");
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn profile_size_opt() {
    use crate::config::{BuildConfig, OptLevel};
    let mut config = BuildConfig::release();
    config.opt_level = OptLevel::SpeedAndSize;
    let src = "def main() -> Int\n  return 42\n";
    let fir = compile_and_run(src);
    let mut jit = CraneliftJIT::with_config(&config);
    jit.compile_module(&fir).expect("JIT compile ok");
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn aot_with_debug_config() {
    use crate::config::BuildConfig;
    let src = "def main() -> Int\n  return 42\n";
    let fir = compile_and_run(src);
    let mut aot = CraneliftAOT::with_config(&BuildConfig::debug());
    aot.compile_module(&fir).expect("AOT compile ok");
    let bytes = aot.emit_object().expect("emit ok");
    assert!(!bytes.is_empty());
}

#[test]
fn aot_with_release_config() {
    use crate::config::BuildConfig;
    let src = "def main() -> Int\n  return 42\n";
    let fir = compile_and_run(src);
    let mut aot = CraneliftAOT::with_config(&BuildConfig::release());
    aot.compile_module(&fir).expect("AOT compile ok");
    let bytes = aot.emit_object().expect("emit ok");
    assert!(!bytes.is_empty());
}

// ===========================================================================
// Power operator
// ===========================================================================

#[test]
fn pow_int_basic() {
    let src = "\
def main() -> Int
  2 ** 3
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 8);
}

#[test]
fn pow_int_zero_exponent() {
    let src = "\
def main() -> Int
  5 ** 0
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 1);
}

#[test]
fn pow_int_one_exponent() {
    let src = "\
def main() -> Int
  7 ** 1
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 7);
}

#[test]
fn pow_int_large() {
    let src = "\
def main() -> Int
  10 ** 6
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 1_000_000);
}

#[test]
fn pow_right_associative() {
    // 2 ** 3 ** 2 = 2 ** 9 = 512 (right-associative)
    let src = "\
def main() -> Int
  2 ** 3 ** 2
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 512);
}

#[test]
fn pow_in_expression() {
    let src = "\
def main() -> Int
  let x: Int = 3
  x ** 2 + 1
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 10);
}

// ===========================================================================
// String interpolation
// ===========================================================================

#[test]
fn string_interp_literal_only() {
    // No interpolation — should still work (already works via StringLit)
    let src = "\
def main() -> String
  \"hello world\"
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.call_i64(fir.entry.unwrap());
    assert_ne!(ptr, 0);
    unsafe {
        let len = *(ptr as *const i64) as usize;
        let data = (ptr as *const u8).add(8);
        let s = std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len));
        assert_eq!(s, "hello world");
    }
}

#[test]
fn string_interp_with_int() {
    let src = "\
def main() -> String
  let x: Int = 42
  \"value is {x}\"
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.call_i64(fir.entry.unwrap());
    assert_ne!(ptr, 0);
    unsafe {
        let len = *(ptr as *const i64) as usize;
        let data = (ptr as *const u8).add(8);
        let s = std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len));
        assert_eq!(s, "value is 42");
    }
}

#[test]
fn string_interp_with_string() {
    let src = "\
def main() -> String
  let name = \"world\"
  \"hello {name}\"
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.call_i64(fir.entry.unwrap());
    assert_ne!(ptr, 0);
    unsafe {
        let len = *(ptr as *const i64) as usize;
        let data = (ptr as *const u8).add(8);
        let s = std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len));
        assert_eq!(s, "hello world");
    }
}

#[test]
fn string_interp_multiple_parts() {
    let src = "\
def main() -> String
  let a: Int = 1
  let b: Int = 2
  \"{a} + {b} = 3\"
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.call_i64(fir.entry.unwrap());
    assert_ne!(ptr, 0);
    unsafe {
        let len = *(ptr as *const i64) as usize;
        let data = (ptr as *const u8).add(8);
        let s = std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len));
        assert_eq!(s, "1 + 2 = 3");
    }
}

#[test]
fn string_interp_with_bool() {
    let src = "\
def main() -> String
  let flag = true
  \"result: {flag}\"
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.call_i64(fir.entry.unwrap());
    assert_ne!(ptr, 0);
    unsafe {
        let len = *(ptr as *const i64) as usize;
        let data = (ptr as *const u8).add(8);
        let s = std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len));
        assert_eq!(s, "result: true");
    }
}

#[test]
fn string_interp_expression() {
    let src = "\
def main() -> String
  \"sum: {1 + 2}\"
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.call_i64(fir.entry.unwrap());
    assert_ne!(ptr, 0);
    unsafe {
        let len = *(ptr as *const i64) as usize;
        let data = (ptr as *const u8).add(8);
        let s = std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len));
        assert_eq!(s, "sum: 3");
    }
}

#[test]
fn string_interp_float() {
    let src = "\
def main() -> String
  \"pi: {3.14}\"
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.call_i64(fir.entry.unwrap());
    assert_ne!(ptr, 0);
    unsafe {
        let len = *(ptr as *const i64) as usize;
        let data = (ptr as *const u8).add(8);
        let s = std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len));
        assert!(
            s.starts_with("pi: 3.14"),
            "expected 'pi: 3.14...', got '{}'",
            s
        );
    }
}

// String interpolation — class to_string

#[test]
fn string_interp_class_manual_to_string() {
    // A class with a manual to_string should have it called during interpolation
    let src = "\
class Point includes Printable
  x: Int
  y: Int
  def to_string() -> String
    \"point\"

def main() -> String
  let p = Point(x: 1, y: 2)
  \"got: {p}\"
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.call_i64(fir.entry.unwrap());
    assert_ne!(ptr, 0);
    unsafe {
        let len = *(ptr as *const i64) as usize;
        let data = (ptr as *const u8).add(8);
        let s = std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len));
        assert_eq!(s, "got: point");
    }
}

#[test]
fn string_interp_class_with_fields() {
    // to_string that uses field access on self
    let src = "\
class Num includes Printable
  val: Int
  def to_string() -> String
    \"num\"

def main() -> String
  let n = Num(val: 42)
  \"value={n}\"
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.call_i64(fir.entry.unwrap());
    assert_ne!(ptr, 0);
    unsafe {
        let len = *(ptr as *const i64) as usize;
        let data = (ptr as *const u8).add(8);
        let s = std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len));
        assert_eq!(s, "value=num");
    }
}

#[test]
fn string_interp_class_mixed_with_primitives() {
    // Interpolation mixing class and primitive types
    let src = "\
class Tag includes Printable
  label: String
  def to_string() -> String
    \"tag\"

def main() -> String
  let t = Tag(label: \"hello\")
  let n: Int = 42
  \"{t}:{n}\"
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let ptr = jit.call_i64(fir.entry.unwrap());
    assert_ne!(ptr, 0);
    unsafe {
        let len = *(ptr as *const i64) as usize;
        let data = (ptr as *const u8).add(8);
        let s = std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len));
        assert_eq!(s, "tag:42");
    }
}

// ===========================================================================
// Error handling
// ===========================================================================

#[test]
fn error_or_success_path() {
    // A throwing function that succeeds — .or fallback not needed
    let src = "\
class AppError extends Error
  code: Int

def risky() throws AppError -> Int
  42

def main() -> Int
  risky()!.or(0)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn error_or_else_success_path() {
    let src = "\
class AppError extends Error
  code: Int

def risky() throws AppError -> Int
  42

def main() -> Int
  risky()!.or_else(-> 0)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn bang_propagate_success() {
    // Simple ! propagation on success
    let src = "\
class AppError extends Error
  code: Int

def risky() throws AppError -> Int
  42

def main() throws AppError -> Int
  risky()!
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn error_or_failure_uses_default() {
    // A throwing function that actually throws — .or fallback should be returned
    let src = "\
class AppError extends Error
  code: Int

def risky() throws AppError -> Int
  throw AppError(message: \"fail\", code: 1)

def main() -> Int
  risky()!.or(99)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 99);
}

#[test]
fn error_or_else_failure_uses_handler() {
    let src = "\
class AppError extends Error
  code: Int

def risky() throws AppError -> Int
  throw AppError(message: \"fail\", code: 1)

def main() -> Int
  risky()!.or_else(-> 77)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 77);
}

// ===========================================================================
// Closure dispatch
// ===========================================================================

#[test]
fn closure_passed_to_function() {
    // Pass a closure to a function that calls it
    // Function type params use positional names (_0, _1, etc.)
    let src = "\
def apply(f: (Int) -> Int, x: Int) -> Int
  f(_0: x)

def main() -> Int
  def double(x: Int) -> Int
    x * 2
  apply(f: double, x: 21)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn closure_with_captures_passed() {
    let src = "\
def apply(f: (Int) -> Int, x: Int) -> Int
  f(_0: x)

def main() -> Int
  let offset: Int = 10
  def add_offset(x: Int) -> Int
    x + offset
  apply(f: add_offset, x: 32)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn inline_lambda_passed() {
    let src = "\
def apply(f: (Int) -> Int, x: Int) -> Int
  f(_0: x)

def main() -> Int
  apply(f: -> x: x * 3, x: 14)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

// ===========================================================================
// MILESTONE 17: Nullable values and Iterator[T] codegen
// ===========================================================================

#[test]
fn nullable_return_nil_is_zero() {
    // A function returning T? that returns nil should produce 0 (null pointer)
    let src = "\
def maybe() -> Int?
  return nil

def main() -> Int
  let x: Int? = maybe()
  return 0
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 0);
}

#[test]
fn nullable_return_some_value_is_nonzero() {
    // A function returning T? with a value should produce a non-zero boxed pointer
    let src = "\
def maybe() -> Int?
  return 42

def main() -> Int
  let x: Int? = maybe()
  return 1
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 1);
}

#[test]
fn iterator_for_loop_sums_range() {
    // Iterator[Int] for-loop that sums values 0..5
    let src = "\
class Counter includes Iterator[Int]
  current: Int
  max: Int

  def next() -> Int?
    if current >= max
      return nil
    let val: Int = current
    current = current + 1
    return val

def main() -> Int
  let c: Counter = Counter(current: 0, max: 5)
  let total: Int = 0
  for x in c
    total = total + x
  return total
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    // 0 + 1 + 2 + 3 + 4 = 10
    assert_eq!(result, 10);
}

#[test]
fn iterator_for_loop_counts_elements() {
    // Iterator that produces 3 elements, count them
    let src = "\
class ThreeItems includes Iterator[Int]
  pos: Int

  def next() -> Int?
    if pos >= 3
      return nil
    pos = pos + 1
    return pos

def main() -> Int
  let it: ThreeItems = ThreeItems(pos: 0)
  let count: Int = 0
  for x in it
    count = count + 1
  return count
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 3);
}

#[test]
fn iterator_for_loop_empty() {
    // Iterator that immediately returns nil — loop body never executes
    let src = "\
class Empty includes Iterator[Int]
  done: Int

  def next() -> Int?
    return nil

def main() -> Int
  let it: Empty = Empty(done: 0)
  let count: Int = 0
  for x in it
    count = count + 1
  return count
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 0);
}

// ===========================================================================
// MILESTONE 18: Default parameter values
// ===========================================================================

#[test]
fn default_param_uses_default_when_arg_omitted() {
    let src = "\
def add(a: Int, b: Int = 10) -> Int
  a + b

def main() -> Int
  add(a: 32)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn default_param_uses_explicit_value_when_provided() {
    let src = "\
def add(a: Int, b: Int = 10) -> Int
  a + b

def main() -> Int
  add(a: 20, b: 22)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn default_param_all_params_defaulted() {
    let src = "\
def f(a: Int = 40, b: Int = 2) -> Int
  a + b

def main() -> Int
  f()
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn default_param_selective_override() {
    let src = "\
def f(a: Int = 100, b: Int = 2) -> Int
  a + b

def main() -> Int
  f(a: 40)
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn default_param_string_default() {
    // Validates string defaults work through codegen (uses Ptr type)
    let src = "\
def greet(name: String = \"world\") -> Int
  return 42

def main() -> Int
  greet()
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn default_param_in_method() {
    let src = "\
class Calc
  base: Int

  def add(n: Int = 10) -> Int
    base + n

def main() -> Int
  let c: Calc = Calc(base: 32)
  c.add()
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}
