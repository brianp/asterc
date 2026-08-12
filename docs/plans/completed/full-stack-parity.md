---
status: deprecated
created: 2026-03-12 00:00
executed: 2026-03-13
deprecated: 2026-03-15
note: "Async/concurrency sections superseded by green-threads.md. Retained as historical reference."
---

# Implementation Plan: Full-Stack Feature Parity

## Goal

Ensure every Aster language feature is implemented through the **entire** compiler pipeline: Parse → Typecheck → FIR Lower → Cranelift Codegen. Establish a parity matrix as the single source of truth, and a process to prevent future drift.

## Prerequisites

- All 548 tests passing (confirmed 2026-03-12)
- Codegen milestones M2–M20 complete
- FIR lowering covers core expressions and statements

---

## Current Parity Matrix

This is the gap analysis — every AST feature mapped across all four layers.

### Statements

| Feature | Parse | Typecheck | FIR Lower | Codegen | Status |
|---------|-------|-----------|-----------|---------|--------|
| `Stmt::Let` | ✅ | ✅ | ✅ | ✅ | **Complete** |
| `Stmt::Const` | ✅ | ✅ | ✅ (as Let) | ✅ | **Complete** |
| `Stmt::Return` | ✅ | ✅ | ✅ | ✅ | **Complete** |
| `Stmt::Expr` | ✅ | ✅ | ✅ | ✅ | **Complete** |
| `Stmt::If` | ✅ | ✅ | ✅ | ✅ | **Complete** |
| `Stmt::While` | ✅ | ✅ | ✅ | ✅ | **Complete** |
| `Stmt::For` | ✅ | ✅ | ✅ (desugar) | ✅ | **Complete** |
| `Stmt::Assignment` | ✅ | ✅ | ✅ | ✅ | **Complete** |
| `Stmt::Break` | ✅ | ✅ | ✅ | ✅ | **Complete** |
| `Stmt::Continue` | ✅ | ✅ | ✅ | ✅ | **Complete** |
| `Stmt::Class` | ✅ | ✅ | ✅ | ✅ | **Complete** |
| `Stmt::Enum` | ✅ | ✅ | ✅ | ✅ | **Complete** |
| `Stmt::Trait` | ✅ | ✅ | N/A (type-only) | N/A | **Complete** (traits are erased) |
| `Stmt::Use` | ✅ | ✅ | N/A (resolved) | N/A | **Complete** (imports resolved pre-FIR) |

### Expressions

| Feature | Parse | Typecheck | FIR Lower | Codegen | Status |
|---------|-------|-----------|-----------|---------|--------|
| `Expr::Int` | ✅ | ✅ | ✅ | ✅ | **Complete** |
| `Expr::Float` | ✅ | ✅ | ✅ | ✅ | **Complete** |
| `Expr::Str` | ✅ | ✅ | ✅ | ✅ | **Complete** |
| `Expr::Bool` | ✅ | ✅ | ✅ | ✅ | **Complete** |
| `Expr::Nil` | ✅ | ✅ | ✅ | ✅ | **Complete** |
| `Expr::Ident` | ✅ | ✅ | ✅ | ✅ | **Complete** |
| `Expr::BinaryOp` | ✅ | ✅ | ✅ | ✅ | **Complete** |
| `Expr::UnaryOp` | ✅ | ✅ | ✅ | ✅ | **Complete** |
| `Expr::Call` | ✅ | ✅ | ✅ | ✅ | **Complete** |
| `Expr::Lambda` | ✅ | ✅ | ✅ | ✅ | **Complete** |
| `Expr::Member` | ✅ | ✅ | ✅ | ✅ | **Complete** |
| `Expr::Index` | ✅ | ✅ | ✅ | ✅ | **Complete** |
| `Expr::ListLiteral` | ✅ | ✅ | ✅ | ✅ | **Complete** |
| `Expr::Match` | ✅ | ✅ | ✅ | ✅ | **Complete** |
| `Expr::StringInterpolation` | ✅ | ✅ | ✅ | ✅ | **Complete** |
| `Expr::AsyncCall` | ✅ | ✅ | ⚠️ Eager | ⚠️ Eager | **Stub** — no true concurrency |
| `Expr::Resolve` | ✅ | ✅ | ⚠️ Identity | ⚠️ Identity | **Stub** — passthrough |
| `Expr::Propagate` | ✅ | ✅ | ⚠️ Identity | ⚠️ Identity | **Stub** — no unwrap/early-return |
| `Expr::Throw` | ✅ | ✅ | ⚠️ Panic | ⚠️ Panic | **Stub** — aborts instead of structured error |
| `Expr::ErrorOr` | ✅ | ✅ | ⚠️ Identity | ⚠️ Identity | **Stub** — no error path |
| `Expr::ErrorOrElse` | ✅ | ✅ | ⚠️ Identity | ⚠️ Identity | **Stub** — no error path |
| `Expr::ErrorCatch` | ✅ | ✅ | ⚠️ Identity | ⚠️ Identity | **Stub** — no catch dispatch |
| `Expr::Map` | ✅ | ✅ | ❌ Missing | ❌ Missing | **Gap** |
| `Expr::DetachedCall` | ✅ | ✅ | ❌ Missing | ❌ Missing | **Gap** |
| `Expr::AsyncScope` | ✅ | ✅ | ❌ Missing | ❌ Missing | **Gap** |

### Types (Runtime Representation)

| Type | FIR Repr | Codegen Repr | Status |
|------|----------|-------------|--------|
| `Int` | `FirType::I64` | `types::I64` | **Complete** |
| `Float` | `FirType::F64` | `types::F64` | **Complete** |
| `Bool` | `FirType::Bool` | `types::I8` | **Complete** |
| `String` | `FirType::Ptr` | `i64` (heap) | **Complete** |
| `Nil` | `FirType::I64` | `i64(0)` | **Complete** |
| `List[T]` | `FirType::Ptr` | `i64` (heap) | **Complete** |
| `Map[K,V]` | — | — | **Missing** |
| `Custom` (class) | `FirType::Struct` | `i64` (heap) | **Complete** |
| `Task[T]` | `FirType::I64` | `i64` | **Stub** (no wrapping) |
| `Nullable (T?)` | `FirType::TaggedUnion` | `i64` | **Partial** (passthrough tags) |
| `Function` | `FirType::FnPtr` | `i64` (closure pair) | **Complete** |

### Protocols (Trait Method Dispatch at Runtime)

| Protocol | Typecheck | FIR Desugar | Codegen | Status |
|----------|-----------|-------------|---------|--------|
| `Eq` (==, !=) | ✅ desugar to `.eq()` | ❌ No vtable/method call | ❌ | **Gap** — primitives work via BinOp, custom types don't |
| `Ord` (<, >, <=, >=) | ✅ desugar to `.cmp()` | ❌ No vtable/method call | ❌ | **Gap** — same as Eq |
| `Printable` (to_string) | ✅ | ⚠️ Partial (interp only) | ⚠️ | **Partial** — works in string interp, not standalone |
| `Iterable` (each, map, etc.) | ✅ | ❌ | ❌ | **Gap** — for-loop uses index, no trait dispatch |
| `From[T]`/`Into[T]` | ✅ | ❌ | ❌ | **Gap** |
| `Hash` | ✅ (invisible) | ❌ | ❌ | **Gap** (blocked on Map) |

### Pattern Matching Completeness

| Pattern | FIR | Codegen | Status |
|---------|-----|---------|--------|
| `Wildcard` | ✅ | ✅ | **Complete** |
| `Ident` (binding) | ✅ | ✅ | **Complete** |
| `Literal` (int/str/bool) | ✅ | ✅ | **Complete** |
| `EnumVariant` (tag check) | ✅ | ✅ | **Complete** |
| Enum variant field extraction | ⚠️ | ⚠️ | **Partial** — tag matches, field destructure unclear |
| Nested patterns | ❌ | ❌ | **Not supported** |
| Guard clauses | ❌ | ❌ | **Not in AST** |

### Class Features at Runtime

| Feature | FIR | Codegen | Status |
|---------|-----|---------|--------|
| Construction | ✅ | ✅ | **Complete** |
| Field access | ✅ | ✅ | **Complete** |
| Method calls | ✅ | ✅ | **Complete** (static dispatch) |
| Inheritance (extends) | ⚠️ | ⚠️ | **Partial** — field layout, no super dispatch |
| Virtual dispatch | ❌ | ❌ | **Gap** — vtable planned but not emitted |
| Trait methods on instances | ❌ | ❌ | **Gap** — needs vtable or monomorphization |

---

## Task Breakdown

### Phase 1: Parity Tracking Infrastructure

#### 1.1 Add compile-time coverage check
- **Files to modify:** `codegen/src/tests.rs`
- **Approach:** Add a meta-test that enumerates all `Expr` and `Stmt` variants via `std::mem::discriminant` and asserts each has at least one codegen test exercising it. This catches new AST variants added without codegen support.
- **Implementation notes:** Use a naming convention like `coverage_all_expr_variants` that lists each variant and maps it to the test(s) that exercise it. Fail if any variant has no test.

#### 1.2 Add `--emit parity` diagnostic to CLI
- **Files to modify:** `src/main.rs`, `fir/src/lower.rs`
- **Approach:** When `asterc check --emit parity` is run on a file, report which features in that file would fail at codegen. Walk the typed AST and flag any node that hits an `UnsupportedFeature` path in FIR lowering. This gives users (and us) instant feedback on "will this compile?"
- **Dependencies:** None
- **Key decision:** Dry-run the lowering and collect errors rather than aborting on first. Return a structured list.

#### 1.3 Maintain the parity matrix in STATUS.md
- **Files to modify:** `STATUS.md`
- **Approach:** Add a "Feature Parity" section to STATUS.md containing the matrix from this plan. Update it as each gap is closed. This becomes the living document.
- **Key decision:** STATUS.md is already the single source of truth — extend it rather than creating a new file.

---

### Phase 2: Close Critical Gaps (Map, Error Handling, Virtual Dispatch)

These are features users can write today (parser + typechecker accept them) but that crash at compile time.

#### 2.1 Map Literals
- **Files to modify:** `fir/src/lower.rs`, `fir/src/exprs.rs`, `codegen/src/translate.rs`, `codegen/src/runtime.rs`, `codegen/src/tests.rs`
- **Dependencies:** None
- **Approach:**
  1. Add `FirExpr::MapNew`, `FirExpr::MapGet`, `FirExpr::MapSet` to exprs.rs
  2. Runtime: implement `aster_map_new`, `aster_map_get`, `aster_map_set`, `aster_map_len` using a simple open-addressing hash table (keys are i64-tagged, values are i64)
  3. Lower `Expr::Map` to: create map, insert each key-value pair
  4. Lower `Expr::Index` on Map types to `MapGet`
  5. Lower `Stmt::Assignment` with Map index target to `MapSet`
  6. Codegen: translate the new FirExpr variants to runtime calls
  7. Tests: M21 milestone — map creation, get, set, iteration
- **Potential issues:** Hash function for strings needs pointer-stable comparison (compare by value, not pointer). Runtime `aster_map_get` must do string content comparison for string keys.

#### 2.2 Structured Error Handling (Result[T, E] at Runtime)
- **Files to modify:** `fir/src/lower.rs`, `fir/src/exprs.rs`, `codegen/src/translate.rs`, `codegen/src/runtime.rs`, `codegen/src/tests.rs`
- **Dependencies:** None (can be done independently of Map)
- **Approach:**
  1. **Encoding:** Use tagged pointer layout: `[tag: i64][value: i64]` where tag=0 is Ok, tag=1 is Error
  2. **`Expr::Throw`** → allocate tagged struct with tag=1, store error value
  3. **`Expr::Propagate` (!)** → check tag; if error, return early from current function with same tagged value; if ok, unwrap value
  4. **`Expr::ErrorOr`** → check tag; if error, evaluate and return default; if ok, unwrap
  5. **`Expr::ErrorOrElse`** → check tag; if error, call handler lambda with error value; if ok, unwrap
  6. **`Expr::ErrorCatch`** → check tag; if error, match error type and dispatch to catch arm; if ok, unwrap
  7. Add `FirExpr::ResultWrap`, `FirExpr::ResultUnwrap`, `FirExpr::ResultCheck` or reuse existing `TagWrap/TagUnwrap/TagCheck`
  8. Tests: M22 milestone — throw+propagate, .or(), .or_else(), .catch with typed arms
- **Key decision:** Reuse `TagWrap`/`TagUnwrap`/`TagCheck` FIR nodes (they already exist for nullable). The encoding is the same concept — tagged unions.
- **Potential issues:** Early return from `!` propagation requires generating a conditional return in the middle of expression lowering. May need to convert the expression into a statement sequence (let tmp = call; if is_error(tmp) { return tmp }; unwrap(tmp)).

#### 2.3 Virtual Dispatch (Trait Methods on Custom Types)
- **Files to modify:** `fir/src/lower.rs`, `fir/src/module.rs`, `codegen/src/translate.rs`, `codegen/src/tests.rs`
- **Dependencies:** None
- **Approach:**
  1. **Strategy: Static dispatch via monomorphization** — at FIR lowering time, resolve `obj.eq(other)` to `ClassName.eq(obj, other)` using the type information from typechecking. No vtable needed for now.
  2. When lowering `Expr::Call` where the function is a `Expr::Member` on a typed object, look up the method in the class/trait hierarchy and emit a direct `FirExpr::Call` to the qualified name.
  3. This already partially works for class methods — extend it to trait-included methods.
  4. For `==`/`!=` on custom types: the typechecker already desugars these to `.eq()` calls. The FIR just needs to resolve `ClassName.eq` correctly.
  5. Tests: M23 milestone — `==` on classes with Eq, `<` on classes with Ord, `to_string()` on classes with Printable
- **Key decision:** Monomorphize (static dispatch) rather than vtable. Aster doesn't have trait objects or dynamic dispatch in the type system, so static dispatch is correct and simpler.
- **Potential issues:** Inherited methods from parent class need lookup chain. The typechecker already resolves this — ensure FIR has access to the resolution.

---

### Phase 3: Close Semantic Gaps (Async, Iterable)

These features have deeper runtime implications and can be deferred after Phase 2.

#### 3.1 Iterable Protocol (Runtime)
- **Files to modify:** `fir/src/lower.rs`, `codegen/src/runtime.rs`, `codegen/src/tests.rs`
- **Dependencies:** Phase 2.3 (virtual dispatch for method calls)
- **Approach:**
  1. `for x in collection` currently desugars to index-based while loop (list-only)
  2. Generalize: desugar to `let iter = collection.each(); while ...` pattern using the Iterable protocol
  3. For lists, `each()` can still use index-based iteration internally
  4. For custom classes implementing Iterable, call the user's `each()` method
  5. Higher-order methods (`.map()`, `.filter()`, etc.) — these take closures, which already work. Just need to resolve the method dispatch.
- **Key decision:** Keep index-based optimization for List. Only use protocol dispatch for custom Iterable implementors.

#### 3.2 Async Runtime (Future Work)
- **Approach:** True async requires a runtime scheduler (green threads or event loop). This is a large effort.
- **Recommendation:** Document as "planned, not blocking" in STATUS.md. The eager execution stub is sufficient for correctness (just not concurrent).
- **No task breakdown** — this needs its own RFC when prioritized.

#### 3.3 DetachedCall / AsyncScope
- **Dependencies:** 3.2 (async runtime)
- **Status:** Blocked until async runtime exists. Document as known gap.

---

### Phase 4: Process for Maintaining Parity Going Forward

#### 4.1 "Full Stack" checklist for new features
- **Files to create:** `docs/contributing/full-stack-checklist<dot>md`
- **Approach:** Every new language feature PR must include:
  1. AST node(s) added
  2. Parser test(s)
  3. Typechecker handling + test(s)
  4. FIR lowering + test(s)
  5. Codegen translation + test(s)
  6. End-to-end test via `check_ok` or JIT execution
  7. Parity matrix row updated in STATUS.md
- **Key decision:** Make this a checklist in the PR template, not a CI check. Keeps it lightweight.

#### 4.2 End-to-end integration tests
- **Files to modify:** `tests/` directory, `codegen/src/tests.rs`
- **Approach:** For every feature that reaches codegen, add a test that goes source text → parse → typecheck → FIR → JIT → assert output. The existing `codegen/src/tests.rs` helper (`compile_and_run`) already does this. Ensure every feature has at least one such test.
- **Naming convention:** `e2e_{feature}_{scenario}` (e.g., `e2e_map_literal_get`, `e2e_error_propagate_early_return`)

#### 4.3 `UnsupportedFeature` audit CI step
- **Files to modify:** Add a test or script
- **Approach:** Grep for `UnsupportedFeature` in `fir/src/lower.rs` and `codegen/src/translate.rs`. Each occurrence should map to a known gap in the parity matrix. If a new `UnsupportedFeature` appears without a corresponding STATUS.md entry, fail.
- **Key decision:** This can be a simple `#[test]` that counts occurrences and asserts against an expected number. When you close a gap, decrement the count.

---

## Priority Order

1. **Phase 1.3** — Update STATUS.md with parity matrix (immediate, no code)
2. **Phase 2.1** — Map literals (users hit this, parser accepts it)
3. **Phase 2.2** — Error handling (widely used in examples, currently crashes at runtime)
4. **Phase 2.3** — Virtual dispatch (unlocks protocols at runtime)
5. **Phase 1.1** — Coverage meta-test (prevents future drift)
6. **Phase 3.1** — Iterable protocol
7. **Phase 4** — Process/checklist (ongoing)
8. **Phase 3.2/3.3** — Async runtime (future RFC)

---

## Unwired Code Audit

- [x] Every `FirExpr` variant in `exprs.rs` has a match arm in `translate.rs` — confirmed, 23/23
- [x] Every `FirStmt` variant has a match arm in `translate_stmt` — confirmed, 8/8
- [x] Every runtime function declared in `runtime.rs` is importable by both JIT and AOT — confirmed
- [x] `Expr::Map` parsed but never lowered — **fixed** (Phase 2.1)
- [ ] `Expr::DetachedCall` parsed but never lowered — **unwired** (Phase 3.3, blocked on async runtime)
- [ ] `Expr::AsyncScope` parsed but never lowered — **unwired** (Phase 3.3, blocked on async runtime)
- [x] `==`/`!=` on custom types desugar to `.eq()` in typechecker but FIR doesn't resolve trait method — **fixed** (Phase 2.3)
- [x] `Expr::Propagate` typechecked but FIR passes through identity — **fixed** (Phase 2.2)
- [ ] Inheritance field layout exists but no super method dispatch — **unwired** (known gap)

## Validation Steps

- [x] `Expr::Map` compiles and runs via JIT (map creation, get, set)
- [x] `throw` + `!` propagation produces correct early-return behavior
- [x] `.or()` and `.or_else()` handle error paths correctly
- [x] `==` on a class with `includes Eq` calls the auto-derived `.eq()` method
- [x] `for x in custom_iterable` calls `.next()` via Iterator protocol
- [x] Meta-test `coverage_all_expr_variants` passes
- [x] `unsupported_feature_audit` test tracks all UnsupportedFeature call sites (16 in lower.rs, 0 in translate.rs)
- [x] Every feature in the parity matrix marked ✅ has at least one end-to-end codegen test
