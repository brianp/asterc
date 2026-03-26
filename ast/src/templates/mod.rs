pub mod lex_errors;
pub mod module_errors;
pub mod parse_errors;
pub mod type_errors;
pub mod warnings;

use serde::{Deserialize, Serialize};

macro_rules! diagnostic_template_enum {
    (
        $(
            $variant:ident($inner:path)
        ),* $(,)?
    ) => {
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        pub enum DiagnosticTemplate {
            $( $variant($inner), )*
        }

        impl DiagnosticTemplate {
            pub fn code(&self) -> &'static str {
                match self {
                    $( Self::$variant(inner) => inner.code(), )*
                }
            }

            pub fn render(&self) -> String {
                match self {
                    $( Self::$variant(inner) => inner.render(), )*
                }
            }
        }
    };
}

diagnostic_template_enum! {
    // Type errors (E001-E028)
    TypeMismatch(type_errors::TypeMismatch),
    UndefinedVariable(type_errors::UndefinedVariable),
    BinaryOpError(type_errors::BinaryOpError),
    ReturnTypeMismatch(type_errors::ReturnTypeMismatch),
    ArgumentTypeMismatch(type_errors::ArgumentTypeMismatch),
    ArgumentCountMismatch(type_errors::ArgumentCountMismatch),
    MissingIterable(type_errors::MissingIterable),
    InvalidAssignment(type_errors::InvalidAssignment),
    UndeclaredAssignment(type_errors::UndeclaredAssignment),
    UnknownField(type_errors::UnknownField),
    MatchError(type_errors::MatchError),
    TaskAlreadyConsumed(type_errors::TaskAlreadyConsumed),
    ErrorPropagation(type_errors::ErrorPropagation),
    TraitError(type_errors::TraitError),
    ConditionTypeError(type_errors::ConditionTypeError),
    IndexTypeError(type_errors::IndexTypeError),
    InconsistentListType(type_errors::InconsistentListType),
    UnaryOpError(type_errors::UnaryOpError),
    ComparisonError(type_errors::ComparisonError),
    LogicalOpError(type_errors::LogicalOpError),
    ConstraintError(type_errors::ConstraintError),
    PrintableError(type_errors::PrintableError),
    TypeConstraintError(type_errors::TypeConstraintError),
    CollectionConstraintError(type_errors::CollectionConstraintError),
    ConstReassignment(type_errors::ConstReassignment),
    TaskNotResolved(type_errors::TaskNotResolved),
    NotCompilable(type_errors::NotCompilable),

    // Parse errors (P001-P003)
    UnexpectedToken(parse_errors::UnexpectedToken),
    ExpectedIndentedBlock(parse_errors::ExpectedIndentedBlock),
    NestingTooDeep(parse_errors::NestingTooDeep),

    // Module errors (M001-M004)
    ModuleNotFound(module_errors::ModuleNotFound),
    SymbolNotExported(module_errors::SymbolNotExported),
    CircularImport(module_errors::CircularImport),
    InvalidImportAlias(module_errors::InvalidImportAlias),

    // Warnings (W001-W004)
    RedundantTypeAnnotation(warnings::RedundantTypeAnnotation),
    UnusedDefaultParam(warnings::UnusedDefaultParam),
    UseAfterMove(warnings::UseAfterMove),
    ShadowedVariable(warnings::ShadowedVariable),

    // Lex errors (L001-L008)
    InterpolationError(lex_errors::InterpolationError),
    UnterminatedString(lex_errors::UnterminatedString),
    TabIndentation(lex_errors::TabIndentation),
    InvalidEscape(lex_errors::InvalidEscape),
    StringTooLong(lex_errors::StringTooLong),
    BadFloatLiteral(lex_errors::BadFloatLiteral),
    IntegerOverflow(lex_errors::IntegerOverflow),
    MissingNewline(lex_errors::MissingNewline),
}
