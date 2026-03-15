// Green thread context switching — x86_64 (Linux + macOS)
//
// MachineContext layout (7 × 8 = 56 bytes):
//   [ 0.. 8)  rbx
//   [ 8..16)  rbp
//   [16..24)  r12
//   [24..32)  r13
//   [32..40)  r14
//   [40..48)  r15
//   [48..56)  rsp

#ifdef __APPLE__
#define SYM(name) _##name
#else
#define SYM(name) name
#endif

.text
.align 16

// void aster_context_switch(MachineContext *old, const MachineContext *new)
// rdi = old, rsi = new
.globl SYM(aster_context_switch)
SYM(aster_context_switch):
    // Save callee-saved registers
    movq %rbx,  0(%rdi)
    movq %rbp,  8(%rdi)
    movq %r12, 16(%rdi)
    movq %r13, 24(%rdi)
    movq %r14, 32(%rdi)
    movq %r15, 40(%rdi)
    movq %rsp, 48(%rdi)

    // Restore callee-saved registers
    movq  0(%rsi), %rbx
    movq  8(%rsi), %rbp
    movq 16(%rsi), %r12
    movq 24(%rsi), %r13
    movq 32(%rsi), %r14
    movq 40(%rsi), %r15
    movq 48(%rsi), %rsp

    ret     // pops return address from restored stack


// void aster_context_init(MachineContext *ctx, void *stack_top,
//                         uintptr_t entry, uintptr_t arg)
// rdi = ctx, rsi = stack_top, rdx = entry, rcx = arg
.globl SYM(aster_context_init)
SYM(aster_context_init):
    // Align stack_top down to 16 bytes
    andq $-16, %rsi

    // Push trampoline address — context_switch's ret will jump here.
    // After ret pops 8 bytes, rsp is 16-aligned (correct for ABI call).
    leaq SYM(aster_green_trampoline)(%rip), %rax
    subq $8, %rsi
    movq %rax, (%rsi)

    // Store entry and arg in callee-saved register slots.
    // rbx = entry, r12 = arg.
    movq %rdx,  0(%rdi)       // rbx slot = entry
    movq $0,    8(%rdi)        // rbp = 0
    movq %rcx, 16(%rdi)       // r12 slot = arg
    movq $0,   24(%rdi)        // r13 = 0
    movq $0,   32(%rdi)        // r14 = 0
    movq $0,   40(%rdi)        // r15 = 0
    movq %rsi, 48(%rdi)        // rsp = prepared stack

    ret


// Trampoline: first code a new green thread executes after context_switch.
// rbx = entry fn ptr, r12 = arg (restored by context_switch)
.globl SYM(aster_green_trampoline)
SYM(aster_green_trampoline):
    movq %r12, %rdi            // arg → first parameter
    callq *%rbx                // call entry(arg) → result in rax
    // Entry returned. rdi = result (i64).
    movq %rax, %rdi
    callq SYM(aster_green_thread_exit)
    ud2                        // trap — exit never returns
