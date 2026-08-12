# Ruby (CRuby/YARV) Architecture Analysis

Comparative analysis of how Rust and Ruby handle core language architecture concerns, with focus on CRuby internals.

---

## 1. Intermediate Representations

### What IR stages does Rust use between parsing and LLVM IR? How does HIR differ from MIR, and why does Rust need both?

Rust uses four major IR stages:

1. **AST** -- the direct parse tree, closely mirroring surface syntax
2. **HIR (High-level IR)** -- desugared AST. `for` loops become iterator calls, `if let` becomes `match`, etc. Still tree-shaped and name-resolved. This is what type checking operates on.
3. **THIR (Typed HIR)** -- fully type-annotated HIR, a transient step used during MIR construction. Exists to make pattern exhaustiveness checking and MIR lowering cleaner.
4. **MIR (Mid-level IR)** -- a control flow graph in SSA-like form. Functions are decomposed into basic blocks with explicit temporaries, drops, and borrows. This is where borrow checking, const-time function evaluation, and Rust-specific optimizations happen.
5. **LLVM IR** -- MIR is lowered to LLVM IR for final optimization and machine code generation.

Rust needs both HIR and MIR because they serve different purposes. HIR preserves enough structure for type inference and trait resolution (which need to understand expressions, patterns, and type annotations). MIR flattens everything into a CFG where ownership, borrowing, and drop semantics can be analyzed mechanically. The borrow checker operates on MIR because it needs to reason about control flow paths, not expression trees.

### How does YARV bytecode work in Ruby? What does the instruction set look like and how does it map to Ruby semantics?

YARV (Yet Another Ruby VM) is a stack-based bytecode virtual machine. Ruby source is parsed into an AST, then compiled (in `compile.c`, ~15,000 lines) into **instruction sequences** (`rb_iseq_t`).

The instruction set contains ~90 instructions defined in `insns.def`. Each instruction specifies its operand types, stack pops, and stack pushes:

**Variable access:** `getlocal`/`setlocal` (with nesting level for closures), `getinstancevariable`/`setinstancevariable`, `getglobal`/`setglobal`, `getconstant`/`setconstant`

**Stack operations:** `pop`, `dup`, `dupn`, `swap`, `topn`, `setn`, `putnil`, `putself`, `putobject`, `putstring`

**Control flow:** `jump`, `branchif`, `branchunless`, `branchnil`, `opt_case_dispatch` (hash-table jump for case/when)

**Method calls:** `send` (with block), `opt_send_without_block`, `invokeblock` (yield), `invokesuper`

**Optimized operations:** `opt_plus`, `opt_minus`, `opt_mult`, `opt_lt`, `opt_eq`, `opt_aref`, `opt_length`, etc. -- these provide fast paths for common operations on known types and fall back to full method dispatch via `CALL_SIMPLE_METHOD` if the optimization doesn't apply.

**Class/method definition:** `defineclass`, `definemethod`, `definesmethod`

**Exception handling:** `throw` (for break/next/redo/return in blocks), with a separate catch table per instruction sequence.

Each instruction sequence (`rb_iseq_t`) has a type: `ISEQ_TYPE_TOP`, `ISEQ_TYPE_METHOD`, `ISEQ_TYPE_BLOCK`, `ISEQ_TYPE_CLASS`, etc. Blocks and lambdas become child ISEQs linked to their parent.

### How do Rust's MIR and Ruby's YARV handle control flow representation differently?

**Rust's MIR** is a proper **control flow graph (CFG)**. Functions are collections of basic blocks, each ending in a terminator (goto, switch, return, drop, unwind). There are no implicit fall-throughs. Every branch is an edge in the graph. This makes dataflow analysis (borrow checking, const propagation, dead code elimination) straightforward.

**YARV bytecode** is **linear with jumps**. Instructions are laid out sequentially in a flat array (`iseq_encoded`). Control flow is represented by `jump`, `branchif`, `branchunless`, and `branchnil` instructions that reference bytecode offsets. There's no explicit basic-block structure -- the VM just follows the program counter, branching as directed. Exception handling uses a separate **catch table** that maps PC ranges to handler offsets, rather than explicit edges.

The key difference: MIR's CFG is designed for static analysis (the compiler must prove properties about all paths). YARV's linear bytecode is designed for efficient interpretation (the VM just executes the next instruction).

### What information is preserved vs discarded at each IR stage in Rust? When do generics get monomorphized?

- **AST -> HIR:** Syntax sugar is removed (for loops, if let, while let, closures desugared). Macro expansions are resolved. Names are resolved. Source spans preserved.
- **HIR -> MIR:** Type information is fully resolved. Patterns are compiled to decision trees. Drops are made explicit. Borrows become explicit operations. Expression trees become flat SSA-like assignments in basic blocks. Match arms become branching CFGs.
- **MIR -> LLVM IR:** Rust-specific concepts (borrows, ownership) are erased -- they've been validated. Generics are **monomorphized** at this stage: each concrete instantiation of a generic function gets its own copy of the LLVM IR. This happens during codegen, after all MIR optimizations.

Monomorphization is a codegen-time operation. MIR itself remains generic -- `fn foo<T: Display>(x: T)` exists as a single MIR body. Only when generating LLVM IR does the compiler stamp out `foo::<i32>`, `foo::<String>`, etc.

---

## 2. Lowering Passes

### How does Rust lower HIR to MIR? What gets desugared?

HIR-to-MIR lowering (via THIR) handles:

- **`for` loops** -> `IntoIterator::into_iter()` + `loop { match iter.next() { Some(x) => body, None => break } }`
- **`?` operator** -> `match expr { Ok(v) => v, Err(e) => return Err(From::from(e)) }`
- **`async/await`** -> state machine struct implementing `Future::poll`. Each `.await` point becomes a state transition.
- **`match` expressions** -> decision trees with explicit temporaries, bindings, and guard evaluation
- **Operator overloads** -> trait method calls (`a + b` -> `Add::add(a, b)`)
- **Closure expressions** -> anonymous struct capturing variables, implementing `Fn`/`FnMut`/`FnOnce`
- **Method calls** -> resolved to concrete function calls or vtable lookups
- **Drop insertion** -> explicit `drop()` calls inserted at scope exits, before moves, and along unwind paths

The result is a flat CFG where every operation is explicit. No implicit conversions, no sugar, no hidden control flow.

### How does Ruby compile AST nodes into YARV instructions? What happens to blocks, procs, and method lookups during compilation?

Ruby's compiler (`compile.c`) walks the AST recursively, emitting instructions into a linked list of `LINK_ELEMENT` nodes (instructions and labels). Key compilations:

**Blocks:** When compiling `foo { |x| body }`, the block body is compiled into a **child ISEQ** of type `ISEQ_TYPE_BLOCK`. The `send` instruction receives both a `CALL_DATA` descriptor and a pointer to this child ISEQ. At runtime, the VM creates a block handler from the ISEQ.

**Procs/Lambdas:** `Proc.new` and `->{}` also compile to child ISEQs. The difference is in how they handle return/break semantics -- lambdas check arity and `return` exits the lambda, while procs have `return` exit the enclosing method. This is tracked at runtime, not compile time.

**Method lookups:** Method calls compile to `send` (with block) or `opt_send_without_block` instructions. The method name (ID) and argument info are packed into `CALL_DATA`. Common operations like `+`, `-`, `[]`, `length` get specialized `opt_*` instructions that check for known receiver types at runtime and fast-path the operation.

**Control flow:** `if/unless` become `branchif`/`branchunless` with labels resolved to offsets in a final assembly pass. `while/until` loops use three labels (redo, next, break) with catch table entries for `break`/`next`/`redo` inside blocks.

### How does Rust handle closure conversion during lowering? How are captured variables represented in MIR?

Rust closures become anonymous structs at the MIR level. Each captured variable becomes a field of this struct. The capture mode determines the field type:

- **By shared reference** (Fn trait) -- field is `&T`
- **By mutable reference** (FnMut) -- field is `&mut T`
- **By value/move** (FnOnce) -- field is `T`

The compiler analyzes how each captured variable is used in the closure body to determine the minimal capture mode. Since Rust 2021, captures are precise -- only the specific fields used are captured, not entire variables.

In MIR, the closure struct is constructed with explicit field assignments, and closure calls are method calls on the struct.

### How does Ruby handle its block/proc/lambda trio during bytecode compilation? How do they differ at the instruction level?

At the bytecode level, blocks, procs, and lambdas all compile to child ISEQs. The differences are in runtime semantics:

**Blocks** are compiled as the `blockiseq` operand of a `send` instruction. They're invoked via `invokeblock` (yield). They're lightweight -- no Proc object is allocated unless `&block` captures it. The `getblockparam` instruction lazily converts the block to a Proc only when needed. `getblockparamproxy` provides a lightweight proxy that avoids Proc allocation when the block is just passed through.

**Procs** (via `Proc.new` or `proc {}`) are Proc objects wrapping a block. `return` inside a proc exits the enclosing method (using the `throw` instruction with `TAG_RETURN`), which can raise `LocalJumpError` if the method has already returned.

**Lambdas** (via `->{}` or `lambda {}`) are Proc objects with stricter semantics. `return` exits only the lambda. Arity is checked. The distinction is a flag on the Proc object, not in the bytecode itself.

The `throw` instruction handles all non-local exits, with a `throw_state` operand distinguishing break, next, redo, retry, and return. The catch table maps these to the appropriate handler.

---

## 3. Type Systems and Inference

### How does Rust's type inference work? What algorithm does it use?

Rust uses a **constraint-based** type inference system, influenced by Hindley-Milner but significantly extended. The key phases:

1. **Type variable introduction:** Unknown types get fresh type variables (inference variables).
2. **Constraint generation:** The type checker walks HIR, generating equality and subtype constraints between types. Method calls generate trait obligation constraints.
3. **Unification:** Constraints are solved via unification -- when two type expressions must be equal, their variables are unified. This is iterative; solving one constraint can unlock others.
4. **Trait resolution:** When a type variable is involved in a trait bound (e.g., `T: Display`), the compiler searches for matching impls. This interleaves with unification.
5. **Fallback:** If inference is incomplete (e.g., integer literals default to `i32`, float literals to `f64`), fallback rules apply.

It's bidirectional in practice: expected types flow inward (e.g., `let x: Vec<i32> = vec.iter().collect()` tells `collect` what to produce), and actual types flow outward.

### Ruby is dynamically typed, but Sorbet and RBS add gradual typing. How do these systems handle inference vs annotation?

Ruby itself performs no static type checking. Types are checked at runtime via method dispatch -- if an object responds to a method, it works (duck typing).

**RBS** is Ruby's official type signature language. It lives in separate `.rbs` files and describes the types of methods, constants, and globals. RBS itself doesn't do inference -- it's a declaration format. Tools like Steep use RBS signatures to type-check Ruby code with flow-sensitive local type inference.

**Sorbet** is Stripe's gradual type system. It uses inline `sig` annotations in Ruby code. Sorbet performs:
- Local type inference within method bodies (flow-sensitive)
- Method-level type checking against declared signatures
- `T.untyped` as an escape hatch for untyped code
- Gradual typing: untyped code interoperates freely with typed code

Both systems are **gradual** -- they check what's annotated and trust the rest. Neither attempts full Hindley-Milner inference across the program.

### How does Rust handle trait resolution and coherence checking? What is the specialization problem?

**Trait resolution** finds which `impl` block satisfies a trait bound. Given `T: Display`, the compiler searches for `impl Display for T`. This involves:
- Checking inherent impls on the type
- Checking blanket impls (`impl<T: Debug> Display for T`)
- Checking where-clause bounds
- Resolving associated types

**Coherence** (the orphan rule) ensures at most one impl exists for any concrete type. You can only implement a trait for a type if you own either the trait or the type. This prevents conflicting impls across crates.

**Specialization** is an unstable feature that would allow a more specific impl to override a more general one (e.g., `impl<T> Foo for T` overridden by `impl Foo for String`). The problem is soundness: the compiler must prove the more specific impl is truly a refinement, not a contradictory override. This interacts poorly with lifetime-dependent impls and has been stuck in nightly for years.

### How do Rust's lifetime annotations interact with type inference? Can lifetimes be fully inferred?

Lifetimes are part of Rust's type system. Within function bodies, lifetimes are **fully inferred** by the borrow checker (NLL -- Non-Lexical Lifetimes). You never annotate lifetimes inside a function.

At function **signatures**, lifetime elision rules handle common cases:
1. Each input reference gets its own lifetime
2. If there's exactly one input lifetime, it's used for all outputs
3. If `&self` or `&mut self` is an input, its lifetime is used for all outputs

When elision rules don't apply (e.g., two input references, one output reference), you must annotate. The annotations constrain the borrow checker but don't change runtime behavior -- they're purely a static analysis tool.

---

## 4. Generics and Monomorphization vs Erasure

### How does Rust monomorphize generic functions? At what compilation stage does this happen?

Monomorphization occurs during **codegen** (MIR -> LLVM IR translation). The compiler collects all concrete instantiations of each generic function across the crate graph, then generates a separate LLVM IR function for each. For example, `Vec<i32>` and `Vec<String>` produce different code.

The process is demand-driven: only instantiations actually used in the program are generated. The compiler starts from non-generic entry points and recursively discovers needed instantiations.

### How does Rust handle trait objects (dyn Trait) as an alternative to monomorphization? What are the vtable layout rules?

`dyn Trait` uses **type erasure with vtables**. A trait object is a fat pointer: `(data_ptr, vtable_ptr)`. The vtable contains:
1. `drop` function pointer
2. `size` and `align` of the concrete type
3. Function pointers for each method of the trait, in declaration order

This allows dynamic dispatch at the cost of:
- Indirect function calls (no inlining)
- Fat pointer overhead (2 words instead of 1)
- Object-safety restrictions (no `Self`-returning methods, no generic methods)

### Ruby has no generics at the language level. How does YARV handle polymorphic call sites? What role do inline caches play?

Ruby handles polymorphism entirely at runtime through **inline method caches** and **call caches**.

Each `send`/`opt_send_without_block` instruction has an associated **call cache** (`rb_callcache`):

```c
struct rb_callcache {
    VALUE klass;                              // receiver class (weak ref)
    rb_callable_method_entry_t *cme_;         // cached method entry
    vm_call_handler call_;                    // direct function pointer
};
```

On a method call:
1. Check if `receiver.class == cc->klass` -- if so, call `cc->call_` directly (fast path)
2. On cache miss, perform full method lookup (traverse ancestor chain via `RCLASS_SUPER()`), populate cache

The cache stores a **direct function pointer** to the method invoker, eliminating dispatch overhead on cache hits. There's a three-level cache hierarchy:
1. Per-instruction call cache (fastest)
2. Per-class call cache table (`RCLASS_WRITABLE_CC_TBL`)
3. Full method table lookup (slowest)

Cache invalidation happens when methods are defined, redefined, removed, or modules are included/prepended. The `rb_callable_method_entry_t` is marked `METHOD_ENTRY_INVALIDATED`, causing cache checks to fail.

YJIT extends this with **type-specialized code paths** -- it generates machine code specialized for the observed receiver types, with guards that deoptimize if the type changes.

### What are the compile-time and binary-size tradeoffs of Rust's monomorphization approach?

**Compile time:** Each generic instantiation generates separate code, leading to more code for LLVM to optimize. `Vec<i32>`, `Vec<String>`, `Vec<MyStruct>` each produce a full copy of Vec's methods. This is a major contributor to Rust's compile times.

**Binary size:** Monomorphization produces larger binaries than type erasure. The same algorithm duplicated across types adds up. The compiler merges identical function bodies when possible, but different types usually produce different code.

**Performance:** The payoff is that monomorphized code can be fully inlined and optimized per type. No vtable indirection, no dynamic dispatch overhead. The compiler knows the exact types and can optimize accordingly.

---

## 5. Memory Management

### How does Rust's ownership/borrowing model work at the compiler level? How are moves, borrows, and drops tracked in MIR?

In MIR, every value has an explicit place (local variable or temporary). The borrow checker tracks:

- **Moves:** An assignment `_2 = move _1` transfers ownership. After the move, `_1` is uninitialized. Using `_1` after the move is an error.
- **Borrows:** `_2 = &_1` or `_2 = &mut _1` create references. The borrow checker ensures no aliasing of mutable references, and that references don't outlive their referents.
- **Drops:** `drop(_1)` is inserted explicitly at scope exits, before reassignment, and along unwind paths. Drop elaboration ensures drops happen exactly once and in the right order (reverse declaration order).

The borrow checker (NLL -- Non-Lexical Lifetimes) computes liveness regions for each borrow. A borrow is live from its creation until its last use. The checker verifies that no conflicting access occurs during a borrow's live range.

**Polonius** is the next-generation borrow checker (still experimental) that uses a different formulation based on "origin" analysis, handling some cases NLL rejects.

### How does Ruby's garbage collector work? What algorithm does CRuby use?

CRuby uses a **generational, incremental, mark-sweep collector with optional compaction**.

**Heap organization:** Ruby uses 5 object pools by slot size (48, 96, 192, 384, 768 bytes on 64-bit). Objects are allocated in 64KB-aligned heap pages, each containing fixed-size slots. A freelist per page enables O(1) allocation.

**Generational collection:** Objects have a 2-bit age field (0-3). Age 3 = old generation. Minor GCs only scan young objects. Old objects that reference young objects are tracked in a **remembered set** via write barriers. This avoids scanning the entire heap on every GC.

**Marking:** Tri-color mark (white = unmarked, grey = marked but children not visited, black = fully marked). The mark stack processes grey objects incrementally. During incremental marking, mutator operations go through write barriers to maintain the tri-color invariant.

**Sweeping:** Lazy sweeping -- pages are swept on-demand during allocation, not all at once. This spreads sweep work across allocations, reducing pause times.

**Compaction:** `GC.compact` optionally moves objects to reduce fragmentation. Uses forwarding pointers (`T_MOVED` type) and requires all references to be updated. C extensions must use `rb_gc_impl_location()` to follow forwarding pointers.

### How does Rust insert drop calls? What is drop elaboration and when does it run?

Drop elaboration runs as a MIR pass after borrow checking. It inserts explicit `drop()` calls by:

1. Computing which variables are initialized at each point (dataflow analysis)
2. At scope exits (block end, return, break), inserting drops for all live variables in reverse declaration order
3. At reassignment (`x = new_value`), inserting a drop for the old value first
4. Along unwind paths (panic), inserting drops for all initialized variables
5. For partial moves (e.g., moving one field of a struct), inserting drops for the remaining fields

Drop elaboration handles conditional initialization: if `x` is only initialized in one branch of an `if`, the drop is guarded by a flag tracking whether `x` was initialized.

### How does Ruby's GC handle object pinning for C extensions? What is the write barrier and why is it needed?

**Object pinning:** C extensions can hold raw pointers to Ruby objects. During compaction, these objects must not move. `rb_gc_impl_mark_and_pin()` marks an object and sets its `pinned_bits`, preventing the compactor from relocating it. Pages with many pinned objects are sorted to minimize fragmentation impact.

**Write barriers** are needed for two reasons:

1. **Generational correctness:** When an old object gains a reference to a young object, the old object must be added to the remembered set. Without this, minor GCs would miss reachable young objects. `gc_writebarrier_generational()` handles this case.

2. **Incremental marking correctness:** During incremental marking, if a black (fully scanned) object gains a reference to a white (unscanned) object, the white object would be missed. `gc_writebarrier_incremental()` re-marks the white object to maintain the tri-color invariant.

C extensions that store VALUE references in C structs are "write-barrier unprotected" -- they're always rescanned during minor GC, which is slower but safe.

### How do Rust's lifetimes get validated? What is the borrow checker's actual algorithm (NLL, Polonius)?

**NLL (Non-Lexical Lifetimes)** is the current production algorithm:

1. Compute the **liveness** of each borrow -- the region of MIR where the borrow is live (from creation to last use along any CFG path)
2. For each borrow, verify that no conflicting access occurs during its live region:
   - Shared borrows conflict with mutations
   - Mutable borrows conflict with any other access
3. Verify that borrowed-from places aren't moved while borrowed
4. Verify that borrows don't escape their scope (function returns, assignments to longer-lived variables)

NLL is "non-lexical" because borrow regions follow dataflow, not syntactic scope. `let r = &x; use(r); mutate(x);` is fine because `r` is dead before `mutate`.

**Polonius** reformulates borrow checking around "origins" (provenance of references). It handles cases like conditional borrows more precisely, accepting some programs NLL rejects. Still experimental.

---

## 6. Async and Concurrency

### How does Rust lower async/await into state machines? What does a desugared Future look like in MIR?

An `async fn` is lowered to a struct implementing `Future`. Each `.await` point becomes a **state** in an enum-like state machine:

```rust
// async fn example() -> i32 {
//     let x = fetch().await;  // state 0 -> 1
//     let y = compute().await; // state 1 -> 2
//     x + y                    // state 2 -> done
// }

// Becomes (conceptually):
// enum ExampleFuture {
//     State0 { fetch_future: FetchFuture },
//     State1 { x: i32, compute_future: ComputeFuture },
//     Done,
// }
```

The `poll` method matches on the current state, polls the inner future, and transitions to the next state on `Poll::Ready`. Local variables that survive across `.await` points are stored as fields of the state machine struct. Variables that don't cross `.await` points are stack-allocated normally.

In MIR, this is a generator with yield points. The compiler performs a "generator transform" pass that moves the MIR into a state-machine shape before codegen.

### How does Ruby implement Fibers and Ractors? How does the GVL/GIL interact with concurrency?

**Fibers** (`cont.c`) are cooperative coroutines with explicit context switching:

- Each fiber has its own stack, allocated from a **fiber pool** (pre-allocated stacks with guard pages)
- Fiber states: `CREATED -> RESUMED -> SUSPENDED -> TERMINATED`
- Context switching uses platform-specific coroutine primitives (`ucontext`, `setjmp/longjmp`, or assembly)
- `Fiber#resume` and `Fiber.yield` transfer control between fibers

**Ractors** (`ractor.c`) are Ruby 3.0+'s parallel execution model:

- Each ractor is an isolated execution context with its own GVL
- Communication via message passing (`Ractor.send`/`Ractor.receive`)
- Objects must be "shareable" to cross ractor boundaries (frozen, or explicitly marked)
- Each ractor can run on a separate OS thread in parallel

**GVL (Global VM Lock):** Within a single ractor, the GVL prevents true parallelism of Ruby threads. Threads can run in parallel during I/O or C extension calls that release the GVL. Ractors each have their own GVL, enabling true CPU parallelism between ractors.

### How does Rust's Pin mechanism work and why is it needed for async?

`Pin<P>` wraps a pointer type `P` and guarantees the pointed-to value won't be moved in memory. This is critical for async because:

1. State machine structs from async/await contain self-referential pointers (e.g., a reference to a local variable stored in the same struct)
2. If the struct moves, these internal pointers become dangling
3. `Pin` prevents moves after the value is pinned, making self-references safe

`Pin` is a library type that works with the `Unpin` trait. Types that are `Unpin` (most types) can be freely moved even when pinned. Only self-referential types (like generated Futures) are `!Unpin` and actually restricted by Pin.

### How does Ruby 3's Fiber Scheduler interface work? How does it compare to green threading?

The Fiber Scheduler interface (`Fiber.set_scheduler`) lets libraries hook into Ruby's blocking operations:

1. A scheduler object implements hooks like `io_wait`, `io_read`, `io_write`, `kernel_sleep`, `block`, `unblock`
2. When a fiber performs a blocking operation (I/O, sleep, mutex), Ruby calls the scheduler hook instead of blocking the OS thread
3. The scheduler can suspend the fiber and resume it when the operation completes
4. This enables event-loop-based concurrency (like Node.js) without changing application code

Compared to green threading: green threads multiplex many threads onto fewer OS threads with preemptive scheduling. Ruby's fiber scheduler is **cooperative** -- fibers only yield at known blocking points. This is simpler and more predictable but requires all blocking operations to be scheduler-aware.

### How does Tokio's work-stealing scheduler differ from Ruby's fiber scheduling?

**Tokio** uses a multi-threaded work-stealing scheduler:
- N worker threads (typically one per CPU core)
- Each worker has a local task queue
- Idle workers steal tasks from busy workers' queues
- Tasks (Futures) are scheduled preemptively at `.await` points
- Fully concurrent: multiple tasks run in parallel on different threads

**Ruby's fiber scheduler** is single-threaded cooperative scheduling:
- One OS thread runs all fibers
- Fibers yield only at explicit blocking points
- No parallelism within a single ractor
- Scheduler decides which fiber to resume next

The fundamental difference: Tokio provides both concurrency and parallelism. Ruby's fiber scheduler provides only concurrency (parallelism requires ractors or threads).

---

## 7. Method Resolution and Dispatch

### How does Rust resolve trait method calls? What is the method resolution order, and how do auto-deref and coercions factor in?

Rust method resolution follows a priority order:

1. **Inherent methods** on the type itself
2. **Trait methods** from in-scope traits
3. **Auto-deref chain:** If no match, deref the receiver (`T` -> `*T` if `Deref` implemented) and retry. This chains: `T` -> `&T` -> `&&T`, or `Vec<T>` -> `[T]` -> `T`
4. **Unsized coercion:** As a last resort, coerce to unsized types (`[T; N]` -> `[T]`)

For trait methods, if multiple in-scope traits define a method with the same name, the call is ambiguous and requires disambiguation with fully-qualified syntax: `Trait::method(&receiver)`.

### How does Ruby's method lookup work? What is the method resolution order across classes, modules, and prepended modules?

Ruby's method resolution order (MRO) traverses the **ancestor chain** via `RCLASS_SUPER()`:

1. Check the receiver's singleton class (if it exists)
2. Check `prepended` modules (inserted before the class in the chain)
3. Check the class itself
4. Check `included` modules (inserted as ICLASS nodes in the super chain)
5. Walk up to superclass, repeat
6. Reach `BasicObject`, then fail

Module inclusion creates **ICLASS** (internal class) nodes in the ancestor chain. The C-level lookup is a simple loop:

```c
for (; klass; klass = RCLASS_SUPER(klass)) {
    me = lookup_method_table(klass, id);
    if (me) return me;
}
```

Prepending a module inserts it between the origin marker and the class, so prepended methods take priority over the class's own methods.

### How does Rust handle static dispatch vs dynamic dispatch (impl Trait vs dyn Trait)?

**`impl Trait` (static):** The compiler monomorphizes -- generates specialized code for each concrete type. `fn foo(x: impl Display)` becomes a separate function for each type passed. Zero overhead, full inlining, but larger binary.

**`dyn Trait` (dynamic):** Uses a fat pointer `(data_ptr, vtable_ptr)`. Method calls go through the vtable. Can't inline, but a single function handles all types. Useful for heterogeneous collections and reducing compile times.

**Return position `impl Trait`:** Returns an opaque type known to the compiler but hidden from the caller. Still statically dispatched.

### How does Ruby optimize method dispatch? What are inline method caches and how do they get invalidated?

Ruby uses a **three-level inline cache** hierarchy:

1. **Per-instruction call cache:** Each `send` instruction has an `rb_callcache` storing the last-seen receiver class, method entry, and a direct function pointer. Cache hit = direct call with no lookup.

2. **Per-class cache table:** `RCLASS_WRITABLE_CC_TBL` maps method IDs to `rb_class_cc_entries` (groups of call caches sharing a method entry). Different call sites for the same method on the same class share the underlying method entry.

3. **Method table lookup:** Full ancestor chain traversal as the slow path.

**Invalidation triggers:**
- Method defined/redefined/removed
- Module included/prepended
- Method visibility changed
- Refinement activated

**Invalidation mechanism:**
- For **leaf classes** (no subclasses): invalidate only local caches
- For **internal classes** (has subclasses): invalidate the callable method entry globally, cascading to all subclasses
- CME (callable method entry) is marked `METHOD_ENTRY_INVALIDATED`
- YJIT is notified to invalidate compiled code blocks depending on the method

**Negative caching:** Methods that don't exist get "negative CME" entries to avoid repeated lookup failures.

### How do both languages handle method_missing / fallback dispatch?

**Rust:** No equivalent. If a method doesn't exist, it's a compile error. Closest analogues are `Deref` (which the compiler auto-follows) and trait blanket impls.

**Ruby:** When method lookup exhausts the ancestor chain, the VM calls `method_missing` on the receiver:

1. Original arguments are shifted: `obj.foo(a, b)` becomes `obj.method_missing(:foo, a, b)`
2. The reason is tracked in `ec->method_missing_reason`: `MISSING_NOENTRY` (doesn't exist), `MISSING_PRIVATE` (visibility), `MISSING_PROTECTED`
3. Default `method_missing` (from `BasicObject`) raises `NoMethodError`
4. Classes can override `method_missing` for dynamic dispatch (common in DSLs, ORMs, proxies)

---

## 8. Pattern Matching and Exhaustiveness

### How does Rust compile match expressions? What is the decision tree / matrix algorithm for pattern compilation?

Rust compiles `match` using a **pattern matrix** algorithm (based on Maranget's work):

1. Build a matrix where rows are match arms and columns are constructor positions
2. Select a column to split on (heuristic: pick the one that discriminates most)
3. Split the matrix by constructor (e.g., `Some` vs `None` for Option)
4. Recursively compile each sub-matrix
5. Generate a decision tree of tests and branches

The result in MIR is a tree of `SwitchInt` terminators (for integer/enum discriminants) and conditional branches (for guard clauses). Bindings become local variable assignments at the appropriate tree nodes.

### How does Ruby 3's pattern matching (in expressions) work internally? How does YARV compile case/in?

Ruby's pattern matching (`case/in`) is compiled in `compile_case3()` in `compile.c`. Each `in` clause generates a sequence of bytecode tests:

**Array patterns** (`in [a, b, c]`):
1. `dup` the value
2. Check `respond_to?(:deconstruct)` -- if not, fail
3. Call `deconstruct` to get an array
4. Check type is `T_ARRAY` and length matches
5. Extract each element and recursively match sub-patterns
6. Cache the `deconstruct` result to avoid re-calling for subsequent patterns

**Hash patterns** (`in {x:, y:}`):
1. Call `deconstruct_keys([:x, :y])` to get a hash
2. Check for required keys
3. Extract values and recursively match

**Constant patterns** (`in Foo`):
1. Use the `===` operator for matching (`checkmatch VM_CHECKMATCH_TYPE_CASE`)

**Or patterns** (`in a | b`):
1. Try first pattern, on failure try second (with label-based branching)

**Pin patterns** (`in ^x`):
1. Evaluate pinned expression and compare with `===`

### How does Rust check exhaustiveness? What algorithm determines if patterns are complete?

Rust uses the **usefulness** algorithm (also from Maranget). A pattern is useful if there exists a value matched by it that isn't matched by earlier patterns. Exhaustiveness is checked by testing if a wildcard pattern `_` at the end would be useful -- if yes, the match is non-exhaustive.

The algorithm works on a pattern matrix, decomposing by constructors and recursing. For enums, it checks all variants. For integers, it checks all values (or proves coverage via ranges). For nested patterns, it recurses into subpatterns.

### How does Rust handle or-patterns, guard clauses, and binding modes in compiled matches?

**Or-patterns** (`A | B`): The decision tree tests both alternatives, merging into the same arm body. Bindings must be identical in both alternatives.

**Guard clauses** (`if condition`): After matching the structural pattern, the guard is evaluated. If it fails, matching continues with the next arm (not the same arm with different bindings). This is why guards can make otherwise-exhaustive matches non-exhaustive.

**Binding modes:** `match &x { Some(y) => ... }` automatically makes `y` a reference. The compiler infers whether bindings should be by-value, by-ref, or by-mut-ref based on the match ergonomics rules.

---

## 9. Error Handling Models

### How does Rust implement Result/Option and the ? operator at the MIR level? What does desugared error propagation look like?

`Result` and `Option` are normal enums. The `?` operator desugars to:

```rust
let val = match expr {
    Ok(v) => v,
    Err(e) => return Err(From::from(e)),
};
```

In MIR, this becomes:
1. Evaluate the expression (produces a `Result`)
2. `SwitchInt` on the discriminant (0 = Ok, 1 = Err)
3. Ok branch: extract the value, continue
4. Err branch: call `From::from()` on the error, return it

The `From::from()` conversion enables error type conversion -- `?` works across different error types if a `From` impl exists.

### How does Ruby implement exceptions? How do raise/rescue work at the YARV level (catch tables, stack unwinding)?

Ruby exceptions use **catch tables** and **throw data** objects:

**Raising an exception:**
1. `raise` creates a `vm_throw_data` imemo object containing the exception object, target frame, and throw state
2. Sets `ec->errinfo` to the throw data
3. VM unwinds the stack, checking each frame's catch table

**Catch table structure:**
```c
struct iseq_catch_table_entry {
    enum rb_catch_type type;  // RESCUE, ENSURE, BREAK, NEXT, REDO, RETRY
    unsigned int start;       // PC range start
    unsigned int end;         // PC range end
    unsigned int cont;        // continuation PC (handler entry)
    unsigned int sp;          // stack pointer at handler entry
};
```

**Rescue execution:**
1. VM searches catch table for a `CATCH_TYPE_RESCUE` entry covering the current PC
2. If found, resets SP and PC to the handler entry
3. Handler bytecode checks if the exception matches the rescue clause (using `===`)
4. If no match, re-raises

**Ensure execution:**
- `CATCH_TYPE_ENSURE` entries always execute their handler
- After ensure block completes, the exception continues propagating (or normal flow continues)

**Non-local exits (break/next/return in blocks):**
- Use the `throw` instruction with appropriate `throw_state`
- Caught by `CATCH_TYPE_BREAK`, `CATCH_TYPE_NEXT`, `CATCH_TYPE_REDO` entries
- These are distinct from exception handling -- they're control flow, not errors

### What are the performance characteristics of Rust's Result vs Ruby's exceptions?

**Rust's Result:** Zero-cost on the happy path. `Result<T, E>` is a normal enum -- no allocation, no unwinding. The `?` operator compiles to a branch and return. Error propagation is just returning a value up the call stack. Cost is one branch per `?`.

**Ruby's exceptions:** Fast when no exception occurs -- the catch table is only consulted during unwinding. Raising is expensive: creates an exception object, captures a backtrace (walking the entire call stack), then unwinds frames one by one. Ruby exceptions should not be used for expected control flow.

### How does Rust handle panic unwinding vs abort? What is the unwinding mechanism?

**Unwinding (default):** `panic!` triggers stack unwinding using the platform's exception mechanism (DWARF unwinding on Unix, SEH on Windows). Each stack frame's destructors (drops) run during unwinding. `catch_unwind` can catch panics.

**Abort:** With `panic = "abort"` in Cargo.toml, panics immediately terminate the process. No unwinding, no destructors. Smaller binaries, faster panics, but no recovery.

In MIR, each function has an unwind path from every statement that can panic. Drop elaboration inserts cleanup blocks along these paths.

---

## 10. Module and Import Systems

### How does Rust's module system work? How do mod, use, pub, and crate visibility interact?

**`mod`** declares a module. `mod foo;` loads from `foo.rs` or `foo/mod.rs`. `mod foo { ... }` defines inline.

**`use`** brings names into scope. `use std::collections::HashMap;` makes `HashMap` available. `use crate::foo::*;` is a glob import.

**Visibility:**
- Default: private to the containing module
- `pub`: visible to all
- `pub(crate)`: visible within the crate
- `pub(super)`: visible to parent module
- `pub(in path)`: visible within a specific path

**`crate`** is the root module of a compilation unit. Each crate is compiled independently. Cross-crate dependencies use serialized metadata (not source parsing).

Name resolution happens early, before type checking. The resolver maps paths to definitions, handling imports, globs, and re-exports.

### How does Ruby's require/load system work? What is the autoload mechanism and how does Zeitwerk improve it?

**`require`** loads a file once:
1. Check `$LOADED_FEATURES` (indexed hash) -- skip if already loaded
2. Search `$LOAD_PATH` directories for matching file (`.rb` tried first, then `.so`/`.bundle`)
3. Execute the file's code in top-level context
4. Add to `$LOADED_FEATURES`

**`load`** is like `require` but always re-executes and can optionally wrap in an anonymous module.

**`autoload`** registers a constant -> file mapping:
- `autoload :Foo, "foo"` -- when `Foo` is first referenced, `require "foo"` is triggered
- Lazy loading: files loaded on demand
- Thread-safe: concurrent access is serialized

**Zeitwerk** improves autoloading by:
- Deriving constant names from file paths (convention over configuration)
- Using `Module#autoload` for each file in the load paths
- Supporting eager loading (for production) and lazy loading (for development)
- Handling reloading in development by removing constants and re-registering autoloads

### How does Rust handle name resolution across crates? What is the role of the resolver?

The resolver runs as an early compiler pass on the AST/HIR. It:
1. Processes `use` declarations and builds an import graph
2. Resolves paths through modules, handling `pub use` re-exports
3. Handles glob imports by lazily expanding them
4. Detects ambiguities (two globs exporting the same name) and reports errors
5. Produces a mapping from every name usage to its definition

Cross-crate resolution uses **crate metadata** -- serialized information about a crate's public API. The resolver looks up external crate paths in this metadata without parsing their source.

### How does Ruby handle constant lookup and nesting? What are the surprising edge cases in module nesting?

Ruby's constant lookup follows two paths:

**Lexical scope (primary):**
1. Get the **CREF chain** from the current execution context (lexically enclosing class/module scopes)
2. Walk the chain from innermost to outermost scope
3. At each scope, check the class's `const_tbl`

**Inheritance (fallback):**
4. If not found lexically, search the class hierarchy (superclasses and included modules)
5. Check `Object` (if not already in the chain)
6. Call `const_missing` as a last resort

**Surprising edge cases:**

- **Nesting matters, not receiver:** `module A; module B; FOO; end; end` searches `A::B`, then `A`, then `Object`. But `module A::B; FOO; end` only searches `A::B` then `Object` -- `A` is NOT in the CREF chain because it wasn't lexically opened.

- **`class_eval`/`module_eval` don't create nesting:** `A.class_eval { FOO }` searches the caller's lexical scope, not `A`'s.

- **Refinements affect constant lookup** when active in the current scope.

The bytecode instruction `opt_getconstant_path` optimizes constant chain lookups (`Foo::Bar::Baz`) with inline caching. The cache stores the full resolution path and is invalidated when constants change.

---

## 11. Closure and Lambda Representation

### How does Rust represent closures? What are the Fn/FnMut/FnOnce traits and how do they map to capture semantics?

Rust closures are anonymous structs with one field per captured variable:

- **`Fn`** -- captures by shared reference (`&T`). Can be called multiple times concurrently. The closure struct has `&T` fields.
- **`FnMut`** -- captures by mutable reference (`&mut T`). Can be called multiple times but not concurrently. Fields are `&mut T`.
- **`FnOnce`** -- captures by value (move). Can only be called once (consumes captured values). Fields are `T`.

The compiler automatically determines the minimum trait based on usage:
- If the closure only reads captures -> `Fn`
- If it mutates captures -> `FnMut`
- If it moves out of captures -> `FnOnce`

`move` keyword forces all captures by value (cloning if needed for non-Copy types).

### How does Ruby represent blocks, procs, and lambdas internally? What is the difference in YARV's handling?

At the bytecode level, all three compile to **child ISEQs** of type `ISEQ_TYPE_BLOCK`. The differences are runtime semantics:

**Blocks:**
- Not first-class objects (no Proc allocated unless captured with `&block`)
- Passed as a "block handler" in the VM frame
- `getblockparamproxy` provides a lightweight proxy object
- `getblockparam` lazily converts to a Proc only when needed
- `invokeblock` yields to the block
- `return` exits the enclosing method

**Procs (`Proc.new`, `proc {}`):**
- Heap-allocated Proc object wrapping a block ISEQ
- `return` exits the enclosing method (can raise `LocalJumpError` if method already returned)
- No arity checking

**Lambdas (`->{}`, `lambda {}`):**
- Proc object with a `is_lambda` flag
- `return` exits only the lambda
- Strict arity checking
- Semantically closer to anonymous methods

The ISEQ is the same; the behavioral differences are checked at runtime by the VM's `throw` handling and arity validation code.

### How does Rust decide between stack allocation and heap allocation for closure environments?

By default, closure environments are **stack-allocated**. The anonymous struct lives on the stack of the creating function.

Heap allocation happens when:
- The closure is coerced to `Box<dyn Fn()>` or similar trait object -- boxed on the heap
- The closure is returned from a function -- `impl Fn()` return types are still stack-allocated (the caller reserves space), but `dyn Fn()` requires boxing
- The closure is moved to another thread via `Arc<dyn Fn()>` or `Box<dyn Fn() + Send>`

The compiler never silently heap-allocates. You always explicitly `Box::new(|| ...)` or use APIs that do it for you.

### How does Ruby handle binding objects and their relationship to closures?

A `Binding` object captures the execution context of a scope -- local variables, `self`, and the block. It's created via `Kernel#binding` and used for dynamic code evaluation:

```ruby
x = 42
b = binding
b.local_variable_get(:x)  # => 42
```

Bindings are closely related to closures because:
- A block's closure environment is essentially its binding
- `Proc#binding` returns the binding captured at proc creation
- Bindings are first-class objects that can be passed around and used later

Internally, a binding references the VM's environment pointer (EP) and the ISEQ's local variable table. The EP points to the stack frame (or heap-allocated environment for escaped closures). When a proc outlives its creating method, the environment is copied to the heap.

---

## 12. Code Generation and Optimization

### How does Rust use LLVM for code generation? What optimizations does MIR enable before handing off to LLVM?

Rust translates MIR to LLVM IR, then LLVM runs its optimization pipeline (inlining, loop unrolling, vectorization, dead code elimination, constant folding, etc.).

**MIR-level optimizations** (before LLVM):
- **Const propagation/evaluation** -- evaluate const expressions at compile time
- **Copy propagation** -- eliminate redundant copies
- **Dead code elimination** -- remove unreachable blocks
- **Inlining** -- inline small functions at the MIR level (cheaper than LLVM inlining)
- **Simplification** -- peephole optimizations on MIR instructions
- **Generator/async transform** -- convert generators to state machines

These MIR optimizations reduce the work LLVM has to do and catch Rust-specific patterns LLVM wouldn't recognize.

### How does YARV's JIT (YJIT/MJIT/RJIT) work? What heuristics trigger compilation and what gets compiled?

**YJIT** (current production JIT, written in Rust) uses **lazy basic block versioning**:

**Compilation trigger:**
- Each method has a `jit_entry_calls` counter incremented on every call
- When counter hits the threshold (30 for small apps, 120 for large apps with >40k ISEQs), compilation triggers
- Global cold threshold (200k calls since first compilation) prevents compiling rarely-executed code
- ISEQs that are too large (>= 65535 instructions) are rejected

**What gets compiled:**
- Individual basic blocks, not entire methods
- Each block is specialized for the observed types (context-based versioning)
- Up to 4 versions per block (configurable), with up to 1000 for inline blocks
- Blocks are compiled lazily -- branch stubs are left until the branch is actually taken

**MJIT** (method-based JIT) has been superseded by YJIT. It compiled entire methods to C, then compiled the C to machine code via a background GCC/clang process. Much higher latency.

**RJIT** was an experimental pure-Ruby JIT that also compiled to machine code but is not the primary JIT.

### What Rust-specific optimizations happen before LLVM?

Key MIR optimizations:
- **Move/copy analysis** -- determines when values can be moved vs copied, enabling LLVM to optimize better
- **Drop elaboration** -- precisely places drops, avoiding redundant destructor calls
- **Enum layout optimization** -- happens at type layout level, feeding into MIR
- **Constant evaluation (CTFE)** -- evaluates const fns and const generics at compile time, reducing runtime work
- **Monomorphization** -- generates specialized code per type, enabling LLVM to see concrete types and optimize accordingly

### How does YJIT's lazy basic block versioning work? How does it speculate on types?

**Lazy compilation:** YJIT doesn't compile an entire method upfront. It compiles the entry block, then leaves **stubs** at branch targets. When a stub is hit at runtime, `branch_stub_hit()` compiles the target block with the current type context. This means code paths that are never taken are never compiled.

**Type specialization:** Each block is compiled with a **Context** encoding:
- Types of stack values (Fixnum, Flonum, Nil, True, False, TString, CArray, etc.)
- Types of local variables
- Type of `self`
- Register mapping (which values are in CPU registers)

When the same block is reached with different type contexts, YJIT creates a **new version** specialized for those types. Up to `max_versions` (default 4) versions per block.

**Type guards:** Generated code includes type checks (guards) at entry. If a guard fails, execution falls back to the interpreter or deoptimizes to a more general version.

**Deferred compilation:** When YJIT can't determine the type at compile time, it inserts a **deferred stub**. At runtime, when the stub is hit, the actual value is inspected via `jit.peek_at_stack()`, and a specialized version is compiled. This is the key insight -- compilation decisions are made with actual runtime values, not predictions.

**Invariant tracking:** YJIT tracks assumptions:
- "Integer#+ hasn't been redefined" -- invalidated by `rb_yjit_bop_redefined()`
- "This method entry is still valid" -- invalidated by `rb_yjit_cme_invalidate()`
- "We're in single-ractor mode" -- invalidated when a new ractor is created

When an invariant is violated, all blocks depending on it are invalidated.

---

## 13. Runtime Object Layout

### How does Rust lay out structs in memory? What are the rules for field ordering, alignment, and padding?

**Default layout:** Rust is free to reorder fields to minimize padding. Fields are aligned to their natural alignment. The compiler may insert padding between fields for alignment and at the end for array alignment.

**`repr(C)`:** Fields are laid out in declaration order, following C ABI rules. Padding inserted as needed for alignment. Used for FFI.

**`repr(packed)`:** No padding between fields. May cause unaligned access (UB to take references to packed fields).

**`repr(transparent)`:** Single-field struct has the same layout as the field. Used for newtype patterns.

### How does Ruby lay out objects? What is the RVALUE structure? How does embedded vs heap-allocated string storage work?

Every Ruby heap object starts with an **RBasic** header (24 bytes on 64-bit):
```c
struct RBasic {
    VALUE flags;       // type bits, frozen, GC marks, encoding, etc.
    VALUE klass;       // class pointer
    VALUE shape_id;    // object shape (layout descriptor)
};
```

Objects are allocated in **slots** from size-segregated pools:
- Pool 0: 48 bytes (RBasic + 3 VALUE slots)
- Pool 1: 96 bytes
- Pool 2: 192 bytes
- Pool 3: 384 bytes
- Pool 4: 768 bytes

**Embedded strings:** Short strings (roughly < 24 bytes) store their content directly in the RString struct's padding after the RBasic header. No separate heap allocation needed. The `RSTRING_NOEMBED` flag distinguishes embedded from heap-allocated.

**Heap-allocated strings:** Longer strings store a pointer (`heap.ptr`), length, and capacity. Strings can also be **shared** (copy-on-write) via `heap.aux.shared`, pointing to a parent string's buffer.

**Frozen string deduplication:** Frozen string literals are globally deduplicated in a concurrent hash table (`fstring_table_obj`). All identical frozen strings share the same object.

### How does Rust handle enum layout optimization (niche filling, discriminant elision)?

**Niche filling:** When an enum variant contains a type with invalid bit patterns (a "niche"), Rust uses those bits for the discriminant. For example:
- `Option<&T>` is the same size as `&T` -- the null pointer represents `None`
- `Option<NonZeroU32>` is 4 bytes -- zero represents `None`
- `Result<T, ()>` can use a niche in `T` if available

**Discriminant elision:** For single-variant enums or enums where the tag can be inferred from the payload type, no explicit discriminant is stored.

**Layout optimization:** The compiler considers multiple layouts and picks the smallest. For `Option<Box<T>>`, this means zero overhead compared to a raw pointer.

### How does Ruby's object shapes system (since 3.2) optimize instance variable storage?

The **shapes system** creates a tree of shape objects tracking how objects' instance variable sets evolve:

```c
struct rb_shape {
    VALUE edges;                    // child shapes (ivar name -> next shape)
    ID edge_name;                   // which ivar was added
    shape_id_t parent_id;           // parent shape
    attr_index_t next_field_index;  // next slot index
    uint8_t type;                   // ROOT, IVAR, OBJ_ID, etc.
};
```

Objects with the same set of instance variables (added in the same order) share a shape. The shape maps ivar names to array indices, enabling O(1) instance variable access without hash table lookup.

**Shape ID encoding (32 bits):**
- Bits 0-18: offset into global shape array
- Bits 19-23: heap index (object pool size)
- Bit 24: frozen flag
- Bit 25: has object_id flag
- Bit 26: "too complex" flag (fallback to hash table)

When an object has too many distinct ivar sets (excessive shape transitions), it's marked "too complex" and falls back to a hash table. This prevents shape explosion from pathological code patterns.

YJIT uses shape IDs in its call caches -- if the shape matches, ivar access is a direct memory offset load.

---

## 14. Iteration and Iterator Protocols

### How does Rust's Iterator trait work? How do iterator adapters get optimized?

The `Iterator` trait requires one method: `fn next(&mut self) -> Option<Self::Item>`. Iterator adapters (`.map()`, `.filter()`, `.take()`, etc.) return new iterator structs that wrap the source, creating a chain of nested types.

**Optimization:** Because each adapter is a concrete type, the compiler monomorphizes the entire chain and inlines `next()` calls across all layers. A chain like `vec.iter().map(f).filter(g).sum()` compiles down to a single tight loop with `f` and `g` inlined. This is **zero-cost abstraction** -- the iterator chain compiles to the same code as a hand-written loop.

LLVM can further optimize: vectorize the loop, unroll it, and eliminate bounds checks.

### How does Ruby's Enumerable module work? How do internal iterators (each with blocks) differ from external iterators (Enumerator)?

**Enumerable** is a mixin module that provides ~50+ methods (map, select, reduce, etc.) built on top of a single required method: `each`. Each Enumerable method calls `each` with a block that implements its logic.

**Internal iterators (`each` with blocks):**
- The collection drives iteration by calling the block for each element
- Control flows into the block and back to the collection
- Simple, efficient (block is a stack-allocated frame)
- Cannot pause and resume -- the block runs to completion for each element

**External iterators (`Enumerator`):**
- Created by calling a method without a block: `[1,2,3].each` returns an Enumerator
- User calls `.next` to pull values on demand
- Implemented internally using a **Fiber** -- the iteration runs in a separate coroutine
- `StopIteration` exception signals exhaustion
- Supports `.peek` (lookahead) and `.feed` (two-way communication)

```c
struct enumerator {
    VALUE obj;       // object being enumerated
    ID meth;         // method to call (:each)
    VALUE fib;       // fiber for external iteration
    VALUE lookahead; // peeked value
    VALUE stop_exc;  // StopIteration exception
    VALUE procs;     // chained transformations
};
```

### How does Rust's for loop desugar into iterator calls? What does the MIR look like?

```rust
for x in collection { body }
```
desugars to:
```rust
let mut iter = IntoIterator::into_iter(collection);
loop {
    match Iterator::next(&mut iter) {
        Some(x) => { body }
        None => break,
    }
}
```

In MIR, this becomes:
1. Call `into_iter()`, store result in temporary
2. Loop header block: call `next()`, switch on discriminant
3. Some branch: extract value, execute body, goto loop header
4. None branch: break to after-loop block

### How does Ruby implement lazy enumerators? How does Enumerator::Lazy chain evaluation?

`Enumerator::Lazy` stores a chain of `proc_entry` structs:
```c
struct proc_entry {
    VALUE proc;                        // transformation proc
    VALUE memo;                        // state
    const lazyenum_funcs *fn;          // precomputed/size/precheck functions
};
```

When `.force` or `.to_a` is called, evaluation proceeds element-by-element through the chain. Each lazy method (`.lazy.map.select.take`) adds a proc to the chain without executing it. Only when a terminal operation demands values does the chain run, pulling one element at a time from the source and passing it through each transformation.

This avoids creating intermediate arrays. `(1..Float::INFINITY).lazy.map { |x| x * 2 }.select(&:even?).first(5)` processes only the minimum elements needed.

---

## 15. String Representation

### How does Rust represent strings? What is the relationship between String, &str, OsStr, and CStr?

**`String`** -- Owned, heap-allocated, growable UTF-8 byte buffer. Essentially `Vec<u8>` with a UTF-8 invariant.

**`&str`** -- Borrowed reference to a UTF-8 byte slice. Fat pointer: `(ptr, len)`. String literals are `&'static str`.

**`OsStr`/`OsString`** -- Platform-native string encoding. On Unix: arbitrary bytes. On Windows: potentially ill-formed UTF-16. Used for file paths, environment variables.

**`CStr`/`CString`** -- Null-terminated byte strings for C interop. No encoding guarantee.

**`Path`/`PathBuf`** -- Wrappers around `OsStr`/`OsString` with path manipulation methods.

The relationships: `String` derefs to `&str`. `OsString` and `CString` are distinct types that can be converted to/from `String` with potential failure (encoding validation).

### How does Ruby represent strings? What is the encoding system and how does copy-on-write work for frozen strings?

**String structure (RString):**
```c
struct RString {
    struct RBasic basic;     // flags include encoding index (bits 10-16)
    long len;
    union {
        struct { char *ptr; union { long capa; VALUE shared; } aux; } heap;
        struct { char ary[1]; } embed;  // inline for short strings
    } as;
};
```

**Encoding system:**
- Each string carries an encoding index in its flags (up to 127 inline encodings)
- Global encoding table maps indices to `rb_encoding` structs
- Supports UTF-8, ASCII, Shift_JIS, EUC-JP, ISO-8859-*, and many more
- Coderange is precomputed and cached: ASCII-only, valid encoding, or broken
- String operations check encoding compatibility and transcode as needed

**Copy-on-write:**
- When a string is duplicated (`dup`, substring), the new string shares the original's buffer
- `STR_SHARED` flag marks a string as sharing another's buffer
- `STR_SHARED_ROOT` marks the original whose buffer is being shared
- On mutation, `rb_str_make_independent()` copies the buffer (copy-on-write)

**Frozen string deduplication:**
- `# frozen_string_literal: true` makes all string literals frozen
- Frozen strings are deduplicated globally via `rb_fstring()` into a concurrent hash table
- Multiple references to the same string literal share a single object
- Reduces memory and allocation pressure significantly

### How does Rust handle UTF-8 validation and indexing? Why is string indexing by integer not directly supported?

Rust strings are guaranteed UTF-8. Indexing by byte position (`s.as_bytes()[i]`) is O(1) but gives a byte, not a character. Indexing by character requires iterating from the start because UTF-8 is variable-width (1-4 bytes per character).

`s[i]` is not supported because it would be misleading -- users expect O(1) character access, but UTF-8 indexing is O(n). Instead, Rust provides:
- `s.chars().nth(i)` -- explicit O(n) character access
- `s[start..end]` -- byte range slicing (panics if not at char boundaries)
- `s.as_bytes()[i]` -- O(1) byte access

This design prevents common bugs: in UTF-8, slicing at an arbitrary byte offset can split a multi-byte character, producing invalid text. Rust makes this impossible at the type level.

### How does Ruby handle string mutability and the frozen_string_literal pragma?

By default, Ruby strings are **mutable**. Any string can be modified in place (`<<`, `gsub!`, `[]=`, etc.).

**Freezing:**
- `str.freeze` makes a string immutable (raises `FrozenError` on mutation)
- `# frozen_string_literal: true` at the top of a file makes all string literals in that file frozen
- Frozen string literals are deduplicated (same content = same object)
- Ruby 3.4+ emits deprecation warnings for mutable string literals ("chilled strings" with `STR_CHILLED_LITERAL` flag), moving toward frozen-by-default

**Chilled strings (transition mechanism):**
- String literals without the pragma are "chilled" -- they appear frozen but emit a deprecation warning on first mutation instead of raising
- Implemented via `STR_CHILLED_LITERAL` and `STR_CHILLED_SYMBOL_TO_S` flags
- After the deprecation period, string literals will default to frozen

**Performance implications:**
- Mutable strings require COW checks on every shared-buffer mutation
- Frozen strings can be safely shared across threads and deduplicated
- The `fstring_table_obj` concurrent hash table enables global deduplication
- `STR_PRECOMPUTED_HASH` flag caches the hash value after the string terminator for frozen strings used as hash keys
