---
status: deprecated
deprecated: 2026-03-15
note: "Async/runtime sections superseded by green-threads.md. Retained as historical reference for codegen milestones M2-M20."
---

# Plan: Code Generation (Cranelift JIT + AOT)

## Context

The Aster compiler currently stops at typechecking. This plan adds a backend that compiles Aster programs to native machine code using Cranelift, a fast JIT/AOT compiler backend from the Bytecode Alliance.

**Depends on:** `diagnostics.md` (spans for error reporting). The AST and type system are already sufficient for codegen.

**Key design constraint:** Both the compiler (`asterc build`) and the REPL (`asterc repl`) consume the same intermediate representation. This plan introduces **FIR (Flat Intermediate Representation)** as a first-class shared crate that both backends depend on.

## Why Cranelift

**Evaluated options:**

| Backend | Compile Speed | Runtime Speed | Complexity | Dependencies |
|---------|--------------|---------------|------------|--------------|
| Tree-walk interpreter | instant | slow (10-100x native) | Low | None |
| Bytecode VM | fast | moderate (5-20x native) | Medium | None |
| **Cranelift** | **fast (~10x faster than LLVM)** | **good (within 2x of LLVM)** | **Medium** | **cranelift crates** |
| LLVM | slow | best | High | libLLVM (~100MB) |

**Community research:**

- Cranelift is designed as a JIT compiler — compilation is ~10x faster than LLVM, producing code within ~2x of LLVM's runtime performance. Perfect for development workflows.
- The Rust compiler has a Cranelift backend (`rustc_codegen_cranelift`) being actively developed for 2025-2026, aiming to be the recommended backend for local development.
- Cranelift uses CLIF (Cranelift IR), an SSA-based IR with block parameters instead of phi nodes. This is simpler to generate than LLVM IR.
- The `cranelift-jit-demo` from Bytecode Alliance provides an excellent reference for building a JIT for a toy language.
- Cranelift's optimization uses e-graphs (equality graphs), a modern approach that's both fast and effective.

**Decision:** FIR is the shared representation. Cranelift consumes FIR for both JIT (REPL/dev) and AOT (production builds) via `cranelift-jit` and `cranelift-object` respectively.

## Architecture Overview

```
                     Typed AST
                         |
                         v
               +-------------------+
               |   FIR Lowering    |  fir/ crate — shared by compiler + REPL
               |   (AST → FIR)     |
               +-------------------+
                         |
                         v
                    FIR Module        ← serializable, incrementally appendable
                    /          \
                   v            v
          +-------------+  +-------------+
          | cranelift-jit|  | cranelift-  |   codegen/ crate — consumes FIR
          | (REPL, dev) |  | object (AOT)|
          +-------------+  +-------------+
                |                  |
                v                  v
           In-memory           .o file
           execution           → linker
                                  → binary
```

Both the REPL and the compiler share everything above the dotted line. The only difference is which Cranelift backend consumes the CLIF output.

## FIR: Flat Intermediate Representation

### Why FIR exists

The typed AST is too high-level for code generation:
- Generics are unresolved (TypeVars, not concrete types)
- Method calls reference trait protocols, not concrete functions
- Classes have no memory layout
- Closures reference captured variables by name
- Error handling (`throws`, `!`, `.catch`) is sugar over control flow

FIR flattens all of this into a representation where:
- Every type is concrete (monomorphized)
- Every call targets a specific function ID
- Every class has computed field offsets and sizes
- Closures are converted to structs + plain functions
- Error propagation is lowered to result tuples or branches
- Nullable `T?` is a tagged union

### Design principles

1. **Shared** — `fir/` is its own workspace crate, depended on by both `codegen/` and the REPL session
2. **Incremental** — `FirModule` supports appending new definitions without invalidating existing ones. The REPL adds one definition per input; the compiler adds them all at once
3. **Serializable** — all FIR types derive `Serialize`/`Deserialize` for `--emit fir` and agent tooling
4. **Stable IDs** — functions and types are referenced by `FunctionId` and `TypeId`, not by name strings. This enables caching and incremental recompilation
5. **No Aster-specific concepts** — FIR has no knowledge of traits, protocols, generics, named args, or Aster syntax. It's a flat, typed, imperative IR that any backend could consume

### FIR crate: `fir/`

```
fir/
  Cargo.toml
  src/
    lib.rs           -- public API: FirModule, lower()
    types.rs         -- FirType, FunctionId, TypeId, ClassId
    module.rs        -- FirModule, FirFunction, FirClass
    stmts.rs         -- FirStmt
    exprs.rs         -- FirExpr
    lower.rs         -- Lowerer: Typed AST → FIR
    monomorphize.rs  -- generic instantiation
    closure.rs       -- closure conversion (capture → struct + fn)
    layout.rs        -- class field layout computation
    error_lower.rs   -- throws/!/catch → result tuples or branches
```

### FIR types

```rust
// fir/src/types.rs

use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FunctionId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ClassId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LocalId(pub u32);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FirType {
    I64,
    F64,
    Bool,
    Ptr,            // pointer to heap object (String, List, Class instance)
    Void,
    Never,
    TaggedUnion {   // nullable T?, Result<T, E>
        tag_bits: u8,
        variants: Vec<FirType>,
    },
    Struct(ClassId),
    FnPtr(FunctionId),
}
```

### FIR module

```rust
// fir/src/module.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirModule {
    pub functions: Vec<FirFunction>,
    pub classes: Vec<FirClass>,
    pub entry: Option<FunctionId>,
}

impl FirModule {
    pub fn new() -> Self { ... }

    /// Append a function. Used by REPL (one at a time) and compiler (batch).
    pub fn add_function(&mut self, func: FirFunction) -> FunctionId { ... }

    /// Append a class layout. Returns its ClassId.
    pub fn add_class(&mut self, class: FirClass) -> ClassId { ... }

    /// Look up a function by ID. O(1).
    pub fn get_function(&self, id: FunctionId) -> &FirFunction { ... }

    /// Look up a class by ID. O(1).
    pub fn get_class(&self, id: ClassId) -> &FirClass { ... }

    /// All function IDs added since a given point. Used by REPL to compile
    /// only new definitions without recompiling the whole module.
    pub fn functions_since(&self, mark: usize) -> &[FirFunction] { ... }

    /// Snapshot the current size for incremental tracking.
    pub fn mark(&self) -> usize { self.functions.len() }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirFunction {
    pub id: FunctionId,
    pub name: String,
    pub params: Vec<(String, FirType)>,
    pub ret_type: FirType,
    pub body: Vec<FirStmt>,
    pub is_entry: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirClass {
    pub id: ClassId,
    pub name: String,
    pub fields: Vec<(String, FirType, usize)>,  // name, type, byte offset
    pub methods: Vec<FunctionId>,
    pub vtable: Vec<(String, FunctionId)>,       // method name → impl
    pub size: usize,
    pub alignment: usize,
    pub parent: Option<ClassId>,                 // extends chain
}
```

### FIR statements and expressions

```rust
// fir/src/stmts.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FirStmt {
    Let { name: LocalId, ty: FirType, value: FirExpr },
    Assign { target: FirPlace, value: FirExpr },
    Return(FirExpr),
    If { cond: FirExpr, then_body: Vec<FirStmt>, else_body: Vec<FirStmt> },
    While { cond: FirExpr, body: Vec<FirStmt> },
    Break,
    Continue,
    Expr(FirExpr),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FirPlace {
    Local(LocalId),
    Field { object: Box<FirExpr>, offset: usize },
    Index { list: Box<FirExpr>, index: Box<FirExpr> },
}
```

```rust
// fir/src/exprs.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FirExpr {
    IntLit(i64),
    FloatLit(f64),
    BoolLit(bool),
    StringLit(String),
    NilLit,
    LocalVar(LocalId, FirType),
    BinaryOp { left: Box<FirExpr>, op: BinOp, right: Box<FirExpr>, result_ty: FirType },
    UnaryOp { op: UnaryOp, operand: Box<FirExpr>, result_ty: FirType },
    Call { func: FunctionId, args: Vec<FirExpr>, ret_ty: FirType },
    FieldGet { object: Box<FirExpr>, offset: usize, ty: FirType },
    FieldSet { object: Box<FirExpr>, offset: usize, value: Box<FirExpr> },
    Construct { class: ClassId, fields: Vec<FirExpr>, ty: FirType },
    ListNew { elements: Vec<FirExpr>, elem_ty: FirType },
    ListGet { list: Box<FirExpr>, index: Box<FirExpr>, elem_ty: FirType },
    ListSet { list: Box<FirExpr>, index: Box<FirExpr>, value: Box<FirExpr> },
    /// Tagged union construction (nullable wrap, result wrap)
    TagWrap { tag: u8, value: Box<FirExpr>, ty: FirType },
    /// Tagged union unwrap (nullable unwrap, result unwrap)
    TagUnwrap { value: Box<FirExpr>, expected_tag: u8, ty: FirType },
    /// Tagged union tag check
    TagCheck { value: Box<FirExpr>, tag: u8 },
    /// Runtime function (print, alloc, string ops, etc.)
    RuntimeCall { name: String, args: Vec<FirExpr>, ret_ty: FirType },
}
```

### AST-to-FIR lowering

```rust
// fir/src/lower.rs

pub struct Lowerer {
    type_env: TypeEnv,              // from typechecking
    module: FirModule,
    mono_cache: HashMap<(String, Vec<FirType>), FunctionId>,  // monomorphization cache
    next_local: u32,
}

impl Lowerer {
    pub fn new(type_env: TypeEnv) -> Self { ... }

    /// Lower an entire module (compiler path).
    pub fn lower_module(&mut self, module: &Module) -> &FirModule { ... }

    /// Lower a single statement (REPL path). Appends to existing FirModule.
    pub fn lower_stmt(&mut self, stmt: &Stmt) -> Result<(), LowerError> { ... }

    /// Lower a bare expression (REPL path). Wraps in a temporary function,
    /// returns its FunctionId for immediate execution.
    pub fn lower_repl_expr(&mut self, expr: &Expr, ty: &Type) -> FunctionId { ... }

    fn lower_expr(&mut self, expr: &Expr) -> FirExpr { ... }

    /// Monomorphize a generic function for concrete type arguments.
    /// Returns cached FunctionId if already instantiated.
    fn monomorphize(&mut self, name: &str, type_args: &[FirType]) -> FunctionId { ... }

    /// Convert a closure to a struct (captured vars) + plain function.
    fn lower_closure(&mut self, lambda: &Expr) -> FirExpr { ... }

    /// Lower throws/!/catch to tagged unions and branches.
    fn lower_error_handling(&mut self, expr: &Expr) -> FirExpr { ... }

    /// Compute class field layout (sizes, offsets, alignment).
    fn compute_layout(&self, class: &ClassInfo) -> FirClass { ... }

    /// Take ownership of the built FirModule.
    pub fn finish(self) -> FirModule { ... }
}
```

### How REPL and compiler use FIR differently

**Compiler (batch):**
```rust
// src/main.rs — build command
let mut lowerer = Lowerer::new(type_env);
lowerer.lower_module(&module);
let fir = lowerer.finish();

let mut backend = CraneliftAOT::new();
for func in &fir.functions {
    backend.compile_function(func)?;
}
backend.emit_object("output.o")?;
```

**REPL (incremental):**
```rust
// src/session.rs — eval loop
// Lowerer and FirModule persist across inputs
let mark = self.fir_module.mark();
self.lowerer.lower_stmt(&stmt)?;

// Only compile new functions added since the mark
for func in self.fir_module.functions_since(mark) {
    self.jit.compile_function(func)?;
}
```

The critical difference: the compiler calls `lower_module` once and discards the lowerer. The REPL keeps the lowerer alive, calling `lower_stmt` or `lower_repl_expr` per input, and only compiles the delta.

## Codegen: Cranelift Translation

The `codegen/` crate consumes `FirModule` and produces machine code. It has no knowledge of Aster's AST, types, or syntax — only FIR.

### Codegen crate: `codegen/`

```
codegen/
  Cargo.toml
  src/
    lib.rs           -- public API
    jit.rs           -- JIT backend (REPL, dev)
    aot.rs           -- AOT backend (compiler, production)
    translate.rs     -- FIR → CLIF translation (shared by both backends)
    runtime.rs       -- builtin functions, allocator
    types.rs         -- FirType → Cranelift type mapping
```

### Cranelift dependencies

```toml
# codegen/Cargo.toml
[dependencies]
fir = { path = "../fir" }
cranelift-codegen = "0.115"
cranelift-frontend = "0.115"
cranelift-jit = "0.115"
cranelift-object = "0.115"
cranelift-module = "0.115"
cranelift-native = "0.115"
target-lexicon = "0.13"
```

### JIT backend (REPL + dev)

```rust
// codegen/src/jit.rs

use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::Module;

pub struct CraneliftJIT {
    module: JITModule,
    builder_context: FunctionBuilderContext,
    ctx: codegen::Context,
    compiled: HashMap<FunctionId, *const u8>,
}

impl CraneliftJIT {
    pub fn new() -> Self {
        let mut flag_builder = settings::builder();
        flag_builder.set("opt_level", "speed").unwrap();
        let isa = cranelift_native::builder().unwrap()
            .finish(settings::Flags::new(flag_builder)).unwrap();

        let mut builder = JITBuilder::with_isa(isa, default_libcall_names());
        register_runtime_builtins(&mut builder);

        let module = JITModule::new(builder);
        Self { module, compiled: HashMap::new(), ... }
    }

    /// Compile a single FIR function. Idempotent — skips if already compiled.
    pub fn compile_function(&mut self, func: &FirFunction) -> Result<*const u8, String> {
        if let Some(ptr) = self.compiled.get(&func.id) {
            return Ok(*ptr);
        }
        let ptr = self.do_compile(func)?;
        self.compiled.insert(func.id, ptr);
        Ok(ptr)
    }

    /// Execute a compiled function by ID. Used by REPL to run expressions.
    pub fn call_entry(&self, id: FunctionId) -> i64 {
        let ptr = self.compiled[&id];
        let f: fn() -> i64 = unsafe { std::mem::transmute(ptr) };
        f()
    }

    fn do_compile(&mut self, func: &FirFunction) -> Result<*const u8, String> { ... }
}
```

### AOT backend (compiler)

```rust
// codegen/src/aot.rs

use cranelift_object::{ObjectBuilder, ObjectModule};

pub struct CraneliftAOT {
    module: ObjectModule,
    builder_context: FunctionBuilderContext,
    ctx: codegen::Context,
}

impl CraneliftAOT {
    pub fn new(target: &str) -> Self { ... }

    /// Compile all functions in a FirModule.
    pub fn compile_module(&mut self, fir: &FirModule) -> Result<(), String> {
        for func in &fir.functions {
            self.compile_function(func)?;
        }
        Ok(())
    }

    /// Emit an object file (.o).
    pub fn emit_object(self, path: &str) -> Result<(), String> {
        let object = self.module.finish();
        std::fs::write(path, object.emit()?)?;
        Ok(())
    }

    fn compile_function(&mut self, func: &FirFunction) -> Result<(), String> { ... }
}
```

### Shared FIR → CLIF translation

Both JIT and AOT use the same translator. The only difference is which `Module` impl they pass in.

```rust
// codegen/src/translate.rs

pub struct CraneliftTranslator<'a, M: Module> {
    builder: &'a mut FunctionBuilder<'a>,
    module: &'a mut M,
    locals: HashMap<LocalId, Variable>,
    next_var: usize,
}

impl<'a, M: Module> CraneliftTranslator<'a, M> {
    fn translate_expr(&mut self, expr: &FirExpr) -> Value {
        match expr {
            FirExpr::IntLit(n) => self.builder.ins().iconst(types::I64, *n),
            FirExpr::FloatLit(f) => self.builder.ins().f64const(*f),
            FirExpr::BoolLit(b) => self.builder.ins().iconst(types::I8, *b as i64),

            FirExpr::BinaryOp { left, op, right, result_ty } => {
                let lhs = self.translate_expr(left);
                let rhs = self.translate_expr(right);
                match (op, result_ty) {
                    (BinOp::Add, FirType::I64) => self.builder.ins().iadd(lhs, rhs),
                    (BinOp::Add, FirType::F64) => self.builder.ins().fadd(lhs, rhs),
                    (BinOp::Sub, FirType::I64) => self.builder.ins().isub(lhs, rhs),
                    (BinOp::Mul, FirType::I64) => self.builder.ins().imul(lhs, rhs),
                    (BinOp::Div, FirType::I64) => self.builder.ins().sdiv(lhs, rhs),
                    (BinOp::Eq, _) => self.builder.ins().icmp(IntCC::Equal, lhs, rhs),
                    (BinOp::Lt, FirType::I64) => self.builder.ins().icmp(IntCC::SignedLessThan, lhs, rhs),
                    _ => todo!(),
                }
            }

            FirExpr::Call { func, args, .. } => {
                let callee = self.module.declare_func_in_func(*func, self.builder.func);
                let arg_values: Vec<Value> = args.iter()
                    .map(|a| self.translate_expr(a))
                    .collect();
                let call = self.builder.ins().call(callee, &arg_values);
                self.builder.inst_results(call)[0]
            }

            FirExpr::LocalVar(id, _) => {
                let var = self.locals[id];
                self.builder.use_var(var)
            }

            FirExpr::TagWrap { tag, value, ty } => {
                // Construct tagged union: write tag byte + payload
                ...
            }

            FirExpr::TagUnwrap { value, expected_tag, ty } => {
                // Check tag, extract payload (or trap on mismatch)
                ...
            }

            FirExpr::RuntimeCall { name, args, .. } => {
                // Call registered runtime function by name
                ...
            }

            _ => todo!(),
        }
    }

    fn translate_stmt(&mut self, stmt: &FirStmt) {
        match stmt {
            FirStmt::Let { name, ty, value } => {
                let val = self.translate_expr(value);
                let var = Variable::new(self.next_var);
                self.next_var += 1;
                self.builder.declare_var(var, fir_type_to_cranelift(ty));
                self.builder.def_var(var, val);
                self.locals.insert(*name, var);
            }

            FirStmt::If { cond, then_body, else_body } => {
                let cond_val = self.translate_expr(cond);
                let then_block = self.builder.create_block();
                let else_block = self.builder.create_block();
                let merge_block = self.builder.create_block();

                self.builder.ins().brif(cond_val, then_block, &[], else_block, &[]);

                self.builder.switch_to_block(then_block);
                self.builder.seal_block(then_block);
                for s in then_body { self.translate_stmt(s); }
                self.builder.ins().jump(merge_block, &[]);

                self.builder.switch_to_block(else_block);
                self.builder.seal_block(else_block);
                for s in else_body { self.translate_stmt(s); }
                self.builder.ins().jump(merge_block, &[]);

                self.builder.switch_to_block(merge_block);
                self.builder.seal_block(merge_block);
            }

            FirStmt::While { cond, body } => {
                let header = self.builder.create_block();
                let body_block = self.builder.create_block();
                let exit = self.builder.create_block();

                self.builder.ins().jump(header, &[]);
                self.builder.switch_to_block(header);
                let cond_val = self.translate_expr(cond);
                self.builder.ins().brif(cond_val, body_block, &[], exit, &[]);

                self.builder.switch_to_block(body_block);
                self.builder.seal_block(body_block);
                for s in body { self.translate_stmt(s); }
                self.builder.ins().jump(header, &[]);

                self.builder.seal_block(header);
                self.builder.switch_to_block(exit);
                self.builder.seal_block(exit);
            }

            FirStmt::Return(expr) => {
                let val = self.translate_expr(expr);
                self.builder.ins().return_(&[val]);
            }

            FirStmt::Expr(expr) => { self.translate_expr(expr); }
            FirStmt::Assign { target, value } => { ... }
            FirStmt::Break => { ... }
            FirStmt::Continue => { ... }
        }
    }
}
```

## Runtime

The runtime provides services that can't be expressed in pure Cranelift IR. It lives in `codegen/src/runtime.rs` and is registered with both the JIT and AOT backends.

### Memory management

Start with a simple bump allocator. GC is a future concern.

```rust
// codegen/src/runtime.rs

extern "C" fn aster_alloc(size: usize) -> *mut u8 {
    let layout = std::alloc::Layout::from_size_align(size, 8).unwrap();
    unsafe { std::alloc::alloc(layout) }
}
```

### Object representation

```
Tagged pointer scheme:
- Int:    immediate i64 value (no heap allocation)
- Float:  immediate f64 value (no heap allocation)
- Bool:   immediate i8 value
- String: heap pointer → { len: u64, data: [u8] }
- List:   heap pointer → { len: u64, cap: u64, data: [*mut u8] }
- Class:  heap pointer → { vtable: *const VTable, fields: [...] }
- Nullable T?: tagged union { tag: u8, payload: T | nil }
- Result<T,E>: tagged union { tag: u8, ok: T | err: E }
```

### Builtin functions

```rust
extern "C" fn aster_print(ptr: *const u8, len: usize) {
    let s = unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, len)) };
    println!("{}", s);
}

extern "C" fn aster_string_concat(
    a_ptr: *const u8, a_len: usize,
    b_ptr: *const u8, b_len: usize,
) -> (*const u8, usize) {
    // allocate new string, copy both halves
    ...
}

fn register_runtime_builtins(builder: &mut JITBuilder) {
    builder.symbol("aster_print", aster_print as *const u8);
    builder.symbol("aster_alloc", aster_alloc as *const u8);
    builder.symbol("aster_string_concat", aster_string_concat as *const u8);
    builder.symbol("aster_list_new", aster_list_new as *const u8);
    builder.symbol("aster_list_get", aster_list_get as *const u8);
    builder.symbol("aster_list_push", aster_list_push as *const u8);
}
```

## Type Mapping: Aster → FIR → Cranelift

| Aster Type | FIR Type | Cranelift Type | Representation |
|------------|----------|----------------|----------------|
| Int | I64 | `types::I64` | 64-bit signed integer |
| Float | F64 | `types::F64` | 64-bit IEEE float |
| Bool | Bool | `types::I8` | 0 or 1 |
| String | Ptr | `types::I64` (pointer) | heap `{len, data}` |
| List[T] | Ptr | `types::I64` (pointer) | heap `{len, cap, data}` |
| Class | Struct(ClassId) | `types::I64` (pointer) | heap struct |
| T? | TaggedUnion | stack struct | `{tag: i8, payload}` |
| Nil | Void | — | no value |
| Void | Void | — | no value |
| Never | Never | — | unreachable |
| Function | FnPtr | `types::I64` (pointer) | function pointer |
| Task[T] | Ptr | `types::I64` (pointer) | runtime task handle |

## Entry Points

### Compiler: `asterc build file.aster -o binary`

```rust
fn build(file: &str, output: &str) {
    let source = read_source(file);
    let tokens = lex(&source)?;
    let module = Parser::new(tokens).parse_module("Main")?;
    let mut checker = TypeChecker::new();
    checker.check_module(&module)?;

    // FIR lowering
    let mut lowerer = Lowerer::new(checker.into_type_env());
    lowerer.lower_module(&module);
    let fir = lowerer.finish();

    // AOT compilation
    let mut aot = CraneliftAOT::new("native");
    aot.compile_module(&fir)?;
    aot.emit_object(&format!("{}.o", output))?;

    // Link
    link_object(&format!("{}.o", output), output)?;
}
```

### REPL: `asterc` or `asterc repl`

```rust
fn repl() {
    let mut session = ReplSession::new();
    loop {
        let input = session.read_input()?;
        let tokens = lex(&input)?;
        let stmts = Parser::new(tokens).parse_repl_input()?;
        session.typecheck(&stmts)?;

        // Incremental FIR lowering
        let mark = session.fir_module.mark();
        for stmt in &stmts {
            session.lowerer.lower_stmt(stmt)?;
        }

        // Compile only new functions
        for func in session.fir_module.functions_since(mark) {
            session.jit.compile_function(func)?;
        }

        // If the input was an expression, execute and display
        if let Some(entry) = session.last_expr_function() {
            let result = session.jit.call_entry(entry);
            println!("{}", format_result(result));
        }
    }
}
```

### Dev runner: `asterc run file.aster`

Uses JIT for fast compilation, same as REPL but batch:

```rust
fn run(file: &str) {
    // Same as build but use JIT instead of AOT
    let fir = lower(file)?;
    let mut jit = CraneliftJIT::new();
    for func in &fir.functions {
        jit.compile_function(func)?;
    }
    if let Some(entry) = fir.entry {
        let code = jit.call_entry(entry);
        std::process::exit(code as i32);
    }
}
```

## Agent-Readable IR Dumps

### `--emit` flags

```
asterc file.aster --emit fir              # dump FIR as JSON
asterc file.aster --emit clif             # dump Cranelift IR (CLIF text format)
asterc file.aster --emit clif-json        # dump CLIF as structured JSON
asterc file.aster --emit layout           # dump class layouts (field offsets, sizes)
asterc file.aster --emit all              # everything: tokens + AST + FIR + CLIF + layout
```

### FIR JSON format

```json
{
  "type": "fir",
  "functions": [
    {
      "id": 0,
      "name": "add",
      "params": [
        {"name": "a", "type": "I64"},
        {"name": "b", "type": "I64"}
      ],
      "ret_type": "I64",
      "body": [
        {
          "stmt": "Return",
          "expr": {
            "expr": "BinaryOp",
            "op": "Add",
            "left": {"expr": "LocalVar", "id": 0, "type": "I64"},
            "right": {"expr": "LocalVar", "id": 1, "type": "I64"},
            "result_type": "I64"
          }
        }
      ]
    }
  ],
  "classes": [],
  "entry": 0
}
```

### Class layout dump

```json
{
  "type": "layout",
  "classes": [
    {
      "name": "User",
      "id": 0,
      "size": 32,
      "alignment": 8,
      "fields": [
        {"name": "name", "type": "Ptr", "offset": 8, "size": 8},
        {"name": "age", "type": "I64", "offset": 16, "size": 8},
        {"name": "active", "type": "Bool", "offset": 24, "size": 1}
      ],
      "vtable_offset": 0,
      "vtable_entries": ["User.to_string", "User.greet"],
      "parent": null
    }
  ]
}
```

### Compilation result envelope

```json
{
  "success": true,
  "file": "examples/03_simple_function.aster",
  "stages": {
    "lex": { "ok": true, "token_count": 47, "duration_ms": 0.2 },
    "parse": { "ok": true, "duration_ms": 0.8 },
    "typecheck": { "ok": true, "duration_ms": 1.2 },
    "lower": { "ok": true, "function_count": 3, "class_count": 1, "duration_ms": 0.5 },
    "codegen": { "ok": true, "duration_ms": 2.1 }
  },
  "diagnostics": [],
  "fir": { ... },
  "clif": { ... },
  "layout": { ... }
}
```

## Workspace Structure (after codegen)

```
asterc/
  Cargo.toml          -- workspace: lexer, ast, parser, typecheck, fir, codegen, src
  lexer/              -- tokens
  ast/                -- AST types, Type, Span, Diagnostic
  parser/             -- source → AST
  typecheck/          -- AST → Typed AST
  fir/                -- Typed AST → FIR (shared by compiler + REPL)
  codegen/            -- FIR → machine code (Cranelift JIT + AOT)
  src/                -- CLI: build, run, repl commands
  tests/
  examples/
```

## Incremental Milestones

Build in this order. Each milestone produces a testable artifact:

1. **FIR crate + integer lowering:** Lower `def add(a: Int, b: Int) -> Int` to FIR. Verify with `--emit fir`. No Cranelift yet.
2. **Cranelift integers:** JIT-compile FIR integer functions. Execute `add(1, 2)` and get `3`.
3. **Control flow:** If/else, while, break/continue in FIR + Cranelift.
4. **Strings:** Heap-allocated strings, `print`, concatenation. First runtime builtins.
5. **Functions:** First-class functions, closures (environment structs).
6. **Classes:** Construction, field access, method dispatch, vtables.
7. **Lists:** Creation, indexing, iteration, Iterable vocabulary.
8. **Error handling:** Tagged unions for `throws`/`!`/`.catch`/`T?`.
9. **Generics:** Monomorphization cache, multiple instantiations.
10. **AOT backend:** Swap JIT for `cranelift-object`, produce binaries.
11. **Async:** Task handles, runtime scheduler (most complex — may be its own plan).

## Dependency Chain

```
diagnostics (spans, structured errors, Serialize)     ← done
    │
    v
fir/ Phase 1 (types, module, lowering)               ← new crate
    │
    ├──────────────────────────┐
    v                          v
codegen/ JIT                codegen/ AOT
(REPL, `asterc run`)       (`asterc build`)
    │                          │
    v                          v
repl.md                    linker integration
(interactive REPL)         (native binaries)
```
