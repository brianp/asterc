use super::*;

fn kinds(tokens: &[Token]) -> Vec<&TokenKind> {
    tokens.iter().map(|t| &t.kind).collect()
}

fn dump(tokens: &[Token]) -> String {
    tokens
        .iter()
        .map(|t| format!("{:?}@{}:{}", t.kind, t.line, t.col))
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn lexes_keywords_and_idents() {
    let src = r#"
def class async blocking return if else true false nil foo bar_baz
"#;
    let toks = lex(src).expect("lex ok");
    let ks = kinds(&toks);

    assert!(matches!(ks.first().unwrap(), TokenKind::Newline));
    assert!(matches!(ks.last().unwrap(), TokenKind::EOF));

    let mut i = 1;
    assert!(matches!(ks[i], TokenKind::Def));
    i += 1;
    assert!(matches!(ks[i], TokenKind::Class));
    i += 1;
    assert!(matches!(ks[i], TokenKind::Async));
    i += 1;
    assert!(matches!(ks[i], TokenKind::Blocking));
    i += 1;
    assert!(matches!(ks[i], TokenKind::Return));
    i += 1;
    assert!(matches!(ks[i], TokenKind::If));
    i += 1;
    assert!(matches!(ks[i], TokenKind::Else));
    i += 1;
    assert!(matches!(ks[i], TokenKind::True));
    i += 1;
    assert!(matches!(ks[i], TokenKind::False));
    i += 1;
    assert!(matches!(ks[i], TokenKind::Nil));
    i += 1;

    match ks[i] {
        TokenKind::Ident(s) => assert_eq!(s, "foo"),
        other => panic!("expected Ident(\"foo\"), got {other:?}"),
    }
    i += 1;
    match ks[i] {
        TokenKind::Ident(s) => assert_eq!(s, "bar_baz"),
        other => panic!("expected Ident(\"bar_baz\"), got {other:?}"),
    }
}

#[test]
fn lexes_punctuation_and_operators() {
    let src = "( ),: . = ->";
    let toks = lex(src).expect("lex ok");
    let ks = kinds(&toks);

    assert_eq!(
        &ks[..8],
        &[
            &TokenKind::LParen,
            &TokenKind::RParen,
            &TokenKind::Comma,
            &TokenKind::Colon,
            &TokenKind::Dot,
            &TokenKind::Equals,
            &TokenKind::Arrow,
            &TokenKind::Newline
        ],
        "got: {}",
        dump(&toks)
    );
    assert!(matches!(ks[8], TokenKind::EOF));
}

#[test]
fn lexes_arithmetic_operators() {
    let src = "+ - * / % **";
    let toks = lex(src).expect("lex ok");
    let ks = kinds(&toks);
    assert_eq!(ks[0], &TokenKind::Plus);
    assert_eq!(ks[1], &TokenKind::Minus);
    assert_eq!(ks[2], &TokenKind::Star);
    assert_eq!(ks[3], &TokenKind::Slash);
    assert_eq!(ks[4], &TokenKind::Percent);
    assert_eq!(ks[5], &TokenKind::StarStar);
}

#[test]
fn lexes_comparison_operators() {
    let src = "== != < > <= >=";
    let toks = lex(src).expect("lex ok");
    let ks = kinds(&toks);
    assert_eq!(ks[0], &TokenKind::EqualEqual);
    assert_eq!(ks[1], &TokenKind::BangEqual);
    assert_eq!(ks[2], &TokenKind::Less);
    assert_eq!(ks[3], &TokenKind::Greater);
    assert_eq!(ks[4], &TokenKind::LessEqual);
    assert_eq!(ks[5], &TokenKind::GreaterEqual);
}

#[test]
fn lexes_logical_keywords() {
    let src = "and or not";
    let toks = lex(src).expect("lex ok");
    let ks = kinds(&toks);
    assert_eq!(ks[0], &TokenKind::And);
    assert_eq!(ks[1], &TokenKind::Or);
    assert_eq!(ks[2], &TokenKind::Not);
}

#[test]
fn lexes_not_vs_ident_prefix() {
    let src = "not nothing";
    let toks = lex(src).expect("lex ok");
    let ks = kinds(&toks);
    assert_eq!(ks[0], &TokenKind::Not);
    assert_eq!(ks[1], &TokenKind::Ident("nothing".into()));
}

#[test]
fn bare_minus_emits_minus_token() {
    let src = "-5";
    let toks = lex(src).expect("lex ok");
    let ks = kinds(&toks);
    assert_eq!(ks[0], &TokenKind::Minus);
    assert_eq!(ks[1], &TokenKind::Int(5));
}

#[test]
fn minus_arrow_still_works() {
    let src = "-> ->";
    let toks = lex(src).expect("lex ok");
    let arrows: Vec<_> = toks.iter().filter(|t| t.kind == TokenKind::Arrow).collect();
    assert_eq!(arrows.len(), 2);
}

#[test]
fn equals_vs_equalequal() {
    let src = "= ==";
    let toks = lex(src).expect("lex ok");
    let ks = kinds(&toks);
    assert_eq!(ks[0], &TokenKind::Equals);
    assert_eq!(ks[1], &TokenKind::EqualEqual);
}

#[test]
fn starstar_vs_two_stars() {
    let src = "**";
    let toks = lex(src).expect("lex ok");
    let ks = kinds(&toks);
    assert_eq!(ks[0], &TokenKind::StarStar);
}

#[test]
fn operators_in_expressions_no_spaces() {
    let src = "a+b*c";
    let toks = lex(src).expect("lex ok");
    let ks = kinds(&toks);
    assert_eq!(ks[0], &TokenKind::Ident("a".into()));
    assert_eq!(ks[1], &TokenKind::Plus);
    assert_eq!(ks[2], &TokenKind::Ident("b".into()));
    assert_eq!(ks[3], &TokenKind::Star);
    assert_eq!(ks[4], &TokenKind::Ident("c".into()));
}

#[test]
fn less_greater_equal_combos() {
    let src = "< <= > >= = ==";
    let toks = lex(src).expect("lex ok");
    let ks = kinds(&toks);
    assert_eq!(ks[0], &TokenKind::Less);
    assert_eq!(ks[1], &TokenKind::LessEqual);
    assert_eq!(ks[2], &TokenKind::Greater);
    assert_eq!(ks[3], &TokenKind::GreaterEqual);
    assert_eq!(ks[4], &TokenKind::Equals);
    assert_eq!(ks[5], &TokenKind::EqualEqual);
}

#[test]
fn standalone_bang_is_valid_token() {
    let src = "!";
    let toks = lex(src).expect("lex ok");
    let ks = kinds(&toks);
    assert_eq!(*ks[0], TokenKind::Bang);
}

#[test]
fn lexes_numbers_int_float_and_mixed_dots() {
    let src = "0 42 1.0 123. .5 123.45.67";
    let toks = lex(src).expect("lex ok");
    let ks = kinds(&toks);

    let expect_prefix = vec![
        &TokenKind::Int(0),
        &TokenKind::Int(42),
        &TokenKind::Float(1.0),
        &TokenKind::Int(123),
        &TokenKind::Dot,
        &TokenKind::Dot,
        &TokenKind::Int(5),
        &TokenKind::Float(123.45),
        &TokenKind::Dot,
        &TokenKind::Int(67),
        &TokenKind::Newline,
    ];

    assert_eq!(
        &ks[..expect_prefix.len()],
        &expect_prefix[..],
        "{}",
        dump(&toks)
    );
    assert!(matches!(ks.last().unwrap(), TokenKind::EOF));
}

#[test]
fn lexes_strings_including_empty_and_reports_unterminated() {
    let src = r#""hello" "" "a b c""#;
    let toks = lex(src).expect("lex ok");
    let mut it = toks.iter().filter_map(|t| match &t.kind {
        TokenKind::Str(s) => Some(s.clone()),
        _ => None,
    });
    assert_eq!(it.next().as_deref(), Some("hello"));
    assert_eq!(it.next().as_deref(), Some(""));
    assert_eq!(it.next().as_deref(), Some("a b c"));

    let bad = "\"unterminated\nnext";
    let err = lex(bad).unwrap_err();
    assert_eq!(
        err.code(),
        Some("L002"),
        "expected L002 (UnterminatedString), got: {}",
        err
    );
}

#[test]
fn tracks_line_and_col_reasonably() {
    let src = "def foo(x, y)\n  return x->y\n";
    let toks = lex(src).expect("lex ok");

    let mut i = 0;
    assert!(matches!(toks[i].kind, TokenKind::Def) && toks[i].line == 1);
    i += 1;
    match &toks[i] {
        Token {
            kind: TokenKind::Ident(s),
            line,
            col,
            ..
        } => {
            assert_eq!(s, "foo");
            assert_eq!(*line, 1);
            assert!(*col > 1);
        }
        t => panic!("unexpected: {t:?}"),
    }
    i += 1;
    assert!(matches!(
        toks[i],
        Token {
            kind: TokenKind::LParen,
            line: 1,
            ..
        }
    ));
    i += 1;
    assert!(matches!(
        toks[i],
        Token {
            kind: TokenKind::Ident(_),
            line: 1,
            ..
        }
    ));
    i += 1;
    assert!(matches!(
        toks[i],
        Token {
            kind: TokenKind::Comma,
            line: 1,
            ..
        }
    ));
    i += 1;
    assert!(matches!(
        toks[i],
        Token {
            kind: TokenKind::Ident(_),
            line: 1,
            ..
        }
    ));
    i += 1;
    assert!(matches!(
        toks[i],
        Token {
            kind: TokenKind::RParen,
            line: 1,
            ..
        }
    ));
    i += 1;
    assert!(matches!(
        toks[i],
        Token {
            kind: TokenKind::Newline,
            line: 1,
            ..
        }
    ));
    i += 1;
    assert!(matches!(
        toks[i],
        Token {
            kind: TokenKind::Indent,
            line: 2,
            col: 1,
            ..
        }
    ));
    i += 1;
    assert!(matches!(
        toks[i],
        Token {
            kind: TokenKind::Return,
            line: 2,
            ..
        }
    ));
    i += 1;
    assert!(matches!(
        toks[i],
        Token {
            kind: TokenKind::Ident(_),
            line: 2,
            ..
        }
    ));
    i += 1;
    assert!(matches!(
        toks[i],
        Token {
            kind: TokenKind::Arrow,
            line: 2,
            ..
        }
    ));
    i += 1;
    assert!(matches!(
        toks[i],
        Token {
            kind: TokenKind::Ident(_),
            line: 2,
            ..
        }
    ));
    i += 1;
    assert!(matches!(
        toks[i],
        Token {
            kind: TokenKind::Newline,
            line: 2,
            ..
        }
    ));
    i += 1;
    assert!(matches!(toks[i].kind, TokenKind::Dedent));
    i += 1;
    assert!(matches!(toks[i].kind, TokenKind::EOF));
}

#[test]
fn handles_blank_and_comment_lines_as_newlines() {
    let src = r#"
# top comment

  # indented comment
x
"#;
    let toks = lex(src).expect("lex ok");
    let nl_count = toks
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Newline))
        .count();
    let line_count = src.lines().count();
    assert_eq!(nl_count, line_count);

    let pos = toks
        .iter()
        .position(|t| matches!(t.kind, TokenKind::Ident(ref s) if s=="x"))
        .expect("found x");
    assert!(matches!(toks[pos - 1].kind, TokenKind::Newline));
    assert!(matches!(toks[pos + 1].kind, TokenKind::Newline));
    assert!(matches!(toks.last().unwrap().kind, TokenKind::EOF));
}

#[test]
fn indentation_and_dedentation_multiple_levels() {
    let src = "\
def x
  a
    b
  c
d
";
    let toks = lex(src).expect("lex ok");
    let ks = kinds(&toks);

    let mut structure = Vec::new();
    for k in ks {
        match k {
            TokenKind::Indent | TokenKind::Dedent | TokenKind::EOF | TokenKind::Newline => {
                structure.push(format!("{k:?}"))
            }
            _ => {}
        }
    }
    let as_str = structure.join(" ");
    assert!(
        as_str.contains("Newline Indent")
            && as_str.contains("Newline Dedent")
            && as_str.matches("Dedent").count() >= 2,
        "structure: {as_str}"
    );
}

#[test]
fn reports_inconsistent_indentation_error() {
    let src = "\
def x
    four
  two
";
    let err = lex(src).unwrap_err();
    assert_eq!(
        err.code(),
        Some("L010"),
        "expected L010 (InconsistentIndentation), got: {}",
        err
    );
}

#[test]
fn emits_trailing_dedents_and_eof_with_correct_line_numbers() {
    let src = "\
def x
  a
  b
";
    let toks = lex(src).expect("lex ok");
    let dedents: Vec<&Token> = toks
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Dedent))
        .collect();
    assert_eq!(dedents.len(), 1);

    let total_lines = src.lines().count();
    assert_eq!(dedents[0].line, total_lines);

    let eof = toks.last().unwrap();
    assert!(matches!(eof.kind, TokenKind::EOF));
    assert_eq!(eof.line, total_lines + 1);
    assert_eq!(eof.col, 0);
}

#[test]
fn hyphen_emits_minus_or_arrow() {
    let src = "- - -> -";
    let toks = lex(src).expect("lex ok");
    let ks = kinds(&toks);

    let arrow_count = ks.iter().filter(|k| matches!(k, TokenKind::Arrow)).count();
    assert_eq!(arrow_count, 1, "tokens: {}", dump(&toks));

    let minus_count = ks.iter().filter(|k| matches!(k, TokenKind::Minus)).count();
    assert_eq!(minus_count, 3, "tokens: {}", dump(&toks));
}

#[test]
fn huge_integer_triggers_parse_error() {
    let src = "9999999999999999999999999999999999999999999";
    let err = lex(src).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("overflows i64 range") || msg.contains("bad int"),
        "unexpected error: {msg}"
    );
}

// ─── Phase 2: Control Flow tokens ───────────────────────────────────

#[test]
fn lexes_while_keyword() {
    let toks = lex("while").expect("lex ok");
    assert_eq!(kinds(&toks)[0], &TokenKind::While);
}

#[test]
fn lexes_for_and_in_keywords() {
    let toks = lex("for x in items").expect("lex ok");
    let ks = kinds(&toks);
    assert_eq!(ks[0], &TokenKind::For);
    assert_eq!(ks[1], &TokenKind::Ident("x".into()));
    assert_eq!(ks[2], &TokenKind::In);
    assert_eq!(ks[3], &TokenKind::Ident("items".into()));
}

#[test]
fn lexes_elif_keyword() {
    let toks = lex("elif").expect("lex ok");
    assert_eq!(kinds(&toks)[0], &TokenKind::Elif);
}

#[test]
fn lexes_break_and_continue_keywords() {
    let toks = lex("break continue").expect("lex ok");
    let ks = kinds(&toks);
    assert_eq!(ks[0], &TokenKind::Break);
    assert_eq!(ks[1], &TokenKind::Continue);
}

#[test]
fn lexes_brackets() {
    let toks = lex("[1, 2]").expect("lex ok");
    let ks = kinds(&toks);
    assert_eq!(ks[0], &TokenKind::LBracket);
    assert_eq!(ks[1], &TokenKind::Int(1));
    assert_eq!(ks[2], &TokenKind::Comma);
    assert_eq!(ks[3], &TokenKind::Int(2));
    assert_eq!(ks[4], &TokenKind::RBracket);
}

#[test]
fn keywords_not_confused_with_idents() {
    let toks = lex("while_loop for_each in_range elif_check breaking continued").expect("lex ok");
    let ks = kinds(&toks);
    // All should be identifiers, not keywords
    for k in &ks[..6] {
        assert!(
            matches!(k, TokenKind::Ident(_)),
            "expected Ident, got {:?}",
            k
        );
    }
}

// ─── Phase 4: Module tokens ─────────────────────────────────────────

#[test]
fn lexes_use_keyword() {
    let toks = lex("use").expect("lex ok");
    assert_eq!(kinds(&toks)[0], &TokenKind::Use);
}

#[test]
fn lexes_pub_keyword() {
    let toks = lex("pub").expect("lex ok");
    assert_eq!(kinds(&toks)[0], &TokenKind::Pub);
}

#[test]
fn lexes_use_not_confused_with_ident() {
    let toks = lex("use_thing user useful").expect("lex ok");
    let ks = kinds(&toks);
    for k in &ks[..3] {
        assert!(
            matches!(k, TokenKind::Ident(_)),
            "expected Ident, got {:?}",
            k
        );
    }
}

#[test]
fn lexes_pub_not_confused_with_ident() {
    let toks = lex("public publisher").expect("lex ok");
    let ks = kinds(&toks);
    for k in &ks[..2] {
        assert!(
            matches!(k, TokenKind::Ident(_)),
            "expected Ident, got {:?}",
            k
        );
    }
}

#[test]
fn lexes_lbrace_rbrace() {
    let toks = lex("{ }").expect("lex ok");
    let ks = kinds(&toks);
    assert_eq!(ks[0], &TokenKind::LBrace);
    assert_eq!(ks[1], &TokenKind::RBrace);
}

#[test]
fn lexes_use_statement_tokens() {
    let toks = lex("use std/http").expect("lex ok");
    let ks = kinds(&toks);
    assert_eq!(ks[0], &TokenKind::Use);
    assert_eq!(ks[1], &TokenKind::Ident("std".into()));
    assert_eq!(ks[2], &TokenKind::Slash);
    assert_eq!(ks[3], &TokenKind::Ident("http".into()));
}

#[test]
fn lexes_selective_import_tokens() {
    let toks = lex("use std/http { Request, Response }").expect("lex ok");
    let ks = kinds(&toks);
    assert_eq!(ks[0], &TokenKind::Use);
    assert_eq!(ks[1], &TokenKind::Ident("std".into()));
    assert_eq!(ks[2], &TokenKind::Slash);
    assert_eq!(ks[3], &TokenKind::Ident("http".into()));
    assert_eq!(ks[4], &TokenKind::LBrace);
    assert_eq!(ks[5], &TokenKind::Ident("Request".into()));
    assert_eq!(ks[6], &TokenKind::Comma);
    assert_eq!(ks[7], &TokenKind::Ident("Response".into()));
    assert_eq!(ks[8], &TokenKind::RBrace);
}

#[test]
fn lexes_pub_def() {
    let toks = lex("pub def foo()").expect("lex ok");
    let ks = kinds(&toks);
    assert_eq!(ks[0], &TokenKind::Pub);
    assert_eq!(ks[1], &TokenKind::Def);
    assert_eq!(ks[2], &TokenKind::Ident("foo".into()));
}

#[test]
fn lexes_pub_class() {
    let toks = lex("pub class Foo").expect("lex ok");
    let ks = kinds(&toks);
    assert_eq!(ks[0], &TokenKind::Pub);
    assert_eq!(ks[1], &TokenKind::Class);
}

#[test]
fn mixes_everything_in_a_small_program() {
    let src = r#"
class Foo
  field x: Int
  def bar(a: Int, b: Float) -> Str
    if true
      return "ok"
    else
      return "nope"
"#;
    let toks = lex(src).expect("lex ok");
    assert!(toks.iter().any(|t| matches!(t.kind, TokenKind::Class)));
    assert!(
        toks.iter()
            .any(|t| matches!(&t.kind, TokenKind::Ident(w) if w == "field"))
    );
    assert!(toks.iter().any(|t| matches!(t.kind, TokenKind::Def)));
    assert!(toks.iter().any(|t| matches!(t.kind, TokenKind::If)));
    assert!(toks.iter().any(|t| matches!(t.kind, TokenKind::Else)));
    assert!(toks.iter().any(|t| matches!(t.kind, TokenKind::Return)));
    assert!(toks.iter().any(|t| matches!(t.kind, TokenKind::Arrow)));
    assert!(toks.iter().any(|t| matches!(t.kind, TokenKind::Colon)));
    assert!(toks.iter().any(|t| matches!(t.kind, TokenKind::Str(_))));
    assert!(toks.iter().any(|t| matches!(t.kind, TokenKind::Ident(_))));
    assert!(toks.iter().any(|t| matches!(t.kind, TokenKind::Indent)));
    assert!(toks.iter().any(|t| matches!(t.kind, TokenKind::Dedent)));
    assert!(matches!(toks.last().unwrap().kind, TokenKind::EOF));
}

// ─── Phase 5: Generics and Traits tokens ────────────────────────────

#[test]
fn lexes_trait_keyword() {
    let toks = lex("trait").expect("lex ok");
    assert_eq!(kinds(&toks)[0], &TokenKind::Trait);
}

#[test]
fn lexes_includes_keyword() {
    let toks = lex("includes").expect("lex ok");
    assert_eq!(kinds(&toks)[0], &TokenKind::Includes);
}

#[test]
fn lexes_trait_not_confused_with_ident() {
    let toks = lex("trait_impl traits").expect("lex ok");
    let ks = kinds(&toks);
    for k in &ks[..2] {
        assert!(
            matches!(k, TokenKind::Ident(_)),
            "expected Ident, got {:?}",
            k
        );
    }
}

#[test]
fn lexes_includes_not_confused_with_ident() {
    let toks = lex("includes_all including").expect("lex ok");
    let ks = kinds(&toks);
    for k in &ks[..2] {
        assert!(
            matches!(k, TokenKind::Ident(_)),
            "expected Ident, got {:?}",
            k
        );
    }
}

#[test]
fn lexes_trait_definition_tokens() {
    let toks = lex("pub trait Printable").expect("lex ok");
    let ks = kinds(&toks);
    assert_eq!(ks[0], &TokenKind::Pub);
    assert_eq!(ks[1], &TokenKind::Trait);
    assert_eq!(ks[2], &TokenKind::Ident("Printable".into()));
}

#[test]
fn lexes_class_with_includes_tokens() {
    let toks = lex("class User includes Printable").expect("lex ok");
    let ks = kinds(&toks);
    assert_eq!(ks[0], &TokenKind::Class);
    assert_eq!(ks[1], &TokenKind::Ident("User".into()));
    assert_eq!(ks[2], &TokenKind::Includes);
    assert_eq!(ks[3], &TokenKind::Ident("Printable".into()));
}

#[test]
fn lexes_generic_class_tokens() {
    let toks = lex("class Stack[T]").expect("lex ok");
    let ks = kinds(&toks);
    assert_eq!(ks[0], &TokenKind::Class);
    assert_eq!(ks[1], &TokenKind::Ident("Stack".into()));
    assert_eq!(ks[2], &TokenKind::LBracket);
    assert_eq!(ks[3], &TokenKind::Ident("T".into()));
    assert_eq!(ks[4], &TokenKind::RBracket);
}

#[test]
fn lexes_dot_dot_exclusive() {
    let toks = lex("1..10").expect("lex ok");
    let ks = kinds(&toks);
    assert_eq!(ks[0], &TokenKind::Int(1));
    assert_eq!(ks[1], &TokenKind::DotDot);
    assert_eq!(ks[2], &TokenKind::Int(10));
}

#[test]
fn lexes_dot_dot_eq_inclusive() {
    let toks = lex("1..=10").expect("lex ok");
    let ks = kinds(&toks);
    assert_eq!(ks[0], &TokenKind::Int(1));
    assert_eq!(ks[1], &TokenKind::DotDotEq);
    assert_eq!(ks[2], &TokenKind::Int(10));
}

#[test]
fn lexes_dot_dot_with_idents() {
    let toks = lex("a..b").expect("lex ok");
    let ks = kinds(&toks);
    assert_eq!(ks[0], &TokenKind::Ident("a".into()));
    assert_eq!(ks[1], &TokenKind::DotDot);
    assert_eq!(ks[2], &TokenKind::Ident("b".into()));
}

#[test]
fn dot_still_works_after_range_tokens() {
    let toks = lex("x.y").expect("lex ok");
    let ks = kinds(&toks);
    assert_eq!(ks[0], &TokenKind::Ident("x".into()));
    assert_eq!(ks[1], &TokenKind::Dot);
    assert_eq!(ks[2], &TokenKind::Ident("y".into()));
}

#[test]
fn lexes_dot_dot_with_spaces() {
    let toks = lex("1 .. 10").expect("lex ok");
    let ks = kinds(&toks);
    assert_eq!(ks[0], &TokenKind::Int(1));
    assert_eq!(ks[1], &TokenKind::DotDot);
    assert_eq!(ks[2], &TokenKind::Int(10));
}

// ─── Non-ASCII in string literals ────────────────────────────────────

#[test]
fn string_with_emoji() {
    let toks = lex("\"hello 🌍\"").expect("lex ok");
    let ks = kinds(&toks);
    assert!(
        matches!(ks[0], TokenKind::Str(s) if s == "hello 🌍"),
        "expected emoji string, got {:?}",
        ks[0]
    );
}

#[test]
fn string_with_accented_chars() {
    let toks = lex("\"café résumé\"").expect("lex ok");
    let ks = kinds(&toks);
    assert!(
        matches!(ks[0], TokenKind::Str(s) if s == "café résumé"),
        "expected accented string, got {:?}",
        ks[0]
    );
}

#[test]
fn string_with_cjk_characters() {
    let toks = lex("\"你好世界\"").expect("lex ok");
    let ks = kinds(&toks);
    assert!(
        matches!(ks[0], TokenKind::Str(s) if s == "你好世界"),
        "expected CJK string, got {:?}",
        ks[0]
    );
}

#[test]
fn non_ascii_outside_string_rejected() {
    let err = lex("let café = 1").unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("Unexpected character")
            || msg.contains("unexpected character")
            || msg.contains("Invalid")
            || msg.contains("invalid"),
        "expected rejection of non-ASCII identifier, got: {msg}"
    );
}

#[test]
fn string_with_unicode_then_more_tokens() {
    let toks = lex("let x = \"héllo\"").expect("lex ok");
    let ks = kinds(&toks);
    assert_eq!(ks[0], &TokenKind::Let);
    assert_eq!(ks[1], &TokenKind::Ident("x".into()));
    assert_eq!(ks[2], &TokenKind::Equals);
    assert!(matches!(ks[3], TokenKind::Str(s) if s == "héllo"));
}
