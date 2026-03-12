use std::collections::HashMap;

use cranelift_codegen::Context;
use cranelift_codegen::ir::types;
use cranelift_codegen::ir::{self, AbiParam, InstBuilder};
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
        let common: Vec<(&str, Vec<ir::Type>, Option<ir::Type>)> = vec![
            ("aster_alloc", vec![types::I64], Some(types::I64)),
            ("aster_print_str", vec![types::I64], None),
            ("aster_print_int", vec![types::I64], None),
            ("aster_print_float", vec![types::F64], None),
            ("aster_print_bool", vec![types::I8], None),
            (
                "aster_string_new",
                vec![types::I64, types::I64],
                Some(types::I64),
            ),
            (
                "aster_string_concat",
                vec![types::I64, types::I64],
                Some(types::I64),
            ),
            ("aster_string_len", vec![types::I64], Some(types::I64)),
            ("aster_list_new", vec![types::I64], Some(types::I64)),
            (
                "aster_list_get",
                vec![types::I64, types::I64],
                Some(types::I64),
            ),
            (
                "aster_list_set",
                vec![types::I64, types::I64, types::I64],
                None,
            ),
            (
                "aster_list_push",
                vec![types::I64, types::I64],
                Some(types::I64),
            ),
            ("aster_list_len", vec![types::I64], Some(types::I64)),
            ("aster_class_alloc", vec![types::I64], Some(types::I64)),
            (
                "aster_pow_int",
                vec![types::I64, types::I64],
                Some(types::I64),
            ),
            ("aster_int_to_string", vec![types::I64], Some(types::I64)),
            ("aster_float_to_string", vec![types::F64], Some(types::I64)),
            ("aster_bool_to_string", vec![types::I8], Some(types::I64)),
            ("aster_map_new", vec![types::I64], Some(types::I64)),
            (
                "aster_map_set",
                vec![types::I64, types::I64, types::I64],
                Some(types::I64),
            ),
            (
                "aster_map_get",
                vec![types::I64, types::I64],
                Some(types::I64),
            ),
            ("aster_error_set", vec![], None),
            ("aster_error_check", vec![], Some(types::I8)),
            ("aster_panic", vec![], None),
        ];

        for (name, params, ret) in &common {
            let mut sig = self.module.make_signature();
            for &p in params {
                sig.params.push(AbiParam::new(p));
            }
            if let Some(r) = ret {
                sig.returns.push(AbiParam::new(*r));
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
            translate::translate_body(&mut builder, &mut state, &func.body);

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
