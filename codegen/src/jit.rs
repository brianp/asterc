use std::collections::HashMap;

use cranelift_codegen::settings::{self, Configurable};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{Linkage, default_libcall_names};

use fir::module::FirModule;
use fir::types::FunctionId;

use crate::compile_shared::CompileState;
use crate::runtime::register_runtime_builtins;

pub struct CraneliftJIT {
    state: CompileState<JITModule>,
    compiled: HashMap<FunctionId, *const u8>,
}

impl CraneliftJIT {
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

        Self {
            state: CompileState::new(module),
            compiled: HashMap::new(),
        }
    }

    pub fn new() -> Self {
        Self::with_config(&crate::config::BuildConfig::release())
    }

    pub fn compile_module(&mut self, fir: &FirModule) -> Result<(), String> {
        self.state.build_function_param_types(&fir.functions);

        for func in &fir.functions {
            if !func.name.is_empty() {
                self.state
                    .declare_function_with_linkage(func, &func.name, Linkage::Local)?;
            }
        }

        self.state
            .compile_declared_functions_with_contexts(&fir.functions, &fir.eval_contexts)?;

        self.state
            .module
            .finalize_definitions()
            .map_err(|e| e.to_string())?;

        for func in &fir.functions {
            if let Some(&func_id) = self.state.declared.get(&func.id) {
                let ptr = self.state.module.get_finalized_function(func_id);
                self.compiled.insert(func.id, ptr);
            }
        }

        Ok(())
    }

    pub fn call_i64(&self, id: FunctionId) -> i64 {
        let ptr = self
            .compiled
            .get(&id)
            .unwrap_or_else(|| panic!("codegen: function {:?} not compiled", id));
        let f: fn() -> i64 = unsafe { std::mem::transmute(*ptr) };
        f()
    }

    pub fn call_i64_i64(&self, id: FunctionId, arg: i64) -> i64 {
        let ptr = self
            .compiled
            .get(&id)
            .unwrap_or_else(|| panic!("codegen: function {:?} not compiled", id));
        let f: fn(i64) -> i64 = unsafe { std::mem::transmute(*ptr) };
        f(arg)
    }

    pub fn call_i64_i64_i64(&self, id: FunctionId, a: i64, b: i64) -> i64 {
        let ptr = self
            .compiled
            .get(&id)
            .unwrap_or_else(|| panic!("codegen: function {:?} not compiled", id));
        let f: fn(i64, i64) -> i64 = unsafe { std::mem::transmute(*ptr) };
        f(a, b)
    }

    pub fn get_function_ptr(&self, id: FunctionId) -> Option<*const u8> {
        self.compiled.get(&id).copied()
    }
}

impl Default for CraneliftJIT {
    fn default() -> Self {
        Self::new()
    }
}
