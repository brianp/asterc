use serde::{Deserialize, Serialize};

/// Trivia: whitespace, comments, and newlines that the compiler ignores
/// but the formatter must preserve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Trivia {
    /// Run of whitespace characters.
    Whitespace(String),
    /// Single-line comment (`# ...` to end of line), text includes the `#`.
    Comment(String),
    /// A newline character.
    Newline,
}

/// A token with attached trivia for the formatter pipeline.
#[derive(Debug, Clone)]
pub struct TriviaToken {
    pub kind: TokenKind,
    pub line: usize,
    pub col: usize,
    pub start: usize,
    pub end: usize,
    /// Comments/whitespace appearing before this token.
    pub leading_trivia: Vec<Trivia>,
    /// Comments/whitespace appearing after this token on the same line.
    pub trailing_trivia: Vec<Trivia>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TokenKind {
    // keywords
    Def,
    Class,
    Async,
    Return,
    If,
    Else,
    Elif,
    While,
    For,
    In,
    Break,
    Continue,
    True,
    False,
    Nil,
    Let,
    Use,
    As,
    Pub,
    Trait,
    Enum,
    Includes,
    Extends,
    Throw,
    Throws,
    Match,
    Catch,
    Resolve,
    Detached,
    Scope,
    Const,
    // structure
    Indent,
    Dedent,
    Newline,
    EOF,
    // punctuation / operators
    LParen,
    RParen,
    Comma,
    Colon,
    Arrow,
    Dot,
    Equals,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    StarStar,
    EqualEqual,
    BangEqual,
    Less,
    Greater,
    LessEqual,
    GreaterEqual,
    And,
    Or,
    Not,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Question,
    Bang,
    FatArrow,
    // literals / idents
    Int(i64),
    Float(f64),
    Str(String),
    Ident(String),
    // String interpolation tokens: "hello {name} world" becomes
    // StringStart("hello "), ...expr tokens..., StringMid(" world"), StringEnd("")
    // or StringStart("hello "), ...expr..., StringEnd(" world")
    StringStart(String),
    StringMid(String),
    StringEnd(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Token {
    pub kind: TokenKind,
    pub line: usize,
    pub col: usize,
    /// Byte offset of the first character of this token in the source.
    pub start: usize,
    /// Byte offset one past the last character of this token in the source.
    pub end: usize,
}
