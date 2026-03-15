// Green thread context switching — aarch64 (Apple Silicon + Linux)
//
// MachineContext layout (21 × 8 = 168 bytes):
//   [  0..80)  x19-x28 (10 callee-saved GPRs)
//   [ 80..88)  x29 (fp)
//   [ 88..96)  x30 (lr)
//   [ 96..104) sp
//   [104..168) d8-d15 (8 callee-saved SIMD)

#ifdef __APPLE__
#define SYM(name) _##name
#else
#define SYM(name) name
#endif

.text
.align 4

// void aster_context_switch(MachineContext *old, const MachineContext *new)
// x0 = old, x1 = new
.globl SYM(aster_context_switch)
SYM(aster_context_switch):
    // Save callee-saved GPRs
    stp x19, x20, [x0, #0]
    stp x21, x22, [x0, #16]
    stp x23, x24, [x0, #32]
    stp x25, x26, [x0, #48]
    stp x27, x28, [x0, #64]
    str x29, [x0, #80]
    str x30, [x0, #88]
    mov x9, sp
    str x9, [x0, #96]
    // Save callee-saved SIMD
    stp d8,  d9,  [x0, #104]
    stp d10, d11, [x0, #120]
    stp d12, d13, [x0, #136]
    stp d14, d15, [x0, #152]

    // Restore callee-saved GPRs
    ldp x19, x20, [x1, #0]
    ldp x21, x22, [x1, #16]
    ldp x23, x24, [x1, #32]
    ldp x25, x26, [x1, #48]
    ldp x27, x28, [x1, #64]
    ldr x29, [x1, #80]
    ldr x30, [x1, #88]
    ldr x9,  [x1, #96]
    mov sp, x9
    // Restore callee-saved SIMD
    ldp d8,  d9,  [x1, #104]
    ldp d10, d11, [x1, #120]
    ldp d12, d13, [x1, #136]
    ldp d14, d15, [x1, #152]

    ret     // branches to restored x30 (lr)


// void aster_context_init(MachineContext *ctx, void *stack_top,
//                         uintptr_t entry, uintptr_t arg)
// x0 = ctx, x1 = stack_top, x2 = entry fn ptr, x3 = arg ptr
.globl SYM(aster_context_init)
SYM(aster_context_init):
    // Align stack_top down to 16 bytes
    and x1, x1, #~0xf

    // Store entry and arg in callee-saved slots restored by context_switch.
    // x19 slot = entry, x20 slot = arg.
    str x2, [x0, #0]          // x19 = entry
    str x3, [x0, #8]          // x20 = arg

    // Zero remaining callee-saved GPRs
    stp xzr, xzr, [x0, #16]   // x21, x22
    stp xzr, xzr, [x0, #32]   // x23, x24
    stp xzr, xzr, [x0, #48]   // x25, x26
    stp xzr, xzr, [x0, #64]   // x27, x28
    str xzr, [x0, #80]        // fp

    // Set lr to trampoline — first code that runs after context_switch
#ifdef __APPLE__
    adrp x9, SYM(aster_green_trampoline)@PAGE
    add  x9, x9, SYM(aster_green_trampoline)@PAGEOFF
#else
    adrp x9, SYM(aster_green_trampoline)
    add  x9, x9, :lo12:SYM(aster_green_trampoline)
#endif
    str x9, [x0, #88]         // lr slot

    // Set sp
    str x1, [x0, #96]

    // Zero SIMD slots
    stp xzr, xzr, [x0, #104]
    stp xzr, xzr, [x0, #120]
    stp xzr, xzr, [x0, #136]
    stp xzr, xzr, [x0, #152]

    ret


// Trampoline: first code a new green thread executes after context_switch.
// x19 = entry fn ptr, x20 = arg (restored by context_switch)
.globl SYM(aster_green_trampoline)
SYM(aster_green_trampoline):
    mov x0, x20              // arg → first parameter
    blr x19                  // call entry(arg) → result in x0
    // Entry returned. x0 = result (i64).
    bl  SYM(aster_green_thread_exit)
    brk #1                   // trap — exit never returns
