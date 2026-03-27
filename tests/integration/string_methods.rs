
// ─── String Methods ────────────────────────────────────────────────
//
// Built-in methods on String type:
//   len, split, contains, starts_with, ends_with,
//   trim, slice, replace, to_upper, to_lower
//
// All methods use character-based (Unicode scalar) indexing.

// ─── len (character count, not byte count) ─────────────────────────

#[test]
fn string_len_method_ascii() {
    crate::common::check_ok(
        r#"let s = "hello"
let n: Int = s.len()
"#,
    );
}

#[test]
fn string_len_method_empty() {
    crate::common::check_ok(
        r#"let s = ""
let n: Int = s.len()
"#,
    );
}

// ─── contains ──────────────────────────────────────────────────────

#[test]
fn string_contains_basic() {
    crate::common::check_ok(
        r#"let s = "hello world"
let found: Bool = s.contains(str: "world")
"#,
    );
}

#[test]
fn string_contains_not_found() {
    crate::common::check_ok(
        r#"let s = "hello"
let found: Bool = s.contains(str: "xyz")
"#,
    );
}

#[test]
fn string_contains_empty() {
    crate::common::check_ok(
        r#"let s = "hello"
let found: Bool = s.contains(str: "")
"#,
    );
}

// ─── starts_with ───────────────────────────────────────────────────

#[test]
fn string_starts_with_true() {
    crate::common::check_ok(
        r#"let s = "hello world"
let yes: Bool = s.starts_with(pre: "hello")
"#,
    );
}

#[test]
fn string_starts_with_false() {
    crate::common::check_ok(
        r#"let s = "hello"
let no: Bool = s.starts_with(pre: "world")
"#,
    );
}

// ─── ends_with ─────────────────────────────────────────────────────

#[test]
fn string_ends_with_true() {
    crate::common::check_ok(
        r#"let s = "hello world"
let yes: Bool = s.ends_with(suf: "world")
"#,
    );
}

#[test]
fn string_ends_with_false() {
    crate::common::check_ok(
        r#"let s = "hello"
let no: Bool = s.ends_with(suf: "xyz")
"#,
    );
}

// ─── trim ──────────────────────────────────────────────────────────

#[test]
fn string_trim_basic() {
    crate::common::check_ok(
        r#"let s = "  hello  "
let t: String = s.trim()
"#,
    );
}

#[test]
fn string_trim_no_whitespace() {
    crate::common::check_ok(
        r#"let s = "hello"
let t: String = s.trim()
"#,
    );
}

// ─── slice ─────────────────────────────────────────────────────────

#[test]
fn string_slice_basic() {
    crate::common::check_ok(
        r#"let s = "hello world"
let sub: String = s.slice(from: 0, to: 5)
"#,
    );
}

#[test]
fn string_slice_middle() {
    crate::common::check_ok(
        r#"let s = "hello world"
let sub: String = s.slice(from: 6, to: 11)
"#,
    );
}

#[test]
fn string_slice_clamp_overflow() {
    crate::common::check_ok(
        r#"let s = "hello"
let sub: String = s.slice(from: 0, to: 100)
"#,
    );
}

// ─── replace ───────────────────────────────────────────────────────

#[test]
fn string_replace_basic() {
    crate::common::check_ok(
        r#"let s = "hello world"
let r: String = s.replace(old: "world", new: "there")
"#,
    );
}

#[test]
fn string_replace_no_match() {
    crate::common::check_ok(
        r#"let s = "hello"
let r: String = s.replace(old: "xyz", new: "abc")
"#,
    );
}

#[test]
fn string_replace_multiple() {
    crate::common::check_ok(
        r#"let s = "aaa"
let r: String = s.replace(old: "a", new: "bb")
"#,
    );
}

// ─── to_upper / to_lower ───────────────────────────────────────────

#[test]
fn string_to_upper_basic() {
    crate::common::check_ok(
        r#"let s = "hello"
let u: String = s.to_upper()
"#,
    );
}

#[test]
fn string_to_lower_basic() {
    crate::common::check_ok(
        r#"let s = "HELLO"
let l: String = s.to_lower()
"#,
    );
}

#[test]
fn string_to_upper_mixed() {
    crate::common::check_ok(
        r#"let s = "Hello World"
let u: String = s.to_upper()
"#,
    );
}

#[test]
fn string_to_lower_mixed() {
    crate::common::check_ok(
        r#"let s = "Hello World"
let l: String = s.to_lower()
"#,
    );
}

// ─── split ─────────────────────────────────────────────────────────

#[test]
fn string_split_basic() {
    crate::common::check_ok(
        r#"let s = "a,b,c"
let parts: List[String] = s.split(sep: ",")
"#,
    );
}

#[test]
fn string_split_space() {
    crate::common::check_ok(
        r#"let s = "hello world foo"
let parts: List[String] = s.split(sep: " ")
"#,
    );
}

#[test]
fn string_split_no_match() {
    crate::common::check_ok(
        r#"let s = "hello"
let parts: List[String] = s.split(sep: ",")
"#,
    );
}

// ─── Error cases: wrong argument types ─────────────────────────────

#[test]
fn string_contains_wrong_type() {
    let err = crate::common::check_err(
        r#"let s = "hello"
let found = s.contains(str: 42)
"#,
    );
    assert!(
        err.contains("String") || err.contains("Int"),
        "Expected type error, got: {}",
        err
    );
}

#[test]
fn string_slice_wrong_type() {
    let err = crate::common::check_err(
        r#"let s = "hello"
let sub = s.slice(from: "a", to: "b")
"#,
    );
    assert!(
        err.contains("Int") || err.contains("String"),
        "Expected type error, got: {}",
        err
    );
}

#[test]
fn string_method_on_int() {
    let err = crate::common::check_err(
        r#"let x = 42
let t = x.trim()
"#,
    );
    assert!(
        err.contains("member") || err.contains("trim") || err.contains("Int"),
        "Expected error about Int not having trim, got: {}",
        err
    );
}

// ─── Chaining ──────────────────────────────────────────────────────

#[test]
fn string_method_chaining() {
    crate::common::check_ok(
        r#"let s = "  Hello World  "
let r: String = s.trim().to_lower()
"#,
    );
}

#[test]
fn string_replace_then_split() {
    crate::common::check_ok(
        r#"let s = "a-b-c"
let parts: List[String] = s.replace(old: "-", new: ",").split(sep: ",")
"#,
    );
}

// ─── Composition with existing features ────────────────────────────

#[test]
fn string_len_in_condition() {
    crate::common::check_ok(
        r#"let s = "hello"
if s.len() > 3
  say("long")
"#,
    );
}

#[test]
fn string_contains_in_if() {
    crate::common::check_ok(
        r#"let s = "hello world"
if s.contains(str: "world")
  say("found it")
"#,
    );
}

#[test]
fn string_split_iterate() {
    crate::common::check_ok(
        r#"let parts = "a,b,c".split(sep: ",")
parts.each(f: -> part: say(part))
"#,
    );
}

// ─── E2E execution tests ──────────────────────────────────────────

#[test]
fn e2e_string_trim() {
    let dir = crate::common::make_temp_dir("str-trim");
    let src = dir.join("trim.aster");
    std::fs::write(
        &src,
        r#"def main() -> Int
  let s = "  hello  "
  let t = s.trim()
  t.len()
"#,
    )
    .unwrap();
    let output = crate::common::cli(&["run", src.to_str().unwrap()]);
    assert_eq!(
        output.status.code(),
        Some(5),
        "trim('  hello  ').len() should be 5: {}",
        crate::common::output_text(&output)
    );
}

#[test]
fn e2e_string_to_upper() {
    let dir = crate::common::make_temp_dir("str-upper");
    let src = dir.join("upper.aster");
    std::fs::write(
        &src,
        r#"def main() -> Int
  say("hello".to_upper())
  0
"#,
    )
    .unwrap();
    let output = crate::common::cli(&["run", src.to_str().unwrap()]);
    assert!(output.status.success(), "{}", crate::common::output_text(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "HELLO", "to_upper should produce HELLO");
}

#[test]
fn e2e_string_to_lower() {
    let dir = crate::common::make_temp_dir("str-lower");
    let src = dir.join("lower.aster");
    std::fs::write(
        &src,
        r#"def main() -> Int
  say("HELLO".to_lower())
  0
"#,
    )
    .unwrap();
    let output = crate::common::cli(&["run", src.to_str().unwrap()]);
    assert!(output.status.success(), "{}", crate::common::output_text(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "hello", "to_lower should produce hello");
}

#[test]
fn e2e_string_contains() {
    let dir = crate::common::make_temp_dir("str-contains");
    let src = dir.join("contains.aster");
    std::fs::write(
        &src,
        r#"def main() -> Int
  let s = "hello world"
  let result = 0
  if s.contains(str: "world")
    result = 42
  result
"#,
    )
    .unwrap();
    let output = crate::common::cli(&["run", src.to_str().unwrap()]);
    assert_eq!(
        output.status.code(),
        Some(42),
        "contains should find 'world': {}",
        crate::common::output_text(&output)
    );
}

#[test]
fn e2e_string_starts_with() {
    let dir = crate::common::make_temp_dir("str-sw");
    let src = dir.join("starts_with.aster");
    std::fs::write(
        &src,
        r#"def main() -> Int
  let s = "hello world"
  let result = 0
  if s.starts_with(pre: "hello")
    result = 42
  result
"#,
    )
    .unwrap();
    let output = crate::common::cli(&["run", src.to_str().unwrap()]);
    assert_eq!(
        output.status.code(),
        Some(42),
        "starts_with should match 'hello': {}",
        crate::common::output_text(&output)
    );
}

#[test]
fn e2e_string_ends_with() {
    let dir = crate::common::make_temp_dir("str-ew");
    let src = dir.join("ends_with.aster");
    std::fs::write(
        &src,
        r#"def main() -> Int
  let s = "hello world"
  let result = 0
  if s.ends_with(suf: "world")
    result = 42
  result
"#,
    )
    .unwrap();
    let output = crate::common::cli(&["run", src.to_str().unwrap()]);
    assert_eq!(
        output.status.code(),
        Some(42),
        "ends_with should match 'world': {}",
        crate::common::output_text(&output)
    );
}

#[test]
fn e2e_string_replace() {
    let dir = crate::common::make_temp_dir("str-replace");
    let src = dir.join("replace.aster");
    std::fs::write(
        &src,
        r#"def main() -> Int
  say("hello world".replace(old: "world", new: "there"))
  0
"#,
    )
    .unwrap();
    let output = crate::common::cli(&["run", src.to_str().unwrap()]);
    assert!(output.status.success(), "{}", crate::common::output_text(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "hello there");
}

#[test]
fn e2e_string_slice() {
    let dir = crate::common::make_temp_dir("str-slice");
    let src = dir.join("slice.aster");
    std::fs::write(
        &src,
        r#"def main() -> Int
  say("hello world".slice(from: 0, to: 5))
  0
"#,
    )
    .unwrap();
    let output = crate::common::cli(&["run", src.to_str().unwrap()]);
    assert!(output.status.success(), "{}", crate::common::output_text(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "hello");
}

#[test]
fn e2e_string_split() {
    let dir = crate::common::make_temp_dir("str-split");
    let src = dir.join("split.aster");
    std::fs::write(
        &src,
        r#"def main() -> Int
  let parts = "a,b,c".split(sep: ",")
  parts.len()
"#,
    )
    .unwrap();
    let output = crate::common::cli(&["run", src.to_str().unwrap()]);
    assert_eq!(
        output.status.code(),
        Some(3),
        "split('a,b,c', ',') should produce 3 parts: {}",
        crate::common::output_text(&output)
    );
}

#[test]
fn e2e_string_split_len() {
    let dir = crate::common::make_temp_dir("str-split-len");
    let src = dir.join("split_len.aster");
    std::fs::write(
        &src,
        r#"def main() -> Int
  let parts = "hello world foo".split(sep: " ")
  parts.len()
"#,
    )
    .unwrap();
    let output = crate::common::cli(&["run", src.to_str().unwrap()]);
    assert_eq!(
        output.status.code(),
        Some(3),
        "split should produce 3 parts: {}",
        crate::common::output_text(&output)
    );
}

#[test]
fn e2e_string_len_unicode() {
    let dir = crate::common::make_temp_dir("str-len-uni");
    let src = dir.join("len_unicode.aster");
    std::fs::write(
        &src,
        "def main() -> Int\n  let s = \"caf\u{00e9}\"\n  s.len()\n",
    )
    .unwrap();
    let output = crate::common::cli(&["run", src.to_str().unwrap()]);
    assert_eq!(
        output.status.code(),
        Some(4),
        "len('café') should be 4 characters: {}",
        crate::common::output_text(&output)
    );
}
