mod check_call;
mod check_class;
mod check_error;
mod check_expr;
pub mod module_loader;
pub mod typechecker;

#[cfg(test)]
mod tests;

pub use typechecker::TypeChecker;
