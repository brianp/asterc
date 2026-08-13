# Asterc Architecture Comparison: Rust and Ruby Reference Analysis

---

## Category 1: Intermediate Representations

**Rust approach:** Four-stage pipeline (AST -> HIR -> THIR -> MIR -> LLVM IR). Each stage has a distinct purpose: HIR preserves tree structure for type checking, THIR adds full type annotations, MIR is a CFG for borrow checking and optimization. Generics survive until codegen.

**Ruby approach:** Single-stage compilation from AST directly to YARV stack-based bytecode (~90 instructions). No intermediate analysis IR. Linear instruction layout with jump-based control flow, designed for efficient interpretation rather than static analysis.

**Asterc current state:** Single IR stage: **FIR (Functional Intermediate Representation)**. Pipeline is `Source -> Lexer -> Parser -> AST -> [Typechecker] -> Lowerer -> FIR -> Codegen (Cranelift)`. FIR has 27 expression variants, 9 statement variants, and 8 type variants. It retains structured control flow (If/While/Block), is not SSA, and is not a CFG. Type checking happens on the AST before lowering, not on FIR.

**Alignment with best practice:**
- The single-IR approach is pragmatically closer to Ruby's philosophy, which is appropriate for a language that doesn't need borrow checking or lifetime analysis
- FIR's expression catalog is well-organized with clear separation between literals, operations, control flow, data structures, tagged unions, closures, and runtime calls
- Type information is split between AST types (rich, for class/method resolution) and FIR types (lean, for codegen), which is a clean design

**Gaps and risks:**
- No CFG representation means dataflow analysis (dead code elimination, constant propagation, escape analysis) cannot be performed before Cranelift
- FIR carries too much responsibility: it serves as both the analysis target and the codegen input, with no opportunity for IR-level optimization passes
- Structured control flow in FIR (If/While/Block with nested bodies) makes it harder to reason about all execution paths compared to a flat CFG
- No SSA form means redundant computations and dead stores can't be easily detected

**Recommendations:**
- **Now:** This is fine for the current stage. The single IR keeps complexity low and Cranelift handles basic optimizations
- **Later:** If performance becomes a priority, consider a split: keep FIR for analysis/lowering, add a CFG-based "LIR" (Low IR) for optimization before Cranelift. This would be closer to Rust's MIR without the complexity of their full pipeline
- A Ruby-style approach (live with the single IR, optimize at the JIT level) is also viable

---

## Category 2: Lowering Passes

**Rust approach:** Multi-stage lowering: AST -> HIR desugars syntax (for loops, ?, async/await), HIR -> THIR resolves operator overloads and coercions, THIR -> MIR compiles patterns into decision trees and eliminates structured control flow. Each stage has clear responsibilities.

**Ruby approach:** Single-pass recursive AST-to-bytecode compilation in `compile.c` (~15,000 lines). Blocks compile to child ISEQs. Method calls become `send` instructions. Control flow uses labels resolved to offsets in a final assembly pass.

**Asterc current state:** Multi-module lowering within a single pass, organized into ~10 specialized modules totaling ~7,000+ lines: mod.rs (orchestration), expr.rs (expressions), stmt.rs (statements), match_lower.rs (patterns), closure.rs (lambda lifting), for_loop.rs (iteration), method.rs (dispatch), iterable.rs (functional vocabulary), synthesize.rs (code generation helpers), introspection.rs (reflection).

**Alignment with best practice:**
- The modular organization is excellent. Each lowering concern has its own file, making the system easier to maintain than Ruby's monolithic compile.c
- The two-pass registration (register names first, then lower bodies) correctly handles forward references
- The "pending statements" mechanism for complex expressions that need temporaries is a pragmatic solution
- Lambda lifting with explicit environment allocation is clean and follows established patterns
- Range literal optimization (direct counter loop vs runtime struct extraction) shows good attention to common cases

**Gaps and risks:**
- Match expressions compile to nested if/else chains rather than decision trees. For matches with many arms (especially enum variants), this produces O(n) sequential comparisons instead of O(log n) or O(1) dispatch
- String interpolation generates a chain of `aster_string_concat()` calls. For `"a{x}b{y}c"`, this creates 4 intermediate strings. A single format-and-allocate call would be more efficient
- Iterable vocabulary methods (map, filter, reduce) are inlined as loop scaffolds. This is good for avoiding call overhead but means the patterns can't be optimized across method chains

**Recommendations:**
- **Now:** Compile match expressions on enum types to jump tables (switch on tag value) instead of sequential if/else. This is a significant performance win for common patterns
- **Now:** Add a `aster_string_concat_n(parts, count)` runtime function for multi-part string interpolation
- **Later:** Consider building a small optimization pass between lowering and codegen that can fuse adjacent iterable operations

---

## Category 3: Type Systems and Inference

**Rust approach:** Constraint-based inference (modified Hindley-Milner) with trait obligations, union-find for eager unification, and bidirectional type flow. The `InferCtxt` manages type variables, region constraints, and speculative inference with snapshot/rollback.

**Ruby approach:** No static type checking. Duck typing at runtime. Optional gradual typing via Sorbet (inline signatures) or RBS (external declarations), both with flow-sensitive local inference.

**Asterc current state:** Bidirectional unification-based type inference. `TypeEnv` uses immutable Rc-based scope stacking. Unification algorithm (`unify_inner`) supports: symmetric TypeVar binding, occurs check, invariant containers (List[Dog] != List[Animal]), covariant subtyping for bare custom types, constraint checking (extends/includes). Multi-pass type checking: (1) register classes/traits/enums, (2) infer return types via fixed-point iteration, (3) check function bodies.

**Alignment with best practice:**
- Invariant containers are the correct default for a language with mutable collections, matching Rust's approach
- The occurs check prevents infinite types, which is essential for soundness
- Bidirectional inference with expected type propagation enables good ergonomics (lambda parameter inference, `.into()` disambiguation)
- The `Type::Error` sentinel for error recovery follows Rust's approach of preventing cascading errors
- Nil-list promotion (`List[Nil]` -> `List[T]` on first push) is a pragmatic feature that reduces annotation burden

**Gaps and risks:**
- No snapshot/rollback for speculative inference. If unification partially succeeds then fails, bindings may be corrupted. Rust's `InferCtxt` uses union-find tables with snapshots for this
- No obligation/trait fulfillment forest. Trait constraints are checked post-unification rather than interleaved, which may miss some constraint propagation opportunities
- Type inference for recursive functions uses fixed-point iteration but may not converge for complex mutual recursion
- No variance annotations on user-defined generic types (everything is invariant). This is safe but restrictive

**Recommendations:**
- **Now:** The current system is sound and practical for the language's scope
- **Later:** If generics become more complex, consider adding snapshot/rollback to the unification engine to handle speculative inference correctly
- **Later:** Consider covariant/contravariant annotations for specific generic positions (e.g., read-only containers could be covariant)

---

## Category 4: Generics and Monomorphization vs Erasure

**Rust approach:** Full monomorphization at codegen time. Each `(function, type-args)` pair gets its own machine code. MIR optimizations run once per generic function. `dyn Trait` provides type-erased vtable dispatch as an alternative.

**Ruby approach:** No compile-time generics. Everything is dynamic dispatch through inline caches and call caches. YJIT specializes code paths based on observed runtime types.

**Asterc current state:** **Type erasure with runtime bitcasts.** All TypeVars erase to `I64` (64-bit word) in FIR. Float and Bool values are bitcast to/from I64 when crossing generic boundaries. Pointer types (String, List, Class) are already I64-width, so no conversion needed. One compiled copy of each generic function exists.

**Alignment with best practice:**
- Type erasure is a legitimate strategy used by Java, Go (pre-generics), and many dynamic languages. It avoids code bloat
- For a GC'd language where all heap objects are already pointer-width, the approach is natural
- The decision to erase to I64 (rather than using void pointers) keeps everything in registers

**Gaps and risks:**
- **Float precision loss:** Bitcasting f64 to i64 and back is safe for bit patterns but prevents Cranelift from optimizing float operations inside generic functions (it sees i64, not f64)
- **No specialization opportunity:** A generic `sum()` function over integers can't use integer-specific instructions because the compiled code works on i64 regardless of actual type
- **Bool values waste 7 bytes:** A bool (1 bit of information) occupies a full 64-bit word in generic contexts
- **No tagged representation:** Unlike Ruby's tagged integers or OCaml's tagged pointers, asterc doesn't distinguish between pointer and immediate values at the runtime level (relies on GC magic byte validation instead)

**Recommendations:**
- **Now:** The erasure approach is appropriate for the current language maturity. It keeps compilation fast and binary size small
- **Later:** Consider selective monomorphization for hot generic functions (e.g., numeric operations) as a JIT optimization
- **Later:** A tagged pointer scheme (low bits indicating type) could eliminate the need for magic-byte validation in the GC and improve generic dispatch

---

## Category 5: Memory Management

**Rust approach:** Zero-cost ownership model. No runtime GC. Compile-time lifetime enforcement via borrow checker on MIR's CFG. Drop elaboration inserts precise destructor calls. Moves tracked via `MoveData` dataflow analysis.

**Ruby approach:** Generational, incremental, compacting mark-sweep GC. Five size-segregated object pools (48-768 bytes). Tri-color marking with write barriers for generational and incremental correctness. Lazy sweeping. Object shapes for cache-friendly ivar access.

**Asterc current state:** Non-moving mark-and-sweep with shadow stack. 24-byte object header (mark, type, magic bytes, size, next pointer). Conservative pointer validation via magic bytes + heap address range. Iterative worklist marking (not recursive). Adaptive threshold (max of survived*2, 256KB). Per-thread GC state. Typed allocation (`aster_class_alloc_typed`) with pointer fields sorted to front for precise tracing.

**Alignment with best practice:**
- The iterative worklist for marking avoids stack overflow on deep object graphs, which is a real concern Ruby had to solve too
- Typed allocation with precise pointer counts (pointer fields sorted to front) is a good optimization that reduces false retention
- The shadow stack approach for root management is well-established (used by many GC implementations)
- Per-thread state is correct for the green thread model
- Adaptive threshold scaling (survived * 2) follows Ruby's approach of growing proportionally to the live set

**Gaps and risks:**
- **No generational collection:** Every GC cycle scans the entire heap. As programs grow, pause times will increase linearly with heap size. Ruby solved this with generational collection (minor GCs only scan young objects)
- **No incremental marking:** The GC is stop-the-world. For a language with green threads, this means all threads pause during collection
- **No compaction:** Memory fragmentation will accumulate over time. Ruby added compaction in 3.0 specifically to address this
- **24-byte header overhead:** Every object pays 24 bytes. Ruby's RBasic is also 24 bytes but serves more purposes (shape ID, flags, class pointer). Asterc's header wastes 4 bytes on magic (which could be a single u32) and has 4 bytes of explicit padding
- **Conservative pointer validation via magic bytes is probabilistic:** While 4 magic bytes give 1-in-2^32 false positive rate, it's not zero. A tagged pointer scheme would be deterministic
- **Singly-linked heap list for sweeping:** Iterating the entire heap for sweeping is O(n) in total objects, not just dead ones

**Recommendations:**
- **Now:** Reduce header to 16 bytes: combine mark+type+ptr_count into a single u32 flags word, use u32 for size, and eliminate explicit padding. This saves 8 bytes per object
- **Soon:** Add generational collection with a write barrier. Young objects (recently allocated) should be collected more frequently. This is the single biggest GC improvement possible
- **Later:** Add incremental marking so GC pauses don't grow with heap size
- **Later:** Consider a free-list or segregated-fit allocator instead of the singly-linked heap list for better sweep performance

---

## Category 6: Async and Concurrency

**Rust approach:** Async/await desugars into zero-cost state machines implementing `Future::poll`. No runtime scheduler required. Pin ensures self-referential state machines don't move. Runtimes (Tokio, async-std) provide work-stealing schedulers as libraries.

**Ruby approach:** Fibers (cooperative coroutines with explicit context switching), Ractors (isolated parallel execution with message passing), and a Fiber Scheduler interface for hooking into blocking operations. GVL prevents true parallelism within a ractor.

**Asterc current state:** **Green threads with work-stealing scheduler.** M:N threading model with 64KB stacks from a pool. Assembly-based context switching (aarch64/x86_64). Preemption via tick counter (yield after 1024 ticks at safepoints). Work stealing: local FIFO -> global injector -> victim queues. Blocking pool (4 threads) for I/O operations. Per-thread GC shadow stack saved/restored on context switch. Channel-based and mutex-based synchronization primitives.

**Alignment with best practice:**
- The work-stealing scheduler is the gold standard for green thread runtimes (used by Go, Tokio, Erlang BEAM)
- Tick-based preemption at safepoints ensures cooperative multitasking without OS timer interrupts
- Saving/restoring GC shadow stack on context switch is essential for correctness
- The blocking pool for I/O prevents green thread starvation
- Channel and mutex primitives provide safe inter-task communication

**Gaps and risks:**
- **Fixed 64KB stacks:** Goroutines start at 8KB and grow dynamically. 64KB is generous but wastes memory for simple tasks and may be insufficient for deeply recursive ones
- **No structured concurrency:** Tasks can outlive their parent scope. This makes reasoning about resource cleanup harder. Rust's async model naturally scopes futures to their owning async block
- **Error propagation across task boundaries:** The per-thread error flag model doesn't naturally compose with task spawning. If a spawned task throws, the parent must explicitly check
- **GC interaction:** Stop-the-world GC requires all green threads to reach a safepoint before collection can proceed. If any thread is blocked in native code (blocking pool), this could cause delays

**Recommendations:**
- **Now:** Consider growable stacks (start at 8-16KB, grow on demand) to reduce memory overhead for many small tasks
- **Soon:** Add structured concurrency (task groups / nurseries) so that child tasks are automatically cancelled when the parent scope exits
- **Later:** Implement GC-safe safepoint coordination so the GC can proceed even when some threads are in native code (similar to Go's preemptive scheduling via signal-based interruption)

---

## Category 7: Method Resolution and Dispatch

**Rust approach:** Compile-time resolution through autoderef chains. Inherent methods shadow trait methods. Static dispatch via monomorphization, dynamic dispatch via vtable for `dyn Trait`. No method_missing; `Deref` provides fallback.

**Ruby approach:** Runtime method lookup traversing the ancestor chain (singleton class -> prepended modules -> class -> included modules -> superclass). Three-level inline cache hierarchy. `method_missing` as fallback. YJIT generates type-specialized code paths.

**Asterc current state:** **Compile-time static resolution only.** Method calls are resolved by walking the class hierarchy (single inheritance chain) at compile time. Methods are qualified as `{ClassName}.{MethodName}` and looked up in a global function registry. Built-in methods (String, List, numeric types) are hardcoded in the lowerer. No vtables at runtime. No dynamic dispatch.

**Alignment with best practice:**
- Static resolution eliminates all dispatch overhead, which is ideal for predictable performance
- The parent chain walk at compile time is correct for single inheritance
- Hardcoded built-in methods allow direct runtime calls without dispatch overhead
- Qualified method names (Class.method) provide a clean namespace

**Gaps and risks:**
- **No virtual dispatch:** If a variable is typed as `Animal` but holds a `Dog`, calling `speak()` will dispatch to `Animal.speak()`, not `Dog.speak()`. This breaks polymorphism. Both Rust (via `dyn Trait`) and Ruby (via runtime lookup) support this
- **No trait method dispatch for trait-typed values:** If a function takes `T includes Printable`, the method resolution must know the concrete type at compile time
- **Built-in methods are not extensible:** Users can't add methods to String or List (no extension methods or monkey-patching). This is intentional but worth noting
- **No method shadowing detection:** If a subclass defines a method that exists on a parent, the compile-time walk finds the subclass method first (correct), but there's no override validation

**Recommendations:**
- **Now:** Add vtable-based dynamic dispatch for class methods when the receiver type is a base class. This is essential for polymorphism. At minimum, class instances should carry a vtable pointer (or class pointer) that enables runtime dispatch
- **Now:** The FirClass already has a `vtable: Vec<(String, FunctionId)>` field, suggesting vtable dispatch is planned. Implement it
- **Later:** Consider sealed class hierarchies (like Rust's enum) where the compiler can optimize dispatch to a switch on the class tag

---

## Category 8: Pattern Matching and Exhaustiveness

**Rust approach:** Decision tree compilation from a pattern matrix (Maranget's algorithm). Exhaustiveness checking via the usefulness algorithm. Or-patterns, guards, and binding modes all supported. Guards are treated conservatively in exhaustiveness analysis.

**Ruby approach:** `case/in` structural pattern matching compiled to bytecode test sequences. Array/hash patterns call `deconstruct`/`deconstruct_keys`. Hash-table dispatch for `case/when`. No compile-time exhaustiveness checking.

**Asterc current state:** Pattern matching supports Literal, Ident, Wildcard, and EnumVariant patterns. Exhaustiveness is enforced at compile time: bools need both arms or wildcard, enums need all variants or wildcard, nullable types need explicit nil arm or wildcard. Patterns compile to **nested if/else chains** with sequential comparison.

**Alignment with best practice:**
- Exhaustiveness checking at compile time is a major correctness win over Ruby
- Nullable type handling is sophisticated: when a nil arm exists, subsequent ident patterns bind to the unwrapped type
- Bool exhaustiveness (requiring both true and false) matches Rust's approach
- Enum exhaustiveness (all variants or wildcard) matches Rust's approach

**Gaps and risks:**
- **No decision trees:** Sequential if/else chains are O(n) in the number of arms. For a 20-variant enum, this means 20 comparisons instead of a single jump table
- **No nested patterns:** Can't match `Some(Pair(x, y))` or destructure into fields. Only top-level pattern kinds are supported
- **No or-patterns:** Can't write `Red | Blue => "primary"`. Each alternative needs its own arm
- **No guard clauses:** Can't write `n if n > 0 => "positive"`. Guards must be in the arm body
- **No binding in enum variants:** `EnumVariant { enum_name, variant, span }` has no field bindings. Can't extract fields during pattern matching

**Recommendations:**
- **Now:** Compile enum matches to jump tables (switch on tag). This is a straightforward codegen change
- **Soon:** Add field binding in enum variant patterns: `Some(value) => use(value)`. This is the most impactful pattern matching feature missing
- **Soon:** Add or-patterns for combining arms
- **Later:** Add nested patterns and guard clauses
- **Later:** Consider implementing Maranget's algorithm for optimal pattern compilation when patterns become more complex

---

## Category 9: Error Handling Models

**Rust approach:** `Result<T, E>` / `Option<T>` with `?` operator desugaring to match + early return. Zero-cost on the happy path (just a branch). Panic unwinding for unrecoverable errors via platform-specific stack unwinding.

**Ruby approach:** Exception model with raise/rescue. Catch tables map PC ranges to handlers. Stack unwinding on raise. Expensive: creates exception object, captures backtrace. `throw` instruction handles non-local exits (break/next/return in blocks).

**Asterc current state:** Hybrid model: **tagged unions for nullable/result types + per-thread error flag for thrown errors.** Nullable types (`T?`) use `TagWrap`/`TagUnwrap`/`TagCheck` (tag=0 for Some, tag=1 for None). Thrown errors set a per-thread `ERROR_FLAG`/`ERROR_TYPE_TAG`/`ERROR_VALUE`. Propagation (`!` operator), recovery (`.or()`, `.or_else()`, `.catch {}`), and throw are all first-class syntax. Every thrown error also captures a stack trace once at `throw` via a native frame-pointer walk, surfaced as `error.trace() -> List[Frame]`.

**Alignment with best practice:**
- Tagged unions for nullable types are the right representation (matches Rust's Option layout)
- The error propagation operator (`!`) is analogous to Rust's `?` and provides clean syntax
- Method-chaining recovery (`.or()`, `.catch {}`) is ergonomic and avoids deeply nested try/catch
- Subtype matching in catch arms (via class hierarchy) provides structured error handling

**Gaps and risks:**
- **Per-thread error flag is a hidden side channel.** Unlike Rust's Result which is a value that must be handled, the error flag can be silently ignored. If a function sets the error flag and the caller doesn't check, the error propagates invisibly
- **Single error slot:** Only one error can be active at a time. If error handling code itself throws, the original error is lost
- **Error flag checked manually:** After each throwing call, the lowerer emits `aster_error_check()`. If this is ever missed (bug in the lowerer), errors silently corrupt execution
- **Tagged union boxing for value types:** `Int?` requires heap allocation of an 8-byte box to distinguish null from zero. Rust avoids this with niche optimization (Option<NonZeroI64> uses 0 for None)

**Recommendations:**
- **Now:** Consider making the error flag check automatic at the codegen level rather than relying on the lowerer to emit it. A post-lowering validation pass could verify all throwing calls are followed by error checks
- **Later:** Consider niche optimization for specific types (e.g., `String?` could use null pointer for None, which it already does for Ptr types)

---

## Category 10: Module and Import Systems

**Rust approach:** `mod` declarations create a module tree. `use` paths bring names into scope. Iterative import resolution with fixed-point loop for glob imports. Three namespaces (Type, Value, Macro). Cross-crate resolution via serialized metadata.

**Ruby approach:** `require`/`load` with autoloading. `$LOADED_FEATURES` deduplication. Zeitwerk derives constants from file paths. Constant lookup follows CREF chain (lexical scope) then inheritance. Reopenable modules/classes.

**Asterc current state:** `use path/to/module { Name }` syntax with selective, namespace (`as alias`), and wildcard import. FileResolver abstraction (FsResolver for production, VirtualResolver for tests). Module cache with circular import detection via in-progress set. Re-exports via `pub use`. Unstable module gating with `--unstable` flag.

**Alignment with best practice:**
- The FileResolver abstraction enabling test mocking is excellent engineering
- Circular import detection via in-progress set is simple and correct
- Cache-based module loading (compile once, reuse exports) is standard practice
- Selective imports (`use foo { Bar, Baz }`) are ergonomic
- Namespace imports (`use foo as ns`) avoid name collisions
- Unstable module gating follows Rust's feature flag philosophy

**Gaps and risks:**
- **No incremental compilation:** Changing one module recompiles everything that imports it (and everything that imports those, etc.). Rust solves this with query-based incremental compilation
- **No diamond dependency handling:** If module A imports B and C, and both B and C import D, D is compiled once (cached) but its exports are merged into both B and C's scopes independently. Name conflicts from different paths to the same module aren't detected
- **No visibility granularity:** Items are either public (exported) or private (not exported). No equivalent of Rust's `pub(crate)` or `pub(super)`
- **Filesystem-coupled resolution:** Module paths map directly to file paths. No support for virtual modules, conditional compilation, or platform-specific module selection

**Recommendations:**
- **Now:** The current module system is solid for the language's scope
- **Later:** Add `pub(module)` or similar for internal visibility within a package
- **Later:** Consider incremental compilation if compilation times become an issue

---

## Category 11: Closure and Lambda Representation

**Rust approach:** Closures are anonymous structs implementing Fn/FnMut/FnOnce. Capture analysis determines minimal capture mode (shared ref, mutable ref, or by-value). Precise field-level capture since Rust 2021. Always stack-allocated unless explicitly boxed.

**Ruby approach:** Blocks, procs, and lambdas all compile to child ISEQs. Blocks are lightweight (no Proc object unless captured). Procs/lambdas differ in return/arity semantics. Binding objects capture the full execution context. Environments heap-allocated when they escape.

**Asterc current state:** Lambda lifting with explicit environment allocation. Capture analysis finds free variables referenced in lambda body. Lifted functions get `__env: Ptr` as first parameter. Environment is a flat heap-allocated byte array with captures at 8-byte offsets. Two call paths: static (direct Call with env) for bound lambda variables, dynamic (ClosureCall via call_indirect) for function-typed parameters.

**Alignment with best practice:**
- Lambda lifting is a well-established technique
- The static vs dynamic call distinction is a good optimization (avoids indirect calls when the closure is known)
- Flat environment layout (no nesting) is simpler than Ruby's environment chain and has better cache locality
- 16-byte closure objects (func_ptr + env_ptr) are compact

**Gaps and risks:**
- **All captures are by value (copy):** No by-reference capture. If a closure modifies a captured variable, the modification is local to the closure, not visible in the enclosing scope. This may surprise users coming from Ruby/JavaScript
- **No capture mode analysis:** Unlike Rust's Fn/FnMut/FnOnce distinction, there's no tracking of how captures are used. Every capture is copied
- **Environment always heap-allocated:** Even closures that never escape their creating scope get heap-allocated environments. Rust stack-allocates by default
- **No empty closure optimization at codegen level:** `ClosureCreate { func, env: NilLit }` still allocates a 16-byte closure object. Could be optimized to just a function pointer

**Recommendations:**
- **Now:** For closures with no captures, skip the closure object allocation and use a direct function pointer. This is a simple codegen optimization
- **Soon:** Document the by-value capture semantics clearly. Users need to know that mutating a captured variable doesn't affect the outer scope
- **Later:** Consider adding mutable capture support (by-reference capture via heap cell) for cases where closures need to share mutable state with their enclosing scope

---

## Category 12: Code Generation and Optimization

**Rust approach:** MIR optimizations (const propagation, inlining, DCE, GVN, SROA, copy propagation, jump threading) before LLVM. Then LLVM's full optimization pipeline. MIR optimizations catch Rust-specific patterns LLVM can't see.

**Ruby approach:** YJIT uses lazy basic block versioning with type specialization. Compilation triggered by call counter threshold. Up to 4 versions per block. Side exits deoptimize to interpreter. No ahead-of-time optimization.

**Asterc current state:** **No pre-Cranelift optimization passes.** FIR is translated directly to Cranelift IR. Three optimization levels mapped to Cranelift settings (None, Speed, SpeedAndSize). JIT mode uses `JITModule` (not PIC), AOT mode uses `ObjectModule` (PIC). No inlining, no constant folding, no dead code elimination at the asterc level.

**Alignment with best practice:**
- Using Cranelift is a good choice for a language that needs both JIT and AOT compilation
- The JIT/AOT split with shared compilation logic avoids code duplication
- Cranelift's Speed optimization level handles basic optimizations (register allocation, instruction selection, simple peepholes)

**Gaps and risks:**
- **No IR-level optimization:** Patterns that Cranelift can't optimize (asterc-specific idioms) pass through unoptimized. For example:
  - Tagged union operations (TagWrap followed by TagCheck) could be folded
  - GC shadow stack push/pop around non-allocating code could be eliminated
  - Iterable vocabulary desugaring (map/filter) creates redundant list allocations that could be fused
- **No inlining:** Small functions (getters, simple wrappers) are never inlined. Cranelift does some inlining but is conservative
- **No constant folding:** `1 + 2` is computed at runtime. AST-level constants aren't propagated
- **No dead code elimination:** Unreachable code after `return` or `break` is still compiled

**Recommendations:**
- **Now:** Add a simple constant folding pass on FIR (fold literal arithmetic, eliminate dead code after return/break). This is low-effort, high-impact
- **Soon:** Add TagWrap/TagCheck folding (if you just wrapped a value, you know the tag)
- **Later:** Consider a small inlining pass for trivially small functions (single-expression bodies)
- **Later:** Shadow stack optimization (elide push/pop for leaf functions that don't allocate)

---

## Category 13: Runtime Object Layout

**Rust approach:** Deterministic struct layout with field reordering for minimal padding. `repr(C)` for FFI. Enum niche optimization (Option<&T> is pointer-sized). Zero-cost abstractions throughout.

**Ruby approach:** 24-byte RBasic header (flags, klass, shape_id). Five size-segregated pools. Object shapes map ivar names to array indices for O(1) access. Embedded storage for short strings. COW for shared buffers.

**Asterc current state:** 24-byte GC header (mark, type, magic bytes, size, next pointer). Object types: OBJ_OPAQUE (strings), OBJ_LIST_HANDLE, OBJ_MAP_HANDLE, OBJ_CLASS (user instances), OBJ_CLOSURE, OBJ_DATA_BLOCK, OBJ_TASK, OBJ_LIST_HANDLE_NOPTR. Class instances have pointer fields sorted to front with ptr_count stored in header for precise GC tracing. All fields are i64 slots.

**Alignment with best practice:**
- Sorting pointer fields to the front for precise GC is a good technique
- Type-tagged headers enable type-specific GC tracing
- The OBJ_LIST_HANDLE_NOPTR distinction avoids unnecessary scanning of value-type lists
- Slab allocation for specific types (closure = 16 bytes, task = 24 bytes) avoids waste

**Gaps and risks:**
- **24-byte header overhead is significant:** A simple integer wrapper class (one i64 field) uses 32 bytes (24 header + 8 payload). Ruby's objects start at 48 bytes but include more metadata
- **No size-segregated pools:** Every allocation goes through the general allocator. Ruby's pooled allocation is O(1) from a freelist; asterc's is a system malloc
- **No niche optimization:** `Optional<ClassRef>` uses a full tagged union instead of null pointer. For Ptr types, null could represent None at zero cost
- **No shape system:** Instance variable access on class objects requires knowing the field offset at compile time. Dynamic field addition or reflection-based access would require a shape-like mechanism
- **Magic bytes waste 4 header bytes:** These could be replaced with a tagged pointer scheme

**Recommendations:**
- **Now:** Use null pointer for `None` in `Optional<Ptr>` types (niche optimization). The codegen already does this for some cases but it should be universal
- **Soon:** Consider reducing header to 16 bytes (mark+type+ptr_count as u16 flags, u16 reserved, u32 size, u64 next)
- **Later:** Add size-segregated allocation pools for common object sizes (16, 32, 64 bytes) to reduce allocator overhead
- **Later:** Consider a shape/hidden-class system if dynamic field access or polymorphic inline caches become important

---

## Category 14: Iteration and Iterator Protocols

**Rust approach:** `Iterator` trait with lazy adapters that compose via monomorphized `next()` chains. LLVM inlines and vectorizes the fused loop. `IntoIterator` bridges collections to iterators. `for` loop desugars to `IntoIterator::into_iter()` + `Iterator::next()` loop.

**Ruby approach:** `Enumerable` module with ~50 methods built on `each`. Internal iterators (block-based) are simple and efficient. External iterators (`Enumerator`) use Fibers for lazy pull-based iteration. `Enumerator::Lazy` chains transformations without intermediate arrays.

**Asterc current state:** Four for-loop lowering strategies: range literal (counter loop), range variable (runtime struct), Iterator protocol (`.next()` method), default list (index-based). Iterable vocabulary methods (map, filter, reduce, etc.) are inlined as explicit while loops. **No lazy composition.** Each operation materializes a new list.

**Alignment with best practice:**
- Range literal optimization (direct counter loop) is a common and effective optimization
- The Iterator protocol (classes with `.next() -> T?`) provides extensibility
- Inlining iterable methods as loops avoids function call overhead

**Gaps and risks:**
- **Eager evaluation creates unnecessary allocations:** `list.map(f).filter(g)` creates an intermediate list for the map result, then another for the filter result. Rust fuses this into a single loop with no intermediate allocation
- **No iterator fusion or lazy chains:** There's no way to compose operations without materializing intermediate results
- **O(n) memory overhead per operation:** A 1M-element list processed through 3 operations uses 4M elements of memory (original + 3 intermediates)
- **No short-circuit optimization for chained operations:** `list.map(f).first()` maps the entire list, then takes the first element. Should process only one element

**Recommendations:**
- **Now:** Add special-case fusion for common patterns: `.map(f).filter(g)` -> single loop, `.map(f).first()` -> map one element and return
- **Soon:** Consider a lazy iterator protocol where operations return iterator objects instead of materialized lists, similar to Rust's adapter chain or Ruby's `Enumerator::Lazy`
- **Later:** Add `collect()` or similar terminal operation that materializes a lazy chain

---

## Category 15: String Representation

**Rust approach:** Guaranteed UTF-8 `String`/`&str`. Explicit encoding boundaries (`OsStr`, `CStr`). No integer indexing (variable-width encoding). Range slicing with boundary validation. Separate types for different encoding guarantees.

**Ruby approach:** Encoding-aware strings with per-string encoding metadata. COW for frozen/shared strings. Embedded storage for short strings (<24 bytes). Global deduplication for frozen string literals. Mutable by default, moving toward frozen-by-default.

**Asterc current state:** Heap-allocated strings: `[i64 len @ offset 0][u8 data @ offset 8]`. UTF-8 encoding throughout. GC-tracked as OBJ_OPAQUE (no child pointers). Character-aware slicing (uses `char_indices()` for indexing). No string interning. No COW. No embedded/small string optimization. Concatenation always allocates new strings.

**Alignment with best practice:**
- UTF-8 throughout is the modern standard
- Character-aware slicing (not byte-indexed) prevents corruption of multi-byte characters
- Using `from_utf8_lossy()` for Rust interop gracefully handles invalid sequences
- OBJ_OPAQUE marking means the GC never scans string contents, which is correct

**Gaps and risks:**
- **No string interning:** Every string literal allocation is separate. Two occurrences of `"hello"` in source code create two heap objects. Both Rust (`&'static str`) and Ruby (frozen string dedup) avoid this
- **No small string optimization:** Even a 1-byte string requires 32 bytes (24-byte GC header + 8-byte length field + 1 byte data, rounded up). A small string optimization could embed short strings in the pointer itself or in a fixed-size inline buffer
- **Concatenation always allocates:** `a + b + c + d` creates 3 intermediate strings. String interpolation `"a{x}b{y}c"` generates a chain of concat calls
- **No COW:** String slicing (`aster_string_slice`) copies the substring. A COW scheme would share the underlying buffer
- **i64 for length is oversized:** String length doesn't need 8 bytes. A u32 (4GB max) would suffice and save 4 bytes per string

**Recommendations:**
- **Now:** Intern string literals at compile time (deduplicate identical string constants in the FIR module). This is a simple change with real memory savings
- **Now:** Add `aster_string_concat_n()` for multi-part string interpolation to avoid intermediate allocations
- **Soon:** Consider small string optimization: strings <= 23 bytes could be stored inline in a 24-byte struct (length byte + 23 data bytes), avoiding heap allocation entirely
- **Later:** Consider COW for string slicing and duplication
- **Later:** Reduce length field to u32

---

## Summary: Priority Actions

### 1. Correctness Risks

1. **No virtual dispatch for class methods** (Category 7): Polymorphism is broken. A `Dog` assigned to an `Animal` variable calls `Animal`'s methods, not `Dog`'s overrides. The vtable field exists in FirClass but isn't used at runtime.

2. **Per-thread error flag can be silently ignored** (Category 9): If a throwing call's error check is ever missed by the lowerer, execution continues with corrupted state. A validation pass should verify all throwing calls are followed by error checks.

3. **Snapshot/rollback missing in unification** (Category 3): Speculative type inference may leave corrupted bindings if a unification attempt partially succeeds then fails.

### 2. Soundness Gaps

4. **Conservative GC pointer validation is probabilistic** (Category 5): The magic-byte scheme has a non-zero (albeit tiny: ~2^-32) false positive rate. A tagged pointer scheme would be deterministic.

5. **All captures are by-value without documentation** (Category 11): Users may expect by-reference semantics (like JavaScript/Ruby). Mutating a captured variable in a closure doesn't affect the outer scope.

### 3. Performance Cliffs

6. **No generational GC** (Category 5): GC pause time scales linearly with total heap size. This is the single biggest scalability concern.

7. **Eager iterable evaluation with intermediate allocation** (Category 14): `list.map(f).filter(g)` creates a full intermediate list. For large collections, this causes O(n) unnecessary memory and allocation pressure.

8. **Match expressions compile to O(n) if/else chains** (Category 8): Enum matches should use jump tables.

9. **No pre-Cranelift optimization** (Category 12): Missing constant folding, dead code elimination, and tagged union operation folding.

10. **String concatenation chains allocate intermediate strings** (Category 15): String interpolation with multiple parts creates unnecessary intermediate allocations.

### 4. Missing Capabilities

11. **No nested patterns in match** (Category 8): Can't destructure enum variant fields, no or-patterns, no guard clauses.

12. **No string interning** (Category 15): Identical string literals create separate heap allocations.

13. **No lazy iterator composition** (Category 14): All iterable operations eagerly materialize results.

### 5. Design Debt

14. **Single IR with no optimization opportunity** (Category 1): FIR serves as both analysis target and codegen input. Adding optimization passes later will require either modifying FIR in-place or introducing a new IR stage.

15. **24-byte object header** (Category 13): Could be reduced to 16 bytes with better field packing, saving 8 bytes per heap object.

16. **Fixed 64KB green thread stacks** (Category 6): Wastes memory for simple tasks, insufficient for deeply recursive ones. Growable stacks would be more flexible.

---

## Summary: Strengths

1. **Pragmatic type erasure for generics** (Category 4): Avoids code bloat while maintaining type safety at compile time. Perfect for a GC'd language where all heap objects are pointer-width.

2. **Work-stealing green thread scheduler** (Category 6): Production-quality concurrency model with preemption, blocking pool, and channel/mutex primitives. More sophisticated than Ruby's Fiber scheduler and more ergonomic than Rust's explicit async/await.

3. **Exhaustiveness checking at compile time** (Category 8): Catches missing match arms for bools, enums, and nullable types. A significant correctness advantage over Ruby.

4. **Ergonomic error handling syntax** (Category 9): The `!` propagation operator, `.or()`, `.or_else()`, and `.catch {}` provide clean error handling without the verbosity of Rust's match arms or the hidden control flow of Ruby's exceptions.

5. **Modular lowering architecture** (Category 2): Clean separation of concerns across ~10 lowering modules, each handling a specific language feature. More maintainable than Ruby's monolithic compile.c.

6. **Precise GC tracing for class instances** (Category 5): Pointer fields sorted to front with ptr_count metadata enables precise tracing without per-type layout tables, balancing precision with simplicity.

7. **Module system with FileResolver abstraction** (Category 10): Clean separation between module resolution logic and filesystem access, enabling easy testing and potential future extension to non-filesystem sources.

8. **Static method resolution with dynamic fallback path** (Category 7/11): Closures known at compile time use direct calls; unknown closures use indirect calls. This two-tier approach avoids unnecessary indirection.
