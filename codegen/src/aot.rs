use std::collections::HashMap;

use cranelift_codegen::Context;
use cranelift_codegen::ir::types;
use cranelift_codegen::ir::{AbiParam, InstBuilder, StackSlotData, StackSlotKind};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{DataDescription, Linkage, Module, default_libcall_names};
use cranelift_object::{ObjectBuilder, ObjectModule};

use fir::module::{FirFunction, FirModule};
use fir::types::FunctionId;

use crate::translate::{self, TranslationState};
use crate::types::fir_type_to_clif;

pub struct CraneliftAOT {
    module: ObjectModule,
    builder_context: FunctionBuilderContext,
    ctx: Context,
    declared: HashMap<FunctionId, cranelift_module::FuncId>,
    runtime_declared: HashMap<String, cranelift_module::FuncId>,
}

impl CraneliftAOT {
    /// Create an AOT compiler with a specific build configuration.
    pub fn with_config(config: &crate::config::BuildConfig) -> Self {
        let mut flag_builder = settings::builder();
        flag_builder
            .set("opt_level", config.cranelift_opt_level())
            .unwrap();
        flag_builder.set("is_pic", "true").unwrap();
        let isa_builder = cranelift_native::builder().unwrap_or_else(|msg| {
            panic!("host machine is not supported: {}", msg);
        });
        let isa = isa_builder
            .finish(settings::Flags::new(flag_builder))
            .unwrap();

        let builder = ObjectBuilder::new(isa, "aster_module", default_libcall_names()).unwrap();
        let module = ObjectModule::new(builder);
        let ctx = module.make_context();

        Self {
            module,
            builder_context: FunctionBuilderContext::new(),
            ctx,
            declared: HashMap::new(),
            runtime_declared: HashMap::new(),
        }
    }

    pub fn new() -> Self {
        Self::with_config(&crate::config::BuildConfig::release())
    }

    /// Compile all functions in a FirModule to an object file.
    pub fn compile_module(&mut self, fir: &FirModule) -> Result<(), String> {
        // Phase 1: Declare all functions (skip placeholders from out-of-order insertion)
        for func in &fir.functions {
            if func.name.is_empty() {
                continue;
            }
            self.declare_function(func)?;
        }

        // Phase 2: Declare runtime functions (imported — will be resolved at link time)
        self.declare_runtime_functions()?;

        // Phase 3: Compile all functions
        for func in &fir.functions {
            if func.name.is_empty() {
                continue;
            }
            self.compile_function(func)?;
        }

        Ok(())
    }

    /// Emit the compiled object file bytes.
    pub fn emit_object(self) -> Result<Vec<u8>, String> {
        let product = self.module.finish();
        product.emit().map_err(|e| e.to_string())
    }

    /// Emit the compiled object file to a path.
    pub fn emit_object_to_file(self, path: &str) -> Result<(), String> {
        let bytes = self.emit_object()?;
        std::fs::write(path, bytes).map_err(|e| e.to_string())
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

        // Entry point (main) is exported as "aster_main" to avoid
        // conflict with C's main; everything else is local.
        let (linkage, export_name) = if func.is_entry {
            (Linkage::Export, "aster_main".to_string())
        } else {
            (Linkage::Local, func.name.clone())
        };

        let func_id = self
            .module
            .declare_function(&export_name, linkage, &sig)
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

        let string_data = self.collect_and_register_strings(func)?;

        {
            let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut self.builder_context);

            let entry_block = builder.create_block();
            builder.append_block_params_for_function_params(entry_block);
            builder.switch_to_block(entry_block);
            builder.seal_block(entry_block);

            let mut func_refs = HashMap::new();
            for (&fir_id, &cf_id) in &self.declared {
                let func_ref = self.module.declare_func_in_func(cf_id, builder.func);
                func_refs.insert(fir_id, func_ref);
            }

            let mut runtime_refs = HashMap::new();
            for (name, &cf_id) in &self.runtime_declared {
                let func_ref = self.module.declare_func_in_func(cf_id, builder.func);
                runtime_refs.insert(name.clone(), func_ref);
            }

            let mut string_gv_map = HashMap::new();
            for (s, (data_id, len)) in &string_data {
                let gv = self.module.declare_data_in_func(*data_id, builder.func);
                string_gv_map.insert(s.clone(), (gv, *len));
            }

            let mut state = TranslationState::new(func_refs, runtime_refs, string_gv_map);
            translate::declare_params(&mut builder, &mut state, func, entry_block);

            // Count ALL Ptr-typed locals (params + body lets) for GC root frame
            let param_roots: usize = func
                .params
                .iter()
                .filter(|(_, ty)| matches!(ty, fir::FirType::Ptr | fir::FirType::Struct(_)))
                .count();
            let body_roots = crate::jit::count_body_gc_roots(&func.body);
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

                // Pre-assign root slots for body Let bindings
                translate::assign_body_gc_root_slots(&func.body, &mut state, &mut root_idx);
            }

            // Store GC frame info for pop-before-return and root slot updates
            state.gc_pop_ref = state.runtime_refs.get("aster_gc_pop_roots").copied();
            state.gc_frame_slot = Some(gc_frame_slot);

            translate::translate_body(&mut builder, &mut state, &func.body);

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

    fn collect_and_register_strings(
        &mut self,
        func: &FirFunction,
    ) -> Result<HashMap<String, (cranelift_module::DataId, usize)>, String> {
        let mut strings = std::collections::HashSet::new();
        crate::jit::collect_string_lits_stmts(&func.body, &mut strings);

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
}

impl Default for CraneliftAOT {
    fn default() -> Self {
        Self::new()
    }
}
