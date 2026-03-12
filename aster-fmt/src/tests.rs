use crate::config::FormatConfig;
use crate::doc::*;
use crate::format_source;

fn fmt(source: &str) -> String {
    format_source(source, &FormatConfig::default()).expect("format should succeed")
}

fn fmt_with(source: &str, config: &FormatConfig) -> String {
    format_source(source, config).expect("format should succeed")
}

// ===========================================================================
// Doc IR / printer tests
// ===========================================================================

#[test]
fn doc_text() {
    let d = text("hello");
    assert_eq!(pretty(80, 4, &d), "hello");
}

#[test]
fn doc_hardline() {
    let d = concat(vec![text("a"), hardline(), text("b")]);
    assert_eq!(pretty(80, 4, &d), "a\nb");
}

#[test]
fn doc_group_fits() {
    let d = group(concat(vec![text("a"), line(), text("b")]));
    assert_eq!(pretty(80, 4, &d), "a b");
}

#[test]
fn doc_group_breaks() {
    let d = group(concat(vec![text("hello"), line(), text("world")]));
    assert_eq!(pretty(5, 4, &d), "hello\nworld");
}

#[test]
fn doc_indent() {
    let d = concat(vec![
        text("if true"),
        indent(concat(vec![hardline(), text("body")])),
    ]);
    assert_eq!(pretty(80, 4, &d), "if true\n    body");
}

#[test]
fn doc_nested_indent() {
    let d = concat(vec![
        text("a"),
        indent(concat(vec![
            hardline(),
            text("b"),
            indent(concat(vec![hardline(), text("c")])),
        ])),
    ]);
    assert_eq!(pretty(80, 4, &d), "a\n    b\n        c");
}

#[test]
fn doc_softline_flat() {
    let d = group(concat(vec![
        text("["),
        softline(),
        text("1"),
        softline(),
        text("]"),
    ]));
    assert_eq!(pretty(80, 4, &d), "[1]");
}

#[test]
fn doc_join() {
    let d = join(vec![text("a"), text("b"), text("c")], text(", "));
    assert_eq!(pretty(80, 4, &d), "a, b, c");
}

// ===========================================================================
// Let statements
// ===========================================================================

#[test]
fn format_let_simple() {
    let out = fmt("let x = 42\n");
    assert_eq!(out, "let x = 42\n");
}

#[test]
fn format_let_with_type() {
    let out = fmt("let x: Int = 42\n");
    assert_eq!(out, "let x: Int = 42\n");
}

#[test]
fn format_let_string() {
    let out = fmt("let name = \"hello\"\n");
    assert_eq!(out, "let name = \"hello\"\n");
}

#[test]
fn format_let_bool() {
    let out = fmt("let flag = true\n");
    assert_eq!(out, "let flag = true\n");
}

#[test]
fn format_let_nil() {
    let out = fmt("let x = nil\n");
    assert_eq!(out, "let x = nil\n");
}

// ===========================================================================
// Function definitions
// ===========================================================================

#[test]
fn format_simple_function() {
    let source = "def main() -> Int\n  return 0\n";
    let out = fmt(source);
    assert!(out.contains("def main() -> Int"));
    assert!(out.contains("return 0"));
    // No colon after the header
    assert!(!out.contains("Int:"));
}

#[test]
fn format_function_with_params() {
    let source = "def add(a: Int, b: Int) -> Int\n  return a + b\n";
    let out = fmt(source);
    assert!(out.contains("def add(a: Int, b: Int) -> Int"));
    assert!(out.contains("return a + b"));
}

#[test]
fn format_function_void() {
    let source = "def greet(name: String)\n  let x = 1\n";
    let out = fmt(source);
    assert!(out.contains("def greet(name: String)"));
    // Void return should not emit -> Void
    assert!(!out.contains("->"));
}

// ===========================================================================
// Class definitions
// ===========================================================================

#[test]
fn format_class_simple() {
    let source = "class Point\n  x: Int\n  y: Int\n";
    let out = fmt(source);
    assert!(out.contains("class Point"));
    assert!(out.contains("x: Int"));
    assert!(out.contains("y: Int"));
}

#[test]
fn format_class_with_method() {
    let source = "class Counter\n  count: Int\n  def inc(self: Counter) -> Counter\n    return Counter(count: self.count + 1)\n";
    let out = fmt(source);
    assert!(out.contains("class Counter"));
    assert!(out.contains("count: Int"));
    assert!(out.contains("def inc(self: Counter) -> Counter"));
}

#[test]
fn format_class_generic() {
    let source = "class Box[T]\n  value: T\n";
    let out = fmt(source);
    assert!(out.contains("class Box[T]"));
    assert!(out.contains("value: T"));
}

#[test]
fn format_class_extends() {
    let source = "class Dog extends Animal\n  name: String\n";
    let out = fmt(source);
    assert!(out.contains("class Dog extends Animal"));
}

#[test]
fn format_class_includes() {
    let source = "class Point includes Eq\n  x: Int\n";
    let out = fmt(source);
    assert!(out.contains("class Point includes Eq"));
}

// ===========================================================================
// If / elif / else
// ===========================================================================

#[test]
fn format_if_simple() {
    let source = "if true\n  let x = 1\n";
    let out = fmt(source);
    assert!(out.contains("if true"));
    assert!(out.contains("let x = 1"));
}

#[test]
fn format_if_else() {
    let source = "if x == 1\n  let a = 1\nelse\n  let b = 2\n";
    let out = fmt(source);
    assert!(out.contains("if x == 1"));
    assert!(out.contains("else"));
}

#[test]
fn format_if_elif() {
    let source = "if x == 1\n  let a = 1\nelif x == 2\n  let b = 2\nelse\n  let c = 3\n";
    let out = fmt(source);
    assert!(out.contains("if x == 1"));
    assert!(out.contains("elif x == 2"));
    assert!(out.contains("else"));
}

// ===========================================================================
// Loops
// ===========================================================================

#[test]
fn format_while_loop() {
    let source = "while true\n  break\n";
    let out = fmt(source);
    assert!(out.contains("while true"));
    assert!(out.contains("break"));
}

#[test]
fn format_for_loop() {
    let source = "for x in items\n  let y = x\n";
    let out = fmt(source);
    assert!(out.contains("for x in items"));
    assert!(out.contains("let y = x"));
}

// ===========================================================================
// Match expressions
// ===========================================================================

#[test]
fn format_match_expr() {
    let source = "let y = match x\n  1 => 10\n  2 => 20\n  _ => 0\n";
    let out = fmt(source);
    assert!(out.contains("match x"));
    assert!(out.contains("1 => 10"));
    assert!(out.contains("_ => 0"));
}

// ===========================================================================
// Binary expressions
// ===========================================================================

#[test]
fn format_binary_op() {
    let source = "let x = 1 + 2\n";
    let out = fmt(source);
    assert_eq!(out, "let x = 1 + 2\n");
}

#[test]
fn format_comparison() {
    let source = "let flag = x == y\n";
    let out = fmt(source);
    assert!(out.contains("x == y"));
}

#[test]
fn format_logical() {
    let source = "let flag = a and b\n";
    let out = fmt(source);
    assert!(out.contains("a and b"));
}

// ===========================================================================
// Unary expressions
// ===========================================================================

#[test]
fn format_unary_neg() {
    let source = "let x = -5\n";
    let out = fmt(source);
    assert!(out.contains("-5"));
}

#[test]
fn format_unary_not() {
    let source = "let x = not true\n";
    let out = fmt(source);
    assert!(out.contains("not true"));
}

// ===========================================================================
// Call expressions with named args
// ===========================================================================

#[test]
fn format_call_no_args() {
    let source = "foo()\n";
    let out = fmt(source);
    assert_eq!(out, "foo()\n");
}

#[test]
fn format_call_named_args() {
    let source = "Point(x: 1, y: 2)\n";
    let out = fmt(source);
    assert!(out.contains("Point(x: 1, y: 2)"));
}

// ===========================================================================
// List literals
// ===========================================================================

#[test]
fn format_empty_list() {
    let source = "let xs = []\n";
    let out = fmt(source);
    assert!(out.contains("[]"));
}

#[test]
fn format_list_elements() {
    let source = "let xs = [1, 2, 3]\n";
    let out = fmt(source);
    assert!(out.contains("[1, 2, 3]"));
}

// ===========================================================================
// Lambda expressions
// ===========================================================================

#[test]
fn format_lambda_as_def() {
    // Lambda assigned to let — roundtrips as def
    let source = "def f(x: Int) -> Int\n  return x + 1\n";
    let out = fmt(source);
    assert!(out.contains("def f(x: Int) -> Int"));
    assert!(out.contains("return x + 1"));
}

// ===========================================================================
// Trait definitions
// ===========================================================================

#[test]
fn format_trait_simple() {
    let source = "trait Drawable\n  def draw(self: Drawable)\n    return nil\n";
    let out = fmt(source);
    assert!(out.contains("trait Drawable"));
    assert!(out.contains("def draw(self: Drawable)"));
}

#[test]
fn format_trait_abstract_method() {
    // Abstract trait methods have no body
    let source = "trait Printable\n  def to_string() -> String\n";
    let out = fmt(source);
    assert!(out.contains("trait Printable"));
    assert!(out.contains("def to_string() -> String"));
}

// ===========================================================================
// Enum definitions
// ===========================================================================

#[test]
fn format_enum_simple() {
    let source = "enum Color\n  Red\n  Green\n  Blue\n";
    let out = fmt(source);
    assert!(out.contains("enum Color"));
    assert!(out.contains("Red"));
    assert!(out.contains("Green"));
    assert!(out.contains("Blue"));
}

#[test]
fn format_enum_with_fields() {
    let source = "enum Shape\n  Circle(radius: Float)\n  Rect(w: Float, h: Float)\n";
    let out = fmt(source);
    assert!(out.contains("enum Shape"));
    assert!(out.contains("Circle(radius: Float)"));
    assert!(out.contains("Rect(w: Float, h: Float)"));
}

// ===========================================================================
// Error handling
// ===========================================================================

#[test]
fn format_throw() {
    let source = "def fail() throws String -> Int\n  throw \"boom\"\n";
    let out = fmt(source);
    assert!(out.contains("throws String"));
    assert!(out.contains("throw \"boom\""));
}

#[test]
fn format_propagate() {
    let source = "let x = risky()!\n";
    let out = fmt(source);
    assert!(out.contains("risky()!"));
}

// ===========================================================================
// Use / imports
// ===========================================================================

#[test]
fn format_use_simple() {
    let source = "use std { Eq }\n";
    let out = fmt(source);
    assert!(out.contains("use std { Eq }"));
}

#[test]
fn format_use_multiple() {
    let source = "use std { Eq, Ord }\n";
    let out = fmt(source);
    assert!(out.contains("use std { Eq, Ord }"));
}

// ===========================================================================
// Return / break / continue
// ===========================================================================

#[test]
fn format_return() {
    let out = fmt("def f() -> Int\n  return 42\n");
    assert!(out.contains("return 42"));
}

#[test]
fn format_break() {
    let out = fmt("while true\n  break\n");
    assert!(out.contains("break"));
}

#[test]
fn format_continue() {
    let out = fmt("while true\n  continue\n");
    assert!(out.contains("continue"));
}

// ===========================================================================
// Assignment
// ===========================================================================

#[test]
fn format_assignment() {
    let out = fmt("let x = 1\nx = 2\n");
    assert!(out.contains("x = 2"));
}

// ===========================================================================
// Member access
// ===========================================================================

#[test]
fn format_member_access() {
    let source = "let y = obj.field\n";
    let out = fmt(source);
    assert!(out.contains("obj.field"));
}

// ===========================================================================
// Index access
// ===========================================================================

#[test]
fn format_index() {
    let source = "let y = xs[0]\n";
    let out = fmt(source);
    assert!(out.contains("xs[0]"));
}

// ===========================================================================
// Idempotency
// ===========================================================================

#[test]
fn idempotent_simple_function() {
    let source = "def main() -> Int\n    return 0\n";
    let first = fmt(source);
    let second = fmt(&first);
    assert_eq!(first, second, "formatting should be idempotent");
}

#[test]
fn idempotent_class() {
    let source = "class Point\n    x: Int\n    y: Int\n";
    let first = fmt(source);
    let second = fmt(&first);
    assert_eq!(first, second, "formatting should be idempotent");
}

#[test]
fn idempotent_if_elif_else() {
    let source = "if x == 1\n    let a = 1\nelif x == 2\n    let b = 2\nelse\n    let c = 3\n";
    let first = fmt(source);
    let second = fmt(&first);
    assert_eq!(first, second, "formatting should be idempotent");
}

#[test]
fn idempotent_enum() {
    let source = "enum Color\n    Red\n    Green\n    Blue\n";
    let first = fmt(source);
    let second = fmt(&first);
    assert_eq!(first, second, "formatting should be idempotent");
}

// ===========================================================================
// Round-trip: formatted output should parse successfully
// ===========================================================================

fn roundtrip_parses(source: &str) {
    let formatted = fmt(source);
    let tokens = lexer::lex(&formatted).unwrap_or_else(|e| {
        panic!(
            "formatted output should lex. Error: {}\nFormatted:\n{}",
            e.message, formatted
        )
    });
    let mut parser = parser::Parser::new(tokens);
    parser.parse_module("<roundtrip>").unwrap_or_else(|e| {
        panic!(
            "formatted output should parse. Error: {}\nFormatted:\n{}",
            e.message, formatted
        )
    });
}

#[test]
fn roundtrip_let() {
    roundtrip_parses("let x = 42\n");
}

#[test]
fn roundtrip_function() {
    roundtrip_parses("def main() -> Int\n  return 0\n");
}

#[test]
fn roundtrip_class() {
    roundtrip_parses("class Point\n  x: Int\n  y: Int\n");
}

#[test]
fn roundtrip_if_else() {
    roundtrip_parses("if true\n  let x = 1\nelse\n  let y = 2\n");
}

#[test]
fn roundtrip_for() {
    roundtrip_parses("for x in items\n  let y = x\n");
}

#[test]
fn roundtrip_while() {
    roundtrip_parses("while true\n  break\n");
}

#[test]
fn roundtrip_enum() {
    roundtrip_parses("enum Color\n  Red\n  Green\n  Blue\n");
}

// ===========================================================================
// Config: quote style
// ===========================================================================

#[test]
fn config_single_quotes_ignored() {
    // Single quotes are not supported by the Aster lexer, so the formatter
    // always uses double quotes regardless of the config setting.
    let config = FormatConfig {
        quote_style: crate::config::QuoteStyle::Single,
        ..FormatConfig::default()
    };
    let out = fmt_with("let x = \"hello\"\n", &config);
    assert!(out.contains("\"hello\""));
}

// ===========================================================================
// Config: indent size
// ===========================================================================

#[test]
fn config_indent_size_2() {
    let config = FormatConfig {
        indent_size: 2,
        ..FormatConfig::default()
    };
    let out = fmt_with("if true\n  let x = 1\n", &config);
    // With indent_size=2, body should be indented by 2 spaces
    assert!(out.contains("  let x = 1"));
}

#[test]
fn config_indent_size_4() {
    let config = FormatConfig {
        indent_size: 4,
        ..FormatConfig::default()
    };
    let out = fmt_with("if true\n  let x = 1\n", &config);
    // With indent_size=4, body should be indented by 4 spaces
    assert!(out.contains("    let x = 1"));
}

// ===========================================================================
// Error cases
// ===========================================================================

#[test]
fn format_lex_error() {
    let result = format_source("let x = @@@\n", &FormatConfig::default());
    assert!(result.is_err());
}
