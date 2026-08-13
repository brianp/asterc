use cranelift_codegen::ir::{self, AbiParam, InstBuilder};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{DataDescription, DataId, Linkage, Module, default_libcall_names};
use cranelift_object::{ObjectBuilder, ObjectModule};

use fir::module::FirModule;

use crate::compile_shared::CompileState;
use crate::runtime::stacktrace::{AOT_SYM_ENTRY, AOT_SYM_HEADER};

pub struct CraneliftAOT {
    state: CompileState<ObjectModule>,
}

/// One symbolization entry while building the AOT symbol table: the Cranelift
/// function id, its FIR function (for name/file/def_line), the finalized code
/// size, and the PC-offset -> line table.
type SymEntry<'a> = (
    cranelift_module::FuncId,
    &'a fir::module::FirFunction,
    u32,
    Vec<(u32, u32)>,
);

impl CraneliftAOT {
    pub fn with_config(config: &crate::config::BuildConfig) -> Self {
        let isa_builder = cranelift_native::builder().unwrap_or_else(|msg| {
            panic!("host machine is not supported: {}", msg);
        });
        let isa = isa_builder.finish(config.cranelift_flags(true)).unwrap();

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

        self.state
            .compile_declared_functions_with_contexts(&fir.functions, &fir.eval_contexts)?;

        // Emit the self-contained symbolization data section (function address
        // relocations + name/file/line tables), then the C `main` wrapper that
        // registers it with the runtime before running the entry point.
        let symtab = self.emit_symbol_table(fir)?;

        // Emit a C-style `main` wrapper that calls `aster_main` and
        // truncates the i64 result to i32.  This lives in the object file
        // itself so the runtime staticlib no longer needs its own `main`.
        self.emit_main_wrapper(symtab)
    }

    /// Emit the AOT symbolization data section: one entry per user function
    /// (address via relocation, size, def line, name/file, PC-offset->line
    /// table) plus a string region and a line region. Returns the `DataId` so
    /// the `main` wrapper can hand its address to `aster_register_symbols`.
    /// Returns `None` when there are no functions to symbolize.
    fn emit_symbol_table(&mut self, fir: &FirModule) -> Result<Option<DataId>, String> {
        // Collect the functions we can symbolize (declared + compiled).
        let mut entries: Vec<SymEntry<'_>> = Vec::new();
        for func in &fir.functions {
            if func.name.is_empty() {
                continue;
            }
            if let (Some(&fid), Some(info)) = (
                self.state.declared.get(&func.id),
                self.state.symbol_info.get(&func.id),
            ) {
                entries.push((fid, func, info.size, info.lines.clone()));
            }
        }
        if entries.is_empty() {
            return Ok(None);
        }

        let count = entries.len();
        let entries_bytes = count * AOT_SYM_ENTRY;
        let string_region_start = AOT_SYM_HEADER + entries_bytes;

        // Build the string region and remember each entry's (off, len).
        let mut strings: Vec<u8> = Vec::new();
        let mut str_spans: Vec<(u32, u32, u32, u32)> = Vec::new(); // name_off,len,file_off,len
        for (_, func, _, _) in &entries {
            let name_off = (string_region_start + strings.len()) as u32;
            strings.extend_from_slice(func.name.as_bytes());
            let name_len = func.name.len() as u32;
            let file_off = (string_region_start + strings.len()) as u32;
            strings.extend_from_slice(func.file.as_bytes());
            let file_len = func.file.len() as u32;
            str_spans.push((name_off, name_len, file_off, file_len));
        }

        let lines_region_start = string_region_start + strings.len();
        // Build the lines region and remember each entry's (off, count).
        let mut lines_bytes: Vec<u8> = Vec::new();
        let mut line_spans: Vec<(u32, u32)> = Vec::new();
        for (_, _, _, lines) in &entries {
            let off = (lines_region_start + lines_bytes.len()) as u32;
            for (code_off, line) in lines {
                lines_bytes.extend_from_slice(&code_off.to_le_bytes());
                lines_bytes.extend_from_slice(&line.to_le_bytes());
            }
            line_spans.push((off, lines.len() as u32));
        }

        let total = lines_region_start + lines_bytes.len();
        let mut blob = vec![0u8; total];
        blob[0..4].copy_from_slice(&(count as u32).to_le_bytes());
        // header bytes 4..8 stay zero (padding)

        let mut desc = DataDescription::new();
        // Reserve the reloc targets before defining contents.
        let mut func_refs = Vec::with_capacity(count);
        for (fid, _, _, _) in &entries {
            func_refs.push(self.state.module.declare_func_in_data(*fid, &mut desc));
        }

        for (i, (_, func, size, _)) in entries.iter().enumerate() {
            let base = AOT_SYM_HEADER + i * AOT_SYM_ENTRY;
            // func_addr (base..base+8) left zero; filled by the relocation below.
            blob[base + 8..base + 12].copy_from_slice(&size.to_le_bytes());
            blob[base + 12..base + 16].copy_from_slice(&func.def_line.to_le_bytes());
            let (name_off, name_len, file_off, file_len) = str_spans[i];
            blob[base + 16..base + 20].copy_from_slice(&name_off.to_le_bytes());
            blob[base + 20..base + 24].copy_from_slice(&name_len.to_le_bytes());
            blob[base + 24..base + 28].copy_from_slice(&file_off.to_le_bytes());
            blob[base + 28..base + 32].copy_from_slice(&file_len.to_le_bytes());
            let (lines_off, lines_count) = line_spans[i];
            blob[base + 32..base + 36].copy_from_slice(&lines_off.to_le_bytes());
            blob[base + 36..base + 40].copy_from_slice(&lines_count.to_le_bytes());
        }
        blob[string_region_start..lines_region_start].copy_from_slice(&strings);
        blob[lines_region_start..total].copy_from_slice(&lines_bytes);

        desc.define(blob.into_boxed_slice());
        for (i, fref) in func_refs.into_iter().enumerate() {
            let base = AOT_SYM_HEADER + i * AOT_SYM_ENTRY;
            desc.write_function_addr(base as u32, fref);
        }

        let data_id = self
            .state
            .module
            .declare_data("__aster_symtab", Linkage::Local, false, false)
            .map_err(|e| e.to_string())?;
        self.state
            .module
            .define_data(data_id, &desc)
            .map_err(|e| e.to_string())?;
        Ok(Some(data_id))
    }

    /// Generate: `int main(int argc, char **argv) { return (int)aster_main(); }`
    fn emit_main_wrapper(&mut self, symtab: Option<DataId>) -> Result<(), String> {
        let ptr_type = self.state.module.target_config().pointer_type();

        // Declare aster_main() -> i64 (already emitted above)
        let mut aster_main_sig = self.state.module.make_signature();
        aster_main_sig.returns.push(AbiParam::new(ir::types::I64));
        let aster_main_id = self
            .state
            .module
            .declare_function("aster_main", Linkage::Local, &aster_main_sig)
            .map_err(|e| e.to_string())?;

        // Declare aster_runtime_init() — records the main thread's native stack
        // bounds so the first throw from `main` walks within recorded bounds.
        let init_sig = self.state.module.make_signature();
        let runtime_init_id = self
            .state
            .module
            .declare_function("aster_runtime_init", Linkage::Import, &init_sig)
            .map_err(|e| e.to_string())?;

        // Declare aster_register_symbols(ptr) — walks the emitted symbol data
        // section into the runtime symbol table for stack-trace symbolization.
        let mut regsym_sig = self.state.module.make_signature();
        regsym_sig.params.push(AbiParam::new(ptr_type));
        let register_symbols_id = self
            .state
            .module
            .declare_function("aster_register_symbols", Linkage::Import, &regsym_sig)
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

            // Initialize the runtime (records main-thread stack bounds) before
            // running the Aster entry point.
            let init_callee = self
                .state
                .module
                .declare_func_in_func(runtime_init_id, builder.func);
            builder.ins().call(init_callee, &[]);

            // Register the symbolization table so throws resolve to real frames.
            if let Some(symtab) = symtab {
                let gv = self.state.module.declare_data_in_func(symtab, builder.func);
                let addr = builder.ins().global_value(ptr_type, gv);
                let regsym_callee = self
                    .state
                    .module
                    .declare_func_in_func(register_symbols_id, builder.func);
                builder.ins().call(regsym_callee, &[addr]);
            }

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
