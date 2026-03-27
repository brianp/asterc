use crate::types::Type;

define_diagnostic!(
    TypeMismatch {
        expected: Type,
        actual: Type
    },
    "E001",
    |self| format!(
        "Type annotation mismatch: expected {}, got {}",
        self.expected, self.actual
    )
);
define_diagnostic!(UndefinedVariable { name: String }, "E002", |self| format!(
    "Unknown identifier '{}'",
    self.name
));
define_diagnostic!(
    BinaryOpError {
        op: String,
        left: Type,
        right: Type
    },
    "E003",
    |self| format!(
        "'{}' used outside of a valid context or with incompatible types {} and {}",
        self.op, self.left, self.right
    )
);
define_diagnostic!(
    ReturnTypeMismatch {
        function: String,
        expected: Type,
        actual: Type
    },
    "E004",
    |self| format!(
        "Return type mismatch in '{}': expected {}, got {}",
        self.function, self.expected, self.actual
    )
);
define_diagnostic!(
    ArgumentTypeMismatch {
        param: String,
        expected: Type,
        actual: Type
    },
    "E005",
    |self| format!(
        "Argument '{}' expects {}, got {}",
        self.param, self.expected, self.actual
    )
);
define_diagnostic!(
    ArgumentCountMismatch {
        expected: usize,
        actual: usize
    },
    "E006",
    |self| format!(
        "Function parameter count mismatch: expected {}, got {}",
        self.expected, self.actual
    )
);
define_diagnostic!(
    MissingIterable { type_name: String },
    "E007",
    |self| format!(
        "Class '{}' does not have required each() method",
        self.type_name
    )
);
define_diagnostic!(InvalidAssignment, "E008", "Invalid assignment target");
define_diagnostic!(
    UndeclaredAssignment { name: String },
    "E009",
    |self| format!("Assignment to undeclared variable '{}'", self.name)
);
define_diagnostic!(
    UnknownField {
        field: String,
        type_name: String
    },
    "E010",
    |self| format!(
        "Unknown field '{}' on type '{}'",
        self.field, self.type_name
    )
);
define_diagnostic!(MatchError { message: String }, "E011", |self| self
    .message
    .clone());
define_diagnostic!(
    TaskAlreadyConsumed { name: String },
    "E012",
    |self| format!("Task '{}' is already consumed", self.name)
);
define_diagnostic!(ErrorPropagation { message: String }, "E013", |self| self
    .message
    .clone());
define_diagnostic!(TraitError { message: String }, "E014", |self| self
    .message
    .clone());
define_diagnostic!(ConditionTypeError { actual: Type }, "E015", |self| format!(
    "Condition must be Bool, got {}",
    self.actual
));
define_diagnostic!(IndexTypeError { actual: Type }, "E016", |self| format!(
    "Index must be Int, got {}",
    self.actual
));
define_diagnostic!(
    InconsistentListType {
        expected: Type,
        actual: Type
    },
    "E017",
    |self| format!(
        "List elements have inconsistent types: expected {}, got {}",
        self.expected, self.actual
    )
);
define_diagnostic!(
    UnaryOpError {
        op: String,
        actual: Type
    },
    "E018",
    |self| format!(
        "Cannot apply '{}' to {} (expected Bool)",
        self.op, self.actual
    )
);
define_diagnostic!(ComparisonError { message: String }, "E019", |self| self
    .message
    .clone());
define_diagnostic!(LogicalOpError, "E020", "'and'/'or' operands must be Bool");
define_diagnostic!(ConstraintError { message: String }, "E021", |self| self
    .message
    .clone());
// Note: error code E022 is unassigned.
define_diagnostic!(PrintableError, "E023", "Expression must be Printable");
define_diagnostic!(TypeConstraintError { message: String }, "E024", |self| self
    .message
    .clone());
define_diagnostic!(
    CollectionConstraintError { message: String },
    "E025",
    |self| self.message.clone()
);
define_diagnostic!(ConstReassignment { name: String }, "E026", |self| format!(
    "const binding '{}' cannot be reassigned",
    self.name
));
define_diagnostic!(TaskNotResolved { name: String }, "E027", |self| format!(
    "Task '{}' created but never resolved",
    self.name
));
define_diagnostic!(NotCompilable { message: String }, "E028", |self| self
    .message
    .clone());
define_diagnostic!(
    ControlFlowError { keyword: String },
    "E029",
    |self| format!("`{}` can only be used inside a loop", self.keyword)
);
define_diagnostic!(SuspensionError { message: String }, "E030", |self| self
    .message
    .clone());
define_diagnostic!(
    VisibilityError {
        member: String,
        class_name: String
    },
    "E031",
    |self| format!(
        "'{}' is private in '{}' and cannot be accessed from outside the module",
        self.member, self.class_name
    )
);
