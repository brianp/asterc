// Machine context for green thread stack switching.
// Layout matches the platform assembly exactly — do not reorder fields.

#[cfg(target_arch = "aarch64")]
const CONTEXT_REGS: usize = 21;
// x19-x28 (10), x29/fp, x30/lr, sp, d8-d15 (8) = 21 slots

#[cfg(target_arch = "x86_64")]
const CONTEXT_REGS: usize = 7;
// rbx, rbp, r12-r15, rsp = 7 slots

#[repr(C)]
pub(crate) struct MachineContext {
    regs: [u64; CONTEXT_REGS],
}

impl MachineContext {
    pub(crate) fn new() -> Self {
        Self {
            regs: [0; CONTEXT_REGS],
        }
    }
}
