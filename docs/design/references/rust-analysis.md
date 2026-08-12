# Rust Compiler Architecture Analysis

A deep investigation of how the Rust compiler (`rustc`) handles 15 core architectural areas, based on source-level analysis of the rust-lang/rust codebase.

---

## 1. Intermediate Representations

### IR Pipeline: AST -> HIR -> THIR -> MIR -> LLVM IR

Rust uses **four** intermediate representations between source text and LLVM IR.

### HIR (High-Level IR)

**Key files:** `compiler/rustc_hir/src/hir.rs`

HIR is produced by AST lowering and is a **desugared but still tree-structured** representation. Core types:

- **`Body<'hir>`** (line 2180): Contains `params` and a single expression `value`.
- **`Expr<'hir>`** (line 2432): Each expression carries `hir_id`, `kind: ExprKind`, and `span`.
- **`ExprKind<'hir>`** (line 2791): A large enum preserving high-level constructs -- `If`, `Loop`, `Match`, `MethodCall`, `Closure`, `Block`, `Assign`, `Binary`, `Field`, `Index`, `AddrOf`, `Break`, `Continue`, `Ret`, etc.

HIR retains Rust-level constructs and is the primary IR for **type checking**, **trait resolution**, **name resolution**, and **lint checking**.

### THIR (Typed High-Level IR)

**Key files:** `compiler/rustc_middle/src/thir.rs`

THIR is an intermediate step between HIR and MIR. It is **fully typed** and further desugared -- method calls and overloaded operators are converted into `ExprKind::Call` instances. It carries full `Ty<'tcx>` type information on every expression and serves as input to **MIR building**, **THIR unsafety checking**, and **pattern exhaustiveness checking**.

### MIR (Mid-Level IR)

**Key files:** `compiler/rustc_middle/src/mir/mod.rs`, `compiler/rustc_middle/src/mir/syntax.rs`

MIR is a **CFG-based** representation that eliminates all structured control flow. Core structures:

- **`Body<'tcx>`** (mod.rs:209): A collection of basic blocks plus local variable declarations. Contains `basic_blocks: BasicBlocks<'tcx>`, `local_decls`, `phase: MirPhase`, debug info, and spans.
- **`BasicBlockData<'tcx>`** (mod.rs:1289): A linear sequence of `Statement`s followed by one `Terminator`. The `is_cleanup` flag marks unwind blocks.
- **`TerminatorKind<'tcx>`** (syntax.rs:702): Defines control flow edges -- `Goto`, `SwitchInt` (the **only** conditional branch), `Return`, `Call`, `Drop`, `Assert`, `Yield`, `UnwindResume`, `UnwindTerminate`, etc.
- **`StatementKind`** (syntax.rs:310): Non-branching operations -- `Assign`, `StorageLive`/`StorageDead`, `SetDiscriminant`, `FakeRead`, `Retag`, `Nop`, etc.
- **`BasicBlocks`** (basic_blocks.rs:15): Lazily computes CFG properties: **predecessor maps**, **reverse postorder**, and **dominator trees**.

### MIR Phases

MIR itself evolves through multiple phases (`MirPhase`, syntax.rs:42):

1. **`Built`** -- initial MIR. Used for unsafety checking, lints.
2. **`Analysis(Initial)`** -- after const promotion. Used by borrow checker (NLL).
3. **`Analysis(PostCleanup)`** -- removes `FalseUnwind`, `FalseEdge`, `FakeRead`, `AscribeUserType`.
4. **`Runtime(Initial)`** -- after drop elaboration and coroutine lowering.
5. **`Runtime(PostCleanup)`** -- `Box` derefs removed.
6. **`Runtime(Optimized)`** -- after MIR optimization passes.

### Why Rust Needs Both HIR and MIR

**HIR** preserves tree-structured, high-level semantics essential for type inference (needs expression structure), trait resolution (needs high-level type information), and diagnostics (needs source-level structure).

**MIR** provides a flat, CFG-based, explicitly typed representation essential for borrow checking (dataflow analysis on the CFG), optimization (const propagation, inlining, DCE on a CFG), code generation (LLVM IR is itself CFG-based), and const evaluation (the MIR interpreter Miri executes MIR directly).

### Information Preserved vs. Discarded

| Transition | Discarded | Preserved |
|---|---|---|
| AST -> HIR | Syntactic sugar (`for`, `?`, `async/await`, `while let`), macros, formatting | Tree structure, type annotations, generics, spans, visibility |
| HIR -> THIR | Method call syntax (resolved to calls), overloaded operators, implicit coercions | Tree structure, full `Ty<'tcx>` types, scope structure, patterns |
| THIR -> MIR | All structured control flow, expression trees, lexical scoping, named variables | Types, **generic parameters** (MIR is still generic), debug info, spans |

### When Generics Get Monomorphized

Generics are monomorphized **after all MIR optimization**, during the **monomorphize/codegen phase**. MIR optimizations run **once per generic function** (not once per instantiation). Only at codegen time are copies made for each concrete type substitution.

---

## 2. Lowering Passes

### HIR to THIR

**Entry point:** `thir_body` query in `compiler/rustc_mir_build/src/thir/cx/mod.rs` (line 16).

The `ThirBuildCx` recursively converts each `hir::Expr` into a `thir::Expr` via `mirror_expr` (in `thir/cx/expr.rs`, line 37). The THIR is fully typed with type adjustments (coercions, autoderefs) applied explicitly.

### THIR to MIR

**Entry point:** `build_mir_inner_impl` in `compiler/rustc_mir_build/src/builder/mod.rs` (line 69).

The `Builder` translates THIR expressions into MIR through category-based methods:
- `expr_into_dest` (into.rs) -- main dispatch
- `as_place.rs` -- lvalue expressions
- `as_rvalue.rs` -- aggregate construction, closures

### What Gets Desugared Where

**For loops** -- Desugared in **AST lowering** (`rustc_ast_lowering/src/expr.rs`, line 1788). Transforms `for <pat> in <head> { <body> }` into `loop { match Iterator::next(&mut iter) { None => break, Some(<pat>) => <body> } }`.

**The `?` operator** -- Desugared in **AST lowering** (`lower_expr_try`, line 1980). Becomes `match Try::branch(<expr>) { ControlFlow::Continue(val) => val, ControlFlow::Break(residual) => return Try::from_residual(residual) }`.

**Async/await** -- Desugared in **AST lowering**. An `async` block becomes a coroutine closure. `.await` is desugared into a loop calling `Future::poll` with a `yield` suspension point.

**Operator overloads** -- Desugared during **HIR-to-THIR**. When `typeck_results.is_method_call(expr)` is true, the operation is converted to `ExprKind::Call`.

**Match arms** -- Lowered from THIR to MIR in `compiler/rustc_mir_build/src/builder/matches/mod.rs`. The `match_expr` method builds a decision tree from patterns, creating `Candidate` objects and generating test-and-branch MIR.

### Closure Conversion During Lowering

Capture analysis is performed during typeck in `rustc_hir_typeck/src/upvar.rs`. Each capture is a `CapturedPlace` recording the variable, the `HirPlace` captured (supporting precise field-level capture per RFC 2229), and the `UpvarCapture` kind:

- **`ByValue`** -- captured by move
- **`ByUse`** -- captured by use
- **`ByRef(BorrowKind)`** -- captured by reference (immutable, unique immutable, or mutable)

In MIR, closures are constructed as `Rvalue::Aggregate(AggregateKind::Closure(...), operands)`. Inside a closure's MIR body, `Local(1)` (`CAPTURE_STRUCT_LOCAL`) is the closure struct. Access to captured variables uses projections from this local: `(*_1).field_i` for by-value captures (via `Fn`/`FnMut`), or `*(*_1).field_i` for by-reference captures.

---

## 3. Type Systems and Inference

### Inference Algorithm

Rust uses a **constraint-based type inference** system -- a modified Hindley-Milner with extensions for subtyping, traits, and lifetimes. It is not a pure HM system, nor purely bidirectional.

**`InferCtxt<'tcx>`** (`compiler/rustc_infer/src/infer/mod.rs`, line 231) is the central type, managing:

- **Type variables** via union-find tables (`eq_relations` for equality, `sub_unification_table` for subtyping)
- **Integer/float variable tables** for numeric literal resolution
- **Region constraints** (`RegionConstraintStorage`) collecting outlives constraints
- **Snapshot/rollback** enabling speculative inference

**How unification works:** When enforcing `?target == source_ty`, `InferCtxt::instantiate_ty_var` generalizes the source type, performs an occurs check, unifies via union-find, then relates the generalized type back to collect sub-constraints. `TypeRelating` (`relate/type_relating.rs`) walks two types structurally tracking variance.

The algorithm is **constraint-based with eager unification**: fresh type variables are generated, equality/subtyping constraints are collected, union-find eagerly unifies, and trait obligations are iteratively processed. Remaining ambiguous variables are resolved via fallback (integer -> `i32`, float -> `f64`).

### Trait Resolution and Coherence

**`SelectionContext`** (`compiler/rustc_trait_selection/src/traits/select/mod.rs`, line 102) drives trait resolution through candidate assembly, selection, and confirmation. **`FulfillmentContext`** (`traits/fulfill.rs`, line 61) uses an **`ObligationForest`** to iteratively process obligations, tracking `stalled_on` variables for re-evaluation after unification.

**Coherence** ensures each trait has at most one implementation per type:
1. **Orphan check** (`coherence/orphan.rs`): Every impl must involve a trait or type from the current crate.
2. **Overlap check** (`traits/coherence.rs`, line 155): For every pair of impls, tries fast rejection, then header equating, then implicit negative reasoning to rule out overlaps.

**The specialization problem:** Specialization (`traits/specialize/mod.rs`) allows a more specific impl to override a more general one. The `min_specialization` gate enforces that specializing impls must be "always applicable" -- their bounds cannot depend on lifetime relationships. This is necessary because trait resolution happens before lifetime resolution. Full specialization remains **unsound** (issue #31844).

### Lifetimes and Type Inference

Lifetimes cannot be fully inferred in Rust. Function signatures use **lifetime elision rules**, struct definitions require explicit lifetime parameters, and within function bodies lifetimes are largely inferred by NLL.

Resolution is **two-phase**: during HIR typeck, region obligations are collected but solving is deferred to borrowck. NLL (`compiler/rustc_borrowck/src/region_infer/mod.rs`) then solves constraints using SCC-based propagation on a CFG-aware region graph.

---

## 4. Generics and Monomorphization

### How Monomorphization Works

**Key files:** `compiler/rustc_monomorphize/src/collector.rs`, `compiler/rustc_middle/src/mir/mono.rs`

Monomorphization happens **after** all MIR optimizations but **before** codegen, via `collect_crate_mono_items` (collector.rs:1800).

**Phase 1 -- Root Discovery** (line 1459): Walks HIR to find public non-generic functions, `main`, `#[no_mangle]` items.

**Phase 2 -- Graph Walk** (line 1819, parallel): Starting from roots, `MirUsedCollector` (line 683) visits MIR to discover further mono items through function calls (`TerminatorKind::Call`), function pointer references, unsizing casts (triggering vtable method instantiation), and drop glue.

Key types:
- **`MonoItem<'tcx>`** (mono.rs:55): `Fn(Instance)`, `Static(DefId)`, `GlobalAsm(ItemId)`
- **`Instance<'tcx>`** (instance.rs:33): A `DefId` paired with concrete `GenericArgs`
- **`InstantiationMode`** (mono.rs:28): `GloballyShared` (one copy, debug builds) vs `LocalCopy` (per-CGU, optimized builds)

### Trait Objects and Vtables

**`VtblEntry<'tcx>`** (`compiler/rustc_middle/src/ty/vtable.rs`, line 13):
- Slots 0-2: `MetadataDropInPlace`, `MetadataSize`, `MetadataAlign`
- `Vacant` for non-dispatchable methods
- `Method(Instance)` for concrete function pointers
- `TraitVPtr(TraitRef)` for supertrait upcasting

**Layout rules:** The first supertrait's vtable is a prefix (zero-cost upcasting). Additional supertraits get separate vtable pointers. Own methods come last.

### Tradeoffs

- Every unique `(function, type-args)` pair generates a separate machine-code copy -- the fundamental source of binary size increase.
- `#[inline(never)]` generics use `GloballyShared` to reduce bloat.
- `dyn Trait` avoids caller-side monomorphization via vtable dispatch, at the cost of indirect call overhead.
- MIR optimizations run once per generic function (not per instantiation), then copies are made at codegen time.

---

## 5. Memory Management

### Ownership, Borrowing, and Move Tracking

**`MoveData`** (`compiler/rustc_mir_dataflow/src/move_paths/mod.rs`, line 168) is the central structure for tracking ownership flow. It contains a tree of `MovePath`s mirroring composite type structure, `MoveOut` records for each move at a MIR `Location`, and `Init` records for initialization events.

**`BorrowSet`** (`compiler/rustc_borrowck/src/borrow_set.rs`, line 16) tracks every `Rvalue::Ref` in MIR. Each `BorrowData` records reservation/activation locations, borrow kind, region, borrowed place, and assigned place.

The borrow checker (`MirBorrowckCtxt`, `lib.rs:787`) uses three combined dataflow analyses:
1. **`Borrows`** -- which borrows are active at each point
2. **`MaybeUninitializedPlaces`** -- which places might be uninitialized
3. **`EverInitializedPlaces`** -- which places have ever been initialized

### Drop Elaboration

**`ElaborateDrops`** (`compiler/rustc_mir_transform/src/elaborate_drops.rs`, line 48) is a required MIR pass that refines conservative drops into precise ones. It runs during analysis-to-runtime phase transition.

The pass:
1. Gathers move data for types needing drop
2. Computes `MaybeInitializedPlaces` and `MaybeUninitializedPlaces` dataflow
3. For each `Drop` terminator, determines `DropStyle`:
   - **`Dead`**: not initialized -- drop removed
   - **`Static`**: definitely initialized -- unconditional drop
   - **`Conditional`**: maybe initialized -- wrapped in `if drop_flag`
   - **`Open`**: multiple child paths with mixed states -- per-field conditional drops

### NLL and Polonius

**NLL** (`compiler/rustc_borrowck/src/nll.rs`):
1. All regions replaced with fresh inference variables
2. MIR type checking generates `OutlivesConstraint`s
3. SCC-based solving propagates region values (sets of CFG points + universal regions)
4. Checks universal region constraints and type tests

**`RegionInferenceContext`** (`region_infer/mod.rs`, line 79) stores definitions per region variable, liveness constraints, the constraint SCC DAG, and inferred SCC values.

**Polonius "next"** (`polonius/mod.rs`) models loan propagation as a **reachability problem** on a combined region-CFG graph. It traces individual loans through a localized constraint graph with variance-aware edges, giving strictly more precise results than NLL.

---

## 6. Async and Concurrency

### Async/Await Desugaring

The transformation happens in two phases:

**Phase 1: AST Lowering** (`compiler/rustc_ast_lowering/src/item.rs`, lines 1444-1600)

An `async fn` becomes a regular function returning a coroutine closure. Arguments are moved into the closure body. Each `.await` (`expr.rs`, lines 852-1050) becomes:

```rust
match IntoFuture::into_future(expr) {
    mut __awaitee => loop {
        match unsafe { Future::poll(Pin::new_unchecked(&mut __awaitee), get_context(task_context)) } {
            Poll::Ready(result) => break result,
            Poll::Pending => {}
        }
        task_context = yield ();  // suspension point
    }
}
```

**Phase 2: MIR State Machine Transformation** (`compiler/rustc_mir_transform/src/coroutine.rs`)

The `StateTransform` pass (line 1463) converts coroutine MIR into a state machine struct:

```rust
struct Coroutine {
    upvars...,       // captured variables
    state: u32,      // discriminant (0=unresumed, 1=returned, 2=poisoned, 3+=suspend points)
    mir_locals...,   // locals live across suspension points
}
```

Steps:
1. **Liveness analysis** (line 707): `MaybeLiveLocals` + `MaybeBorrowedLocals` determine which locals are live across `Yield` terminators.
2. **Layout computation** (line 967): Each suspension state is a variant containing only live locals. Storage conflicts allow overlapping placement.
3. **Yield/Return rewriting** (line 441): `Yield` -> set state + `Return` with `Poll::Pending`. `Return` -> `Poll::Ready(val)`.
4. **Resume function** (line 1247): Entry `SwitchInt` dispatches on state to the appropriate resume block.

### Pin and Why Async Needs It

**`Pin<Ptr>`** (`library/core/src/pin.rs`, line 1092) is a `#[repr(transparent)]` wrapper that prevents obtaining `&mut T` through safe APIs for `!Unpin` types.

Async state machines are **self-referential**: references to local variables across `.await` points are stored alongside the locals they point to. If the struct moved, internal pointers would dangle. The `Future::poll` signature takes `Pin<&mut Self>` to enforce this.

Immovable coroutines (from `async fn`) are **never `Unpin`**. The liveness analysis conservatively unions borrowed locals with live locals for immovable coroutines.

---

## 7. Method Resolution and Dispatch

### Method Resolution Pipeline

**Key files:** `compiler/rustc_hir_typeck/src/method/probe.rs`

When the compiler sees `receiver.method(...)`, `probe_op` (line 378) orchestrates the search:

1. **Compute autoderef steps**: The `Autoderef` iterator (`compiler/rustc_hir_analysis/src/autoderef.rs`) repeatedly dereferences via builtin deref (`&T -> T`) and overloaded deref (`Deref::Target`).

2. **Assemble candidates** for each step:
   - **Inherent candidates**: from impl blocks, trait object methods, where-clause bounds
   - **Extension candidates**: from all in-scope traits

3. **Pick the best method** (`pick_all_method`, line 1275). For each autoderef step, tries in order:
   1. By value (`self`)
   2. Autoref `&self`
   3. Autoref `&mut self`
   4. `*mut T` -> `*const T`
   5. Pin reborrow

   **Inherent methods always shadow trait methods.** The first match at the earliest autoderef step wins.

### Static vs Dynamic Dispatch

Dispatch is distinguished at the codegen level via `InstanceKind` (`compiler/rustc_middle/src/ty/instance.rs`):
- **Static dispatch** (`impl Trait`, concrete types): `InstanceKind::Item(DefId)` -- direct call to monomorphized function
- **Dynamic dispatch** (`dyn Trait`): `InstanceKind::Virtual(DefId, usize)` -- indirect call through vtable slot

### Deref as "method_missing"

Rust has no `method_missing`. The `Deref` trait provides fallback -- `SmartPtr<T>` implementing `Deref<Target = T>` transparently "inherits" all methods of `T`. Methods on `SmartPtr` take priority (earlier autoderef step).

---

## 8. Pattern Matching and Exhaustiveness

### Match Compilation

**Key files:** `compiler/rustc_mir_build/src/builder/matches/mod.rs`

Match lowering uses a **backtracking automaton** (not a full decision tree), prioritizing smaller code size over optimal execution paths.

The core algorithm `match_candidates_inner` (line 1765):
1. If no candidates, return (becomes `otherwise_block`)
2. If first candidate is fully matched, bind it and continue with the rest
3. If any candidate starts with an or-pattern, expand subcandidates
4. Otherwise, `test_candidates`: pick a test, partition candidates into buckets by outcome, recursively build subtrees, emit MIR test instruction

Key data structures:
- **`Candidate`** (line 1031): Match pairs to test, subcandidates, guard info, output blocks
- **`MatchPairTree`** (line 1282): Pairs a `Place` with a `TestableCase` and child tests
- **`TestKind`** (line 1326): `Switch`, `SwitchInt`, `If`, `StringEq`, `Range`, `SliceLen`, etc.

### Exhaustiveness Checking

**Key files:** `compiler/rustc_pattern_analysis/src/usefulness.rs`

Based on Maranget's algorithm. A pattern `q` is **useful** if there exists a value matched by `q` and by none of the patterns above it. From this:
- A pattern is **redundant** iff not useful w.r.t. patterns above it
- A match is **exhaustive** iff wildcard `_` is not useful w.r.t. all arms

The core function `compute_exhaustiveness_and_usefulness` (line 1704):
1. **Split constructors**: `ConstructorSet::split()` analyzes column constructors and produces minimal disjoint coverage
2. **Specialize** the matrix by each constructor
3. **Recurse** on specialized matrices
4. **Unspecialize** witnesses back

The `Missing` constructor represents "all constructors not present in the column" -- specializing with it discovers non-exhaustive cases.

### Or-patterns, Guards, Binding Modes

- **Or-patterns**: Expanded into subcandidates preserving order. Usefulness tracks per-subpattern for redundancy detection.
- **Guards**: Treated conservatively -- guarded rows don't "use up" their position in exhaustiveness. In MIR, guard bindings are created by reference, with fake borrows to prevent scrutinee mutation.
- **Binding modes**: `ByRef` or by value, collected during pattern lowering. For guards, by-value bindings are temporarily bound by reference.

---

## 9. Error Handling Models

### Result/Option and the `?` Operator

The `?` operator is built on three traits (`library/core/src/ops/try_trait.rs`):

- **`Try`** (line 133): `branch(self) -> ControlFlow<Residual, Output>` -- splits success/error
- **`FromResidual`** (line 310): `from_residual(residual) -> Self` -- converts residual back (enables cross-type `?` via `From`)
- **`ControlFlow`** (line 89): `Continue(C)` / `Break(B)` -- deliberately ordered so `ControlFlow<A, B>` has same layout as `Result<B, A>`

**Desugaring** (`compiler/rustc_ast_lowering/src/expr.rs`, line 1980): `expr?` becomes `match Try::branch(expr) { Continue(val) => val, Break(residual) => return FromResidual::from_residual(residual) }`.

At the MIR level, there is no special `?` representation. The desugared `match` is a normal `SwitchInt`. With inlining, `branch()` and `from_residual()` often optimize to nothing for `Result`.

### Panic Unwinding vs Abort

**`PanicStrategy`** (`compiler/rustc_target/src/spec/mod.rs`, line 834): `Unwind`, `Abort`, `ImmediateAbort`.

**Unwinding** uses platform-specific mechanisms:
- Unix/GNU: `_Unwind_RaiseException` (libunwind/libgcc) -- two-phase unwinding (search then cleanup)
- MSVC: Structured Exception Handling
- The personality function (`library/std/src/sys/personality/gcc.rs`, `#[lang = "eh_personality"]`) reads LSDA from unwind tables

**MIR representation** -- `UnwindAction` (syntax.rs:1035):
- `Continue` -- let unwinding pass through
- `Unreachable` -- UB if unwind happens
- `Terminate(Reason)` -- abort on unwind (ABI mismatch or double-panic)
- `Cleanup(BasicBlock)` -- run cleanup block, then resume

The `AbortUnwindingCalls` MIR pass enforces panic=abort semantics by replacing `UnwindResume` with `UnwindTerminate`.

---

## 10. Module and Import Systems

### Module Structure

**Key files:** `compiler/rustc_resolve/src/lib.rs`, `compiler/rustc_resolve/src/build_reduced_graph.rs`

`ModuleData` (lib.rs:645) represents a node in the module tree. Modules include `mod` blocks, the crate root, `enum` definitions (containing variants), `trait` definitions (containing associated items), and anonymous blocks.

Names are resolved in three **namespaces** (`PerNS`): TypeNS (types, modules, traits), ValueNS (functions, constants, statics), MacroNS (macros).

### Import Resolution

`use` statements are processed by `build_reduced_graph_for_use_tree` (build_reduced_graph.rs:572), creating `ImportData` with `ImportKind`: `Single`, `Glob`, `ExternCrate`, `MacroUse`, `MacroExport`.

Resolution is **iterative**: `resolve_imports()` (imports.rs:607) repeatedly attempts to resolve indeterminate imports in a fixed-point loop. Non-glob declarations always shadow glob declarations.

### Visibility

- `pub` -> `Visibility::Public`
- No modifier -> `Visibility::Restricted(nearest_parent_mod)`
- `pub(crate)` -> `Visibility::Restricted(CRATE_DEF_ID)`
- `pub(super)` -> restricted to parent module
- `pub(in path)` -> restricted to ancestor module

### Name Resolution Algorithm

`resolve_path_with_ribs` (ident.rs:1765) handles each path segment:
1. Special keywords at position 0: `self`, `super`, `crate`, `$crate`, `::`
2. First non-keyword segment: searches through scopes via `visit_scopes`
3. Subsequent segments: looks up in resolved module

Search priority for types: module names (non-glob, then glob) up through hygienic parents -> extern prelude -> tool modules -> std prelude -> built-in types.

Cross-crate resolution uses `DefId` (`compiler/rustc_span/src/def_id.rs:230`) -- a `(CrateNum, DefIndex)` pair uniquely identifying every named item across all crates.

---

## 11. Closure and Lambda Representation

### Closures Are Anonymous Structs

**Key files:** `compiler/rustc_type_ir/src/ty_kind/closure.rs` (lines 14-91)

Each closure is modeled as:
```
struct Closure<'l0...'li, T0...Tj, CK, CS, U>(...U);
```
where `CK` is closure kind (Fn/FnMut/FnOnce), `CS` is the signature, and `U` is a tuple of upvar types.

**`ClosureKind`** (`compiler/rustc_type_ir/src/lib.rs:407`): `Fn` (captures by `&self`), `FnMut` (`&mut self`), `FnOnce` (`self`). These form a lattice: `Fn` < `FnMut` < `FnOnce`, starting at `Fn` and escalating based on usage.

The self parameter type is determined by `closure_env_ty()` (ty/util.rs:663): `Fn` -> `&self`, `FnMut` -> `&mut self`, `FnOnce` -> `self` by value.

### Capture Analysis

`analyze_closure` (upvar.rs:166) uses `ExprUseVisitor` to determine how each upvar is used, then escalates the closure kind accordingly. With RFC 2229, captures are precise to individual fields (e.g., `a.b.c`).

### Stack Allocation Only

**The compiler always stack-allocates closure environments.** Layout is computed via `univariant()` -- the same path as plain structs. In MIR, closures are `Rvalue::Aggregate`. In DWARF, they're emitted as `Stub::Struct`. Heap allocation only happens through explicit user code (`Box::new(|| ...)`, `Box<dyn Fn()>`).

---

## 12. Code Generation and Optimization

### MIR to LLVM IR Pipeline

**Key files:** `compiler/rustc_codegen_llvm/src/lib.rs`, `compiler/rustc_codegen_ssa/src/base.rs`

1. Monomorphized items are partitioned into `CodegenUnit`s
2. Each CGU compiled via `compile_codegen_unit()` (base.rs:58)
3. `codegen_mir()` (mir/mod.rs:176) fetches optimized MIR, sets up LLVM function, translates basic blocks into LLVM IR
4. `optimize()` (write.rs:888) runs LLVM's new pass manager
5. `codegen()` (write.rs:979) emits object files, bitcode, assembly

### MIR Optimization Pipeline

**Key files:** `compiler/rustc_mir_transform/src/lib.rs` (line 684)

The full pipeline in order:

**Pre-Inlining:**
- `LowerSliceLenCalls`, `InstSimplify::BeforeInline`

**Inlining:**
- `ForceInline` (`#[rustc_force_inline]`)
- `Inline` (inline.rs) -- cost model with `INSTR_COST=5`, `CALL_PENALTY=25`, `LANDINGPAD_PENALTY=50`, depth limits

**Post-Inlining:**
- `RemoveStorageMarkers`, `RemoveZsts`, `RemoveUnneededDrops`
- `UnreachableEnumBranching`, `UnreachablePropagation`

**Core Optimizations:**
- **`ReferencePropagation`** -- eliminates borrow-dereference patterns
- **`ScalarReplacementOfAggregates`** (SROA) -- breaks aggregates into scalars
- **`DeadStoreElimination`** -- removes stores to never-read locals
- **`GVN`** -- Global Value Numbering, detects redundant computations
- **`SsaRangePropagation`** -- propagates integer ranges through branches
- **`DataflowConstProp`** -- dataflow-based constant propagation
- **`JumpThreading`** -- replaces join-then-switch with direct jumps
- **`CopyProp`** -- SSA-based copy propagation
- **`DestinationPropagation`** -- NRVO-like merging of `dest = src` (compensates for LLVM weakness, refs rust-lang/rust#32966)
- **`EnumSizeOpt`** -- optimizes large enum layouts (threshold: 128 bytes)

**Key insight:** MIR optimizations handle patterns that LLVM cannot optimize well. GVN and dataflow const-prop operate on type-rich MIR with access to enum discriminants, borrow semantics, and type layouts that would be lost in LLVM IR.

---

## 13. Runtime Object Layout

### Struct Layout

**Key files:** `compiler/rustc_abi/src/lib.rs`, `compiler/rustc_abi/src/layout.rs`

**`LayoutData`** (lib.rs:2011) captures every type's layout: `fields: FieldsShape`, `variants: Variants`, `backend_repr: BackendRepr`, `largest_niche`, `align`, `size`.

**Default (repr(Rust)) field ordering** (`univariant_biased`, layout.rs:1154):
1. Fields sorted by descending "alignment group" -- groups `[u8; 4]` with align-4 fields, reducing padding
2. With `-Z randomize-layout`, fields are shuffled deterministically
3. Niche placement bias: fields with larger niches positioned at struct edges for enum optimization
4. Two-pass optimization tries both start-biased and end-biased niche placement

**repr(C) differences:**
- Fields in declaration order (no reordering)
- No enum layout optimization (niche filling disabled)
- No newtype ABI optimization (scalar newtypes stay aggregates)

### Enum Layout Optimization

Two strategies, best one wins:

**Niche Filling** (layout.rs:634): Uses invalid bit patterns in the largest variant's scalar fields to encode other variants. Example: `Option<&T>` uses null pointer for `None`. `niche.reserve(count)` preferentially places `None` at value zero.

**Direct Tag** (layout.rs:791): Explicit integer discriminant field. Tag may be widened to match field alignment.

Niche filling wins when it produces a **smaller size** than direct tag.

### Key Types

- **`Niche { offset, value: Primitive, valid_range }`** (lib.rs:1924): Identifies a scalar field with invalid values. `available()` computes how many niche values exist.
- **`TagEncoding`**: `Direct` (explicit discriminant) or `Niche { untagged_variant, niche_variants, niche_start }` (packed into existing field)

---

## 14. Iteration and Iterator Protocols

### The Iterator Trait

**Key files:** `library/core/src/iter/traits/iterator.rs` (line 41)

Single required method: `fn next(&mut self) -> Option<Self::Item>`. 70+ provided methods built on `next()`.

`IntoIterator` (`collect.rs:283`) bridges collections to iterators. Every `Iterator` automatically implements `IntoIterator`.

### How Adapters Get Optimized

**Loop fusion** via `fold`/`try_fold` composition: Each adapter overrides `fold`/`try_fold` to compose its closure with the consumer's. A chain like `.map(f).filter(g).fold(...)` collapses into a single tight loop -- closures composed at compile time, inlined by LLVM.

**Marker traits for unsafe optimizations:**
- **`TrustedLen`** (marker.rs:66): Exact `size_hint()` -- allows single-allocation `collect`
- **`TrustedRandomAccess`** (zip.rs:572): Enables indexed `Zip` -- allows LLVM vectorization
- **`InPlaceIterable` + `SourceIter`**: Enables in-place collection -- `vec.into_iter().map(f).collect::<Vec<_>>()` reuses the input allocation

### For Loop Desugaring

`lower_expr_for` (`compiler/rustc_ast_lowering/src/expr.rs`, line 1804):

```rust
// for <pat> in <head> { <body> }  becomes:
match IntoIterator::into_iter(<head>) {
    mut iter => loop {
        match Iterator::next(&mut iter) {
            None => break,
            Some(<pat>) => <body>,
        }
    }
}
```

In MIR: a call to `into_iter()`, a loop header calling `next()`, a `SwitchInt` on the `Option` discriminant, the body block, and a goto back to the header. After inlining, the `Option` check often simplifies to direct comparisons.

---

## 15. String Representation

### String Types and Their Relationships

**`str`** (primitive): An unsized type -- a contiguous sequence of bytes guaranteed valid UTF-8. `&str` is a fat pointer (data pointer + byte length).

**`String`** (`library/alloc/src/string.rs`, line 353): `struct String { vec: Vec<u8> }` -- literally a `Vec<u8>` with the UTF-8 invariant. `Deref`s to `&str`.

**`OsStr` / `OsString`** (`library/std/src/ffi/os_str.rs`): Platform-dependent:
- **Unix**: wraps `[u8]` / `Vec<u8>` -- raw bytes, no encoding guarantee
- **Windows**: wraps `Wtf8` / `Wtf8Buf` -- WTF-8 (superset of UTF-8 handling unpaired surrogates)
- **Other**: wraps `str` / `String` -- guaranteed UTF-8

**`CStr`** (`library/core/src/ffi/c_str.rs`, line 102): `struct CStr { inner: [c_char] }` -- nul-terminated byte slice, layout-compatible with `[u8]`.

Conversions: `String` -> `OsString` is lossless. `OsStr` -> `&str` may fail (`to_str()` returns `Option`). `CStr` -> `&str` requires UTF-8 validation.

### UTF-8 Validation

`run_utf8_validation` (`library/core/src/str/validations.rs`, line 126): Validates per RFC 3629. ASCII bytes use a fast path reading 2 machine words at a time (checking entire `usize` chunks for absence of high bits). Multi-byte sequences validated for overlong encodings, surrogates, and range.

### Why Integer Indexing Is Not Supported

`SliceIndex<str>` is implemented for range types but **NOT** for `usize`. Reasons:
1. **Variable-width encoding**: `s[i]` could land mid-character
2. **O(1) vs O(n) ambiguity**: Byte indexing could return invalid data; character indexing would be O(n)
3. **Range indexing works**: `&s[start..end]` checks `is_char_boundary()` at both ends, panicking on invalid boundaries

A byte is a char boundary if it is NOT a continuation byte (`10xxxxxx`). To access characters: `s.chars()`, `s.char_indices()`, or `s.as_bytes()[i]` for raw byte access.
