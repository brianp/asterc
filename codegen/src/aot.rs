use cranelift_codegen::ir::{self, AbiParam, InstBuilder};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{Linkage, Module, default_libcall_names};
use cranelift_object::{ObjectBuilder, ObjectModule};

use fir::module::FirModule;

use crate::compile_shared::CompileState;

pub struct CraneliftAOT {
    state: CompileState<ObjectModule>,
}

impl CraneliftAOT {
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

        Self {
            state: CompileState::new(module),
        }
    }

    pub fn new() -> Self {
        Self::with_config(&crate::config::BuildConfig::release())
    }

    pub fn compile_module(&mut self, fir: &FirModule) -> Result<(), String> {
        self.state.build_function_param_types(&fir.functions);

        for func in &fir.functions {
            if !func.name.is_empty() {
                let (linkage, export_name) = if func.is_entry {
                    (Linkage::Export, "aster_main".to_string())
                } else {
                    (Linkage::Local, func.name.clone())
                };
                self.state
                    .declare_function_with_linkage(func, &export_name, linkage)?;
            }
        }

        self.state.compile_declared_functions(&fir.functions)?;

        // Emit a C-style `main` wrapper that calls `aster_main` and
        // truncates the i64 result to i32.  This lives in the object file
        // itself so the runtime staticlib no longer needs its own `main`.
        self.emit_main_wrapper()
    }

    /// Generate: `int main(int argc, char **argv) { return (int)aster_main(); }`
    fn emit_main_wrapper(&mut self) -> Result<(), String> {
        let ptr_type = self.state.module.target_config().pointer_type();

        // Declare aster_main() -> i64 (already emitted above)
        let mut aster_main_sig = self.state.module.make_signature();
        aster_main_sig.returns.push(AbiParam::new(ir::types::I64));
        let aster_main_id = self
            .state
            .module
            .declare_function("aster_main", Linkage::Local, &aster_main_sig)
            .map_err(|e| e.to_string())?;

        // Declare main(i32, ptr) -> i32
        let mut main_sig = self.state.module.make_signature();
        main_sig.params.push(AbiParam::new(ir::types::I32));
        main_sig.params.push(AbiParam::new(ptr_type));
        main_sig.returns.push(AbiParam::new(ir::types::I32));
        let main_id = self
            .state
            .module
            .declare_function("main", Linkage::Export, &main_sig)
            .map_err(|e| e.to_string())?;

        // Build function body
        self.state.ctx.func.signature = main_sig;
        let mut fbctx = FunctionBuilderContext::new();
        {
            let mut builder = FunctionBuilder::new(&mut self.state.ctx.func, &mut fbctx);
            let block = builder.create_block();
            builder.append_block_params_for_function_params(block);
            builder.switch_to_block(block);
            builder.seal_block(block);

            let callee = self
                .state
                .module
                .declare_func_in_func(aster_main_id, builder.func);
            let call = builder.ins().call(callee, &[]);
            let result_i64 = builder.inst_results(call)[0];
            let result_i32 = builder.ins().ireduce(ir::types::I32, result_i64);
            builder.ins().return_(&[result_i32]);
            builder.finalize();
        }

        self.state
            .module
            .define_function(main_id, &mut self.state.ctx)
            .map_err(|e| e.to_string())?;
        self.state.module.clear_context(&mut self.state.ctx);
        Ok(())
    }

    pub fn emit_object(self) -> Result<Vec<u8>, String> {
        let product = self.state.module.finish();
        product.emit().map_err(|e| e.to_string())
    }

    pub fn emit_object_to_file(self, path: &str) -> Result<(), String> {
        let bytes = self.emit_object()?;
        std::fs::write(path, bytes).map_err(|e| e.to_string())
    }
}

impl Default for CraneliftAOT {
    fn default() -> Self {
        Self::new()
    }
}
