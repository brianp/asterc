pub mod aot;
pub mod asm_source;
pub(crate) mod compile_shared;
pub mod config;
pub mod eval_pipeline;
pub(crate) mod green;
pub mod jit;
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub mod runtime;
pub mod runtime_sigs;
pub mod translate;
pub mod types;

pub use aot::CraneliftAOT;
pub use config::{BuildConfig, OptLevel, Profile};
pub use eval_pipeline::{RuntimeEvalError, jit_compile_and_run};
pub use jit::CraneliftJIT;

#[cfg(test)]
mod tests;
