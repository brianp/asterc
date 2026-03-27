define_diagnostic!(
    InterpolationError,
    "L001",
    "Unexpected character in string interpolation"
);
define_diagnostic!(
    UnterminatedString,
    "L002",
    "Unterminated string or escape sequence"
);
define_diagnostic!(
    TabIndentation,
    "L003",
    "Tab character found (use spaces for indentation)"
);
define_diagnostic!(InvalidEscape { sequence: String }, "L004", |self| format!(
    "Invalid escape sequence '\\{}'",
    self.sequence
));
define_diagnostic!(StringTooLong, "L005", "String exceeds maximum length");
define_diagnostic!(BadFloatLiteral { line: usize }, "L006", |self| format!(
    "Bad float literal at line {}",
    self.line
));
define_diagnostic!(
    IntegerOverflow,
    "L007",
    "Integer literal overflows i64 range"
);
define_diagnostic!(MissingNewline, "L008", "File must end with newline");
define_diagnostic!(
    InputTooLarge {
        size: usize,
        limit: usize
    },
    "L009",
    |self| format!(
        "input is {} bytes, exceeding the maximum of {} bytes",
        self.size, self.limit
    )
);
define_diagnostic!(
    InconsistentIndentation,
    "L010",
    "indentation does not match any previous indentation level"
);
define_diagnostic!(UnexpectedCharacter { ch: char }, "L011", |self| format!(
    "unexpected character '{}'",
    self.ch
));
define_diagnostic!(
    BadIntegerLiteral { literal: String },
    "L012",
    |self| format!("invalid integer literal '{}'", self.literal)
);
