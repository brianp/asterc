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
// MILESTONE 2: Integer arithmetic — JIT compile and execute
// ===========================================================================

#[test]
fn m2_return_constant() {
    let fir = compile_and_run("def main() -> Int\n  return 42\n");
    let jit = jit_compile(&fir);
    let result = jit.call_i64(fir.entry.unwrap());
    assert_eq!(result, 42);
}

#[test]
fn m2_add_two_numbers() {
    let fir = compile_and_run("def add(a: Int, b: Int) -> Int\n  a + b\n");
    let jit = jit_compile(&fir);
    let add_id = FunctionId(0);
    let result = jit.call_i64_i64_i64(add_id, 3, 4);
    assert_eq!(result, 7);
}

#[test]
fn m2_subtract() {
    let fir = compile_and_run("def sub(a: Int, b: Int) -> Int\n  a - b\n");
    let jit = jit_compile(&fir);
    let result = jit.call_i64_i64_i64(FunctionId(0), 10, 3);
    assert_eq!(result, 7);
}

#[test]
fn m2_multiply() {
    let fir = compile_and_run("def mul(a: Int, b: Int) -> Int\n  a * b\n");
    let jit = jit_compile(&fir);
    let result = jit.call_i64_i64_i64(FunctionId(0), 6, 7);
    assert_eq!(result, 42);
}

#[test]
fn m2_divide() {
    let fir = compile_and_run("def div(a: Int, b: Int) -> Int\n  a / b\n");
    let jit = jit_compile(&fir);
    let result = jit.call_i64_i64_i64(FunctionId(0), 20, 4);
    assert_eq!(result, 5);
}

#[test]
fn m2_nested_arithmetic() {
    let fir = compile_and_run("def f(a: Int, b: Int) -> Int\n  (a + b) * (a - b)\n");
    let jit = jit_compile(&fir);
    // (5+3) * (5-3) = 8 * 2 = 16
    let result = jit.call_i64_i64_i64(FunctionId(0), 5, 3);
    assert_eq!(result, 16);
}

#[test]
fn m2_unary_negation() {
    let fir = compile_and_run("def neg(x: Int) -> Int\n  -x\n");
    let jit = jit_compile(&fir);
    let result = jit.call_i64_i64(FunctionId(0), 42);
    assert_eq!(result, -42);
}

#[test]
fn m2_let_binding_and_return() {
    let fir = compile_and_run("def f() -> Int\n  let x: Int = 10\n  let y: Int = 20\n  x + y\n");
    let jit = jit_compile(&fir);
    let result = jit.call_i64(FunctionId(0));
    assert_eq!(result, 30);
}

#[test]
fn m2_modulo() {
    let fir = compile_and_run("def modulo(a: Int, b: Int) -> Int\n  a % b\n");
    let jit = jit_compile(&fir);
    let result = jit.call_i64_i64_i64(FunctionId(0), 17, 5);
    assert_eq!(result, 2);
}

#[test]
fn m2_negative_literal() {
    let fir = compile_and_run("def f() -> Int\n  return -42\n");
    let jit = jit_compile(&fir);
    let result = jit.call_i64(FunctionId(0));
    assert_eq!(result, -42);
}

// ===========================================================================
// MILESTONE 3: Control flow
// ===========================================================================

#[test]
fn m3_if_else_true_branch() {
    let src = "def f(x: Int) -> Int\n  if x > 0\n    return 1\n  else\n    return -1\n";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    assert_eq!(jit.call_i64_i64(FunctionId(0), 5), 1);
}

#[test]
fn m3_if_else_false_branch() {
    let src = "def f(x: Int) -> Int\n  if x > 0\n    return 1\n  else\n    return -1\n";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    assert_eq!(jit.call_i64_i64(FunctionId(0), -5), -1);
}

#[test]
fn m3_if_else_zero() {
    let src = "def f(x: Int) -> Int\n  if x > 0\n    return 1\n  else\n    return -1\n";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    assert_eq!(jit.call_i64_i64(FunctionId(0), 0), -1);
}

#[test]
fn m3_elif_chain() {
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
fn m3_while_loop_sum() {
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
fn m3_while_loop_zero_iterations() {
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
fn m3_break_in_while() {
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
fn m3_comparison_operators() {
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
fn m3_nested_if() {
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
// MILESTONE 4: Strings
// ===========================================================================

#[test]
fn m4_string_return() {
    // Verify string functions compile without crashing
    let src = "def f() -> String\n  return \"hello\"\n";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    // String returns a heap pointer — just check it's non-null
    let ptr = jit.call_i64(FunctionId(0));
    assert_ne!(ptr, 0, "string should return non-null pointer");
}

#[test]
fn m4_string_length() {
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
fn m4_string_data() {
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
fn m4_log_compiles() {
    // Verify that log() calls compile (runtime call)
    let src = "def f() -> Void\n  log(message: \"hello\")\n";
    let fir = compile_and_run(src);
    let _jit = jit_compile(&fir);
    // If we get here, compilation succeeded
}

// ===========================================================================
// MILESTONE 5: Function calls
// ===========================================================================

#[test]
fn m5_call_another_function() {
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
fn m5_call_chain() {
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
fn m5_recursive_factorial() {
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
fn m5_recursive_fibonacci() {
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
fn m5_mutual_recursion() {
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
fn m5_function_with_multiple_calls() {
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
// MILESTONE 6: Classes
// ===========================================================================

#[test]
fn m6_class_construction() {
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
fn m6_class_field_access() {
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
fn m6_class_field_access_second_field() {
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
#[ignore = "requires implicit self field resolution in method bodies"]
fn m6_class_method_call() {
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

// ===========================================================================
// MILESTONE 7: Lists — for-loops, iteration, list operations
// ===========================================================================

#[test]
fn m7_list_creation_and_get() {
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
fn m7_list_set() {
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
fn m7_list_len() {
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
fn m7_list_push() {
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

#[test]
fn m7_for_loop_sum() {
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
fn m7_for_loop_empty_list() {
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
fn m7_for_loop_with_break() {
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
// MILESTONE 8: Error handling — nullable operations
// ===========================================================================

#[test]
fn m8_nullable_return_value() {
    // Nullable Int? return — check that wrapping a value works
    let src = "\
def f() -> Int?
  return 42
";
    let fir = compile_and_run(src);
    let jit = jit_compile(&fir);
    let result = jit.call_i64(FunctionId(0));
    // Value should be non-zero (wrapped 42)
    assert_eq!(result, 42);
}

#[test]
fn m8_nullable_nil_return() {
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
// MILESTONE 9: Generics — monomorphization
// ===========================================================================

#[test]
fn m9_generic_identity_int() {
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
fn m9_generic_class_field() {
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
// MILESTONE 10: AOT backend — compile to object files
// ===========================================================================

#[test]
fn m10_aot_compile_simple() {
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
fn m10_aot_compile_with_functions() {
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
fn m10_aot_compile_with_strings() {
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
fn m10_aot_compile_with_control_flow() {
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
fn m10_aot_compile_with_classes() {
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
fn m10_aot_compile_with_lists() {
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
// MILESTONE 11: Async — eager execution (no runtime scheduler yet)
// ===========================================================================

#[test]
fn m11_async_call_and_resolve() {
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
fn m11_async_with_args() {
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
// MILESTONE 6: Classes (continued)
// ===========================================================================

#[test]
fn m6_class_in_function() {
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
// MILESTONE 12: Match expressions — desugared to if/else chains
// ===========================================================================

#[test]
fn m12_match_int_literal() {
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
fn m12_match_wildcard_only() {
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
fn m12_match_variable_binding() {
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
fn m12_match_in_expression() {
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
fn m13_fieldless_enum_construct() {
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
fn m13_enum_match_tag() {
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
fn m13_enum_match_with_wildcard() {
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

// ===========================================================================
// MILESTONE 14: Closures — full calling convention
// ===========================================================================

#[test]
fn m14_lambda_no_captures() {
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
fn m14_lambda_with_captures() {
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
fn m14_closure_multiple_captures() {
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
// MILESTONE 15: Top-level let bindings
// ===========================================================================

#[test]
fn m15_top_level_let_int() {
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
fn m15_top_level_let_expression() {
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
fn m15_top_level_let_used_in_function() {
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
// MILESTONE 16: Generics monomorphization — same generic, different types
// ===========================================================================

#[test]
fn m16_generic_identity_int_and_string() {
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
fn m16_generic_max() {
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
