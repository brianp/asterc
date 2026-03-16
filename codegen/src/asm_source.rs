/// Assembly source for AOT green thread context switching.
/// These are the same `.S` files used by the JIT runtime (compiled via build.rs),
/// embedded as string constants so the AOT build can write them to temp files
/// and compile them alongside the C runtime.
pub fn asm_source_for_target() -> &'static str {
    #[cfg(target_arch = "aarch64")]
    {
        AARCH64_ASM
    }
    #[cfg(target_arch = "x86_64")]
    {
        X86_64_ASM
    }
}

#[cfg(target_arch = "aarch64")]
const AARCH64_ASM: &str = include_str!("green/asm/aarch64.S");

#[cfg(target_arch = "x86_64")]
const X86_64_ASM: &str = include_str!("green/asm/x86_64.S");
