pub mod aot;
pub mod async_runtime;
pub mod config;
pub mod jit;
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub mod runtime;
pub mod runtime_sigs;
pub mod runtime_source;
pub mod translate;
pub mod types;

pub use aot::CraneliftAOT;
pub use config::{BuildConfig, OptLevel, Profile};
pub use jit::CraneliftJIT;

#[cfg(test)]
mod tests;
