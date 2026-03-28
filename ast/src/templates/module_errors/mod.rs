define_diagnostic!(ModuleNotFound { name: String }, "M001", |self| format!(
    "Module '{}' not found",
    self.name
));
define_diagnostic!(
    SymbolNotExported {
        symbol: String,
        module: String
    },
    "M002",
    |self| format!(
        "'{}' is not exported by module '{}'",
        self.symbol, self.module
    )
);
define_diagnostic!(CircularImport { module: String }, "M003", |self| format!(
    "Circular import detected involving '{}'",
    self.module
));
define_diagnostic!(
    InvalidImportAlias,
    "M004",
    "Cannot use both selective import and 'as' alias"
);
define_diagnostic!(
    UnstableRequired,
    "M005",
    "Importing from std/unstable requires the --unstable compiler flag or ASTER_UNSTABLE=1 environment variable"
);
