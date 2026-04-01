define_diagnostic!(UnusedDefaultParam { name: String }, "W001", |self| format!(
    "Variable '{}' has default parameter",
    self.name
));
define_diagnostic!(UseAfterMove { name: String }, "W002", |self| format!(
    "Variable '{}' used after copy/move boundary",
    self.name
));
define_diagnostic!(ShadowedVariable { name: String }, "W003", |self| format!(
    "Variable '{}' shadows a previous binding",
    self.name
));
define_diagnostic!(
    RedundantTypeAnnotation { type_name: String },
    "W004",
    |self| format!(
        "redundant type annotation: type `{}` can be inferred",
        self.type_name
    )
);
define_diagnostic!(RedundantMainReturn {}, "W005", |self| {
    "main() implicitly returns exit code 0; remove `-> Int` and the trailing `0`".to_string()
});
