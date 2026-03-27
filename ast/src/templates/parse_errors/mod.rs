define_diagnostic!(
    UnexpectedToken {
        expected: String,
        found: String
    },
    "P001",
    |self| format!("Expected {}, found {}", self.expected, self.found)
);
define_diagnostic!(
    ExpectedIndentedBlock,
    "P002",
    "Expected indented block after colon"
);
define_diagnostic!(NestingTooDeep, "P003", "Maximum nesting depth exceeded");
