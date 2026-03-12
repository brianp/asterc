use std::collections::HashMap;

use cranelift_codegen::Context;
use cranelift_codegen::ir::types;
use cranelift_codegen::ir::{AbiParam, InstBuilder};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{DataDescription, Linkage, Module, default_libcall_names};

use fir::exprs::FirExpr;
use fir::module::{FirFunction, FirModule};
use fir::types::FunctionId;

use crate::runtime::register_runtime_builtins;
use crate::translate::{self, TranslationState};
use crate::types::fir_type_to_clif;

pub struct CraneliftJIT {
    module: JITModule,
    builder_context: FunctionBuilderContext,
    ctx: Context,
    /// Maps FunctionId → compiled function pointer.
    compiled: HashMap<FunctionId, *const u8>,
    /// Maps FunctionId → cranelift FuncId (declared in the module).
    declared: HashMap<FunctionId, cranelift_module::FuncId>,
    /// Maps runtime function names → cranelift FuncId.
    runtime_declared: HashMap<String, cranelift_module::FuncId>,
}

impl CraneliftJIT {
    /// Create a JIT with a specific build configuration.
    pub fn with_config(config: &crate::config::BuildConfig) -> Self {
        let mut flag_builder = settings::builder();
        flag_builder
            .set("opt_level", config.cranelift_opt_level())
            .unwrap();
        flag_builder.set("is_pic", "false").unwrap();
        let isa_builder = cranelift_native::builder().unwrap_or_else(|msg| {
            panic!("host machine is not supported: {}", msg);
        });
        let isa = isa_builder
            .finish(settings::Flags::new(flag_builder))
            .unwrap();

        let mut builder = JITBuilder::with_isa(isa, default_libcall_names());
        register_runtime_builtins(&mut builder);

        let module = JITModule::new(builder);
        let ctx = module.make_context();

        Self {
            module,
            builder_context: FunctionBuilderContext::new(),
            ctx,
            compiled: HashMap::new(),
            declared: HashMap::new(),
            runtime_declared: HashMap::new(),
        }
    }

    pub fn new() -> Self {
        Self::with_config(&crate::config::BuildConfig::release())
    }

    /// Compile an entire FIR module.
    pub fn compile_module(&mut self, fir: &FirModule) -> Result<(), String> {
        // Phase 1: Declare all functions (skip placeholders from out-of-order insertion)
        for func in &fir.functions {
            if func.name.is_empty() {
                continue;
            }
            self.declare_function(func)?;
        }

        // Phase 2: Declare runtime functions
        self.declare_runtime_functions()?;

        // Phase 3: Compile all functions
        for func in &fir.functions {
            if func.name.is_empty() {
                continue;
            }
            self.compile_function(func)?;
        }

        // Finalize
        self.module
            .finalize_definitions()
            .map_err(|e| e.to_string())?;

        // Get function pointers
        for func in &fir.functions {
            if let Some(&func_id) = self.declared.get(&func.id) {
                let ptr = self.module.get_finalized_function(func_id);
                self.compiled.insert(func.id, ptr);
            }
        }

        Ok(())
    }

    fn declare_function(&mut self, func: &FirFunction) -> Result<(), String> {
        if self.declared.contains_key(&func.id) {
            return Ok(());
        }

        let mut sig = self.module.make_signature();
        for (_, ty) in &func.params {
            sig.params.push(AbiParam::new(fir_type_to_clif(ty)));
        }
        sig.returns
            .push(AbiParam::new(fir_type_to_clif(&func.ret_type)));

        let func_id = self
            .module
            .declare_function(&func.name, Linkage::Local, &sig)
            .map_err(|e| e.to_string())?;

        self.declared.insert(func.id, func_id);
        Ok(())
    }

    fn declare_runtime_functions(&mut self) -> Result<(), String> {
        for &(name, params, ret) in crate::runtime_sigs::RUNTIME_SIGS {
            let mut sig = self.module.make_signature();
            for &p in params {
                sig.params.push(AbiParam::new(p));
            }
            if let Some(r) = ret {
                sig.returns.push(AbiParam::new(r));
            }

            let func_id = self
                .module
                .declare_function(name, Linkage::Import, &sig)
                .map_err(|e| e.to_string())?;
            self.runtime_declared.insert(name.to_string(), func_id);
        }

        Ok(())
    }

    fn compile_function(&mut self, func: &FirFunction) -> Result<(), String> {
        let clif_func_id = *self
            .declared
            .get(&func.id)
            .ok_or_else(|| format!("codegen: undeclared function {:?} ({})", func.id, func.name))?;

        // Build signature
        self.ctx.func.signature.params.clear();
        self.ctx.func.signature.returns.clear();
        for (_, ty) in &func.params {
            self.ctx
                .func
                .signature
                .params
                .push(AbiParam::new(fir_type_to_clif(ty)));
        }
        self.ctx
            .func
            .signature
            .returns
            .push(AbiParam::new(fir_type_to_clif(&func.ret_type)));

        // Register string literal data sections
        let string_data = self.collect_and_register_strings(func)?;

        // Build
        {
            let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut self.builder_context);

            let entry_block = builder.create_block();
            builder.append_block_params_for_function_params(entry_block);
            builder.switch_to_block(entry_block);
            builder.seal_block(entry_block);

            // Build func_refs map
            let mut func_refs = HashMap::new();
            for (&fir_id, &cf_id) in &self.declared {
                let func_ref = self.module.declare_func_in_func(cf_id, builder.func);
                func_refs.insert(fir_id, func_ref);
            }

            // Build runtime_refs map
            let mut runtime_refs = HashMap::new();
            for (name, &cf_id) in &self.runtime_declared {
                let func_ref = self.module.declare_func_in_func(cf_id, builder.func);
                runtime_refs.insert(name.clone(), func_ref);
            }

            // Build string_data global value map
            let mut string_gv_map = HashMap::new();
            for (s, (data_id, len)) in &string_data {
                let gv = self.module.declare_data_in_func(*data_id, builder.func);
                string_gv_map.insert(s.clone(), (gv, *len));
            }

            // Translate
            let mut state = TranslationState::new(func_refs, runtime_refs, string_gv_map);
            translate::declare_params(&mut builder, &mut state, func, entry_block);
            translate::translate_body(&mut builder, &mut state, &func.body);

            // Default return if body didn't terminate
            if !state.terminated {
                let ret_ty = fir_type_to_clif(&func.ret_type);
                let default_val = if ret_ty == types::F64 {
                    builder.ins().f64const(0.0)
                } else {
                    builder.ins().iconst(ret_ty, 0)
                };
                builder.ins().return_(&[default_val]);
            }

            builder.finalize();
        }

        self.module
            .define_function(clif_func_id, &mut self.ctx)
            .map_err(|e| format!("compile error in {}: {}", func.name, e))?;

        self.module.clear_context(&mut self.ctx);
        Ok(())
    }

    fn collect_and_register_strings(
        &mut self,
        func: &FirFunction,
    ) -> Result<HashMap<String, (cranelift_module::DataId, usize)>, String> {
        let mut strings = std::collections::HashSet::new();
        collect_string_lits_stmts(&func.body, &mut strings);

        let mut result = HashMap::new();
        for s in strings {
            let data_id = self
                .module
                .declare_data(
                    &format!("str_{}_{}", func.name, result.len()),
                    Linkage::Local,
                    false,
                    false,
                )
                .map_err(|e| e.to_string())?;

            let mut desc = DataDescription::new();
            desc.define(s.as_bytes().to_vec().into_boxed_slice());

            self.module
                .define_data(data_id, &desc)
                .map_err(|e| e.to_string())?;

            let len = s.len();
            result.insert(s, (data_id, len));
        }

        Ok(result)
    }

    /// Execute a compiled function by ID (no args, returns i64).
    pub fn call_i64(&self, id: FunctionId) -> i64 {
        let ptr = self
            .compiled
            .get(&id)
            .unwrap_or_else(|| panic!("codegen: function {:?} not compiled", id));
        let f: fn() -> i64 = unsafe { std::mem::transmute(*ptr) };
        f()
    }

    /// Execute with one i64 arg.
    pub fn call_i64_i64(&self, id: FunctionId, arg: i64) -> i64 {
        let ptr = self
            .compiled
            .get(&id)
            .unwrap_or_else(|| panic!("codegen: function {:?} not compiled", id));
        let f: fn(i64) -> i64 = unsafe { std::mem::transmute(*ptr) };
        f(arg)
    }

    /// Execute with two i64 args.
    pub fn call_i64_i64_i64(&self, id: FunctionId, a: i64, b: i64) -> i64 {
        let ptr = self
            .compiled
            .get(&id)
            .unwrap_or_else(|| panic!("codegen: function {:?} not compiled", id));
        let f: fn(i64, i64) -> i64 = unsafe { std::mem::transmute(*ptr) };
        f(a, b)
    }

    /// Get raw function pointer.
    pub fn get_function_ptr(&self, id: FunctionId) -> Option<*const u8> {
        self.compiled.get(&id).copied()
    }
}

impl Default for CraneliftJIT {
    fn default() -> Self {
        Self::new()
    }
}

// --- String literal collection ---

pub fn collect_string_lits_stmts(
    stmts: &[fir::stmts::FirStmt],
    strings: &mut std::collections::HashSet<String>,
) {
    use fir::stmts::FirStmt;
    for stmt in stmts {
        match stmt {
            FirStmt::Expr(e) | FirStmt::Return(e) | FirStmt::Let { value: e, .. } => {
                collect_string_lits_expr(e, strings);
            }
            FirStmt::If {
                cond,
                then_body,
                else_body,
            } => {
                collect_string_lits_expr(cond, strings);
                collect_string_lits_stmts(then_body, strings);
                collect_string_lits_stmts(else_body, strings);
            }
            FirStmt::While { cond, body } => {
                collect_string_lits_expr(cond, strings);
                collect_string_lits_stmts(body, strings);
            }
            FirStmt::Assign { target, value } => {
                collect_string_lits_expr(value, strings);
                match target {
                    fir::stmts::FirPlace::Field { object, .. } => {
                        collect_string_lits_expr(object, strings);
                    }
                    fir::stmts::FirPlace::Index { list, index } => {
                        collect_string_lits_expr(list, strings);
                        collect_string_lits_expr(index, strings);
                    }
                    fir::stmts::FirPlace::Local(_) => {}
                }
            }
            FirStmt::Break | FirStmt::Continue => {}
        }
    }
}

pub fn collect_string_lits_expr(expr: &FirExpr, strings: &mut std::collections::HashSet<String>) {
    match expr {
        FirExpr::StringLit(s) => {
            strings.insert(s.clone());
        }
        FirExpr::BinaryOp { left, right, .. } => {
            collect_string_lits_expr(left, strings);
            collect_string_lits_expr(right, strings);
        }
        FirExpr::UnaryOp { operand, .. } => {
            collect_string_lits_expr(operand, strings);
        }
        FirExpr::Call { args, .. } | FirExpr::RuntimeCall { args, .. } => {
            for arg in args {
                collect_string_lits_expr(arg, strings);
            }
        }
        FirExpr::ListNew { elements, .. } => {
            for elem in elements {
                collect_string_lits_expr(elem, strings);
            }
        }
        FirExpr::Construct { fields, .. } => {
            for f in fields {
                collect_string_lits_expr(f, strings);
            }
        }
        FirExpr::ClosureCreate { env, .. } => {
            collect_string_lits_expr(env, strings);
        }
        FirExpr::ClosureCall { closure, args, .. } => {
            collect_string_lits_expr(closure, strings);
            for arg in args {
                collect_string_lits_expr(arg, strings);
            }
        }
        FirExpr::EnvLoad { env, .. } => {
            collect_string_lits_expr(env, strings);
        }
        FirExpr::FieldGet { object, .. } => {
            collect_string_lits_expr(object, strings);
        }
        FirExpr::FieldSet { object, value, .. } => {
            collect_string_lits_expr(object, strings);
            collect_string_lits_expr(value, strings);
        }
        FirExpr::ListGet { list, index, .. } => {
            collect_string_lits_expr(list, strings);
            collect_string_lits_expr(index, strings);
        }
        FirExpr::ListSet {
            list, index, value, ..
        } => {
            collect_string_lits_expr(list, strings);
            collect_string_lits_expr(index, strings);
            collect_string_lits_expr(value, strings);
        }
        FirExpr::TagWrap { value, .. } | FirExpr::TagUnwrap { value, .. } => {
            collect_string_lits_expr(value, strings);
        }
        FirExpr::TagCheck { value, .. } => {
            collect_string_lits_expr(value, strings);
        }
        FirExpr::IntLit(_)
        | FirExpr::FloatLit(_)
        | FirExpr::BoolLit(_)
        | FirExpr::NilLit
        | FirExpr::LocalVar(_, _)
        | FirExpr::GlobalFunc(_) => {}
    }
}
