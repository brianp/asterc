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
    /// When true, register compiled function pointers in the global
    /// host function registry after finalization. Only set for the
    /// "host" JIT, not for nested eval JITs.
    register_host_functions: bool,
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
            register_host_functions: false,
        }
    }

    pub fn new() -> Self {
        Self::with_config(&crate::config::BuildConfig::release())
    }

    /// Create a JIT that registers compiled function pointers in the global
    /// host function registry after compilation. Used for the top-level host
    /// JIT so nested evaluate() calls can resolve host function addresses.
    pub fn new_host() -> Self {
        let mut jit = Self::new();
        jit.register_host_functions = true;
        jit
    }

    /// Create a JIT with extra symbols pre-registered for linking.
    /// Used by the eval pipeline to make host function pointers available
    /// to JIT-compiled evaluated code.
    pub fn with_extra_symbols(symbols: &[(&str, *const u8)]) -> Self {
        let config = crate::config::BuildConfig::release();
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
        // Register extra host function symbols
        let owned: Vec<(&str, *const u8)> = symbols.to_vec();
        builder.symbols(owned);

        let module = JITModule::new(builder);

        Self {
            state: CompileState::new(module),
            compiled: HashMap::new(),
            register_host_functions: false,
        }
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

        // Register compiled function pointers in the global host function
        // registry so nested evaluate() calls can resolve them.
        if self.register_host_functions {
            crate::host_function_registry::register_batch(fir.functions.iter().filter_map(
                |func| {
                    if func.name.is_empty() {
                        return None;
                    }
                    self.compiled
                        .get(&func.id)
                        .map(|&ptr| (func.name.clone(), ptr))
                },
            ));
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

    /// Declare host functions from a context snapshot as imported functions
    /// in the JIT module, so they appear in `runtime_refs` during codegen.
    /// Must be called before `compile_module`.
    pub fn declare_extra_functions(
        &mut self,
        snapshot: &ast::ContextSnapshot,
    ) -> Result<(), crate::eval_pipeline::RuntimeEvalError> {
        use cranelift_codegen::ir::AbiParam;
        use cranelift_module::Module;

        // Collect (qualified_name, param_count) for each host function
        let mut host_fns: Vec<(
            String,
            Vec<cranelift_codegen::ir::Type>,
            Option<cranelift_codegen::ir::Type>,
        )> = Vec::new();

        if let Some(class_name) = &snapshot.current_class
            && let Some(ci) = &snapshot.class_info
        {
            for (method_name, method_ty) in &ci.methods {
                let qualified = format!("{}.{}", class_name, method_name);
                if snapshot.function_pointers.contains_key(&qualified) {
                    let (params, ret) = ast_func_type_to_clif(method_ty, true);
                    host_fns.push((qualified, params, ret));
                }
            }
        }

        for (func_name, func_ty) in &snapshot.functions {
            if snapshot.function_pointers.contains_key(func_name) {
                let (params, ret) = ast_func_type_to_clif(func_ty, false);
                host_fns.push((func_name.clone(), params, ret));
            }
        }

        for (name, params, ret) in host_fns {
            let mut sig = self.state.module.make_signature();
            for p in &params {
                sig.params.push(AbiParam::new(*p));
            }
            if let Some(r) = ret {
                sig.returns.push(AbiParam::new(r));
            }
            let func_id = self
                .state
                .module
                .declare_function(&name, Linkage::Import, &sig)
                .map_err(|e| crate::eval_pipeline::RuntimeEvalError {
                    kind: "codegen",
                    message: format!("failed to declare host function {}: {}", name, e),
                })?;
            self.state.runtime_declared.insert(name, func_id);
        }

        Ok(())
    }
}

impl Default for CraneliftJIT {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert an `ast::Type` function signature to Cranelift IR types.
/// If `has_self` is true, prepend an I64 (pointer) parameter for `self`.
fn ast_func_type_to_clif(
    ty: &ast::Type,
    has_self: bool,
) -> (
    Vec<cranelift_codegen::ir::Type>,
    Option<cranelift_codegen::ir::Type>,
) {
    use cranelift_codegen::ir::types;

    let (param_types, ret_type) = match ty {
        ast::Type::Function { params, ret, .. } => (params.as_slice(), ret.as_ref()),
        _ => return (vec![], None),
    };

    let mut clif_params = Vec::new();
    if has_self {
        clif_params.push(types::I64); // self pointer
    }
    for pt in param_types {
        clif_params.push(ast_type_to_clif_scalar(pt));
    }

    let clif_ret = match ret_type {
        ast::Type::Void => None,
        other => Some(ast_type_to_clif_scalar(other)),
    };

    (clif_params, clif_ret)
}

/// Map a single AST type to its Cranelift IR scalar type.
fn ast_type_to_clif_scalar(ty: &ast::Type) -> cranelift_codegen::ir::Type {
    use cranelift_codegen::ir::types;
    match ty {
        ast::Type::Int => types::I64,
        ast::Type::Float => types::F64,
        ast::Type::Bool => types::I8,
        ast::Type::Void => types::I64, // shouldn't happen for params, but safe default
        // All heap types (String, List, Custom classes, etc.) are pointers
        _ => types::I64,
    }
}
