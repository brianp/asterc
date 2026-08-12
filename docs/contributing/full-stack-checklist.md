# Full-Stack Feature Checklist

Every new language feature must be implemented through the entire compiler pipeline. Use this checklist when adding new syntax, expressions, statements, or type system features.

## Required Layers

### 1. AST
- [ ] Add new `Expr`, `Stmt`, `MatchPattern`, or `Type` variant to `ast/src/expr.rs` or `ast/src/types.rs`
- [ ] Include `Span` in the new node for error reporting
- [ ] Update `impl Display` if the node has a printable representation

### 2. Parser
- [ ] Parse the new syntax in `parser/src/lib.rs`, `parser/src/expr.rs`, or `parser/src/class_trait.rs`
- [ ] Add parser tests covering valid syntax
- [ ] Add parser error tests covering malformed syntax (`check_parse_err`)

### 3. Typechecker
- [ ] Handle the new node in `typecheck/src/check_expr.rs`, `typecheck/src/check_class.rs`, or `typecheck/src/typechecker.rs`
- [ ] Infer or validate types correctly
- [ ] Add diagnostic error code if the feature introduces new error conditions
- [ ] Add typechecker tests (`check_ok`, `check_err`)

### 4. FIR Lowering
- [ ] Lower the new AST node in `fir/src/lower.rs` (`lower_expr`, `lower_stmt_inner`, or `lower_top_level_stmt`)
- [ ] Add new `FirExpr` or `FirStmt` variants to `fir/src/exprs.rs` / `fir/src/stmts.rs` if needed
- [ ] If the feature desugars (e.g., `for` → `while`), document the desugaring in a comment
- [ ] Verify no `UnsupportedFeature` fallthrough remains for the new node

### 5. Codegen (Cranelift)
- [ ] Translate the new `FirExpr`/`FirStmt` in `codegen/src/translate.rs`
- [ ] Add runtime functions to `codegen/src/runtime.rs` if heap allocation or builtins are needed
- [ ] Register new runtime functions in both JIT (`jit.rs`) and AOT (`aot.rs`)
- [ ] Add codegen tests in `codegen/src/tests.rs` using `compile_and_run`

### 6. End-to-End
- [ ] Add at least one test that goes source text → parse → typecheck → FIR → JIT → assert result
- [ ] If AOT-relevant, add an AOT compilation test

### 7. Documentation
- [ ] Update the parity matrix in `STATUS.md` (Feature Parity section)
- [ ] Add examples to `examples/` if the feature is user-facing

## Common Mistakes

- **Adding syntax without codegen**: The parser and typechecker accept it, but `asterc run` crashes with `UnsupportedFeature`. Always implement through to codegen or return a clear "not yet supported" error from the CLI.
- **Adding FIR nodes without codegen arms**: New `FirExpr` variants cause `match` exhaustiveness errors in `translate.rs` — this is by design. Don't add a panic arm; implement the translation.
- **Forgetting AOT**: Runtime functions declared in `runtime.rs` must be registered in both `jit.rs` (symbol mapping) and `aot.rs` (import declarations). Missing one means JIT works but native binaries crash at link time.
- **Skipping the parity matrix**: If STATUS.md isn't updated, the gap becomes invisible. Update it in the same PR.
