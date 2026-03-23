use cranelift_codegen::settings::{self, Configurable};
use cranelift_module::{Linkage, default_libcall_names};
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
        self.state.declare_runtime_functions()?;
        self.state.declare_async_entry_shims(&fir.functions)?;

        for func in &fir.functions {
            if !func.name.is_empty() {
                self.state.compile_function(func)?;
            }
        }
        for func in &fir.functions {
            if !func.name.is_empty() {
                self.state.compile_async_entry_shim(func)?;
            }
        }

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
