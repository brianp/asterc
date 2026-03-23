use std::collections::HashMap;

use cranelift_codegen::Context;
use cranelift_codegen::ir::types;
use cranelift_codegen::ir::{self, AbiParam, InstBuilder, MemFlags, StackSlotData, StackSlotKind};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{DataDescription, Linkage, Module, default_libcall_names};

use fir::exprs::FirExpr;
use fir::module::{FirFunction, FirModule};
use fir::types::{FirType, FunctionId};

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
    async_entry_declared: HashMap<FunctionId, cranelift_module::FuncId>,
    function_param_types: HashMap<FunctionId, Vec<FirType>>,
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
            async_entry_declared: HashMap::new(),
            function_param_types: HashMap::new(),
        }
    }

    pub fn new() -> Self {
        Self::with_config(&crate::config::BuildConfig::release())
    }

    /// Compile an entire FIR module.
    pub fn compile_module(&mut self, fir: &FirModule) -> Result<(), String> {
        self.function_param_types = fir
            .functions
            .iter()
            .filter(|func| !func.name.is_empty())
            .map(|func| {
                (
                    func.id,
                    func.params
                        .iter()
                        .map(|(_, ty)| ty.clone())
                        .collect::<Vec<_>>(),
                )
            })
            .collect();

        // Phase 1: Declare all functions (skip placeholders from out-of-order insertion)
        for func in &fir.functions {
            if func.name.is_empty() {
                continue;
            }
            self.declare_function(func)?;
        }

        // Phase 2: Declare runtime functions
        self.declare_runtime_functions()?;

        // Phase 3: Declare async entry shims.
        self.declare_async_entry_shims(fir)?;

        // Phase 4: Compile all functions
        for func in &fir.functions {
            if func.name.is_empty() {
                continue;
            }
            self.compile_function(func)?;
        }

        // Phase 5: Compile async entry shims.
        self.compile_async_entry_shims(fir)?;

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

    fn declare_async_entry_shims(&mut self, fir: &FirModule) -> Result<(), String> {
        for func in &fir.functions {
            if func.name.is_empty() {
                continue;
            }
            let mut sig = self.module.make_signature();
            sig.params.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I64));
            let func_id = self
                .module
                .declare_function(&format!("{}__async_entry", func.name), Linkage::Local, &sig)
                .map_err(|e| e.to_string())?;
            self.async_entry_declared.insert(func.id, func_id);
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

            let mut async_entry_refs = HashMap::new();
            for (&fir_id, &cf_id) in &self.async_entry_declared {
                let func_ref = self.module.declare_func_in_func(cf_id, builder.func);
                async_entry_refs.insert(fir_id, func_ref);
            }

            // Build string_data global value map
            let mut string_gv_map = HashMap::new();
            for (s, (data_id, len)) in &string_data {
                let gv = self.module.declare_data_in_func(*data_id, builder.func);
                string_gv_map.insert(s.clone(), (gv, *len));
            }

            // Translate
            let mut state = TranslationState::new(
                self.function_param_types.clone(),
                func_refs,
                async_entry_refs,
                runtime_refs,
                string_gv_map,
            );
            translate::declare_params(&mut builder, &mut state, func, entry_block);

            // Count ALL Ptr-typed locals (params + body lets) for GC root frame
            let param_roots: usize = func
                .params
                .iter()
                .filter(|(_, ty)| matches!(ty, fir::FirType::Ptr | fir::FirType::Struct(_)))
                .count();
            let body_roots = count_body_gc_roots(&func.body);
            let total_gc_roots = param_roots + body_roots;

            // Allocate shadow stack frame: [prev: i64][count: i64][roots: i64 * N]
            let gc_frame_size = (2 + total_gc_roots) * 8;
            let gc_frame_slot = builder.create_sized_stack_slot(StackSlotData::new(
                StackSlotKind::ExplicitSlot,
                gc_frame_size as u32,
                0,
            ));
            let gc_frame_addr = builder.ins().stack_addr(types::I64, gc_frame_slot, 0);

            // Push GC frame with total root count
            if let Some(&push_ref) = state.runtime_refs.get("aster_gc_push_roots") {
                let count_val = builder.ins().iconst(types::I64, total_gc_roots as i64);
                builder.ins().call(push_ref, &[gc_frame_addr, count_val]);

                // Store Ptr-typed params into initial root slots
                let mut root_idx: i32 = 0;
                for (i, (_, ty)) in func.params.iter().enumerate() {
                    if matches!(ty, fir::FirType::Ptr | fir::FirType::Struct(_)) {
                        let local_id = fir::LocalId(i as u32);
                        let slot_offset = (2 + root_idx) * 8;
                        state.gc_root_slots.insert(local_id, slot_offset);
                        if let Some(&var) = state.locals.get(&local_id) {
                            let val = builder.use_var(var);
                            builder.ins().stack_store(val, gc_frame_slot, slot_offset);
                        }
                        root_idx += 1;
                    }
                }

                // Pre-assign root slots for body Let bindings (they'll be filled during translation)
                translate::assign_body_gc_root_slots(&func.body, &mut state, &mut root_idx);
            }

            // Store GC frame info for pop-before-return and root slot updates
            state.gc_pop_ref = state.runtime_refs.get("aster_gc_pop_roots").copied();
            state.gc_frame_slot = Some(gc_frame_slot);

            translate::translate_body(&mut builder, &mut state, &func.body);

            // Default return if body didn't terminate
            if !state.terminated {
                // Pop GC frame before return
                if let Some(pop_ref) = state.gc_pop_ref {
                    builder.ins().call(pop_ref, &[]);
                }
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

    fn compile_async_entry_shims(&mut self, fir: &FirModule) -> Result<(), String> {
        for func in &fir.functions {
            if func.name.is_empty() {
                continue;
            }
            self.compile_async_entry_shim(func)?;
        }
        Ok(())
    }

    fn compile_async_entry_shim(&mut self, func: &FirFunction) -> Result<(), String> {
        let shim_id = *self
            .async_entry_declared
            .get(&func.id)
            .ok_or_else(|| format!("missing async entry shim for {}", func.name))?;
        let target_id = *self
            .declared
            .get(&func.id)
            .ok_or_else(|| format!("missing function declaration for {}", func.name))?;

        self.ctx.func.signature.params.clear();
        self.ctx.func.signature.returns.clear();
        self.ctx
            .func
            .signature
            .params
            .push(AbiParam::new(types::I64));
        self.ctx
            .func
            .signature
            .returns
            .push(AbiParam::new(types::I64));

        let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut self.builder_context);
        let entry_block = builder.create_block();
        builder.append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);

        let packet = builder.block_params(entry_block)[0];
        let target_ref = self.module.declare_func_in_func(target_id, builder.func);
        let mut args = Vec::with_capacity(func.params.len());
        for (index, (_, ty)) in func.params.iter().enumerate() {
            let raw = builder.ins().load(
                types::I64,
                MemFlags::new(),
                packet,
                ir::immediates::Offset32::new((index * 8) as i32),
            );
            args.push(unpack_shim_arg(&mut builder, raw, ty));
        }

        let call = builder.ins().call(target_ref, &args);
        let value = builder.inst_results(call)[0];
        let packed = pack_shim_result(&mut builder, value, &func.ret_type);
        builder.ins().return_(&[packed]);
        builder.finalize();

        self.module
            .define_function(shim_id, &mut self.ctx)
            .map_err(|e| format!("compile error in {}__async_entry: {}", func.name, e))?;
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

// --- GC root counting ---

/// Count Ptr/Struct-typed Let bindings in a function body (recursive).
/// These need root slots in the GC shadow stack frame.
pub(crate) fn count_body_gc_roots(stmts: &[fir::stmts::FirStmt]) -> usize {
    use fir::stmts::FirStmt;
    let mut count = 0;
    for stmt in stmts {
        match stmt {
            FirStmt::Let { ty, .. } => {
                if matches!(ty, fir::FirType::Ptr | fir::FirType::Struct(_)) {
                    count += 1;
                }
            }
            FirStmt::If {
                then_body,
                else_body,
                ..
            } => {
                count += count_body_gc_roots(then_body);
                count += count_body_gc_roots(else_body);
            }
            FirStmt::While {
                body, increment, ..
            } => {
                count += count_body_gc_roots(body);
                count += count_body_gc_roots(increment);
            }
            _ => {}
        }
    }
    count
}

// --- String literal collection ---

pub(crate) fn collect_string_lits_stmts(
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
            FirStmt::While {
                cond,
                body,
                increment,
            } => {
                collect_string_lits_expr(cond, strings);
                collect_string_lits_stmts(body, strings);
                collect_string_lits_stmts(increment, strings);
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
                    fir::stmts::FirPlace::MapIndex { map, key } => {
                        collect_string_lits_expr(map, strings);
                        collect_string_lits_expr(key, strings);
                    }
                    fir::stmts::FirPlace::Local(_) => {}
                }
            }
            FirStmt::Break | FirStmt::Continue => {}
        }
    }
}

pub(crate) fn collect_string_lits_expr(
    expr: &FirExpr,
    strings: &mut std::collections::HashSet<String>,
) {
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
        FirExpr::Call { args, .. }
        | FirExpr::Spawn { args, .. }
        | FirExpr::BlockOn { args, .. }
        | FirExpr::RuntimeCall { args, .. } => {
            for arg in args {
                collect_string_lits_expr(arg, strings);
            }
        }
        FirExpr::ResolveTask { task, .. } => {
            collect_string_lits_expr(task, strings);
        }
        FirExpr::CancelTask { task } | FirExpr::WaitCancel { task } => {
            collect_string_lits_expr(task, strings);
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
        FirExpr::IntToFloat(inner) | FirExpr::Bitcast { value: inner, .. } => {
            collect_string_lits_expr(inner, strings);
        }
        FirExpr::IntLit(_)
        | FirExpr::FloatLit(_)
        | FirExpr::BoolLit(_)
        | FirExpr::Safepoint
        | FirExpr::NilLit
        | FirExpr::LocalVar(_, _)
        | FirExpr::GlobalFunc(_) => {}
    }
}

fn unpack_shim_arg(builder: &mut FunctionBuilder, raw: ir::Value, ty: &FirType) -> ir::Value {
    match ty {
        FirType::F64 => builder.ins().bitcast(types::F64, MemFlags::new(), raw),
        FirType::Bool => builder.ins().ireduce(types::I8, raw),
        _ => raw,
    }
}

fn pack_shim_result(builder: &mut FunctionBuilder, value: ir::Value, ty: &FirType) -> ir::Value {
    match ty {
        FirType::F64 => builder.ins().bitcast(types::I64, MemFlags::new(), value),
        FirType::Bool => builder.ins().uextend(types::I64, value),
        _ => value,
    }
}
