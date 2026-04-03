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
    assert_eq!(pretty(80, 2, &d), "hello");
}

#[test]
fn doc_hardline() {
    let d = concat(vec![text("a"), hardline(), text("b")]);
    assert_eq!(pretty(80, 2, &d), "a\nb");
}

#[test]
fn doc_group_fits() {
    let d = group(concat(vec![text("a"), line(), text("b")]));
    assert_eq!(pretty(80, 2, &d), "a b");
}

#[test]
fn doc_group_breaks() {
    let d = group(concat(vec![text("hello"), line(), text("world")]));
    assert_eq!(pretty(5, 2, &d), "hello\nworld");
}

#[test]
fn doc_indent() {
    let d = concat(vec![
        text("if true"),
        indent(concat(vec![hardline(), text("body")])),
    ]);
    assert_eq!(pretty(80, 2, &d), "if true\n  body");
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
    assert_eq!(pretty(80, 2, &d), "a\n  b\n    c");
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
    assert_eq!(pretty(80, 2, &d), "[1]");
}

#[test]
fn doc_join() {
    let d = join(vec![text("a"), text("b"), text("c")], text(", "));
    assert_eq!(pretty(80, 2, &d), "a, b, c");
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
// Function definitions — implicit return (last return stripped)
// ===========================================================================

#[test]
fn format_simple_function() {
    // main() -> Int returning 0 is redundant; formatter strips it
    let source = "def main() -> Int\n  return 0\n";
    let out = fmt(source);
    assert_eq!(out, "def main()\n");
}

#[test]
fn format_function_with_params() {
    let source = "def add(a: Int, b: Int) -> Int\n  return a + b\n";
    let out = fmt(source);
    assert_eq!(out, "def add(a: Int, b: Int) -> Int\n  a + b\n");
}

#[test]
fn format_function_void() {
    let source = "def greet(name: String)\n  let x = 1\n";
    let out = fmt(source);
    assert!(out.contains("def greet(name: String)"));
    // Void return should not emit -> Void
    assert!(!out.contains("->"));
}

#[test]
fn format_function_early_return_kept() {
    let source = "def f(x: Int) -> Int\n  if x > 0\n    return x\n  return 0\n";
    let out = fmt(source);
    // Early return in if is kept
    assert!(out.contains("return x"));
    // Last return is stripped
    assert!(out.contains("\n  0\n"));
}

#[test]
fn format_function_multi_stmt_last_stripped() {
    let source = "def f(x: Int) -> Int\n  let y = x * 2\n  return y\n";
    let out = fmt(source);
    assert!(out.contains("let y = x * 2"));
    // Last return stripped
    assert!(!out.contains("return y"));
    assert!(out.contains("\n  y\n"));
}

#[test]
fn format_blocking_call() {
    let source = "def fetch() -> Int\n  async fetch_child()\n  42\n\ndef main() -> Int\n  blocking fetch()\n";
    let out = fmt(source);
    assert!(out.contains("blocking fetch()"));
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
    // Lambda assigned to let — roundtrips as def, return stripped
    let source = "def f(x: Int) -> Int\n  return x + 1\n";
    let out = fmt(source);
    assert!(out.contains("def f(x: Int) -> Int"));
    assert!(out.contains("x + 1"));
    assert!(!out.contains("return"));
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
// Use / imports — merging, alphabetizing, grouping
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

#[test]
fn import_merge_same_path() {
    let source = "use std { Ord }\nuse std { Eq }\n";
    let out = fmt(source);
    // Should merge into one use statement with alphabetized names
    assert!(out.contains("use std { Eq, Ord }"));
    // Should NOT have two separate use statements
    assert_eq!(out.matches("use std").count(), 1);
}

#[test]
fn import_alphabetize_names() {
    let source = "use std { Ord, Eq, Printable }\n";
    let out = fmt(source);
    assert!(out.contains("use std { Eq, Ord, Printable }"));
}

#[test]
fn import_alphabetize_statements() {
    let source = "use std/fmt { Printable }\nuse std/cmp { Eq }\n";
    let out = fmt(source);
    // std/cmp comes before std/fmt alphabetically
    let cmp_pos = out.find("std/cmp").unwrap();
    let fmt_pos = out.find("std/fmt").unwrap();
    assert!(cmp_pos < fmt_pos, "std/cmp should come before std/fmt");
}

#[test]
fn import_grouping_three_groups() {
    let source = "use myapp { Foo }\nuse http { Request }\nuse std { Eq }\n";
    let out = fmt(source);
    // std first, then third-party (http), then app (myapp)
    let std_pos = out.find("use std").unwrap();
    let http_pos = out.find("use http").unwrap();
    let app_pos = out.find("use myapp").unwrap();
    assert!(std_pos < http_pos, "std should come before http");
    assert!(http_pos < app_pos, "http should come before myapp");
}

#[test]
fn import_merge_dedup() {
    let source = "use std { Eq }\nuse std { Eq, Ord }\n";
    let out = fmt(source);
    assert!(out.contains("use std { Eq, Ord }"));
    assert_eq!(out.matches("use std").count(), 1);
}

// ===========================================================================
// Return / break / continue
// ===========================================================================

#[test]
fn format_return_last_stripped() {
    // Last return in function body is stripped (implicit return)
    let out = fmt("def f() -> Int\n  return 42\n");
    assert_eq!(out, "def f() -> Int\n  42\n");
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
// Idempotency (2-space indent)
// ===========================================================================

#[test]
fn idempotent_simple_function() {
    let source = "def main() -> Int\n  0\n";
    let first = fmt(source);
    let second = fmt(&first);
    assert_eq!(first, second, "formatting should be idempotent");
}

#[test]
fn idempotent_class() {
    let source = "class Point\n  x: Int\n  y: Int\n";
    let first = fmt(source);
    let second = fmt(&first);
    assert_eq!(first, second, "formatting should be idempotent");
}

#[test]
fn idempotent_if_elif_else() {
    let source = "if x == 1\n  let a = 1\nelif x == 2\n  let b = 2\nelse\n  let c = 3\n";
    let first = fmt(source);
    let second = fmt(&first);
    assert_eq!(first, second, "formatting should be idempotent");
}

#[test]
fn idempotent_enum() {
    let source = "enum Color\n  Red\n  Green\n  Blue\n";
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
    let config = FormatConfig {
        quote_style: crate::config::QuoteStyle::Single,
        ..FormatConfig::default()
    };
    let out = fmt_with("let x = \"hello\"\n", &config);
    assert!(out.contains("\"hello\""));
}

// ===========================================================================
// Config: default indent is 2
// ===========================================================================

#[test]
fn config_default_indent_is_2() {
    assert_eq!(FormatConfig::default().indent_size, 2);
}

#[test]
fn config_default_indent_produces_2_spaces() {
    let out = fmt("if true\n  let x = 1\n");
    assert_eq!(out, "if true\n  let x = 1\n");
}

#[test]
fn config_indent_size_4_explicit() {
    let config = FormatConfig {
        indent_size: 4,
        ..FormatConfig::default()
    };
    let out = fmt_with("if true\n  let x = 1\n", &config);
    assert!(out.contains("    let x = 1"));
}

// ===========================================================================
// Signature wrapping — packs to 2/3 width, aligns to paren
// ===========================================================================

#[test]
fn sig_wrap_short_fits_one_line() {
    let source = "def add(x: Int, y: Int) -> Int\n  x + y\n";
    let out = fmt(source);
    assert!(out.contains("def add(x: Int, y: Int) -> Int"));
}

#[test]
fn sig_long_params_stay_on_one_line() {
    // Aster's parser does not support multi-line parameter lists, so even
    // long signatures must stay on a single line.
    let source = "def transform(input: String, count: Int, flag: Bool, callback: Fn(Int) -> String) -> String\n  input\n";
    let out = fmt(source);
    assert!(out.contains("def transform(input: String, count: Int, flag: Bool, callback: (_0: Int) -> String) -> String"));
    // No newlines inside the signature.
    let sig_line = out.lines().next().unwrap();
    assert!(sig_line.starts_with("def transform("));
}

#[test]
fn call_long_args_stay_on_one_line() {
    // Same as above: calls can't wrap either.
    let source =
        "do_thing(name: \"hello world\", count: 42, flag: true, timeout: 30, retries: 3)\n";
    let out = fmt(source);
    assert_eq!(out.lines().count(), 1, "call should stay on one line");
}

// ===========================================================================
// Error cases
// ===========================================================================

#[test]
fn format_lex_error() {
    let result = format_source("let x = @@@\n", &FormatConfig::default());
    assert!(result.is_err());
}

// ===========================================================================
// Comment preservation
// ===========================================================================

#[test]
fn comment_before_let() {
    let source = "# this is a comment\nlet x = 1\n";
    let out = fmt(source);
    assert!(
        out.contains("# this is a comment"),
        "comment should be preserved"
    );
    assert!(out.contains("let x = 1"));
    // Comment should appear before the let
    let comment_pos = out.find("# this is a comment").unwrap();
    let let_pos = out.find("let x = 1").unwrap();
    assert!(comment_pos < let_pos);
}

#[test]
fn comment_between_stmts() {
    let source = "let x = 1\n# between\nlet y = 2\n";
    let out = fmt(source);
    assert!(out.contains("# between"));
    let x_pos = out.find("let x = 1").unwrap();
    let comment_pos = out.find("# between").unwrap();
    let y_pos = out.find("let y = 2").unwrap();
    assert!(x_pos < comment_pos);
    assert!(comment_pos < y_pos);
}

#[test]
fn multiple_comments_before_stmt() {
    let source = "# first\n# second\nlet x = 1\n";
    let out = fmt(source);
    assert!(out.contains("# first"));
    assert!(out.contains("# second"));
    let first_pos = out.find("# first").unwrap();
    let second_pos = out.find("# second").unwrap();
    let let_pos = out.find("let x = 1").unwrap();
    assert!(first_pos < second_pos);
    assert!(second_pos < let_pos);
}

#[test]
fn comment_preserved_roundtrip() {
    let source = "# hello\nlet x = 1\n";
    let first = fmt(source);
    let second = fmt(&first);
    assert_eq!(first, second, "comment formatting should be idempotent");
}

#[test]
fn comment_before_function() {
    let source = "# greets a person\ndef greet(name: String)\n  let x = 1\n";
    let out = fmt(source);
    assert!(out.contains("# greets a person"));
    assert!(out.contains("def greet(name: String)"));
}

// ===========================================================================
// Diff output
// ===========================================================================

#[test]
fn diff_no_changes() {
    let source = "let x = 1\n";
    let formatted = fmt(source);
    assert_eq!(source, formatted);
    let diffs = crate::format_diff(source, &FormatConfig::default()).unwrap();
    assert!(diffs.is_empty());
}

#[test]
fn diff_with_changes() {
    // Extra return that gets stripped
    let source = "def f() -> Int\n  return 42\n";
    let diffs = crate::format_diff(source, &FormatConfig::default()).unwrap();
    assert!(!diffs.is_empty());
    // Line 2 should show the return being stripped
    let line2_diff = diffs.iter().find(|d| d.line == 2);
    assert!(line2_diff.is_some());
    assert!(line2_diff.unwrap().original.contains("return 42"));
    assert_eq!(line2_diff.unwrap().formatted.trim(), "42");
}

// ===========================================================================
// Magic trailing comma
// ===========================================================================

#[test]
fn trailing_comma_list_forces_vertical() {
    let source = "let items = [1, 2, 3,]\n";
    let out = fmt(source);
    // Should be vertical layout with trailing comma preserved
    assert!(
        out.contains("[\n"),
        "trailing comma should force vertical layout"
    );
    assert!(out.contains(",\n"), "items should be on separate lines");
}

#[test]
fn no_trailing_comma_list_can_be_flat() {
    let source = "let items = [1, 2, 3]\n";
    let out = fmt(source);
    // Short list without trailing comma can be on one line
    assert!(
        out.contains("[1, 2, 3]"),
        "short list should stay on one line"
    );
}

#[test]
fn trailing_comma_list_idempotent() {
    let source = "let items = [1, 2, 3,]\n";
    let first = fmt(source);
    let second = fmt(&first);
    assert_eq!(
        first, second,
        "trailing comma formatting should be idempotent"
    );
}

#[test]
fn trailing_comma_call_forces_vertical() {
    let source = "foo(x: 1, y: 2,)\n";
    let out = fmt(source);
    assert!(
        out.contains("(\n"),
        "trailing comma should force vertical call layout"
    );
}

#[test]
fn no_trailing_comma_call_flat() {
    let source = "foo(x: 1, y: 2)\n";
    let out = fmt(source);
    assert!(out.contains("foo(x: 1, y: 2)"));
}

// ===========================================================================
// Binary expression precedence tests
// ===========================================================================

#[test]
fn binop_same_precedence_no_parens() {
    // a + b + c doesn't need parens (left-assoc, same op)
    let source = "let x = 1 + 2 + 3\n";
    let out = fmt(source);
    assert_eq!(out.trim(), "let x = 1 + 2 + 3");
}

#[test]
fn binop_higher_prec_child_no_parens() {
    // a + b * c: mul is higher prec, no parens needed
    let source = "let x = 1 + 2 * 3\n";
    let out = fmt(source);
    assert_eq!(out.trim(), "let x = 1 + 2 * 3");
}

#[test]
fn binop_lower_prec_child_gets_parens() {
    // a * (b + c): add is lower prec than mul, needs parens
    let source = "let x = 3 * (1 + 2)\n";
    let out = fmt(source);
    assert_eq!(out.trim(), "let x = 3 * (1 + 2)");
}

#[test]
fn binop_right_child_different_op_same_prec() {
    // a - (b + c): sub and add are same prec, right child differs, needs parens
    let source = "let x = 5 - (2 + 1)\n";
    let out = fmt(source);
    assert_eq!(out.trim(), "let x = 5 - (2 + 1)");
}

#[test]
fn binop_nested_lower_prec_both_sides() {
    // (a or b) and (c or d): or is lower than and
    let source = "let x = (true or false) and (true or false)\n";
    let out = fmt(source);
    assert_eq!(out.trim(), "let x = (true or false) and (true or false)");
}

#[test]
fn binop_precedence_roundtrip() {
    // Verify the formatter output re-parses and re-formats identically
    let source = "let x = 3 * (1 + 2)\n";
    let out1 = fmt(source);
    let out2 = fmt(&out1);
    assert_eq!(out1, out2, "binop precedence formatting must be idempotent");
}

// ===========================================================================
// Escape string tests
// ===========================================================================

#[test]
fn plain_string_no_brace_escape() {
    // Plain strings should NOT escape braces
    let source = "let x = \"hello {world}\"\n";
    let out = fmt(source);
    assert!(
        out.contains("\"hello {world}\""),
        "plain strings should not escape braces, got: {}",
        out
    );
}

// ===========================================================================
// Blank line grouping tests
// ===========================================================================

#[test]
fn blank_line_before_block_in_body() {
    // An if block after a let should get a blank line between them.
    let source = "def foo()\n  let x = 1\n  if x > 0\n    let y = 2\n";
    let out = fmt(source);
    assert_eq!(out, "def foo()\n  let x = 1\n\n  if x > 0\n    let y = 2\n");
}

#[test]
fn blank_line_after_block_in_body() {
    // A let after an if block should get a blank line between them.
    let source = "def foo()\n  if true\n    let a = 1\n  let x = 2\n";
    let out = fmt(source);
    assert_eq!(out, "def foo()\n  if true\n    let a = 1\n\n  let x = 2\n");
}

#[test]
fn blank_line_between_let_and_assignment_groups() {
    // Switching from let bindings to assignments inserts a blank line.
    let source = "def foo()\n  let x = 1\n  let y = 2\n  x = 10\n  y = 20\n";
    let out = fmt(source);
    assert_eq!(
        out,
        "def foo()\n  let x = 1\n  let y = 2\n\n  x = 10\n  y = 20\n"
    );
}

#[test]
fn no_blank_line_within_let_group() {
    // Consecutive lets stay together without blank lines.
    let source = "def foo()\n  let a = 1\n  let b = 2\n  let c = 3\n";
    let out = fmt(source);
    assert_eq!(out, "def foo()\n  let a = 1\n  let b = 2\n  let c = 3\n");
}

#[test]
fn no_blank_line_within_assignment_group() {
    // Consecutive assignments stay together without blank lines.
    let source = "def foo(x: Int, y: Int)\n  x = 1\n  y = 2\n";
    let out = fmt(source);
    assert_eq!(out, "def foo(x: Int, y: Int)\n  x = 1\n  y = 2\n");
}

#[test]
fn blank_line_between_blocks() {
    // Two consecutive blocks get a blank line between them.
    let source = "def foo()\n  if true\n    let a = 1\n  while true\n    let b = 2\n";
    let out = fmt(source);
    assert_eq!(
        out,
        "def foo()\n  if true\n    let a = 1\n\n  while true\n    let b = 2\n"
    );
}

#[test]
fn blank_line_function_def_in_body() {
    // A function definition (let + lambda) is treated as a block.
    let source = "def outer()\n  let x = 1\n  def inner()\n    let y = 2\n  let z = 3\n";
    let out = fmt(source);
    assert_eq!(
        out,
        "def outer()\n  let x = 1\n\n  def inner()\n    let y = 2\n\n  let z = 3\n"
    );
}

#[test]
fn blank_line_grouping_idempotent() {
    // Formatting twice should yield the same result.
    let source = "def foo()\n  let x = 1\n  if true\n    let a = 1\n  x = 10\n";
    let out1 = fmt(source);
    let out2 = fmt(&out1);
    assert_eq!(out1, out2, "blank line grouping must be idempotent");
}

#[test]
fn blank_line_for_loop_separation() {
    // A for loop after assignments gets a blank line.
    let source = "def foo()\n  let x = 1\n  for i in [1, 2, 3]\n    let y = i\n";
    let out = fmt(source);
    assert_eq!(
        out,
        "def foo()\n  let x = 1\n\n  for i in [1, 2, 3]\n    let y = i\n"
    );
}

#[test]
fn blank_line_mixed_groups() {
    // lets -> assignments -> expression calls should all get blank lines between groups.
    let source =
        "def foo(x: Int)\n  let a = 1\n  let b = 2\n  x = 10\n  x = 20\n  bar()\n  baz()\n";
    let out = fmt(source);
    assert_eq!(
        out,
        "def foo(x: Int)\n  let a = 1\n  let b = 2\n\n  x = 10\n  x = 20\n\n  bar()\n\n  baz()\n"
    );
}

#[test]
fn blank_line_before_implicit_return() {
    // The last expression (implicit return) in a function body gets a blank line.
    let source = "def add(a: Int, b: Int) -> Int\n  let result = a + b\n  result\n";
    let out = fmt(source);
    assert_eq!(
        out,
        "def add(a: Int, b: Int) -> Int\n  let result = a + b\n\n  result\n"
    );
}

#[test]
fn blank_line_before_explicit_return() {
    // An explicit return at the end gets stripped and separated by a blank line.
    let source = "def add(a: Int, b: Int) -> Int\n  let result = a + b\n  return result\n";
    let out = fmt(source);
    assert_eq!(
        out,
        "def add(a: Int, b: Int) -> Int\n  let result = a + b\n\n  result\n"
    );
}

#[test]
fn no_blank_line_single_return() {
    // A function with only a single expression body has no blank line.
    let source = "def one() -> Int\n  1\n";
    let out = fmt(source);
    assert_eq!(out, "def one() -> Int\n  1\n");
}

// ===========================================================================
// Implicit main() return — formatter strips redundant -> Int and 0
// ===========================================================================

#[test]
fn format_main_strip_int_return_with_zero() {
    // def main() -> Int\n  0  =>  def main()\n
    let source = "def main() -> Int\n  0\n";
    let out = fmt(source);
    assert_eq!(out, "def main()\n");
}

#[test]
fn format_main_strip_int_return_with_return_zero() {
    // def main() -> Int\n  return 0  =>  def main()\n
    let source = "def main() -> Int\n  return 0\n";
    let out = fmt(source);
    assert_eq!(out, "def main()\n");
}

#[test]
fn format_main_keep_int_return_with_nonzero() {
    // def main() -> Int\n  42  => keep as-is
    let source = "def main() -> Int\n  42\n";
    let out = fmt(source);
    assert_eq!(out, "def main() -> Int\n  42\n");
}

#[test]
fn format_main_keep_int_return_with_complex_body() {
    // def main() -> Int with multiple statements should keep -> Int
    let source = "def main() -> Int\n  let x = 1\n  return x\n";
    let out = fmt(source);
    assert!(
        out.contains("-> Int"),
        "should keep -> Int for complex body: {}",
        out
    );
}

#[test]
fn format_main_keep_void_return() {
    let source = "def main() -> Void\n  log(message: \"hi\")\n";
    let out = fmt(source);
    assert!(
        !out.contains("-> Void"),
        "Void return should already be hidden: {}",
        out
    );
}

#[test]
fn format_main_no_return_type_unchanged() {
    let source = "def main()\n  log(message: \"hi\")\n";
    let out = fmt(source);
    assert!(
        !out.contains("->"),
        "no return type should stay that way: {}",
        out
    );
}

#[test]
fn format_non_main_int_return_zero_unchanged() {
    // Non-main functions returning 0 should NOT be stripped
    let source = "def helper() -> Int\n  0\n";
    let out = fmt(source);
    assert_eq!(out, "def helper() -> Int\n  0\n");
}

#[test]
fn format_main_multi_statement_strips_trailing_zero() {
    // main() -> Int with multiple statements ending in 0 should strip -> Int and trailing 0
    let source = "def main() -> Int\n  let x = 1\n\n  say(message: \"hi\")\n\n  0\n";
    let out = fmt(source);
    assert!(!out.contains("-> Int"), "should strip -> Int: {out}");
    assert!(!out.ends_with("  0\n"), "should strip trailing 0: {out}");
    assert!(out.contains("let x = 1"), "body preserved: {out}");
    assert!(
        out.contains("say(message: \"hi\")"),
        "body preserved: {out}"
    );
}

#[test]
fn format_main_multi_statement_strips_trailing_return_zero() {
    let source = "def main() -> Int\n  say(message: \"hi\")\n\n  return 0\n";
    let out = fmt(source);
    assert!(!out.contains("-> Int"), "should strip -> Int: {out}");
    assert!(!out.contains("return 0"), "should strip return 0: {out}");
    assert!(
        out.contains("say(message: \"hi\")"),
        "body preserved: {out}"
    );
}

#[test]
fn format_main_non_zero_return_preserved() {
    // main() returning 1 (error) should NOT be stripped
    let source = "def main() -> Int\n  say(message: \"fail\")\n\n  1\n";
    let out = fmt(source);
    assert!(
        out.contains("-> Int"),
        "-> Int preserved for non-zero: {out}"
    );
    assert!(out.contains("1"), "return value preserved: {out}");
}

#[test]
fn format_main_strips_throws_error() {
    let source = "def main() throws Error -> Int\n  0\n";
    let out = fmt(source);
    assert!(!out.contains("throws"), "throws stripped from main: {out}");
    assert!(!out.contains("-> Int"), "-> Int stripped: {out}");
}

#[test]
fn format_main_strips_throws_with_body() {
    let source = "def main() throws Error\n  let x = 1\n\n  say(message: \"hi\")\n";
    let out = fmt(source);
    assert!(!out.contains("throws"), "throws stripped from main: {out}");
    assert!(out.contains("let x = 1"), "body preserved: {out}");
}

#[test]
fn format_main_strips_any_throws_type() {
    // Any throws type on main is redundant, not just Error
    let source = "def main() throws AppError -> Int\n  0\n";
    let out = fmt(source);
    assert!(
        !out.contains("throws"),
        "any throws stripped from main: {out}"
    );
}

#[test]
fn format_non_main_keeps_throws() {
    let source = "def helper() throws Error -> Int\n  42\n";
    let out = fmt(source);
    assert!(out.contains("throws Error"), "non-main keeps throws: {out}");
}

// ===========================================================================
// Comment preservation inside function bodies
// ===========================================================================

#[test]
fn comment_inside_function_body() {
    let source = "def foo() -> Int\n  # inside\n  42\n";
    let out = fmt(source);
    assert!(out.contains("# inside"), "comment inside body: {out}");
    let comment_pos = out.find("# inside").unwrap();
    let val_pos = out.find("42").unwrap();
    assert!(comment_pos < val_pos);
}

#[test]
fn comment_inside_function_body_idempotent() {
    let source = "def foo() -> Int\n  # inside\n  42\n";
    let first = fmt(source);
    let second = fmt(&first);
    assert_eq!(
        first, second,
        "body comment formatting should be idempotent"
    );
}

#[test]
fn comment_between_functions_not_lost() {
    let source = "def foo() -> Int\n  0\n\n\n# between\ndef bar() -> Int\n  1\n";
    let out = fmt(source);
    assert!(out.contains("# between"), "between-function comment: {out}");
    let foo_pos = out.find("def foo").unwrap();
    let comment_pos = out.find("# between").unwrap();
    let bar_pos = out.find("def bar").unwrap();
    assert!(foo_pos < comment_pos);
    assert!(comment_pos < bar_pos);
}

#[test]
fn comment_inside_function_not_relocated() {
    let source = "def foo() -> Int\n  # inside foo\n  42\n\n\ndef bar() -> Int\n  0\n";
    let out = fmt(source);
    assert!(
        out.contains("# inside foo"),
        "body comment preserved: {out}"
    );
    let comment_pos = out.find("# inside foo").unwrap();
    let bar_pos = out.find("def bar").unwrap();
    assert!(
        comment_pos < bar_pos,
        "comment should stay inside foo, not move to bar: {out}"
    );
    // Should only appear once (no duplication)
    assert_eq!(
        out.matches("# inside foo").count(),
        1,
        "no duplication: {out}"
    );
}

#[test]
fn comment_inside_if_body() {
    let source = "def foo() -> Int\n  if true\n    # in if\n    42\n  0\n";
    let out = fmt(source);
    assert!(out.contains("# in if"), "comment inside if body: {out}");
    assert_eq!(out.matches("# in if").count(), 1, "no duplication: {out}");
}

#[test]
fn comment_inside_while_body() {
    let source = "def foo()\n  while true\n    # loop comment\n    break\n";
    let out = fmt(source);
    assert!(
        out.contains("# loop comment"),
        "comment inside while: {out}"
    );
    assert_eq!(
        out.matches("# loop comment").count(),
        1,
        "no duplication: {out}"
    );
}

#[test]
fn comment_inside_nested_if_not_duplicated() {
    let source = "def bar() -> Int\n  let y = 10\n\n  # before if\n  if y > 5\n    # nested\n    y = y + 1\n\n  y\n";
    let out = fmt(source);
    assert!(out.contains("# before if"), "before-if comment: {out}");
    assert!(out.contains("# nested"), "nested comment: {out}");
    assert_eq!(
        out.matches("# nested").count(),
        1,
        "nested comment not duplicated: {out}"
    );
    assert_eq!(
        out.matches("# before if").count(),
        1,
        "before-if comment not duplicated: {out}"
    );
}

#[test]
fn multiple_comments_inside_function() {
    let source = "def foo() -> Int\n  # first\n  let x = 1\n\n  # second\n  let y = 2\n\n  x + y\n";
    let out = fmt(source);
    assert!(out.contains("# first"), "first comment: {out}");
    assert!(out.contains("# second"), "second comment: {out}");
    let first_pos = out.find("# first").unwrap();
    let x_pos = out.find("let x").unwrap();
    let second_pos = out.find("# second").unwrap();
    let y_pos = out.find("let y").unwrap();
    assert!(first_pos < x_pos);
    assert!(second_pos < y_pos);
    assert!(x_pos < second_pos);
}

#[test]
fn comment_inside_function_full_roundtrip() {
    let source = "\
# Module comment
def foo() -> Int
  # Compute result
  let x = 42

  # Return it
  x


# Helper
def bar() -> Int
  0
";
    let first = fmt(source);
    let second = fmt(&first);
    assert_eq!(first, second, "full roundtrip idempotent: {first}");
    assert!(first.contains("# Compute result"));
    assert!(first.contains("# Return it"));
    assert!(first.contains("# Module comment"));
    assert!(first.contains("# Helper"));
}

// ===========================================================================
// Class body: blank lines between fields and methods
// ===========================================================================

#[test]
fn class_blank_line_between_fields_and_methods() {
    let source = "class Foo\n  x: Int\n  pub def bar()\n    let y = 1\n";
    let out = fmt(source);
    // There should be a blank line between the field and the method
    assert!(
        out.contains("x: Int\n\n  pub def bar"),
        "blank line between field and method: {out}"
    );
}

#[test]
fn class_blank_line_between_methods() {
    let source = "class Foo\n  pub def bar()\n    let x = 1\n  pub def baz()\n    let y = 2\n";
    let out = fmt(source);
    // Blank line between consecutive methods
    let bar_end = out.find("let x = 1").unwrap();
    let baz_start = out.find("pub def baz").unwrap();
    let between = &out[bar_end..baz_start];
    assert!(
        between.contains("\n\n"),
        "blank line between methods: {out}"
    );
}

#[test]
fn class_fields_stay_together() {
    let source = "class Foo\n  x: Int\n  y: String\n  pub def bar()\n    let z = 1\n";
    let out = fmt(source);
    // Fields should NOT have blank lines between them
    assert!(
        out.contains("x: Int\n  y: String"),
        "fields stay together: {out}"
    );
    // But blank line before method
    assert!(
        out.contains("y: String\n\n  pub def bar"),
        "blank line before method: {out}"
    );
}

#[test]
fn class_spacing_idempotent() {
    let source = "\
class Foo
  x: Int
  y: String

  pub def bar()
    let z = 1

  pub def baz()
    let w = 2
";
    let first = fmt(source);
    let second = fmt(&first);
    assert_eq!(first, second, "class spacing idempotent: {first}");
}
