use std::collections::HashMap;

use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::immediates::Offset32;
use cranelift_codegen::ir::instructions::BlockArg;
use cranelift_codegen::ir::types;
use cranelift_codegen::ir::{self, AbiParam, InstBuilder, Value};
use cranelift_frontend::{FunctionBuilder, Variable};

use fir::exprs::{BinOp, FirExpr, UnaryOp};
use fir::module::FirFunction;
use fir::stmts::{FirPlace, FirStmt};
use fir::types::{FirType, FunctionId, LocalId};

use crate::types::{fir_type_to_clif, is_float};

/// State tracked during translation of a single function.
pub struct TranslationState {
    pub locals: HashMap<LocalId, Variable>,
    pub local_types: HashMap<LocalId, FirType>,
    pub function_params: HashMap<FunctionId, Vec<FirType>>,
    pub func_refs: HashMap<FunctionId, ir::FuncRef>,
    pub async_entry_refs: HashMap<FunctionId, ir::FuncRef>,
    pub runtime_refs: HashMap<String, ir::FuncRef>,
    pub string_data: HashMap<String, (ir::GlobalValue, usize)>,
    pub loop_exit: Option<ir::Block>,
    pub loop_header: Option<ir::Block>,
    /// Target for `continue` — points to latch block (increment) in for-loops,
    /// or header block in plain while loops.
    pub loop_continue: Option<ir::Block>,
    /// True if the current block has been terminated (return/break/continue/jump).
    pub terminated: bool,
    /// FuncRef for aster_gc_pop_roots — emitted before every return.
    pub gc_pop_ref: Option<ir::FuncRef>,
    /// GC root slot tracking: maps LocalId → offset in the GC frame's root array.
    /// Only populated for Ptr/Struct-typed locals.
    pub gc_root_slots: HashMap<LocalId, i32>,
    /// Stack slot for the GC shadow stack frame (if any).
    pub gc_frame_slot: Option<ir::StackSlot>,
}

impl TranslationState {
    pub fn new(
        function_params: HashMap<FunctionId, Vec<FirType>>,
        func_refs: HashMap<FunctionId, ir::FuncRef>,
        async_entry_refs: HashMap<FunctionId, ir::FuncRef>,
        runtime_refs: HashMap<String, ir::FuncRef>,
        string_data: HashMap<String, (ir::GlobalValue, usize)>,
    ) -> Self {
        Self {
            locals: HashMap::new(),
            local_types: HashMap::new(),
            function_params,
            func_refs,
            async_entry_refs,
            runtime_refs,
            string_data,
            loop_exit: None,
            loop_header: None,
            loop_continue: None,
            terminated: false,
            gc_pop_ref: None,
            gc_root_slots: HashMap::new(),
            gc_frame_slot: None,
        }
    }
}

fn pack_async_arg(
    builder: &mut FunctionBuilder,
    state: &mut TranslationState,
    arg: &FirExpr,
    ty: &FirType,
) -> Value {
    let value = translate_expr(builder, state, arg);
    match ty {
        FirType::F64 => builder
            .ins()
            .bitcast(types::I64, ir::MemFlags::new(), value),
        FirType::Bool => builder.ins().uextend(types::I64, value),
        _ => value,
    }
}

fn unpack_async_result(builder: &mut FunctionBuilder, raw: Value, ty: &FirType) -> Value {
    match ty {
        FirType::F64 => builder.ins().bitcast(types::F64, ir::MemFlags::new(), raw),
        FirType::Bool => builder.ins().ireduce(types::I8, raw),
        _ => raw,
    }
}

fn lower_async_call_packet(
    builder: &mut FunctionBuilder,
    state: &mut TranslationState,
    args: &[FirExpr],
    param_types: &[FirType],
) -> Value {
    let size = (args.len() * 8) as i64;
    let packet_ptr = if let Some(&alloc_ref) = state.runtime_refs.get("aster_alloc") {
        let alloc_size = builder.ins().iconst(types::I64, size.max(1));
        let call = builder.ins().call(alloc_ref, &[alloc_size]);
        builder.inst_results(call)[0]
    } else {
        builder.ins().iconst(types::I64, 0)
    };

    for (index, (arg, ty)) in args.iter().zip(param_types.iter()).enumerate() {
        let packed = pack_async_arg(builder, state, arg, ty);
        let offset = i32::try_from(index * 8).expect("async call packet offset fits in i32");
        builder.ins().store(
            ir::MemFlags::new(),
            packed,
            packet_ptr,
            Offset32::new(offset),
        );
    }
    packet_ptr
}

/// Pre-assign GC root slot offsets for Ptr-typed Let bindings in a function body.
/// Called before translation so that when translate_stmt encounters a Let, the
/// root slot offset is already in state.gc_root_slots.
pub fn assign_body_gc_root_slots(
    stmts: &[FirStmt],
    state: &mut TranslationState,
    next_root_idx: &mut i32,
) {
    for stmt in stmts {
        match stmt {
            FirStmt::Let { name, ty, .. } => {
                if matches!(ty, FirType::Ptr | FirType::Struct(_)) {
                    let slot_offset = (2 + *next_root_idx) * 8;
                    state.gc_root_slots.insert(*name, slot_offset);
                    *next_root_idx += 1;
                }
            }
            FirStmt::If {
                then_body,
                else_body,
                ..
            } => {
                assign_body_gc_root_slots(then_body, state, next_root_idx);
                assign_body_gc_root_slots(else_body, state, next_root_idx);
            }
            FirStmt::While {
                body, increment, ..
            } => {
                assign_body_gc_root_slots(body, state, next_root_idx);
                assign_body_gc_root_slots(increment, state, next_root_idx);
            }
            _ => {}
        }
    }
}

/// Declare function parameters as variables.
pub fn declare_params(
    builder: &mut FunctionBuilder,
    state: &mut TranslationState,
    func: &FirFunction,
    entry_block: ir::Block,
) {
    let params = builder.block_params(entry_block).to_vec();
    for (i, (_name, ty)) in func.params.iter().enumerate() {
        let clif_ty = fir_type_to_clif(ty);
        let var = builder.declare_var(clif_ty);
        builder.def_var(var, params[i]);
        state.locals.insert(LocalId(i as u32), var);
        state.local_types.insert(LocalId(i as u32), ty.clone());
    }
}

pub fn translate_body(
    builder: &mut FunctionBuilder,
    state: &mut TranslationState,
    body: &[FirStmt],
) {
    for stmt in body {
        if state.terminated {
            break;
        }
        translate_stmt(builder, state, stmt);
    }
}

fn translate_stmt(builder: &mut FunctionBuilder, state: &mut TranslationState, stmt: &FirStmt) {
    match stmt {
        FirStmt::Let { name, ty, value } => {
            let val = translate_expr(builder, state, value);
            let clif_ty = fir_type_to_clif(ty);
            let var = builder.declare_var(clif_ty);
            builder.def_var(var, val);
            state.locals.insert(*name, var);
            state.local_types.insert(*name, ty.clone());

            // Update GC root slot if this is a Ptr-typed local
            if let (Some(slot), Some(&offset)) =
                (state.gc_frame_slot, state.gc_root_slots.get(name))
            {
                builder.ins().stack_store(val, slot, offset);
            }
        }

        FirStmt::Assign { target, value } => {
            let val = translate_expr(builder, state, value);
            match target {
                FirPlace::Local(id) => {
                    let var = *state
                        .locals
                        .get(id)
                        .unwrap_or_else(|| panic!("codegen: assign to undefined local {:?}", id));
                    builder.def_var(var, val);

                    // Update GC root slot on reassignment
                    if let (Some(slot), Some(&offset)) =
                        (state.gc_frame_slot, state.gc_root_slots.get(id))
                    {
                        builder.ins().stack_store(val, slot, offset);
                    }
                }
                FirPlace::Field { object, offset } => {
                    let obj_ptr = translate_expr(builder, state, object);
                    let off =
                        i32::try_from(*offset).expect("codegen: field offset exceeds i32::MAX");
                    builder
                        .ins()
                        .store(ir::MemFlags::new(), val, obj_ptr, Offset32::new(off));
                }
                FirPlace::Index { list, index } => {
                    let list_val = translate_expr(builder, state, list);
                    let idx_val = translate_expr(builder, state, index);
                    if let Some(&func_ref) = state.runtime_refs.get("aster_list_set") {
                        builder.ins().call(func_ref, &[list_val, idx_val, val]);
                    }
                }
                FirPlace::MapIndex { map, key } => {
                    let map_val = translate_expr(builder, state, map);
                    let key_val = translate_expr(builder, state, key);
                    if let Some(&func_ref) = state.runtime_refs.get("aster_map_set") {
                        builder.ins().call(func_ref, &[map_val, key_val, val]);
                    }
                }
            }
        }

        FirStmt::Return(expr) => {
            let val = translate_expr(builder, state, expr);
            // Pop GC shadow stack frame before returning
            if let Some(pop_ref) = state.gc_pop_ref {
                builder.ins().call(pop_ref, &[]);
            }
            builder.ins().return_(&[val]);
            state.terminated = true;
        }

        FirStmt::If {
            cond,
            then_body,
            else_body,
        } => {
            let cond_val = translate_expr(builder, state, cond);
            let branch_cond = normalize_branch_condition(builder, cond_val);
            let then_block = builder.create_block();
            let else_block = builder.create_block();
            let merge_block = builder.create_block();

            builder
                .ins()
                .brif(branch_cond, then_block, &[], else_block, &[]);

            builder.switch_to_block(then_block);
            builder.seal_block(then_block);
            state.terminated = false;
            translate_body(builder, state, then_body);
            let then_terminated = state.terminated;
            if !then_terminated {
                builder.ins().jump(merge_block, &[]);
            }

            builder.switch_to_block(else_block);
            builder.seal_block(else_block);
            state.terminated = false;
            translate_body(builder, state, else_body);
            let else_terminated = state.terminated;
            if !else_terminated {
                builder.ins().jump(merge_block, &[]);
            }

            builder.switch_to_block(merge_block);
            builder.seal_block(merge_block);
            // If both branches terminated, the merge block is unreachable
            state.terminated = then_terminated && else_terminated;
        }

        FirStmt::While {
            cond,
            body,
            increment,
        } => {
            let header_block = builder.create_block();
            let body_block = builder.create_block();
            let exit_block = builder.create_block();

            let saved_exit = state.loop_exit.replace(exit_block);
            let saved_header = state.loop_header.replace(header_block);

            // If there's an increment (for-loops), create a latch block so that
            // `continue` runs the increment before jumping back to the header.
            let latch_block = if !increment.is_empty() {
                Some(builder.create_block())
            } else {
                None
            };
            let continue_target = latch_block.unwrap_or(header_block);
            let saved_continue = state.loop_continue.replace(continue_target);

            builder.ins().jump(header_block, &[]);

            builder.switch_to_block(header_block);
            let cond_val = translate_expr(builder, state, cond);
            let branch_cond = normalize_branch_condition(builder, cond_val);
            builder
                .ins()
                .brif(branch_cond, body_block, &[], exit_block, &[]);

            builder.switch_to_block(body_block);
            builder.seal_block(body_block);
            state.terminated = false;
            translate_body(builder, state, body);
            if !state.terminated {
                builder.ins().jump(continue_target, &[]);
            }

            // Emit the latch block (increment) if present
            if let Some(latch) = latch_block {
                builder.switch_to_block(latch);
                builder.seal_block(latch);
                state.terminated = false;
                translate_body(builder, state, &increment);
                if !state.terminated {
                    builder.ins().jump(header_block, &[]);
                }
            }

            builder.seal_block(header_block);
            builder.switch_to_block(exit_block);
            builder.seal_block(exit_block);

            state.loop_exit = saved_exit;
            state.loop_header = saved_header;
            state.loop_continue = saved_continue;
            state.terminated = false;
        }

        FirStmt::Break => {
            if let Some(exit) = state.loop_exit {
                builder.ins().jump(exit, &[]);
                state.terminated = true;
            }
        }

        FirStmt::Continue => {
            if let Some(target) = state.loop_continue {
                builder.ins().jump(target, &[]);
                state.terminated = true;
            }
        }

        FirStmt::Expr(expr) => {
            translate_expr(builder, state, expr);
        }
    }
}

fn translate_expr(
    builder: &mut FunctionBuilder,
    state: &mut TranslationState,
    expr: &FirExpr,
) -> Value {
    match expr {
        FirExpr::IntLit(n) => builder.ins().iconst(types::I64, *n),
        FirExpr::FloatLit(f) => builder.ins().f64const(*f),
        FirExpr::BoolLit(b) => builder.ins().iconst(types::I8, *b as i64),

        FirExpr::StringLit(s) => {
            if let Some(&(gv, len)) = state.string_data.get(s.as_str()) {
                let ptr = builder.ins().global_value(types::I64, gv);
                let len_val = builder.ins().iconst(types::I64, len as i64);
                if let Some(&func_ref) = state.runtime_refs.get("aster_string_new") {
                    let call = builder.ins().call(func_ref, &[ptr, len_val]);
                    builder.inst_results(call)[0]
                } else {
                    ptr
                }
            } else {
                builder.ins().iconst(types::I64, 0)
            }
        }

        FirExpr::NilLit => builder.ins().iconst(types::I64, 0),

        FirExpr::LocalVar(id, _ty) => {
            let var = *state
                .locals
                .get(id)
                .unwrap_or_else(|| panic!("codegen: use of undefined local {:?}", id));
            builder.use_var(var)
        }

        FirExpr::BinaryOp {
            left,
            op: op @ (BinOp::And | BinOp::Or),
            right,
            ..
        } => translate_short_circuit(builder, state, op, left, right),

        FirExpr::BinaryOp {
            left, op, right, ..
        } => {
            let lhs = translate_expr(builder, state, left);
            let rhs = translate_expr(builder, state, right);
            translate_binop(builder, state, op, lhs, rhs, left)
        }

        FirExpr::UnaryOp { op, operand, .. } => {
            let val = translate_expr(builder, state, operand);
            translate_unaryop(builder, state, op, val, operand)
        }

        FirExpr::Call { func, args, .. } => {
            if let Some(&func_ref) = state.func_refs.get(func) {
                let arg_vals: Vec<Value> = args
                    .iter()
                    .map(|a| translate_expr(builder, state, a))
                    .collect();
                let call = builder.ins().call(func_ref, &arg_vals);
                builder.inst_results(call)[0]
            } else {
                builder.ins().iconst(types::I64, 0)
            }
        }

        FirExpr::Spawn {
            func, args, scope, ..
        } => {
            let entry_ref = state.async_entry_refs.get(func).copied();
            let param_types = state.function_params.get(func).cloned();
            let spawn_ref = state.runtime_refs.get("aster_task_spawn").copied();
            if let (Some(entry_ref), Some(param_types), Some(spawn_ref)) =
                (entry_ref, param_types, spawn_ref)
            {
                let packet_ptr = lower_async_call_packet(builder, state, args, &param_types);
                let entry_ptr = builder.ins().func_addr(types::I64, entry_ref);
                let scope_ptr = scope
                    .and_then(|scope_id| state.locals.get(&scope_id).copied())
                    .map(|var| builder.use_var(var))
                    .unwrap_or_else(|| builder.ins().iconst(types::I64, 0));
                let call = builder
                    .ins()
                    .call(spawn_ref, &[entry_ptr, packet_ptr, scope_ptr]);
                builder.inst_results(call)[0]
            } else {
                builder.ins().iconst(types::I64, 0)
            }
        }

        FirExpr::BlockOn { func, args, ret_ty } => {
            let entry_ref = state.async_entry_refs.get(func).copied();
            let param_types = state.function_params.get(func).cloned();
            let block_on_ref = state.runtime_refs.get("aster_task_block_on").copied();
            if let (Some(entry_ref), Some(param_types), Some(block_on_ref)) =
                (entry_ref, param_types, block_on_ref)
            {
                let packet_ptr = lower_async_call_packet(builder, state, args, &param_types);
                let entry_ptr = builder.ins().func_addr(types::I64, entry_ref);
                let call = builder.ins().call(block_on_ref, &[entry_ptr, packet_ptr]);
                let raw = builder.inst_results(call)[0];
                unpack_async_result(builder, raw, ret_ty)
            } else {
                builder.ins().iconst(types::I64, 0)
            }
        }

        FirExpr::ResolveTask { task, ret_ty } => {
            let task_val = translate_expr(builder, state, task);
            let resolve_name = task_resolve_runtime_name(ret_ty);
            if let Some(&resolve_ref) = state.runtime_refs.get(resolve_name) {
                let call = builder.ins().call(resolve_ref, &[task_val]);
                let results = builder.inst_results(call);
                if !results.is_empty() {
                    results[0]
                } else {
                    builder.ins().iconst(types::I64, 0)
                }
            } else {
                builder.ins().iconst(types::I64, 0)
            }
        }

        FirExpr::CancelTask { task } => {
            let task_val = translate_expr(builder, state, task);
            if let Some(&cancel_ref) = state.runtime_refs.get("aster_task_cancel") {
                builder.ins().call(cancel_ref, &[task_val]);
            }
            builder.ins().iconst(types::I64, 0)
        }

        FirExpr::WaitCancel { task } => {
            let task_val = translate_expr(builder, state, task);
            if let Some(&wait_ref) = state.runtime_refs.get("aster_task_wait_cancel") {
                builder.ins().call(wait_ref, &[task_val]);
            }
            builder.ins().iconst(types::I64, 0)
        }

        FirExpr::Safepoint => {
            if let Some(&safepoint_ref) = state.runtime_refs.get("aster_safepoint") {
                builder.ins().call(safepoint_ref, &[]);
            }
            builder.ins().iconst(types::I64, 0)
        }

        FirExpr::RuntimeCall { name, args, .. } => {
            translate_runtime_call(builder, state, name, args)
        }

        FirExpr::FieldGet { object, offset, ty } => {
            let obj_ptr = translate_expr(builder, state, object);
            let clif_ty = fir_type_to_clif(ty);
            let off = i32::try_from(*offset).expect("codegen: field offset exceeds i32::MAX");
            builder
                .ins()
                .load(clif_ty, ir::MemFlags::new(), obj_ptr, Offset32::new(off))
        }

        FirExpr::FieldSet {
            object,
            offset,
            value,
        } => {
            let obj_ptr = translate_expr(builder, state, object);
            let val = translate_expr(builder, state, value);
            let off = i32::try_from(*offset).expect("codegen: field offset exceeds i32::MAX");
            builder
                .ins()
                .store(ir::MemFlags::new(), val, obj_ptr, Offset32::new(off));
            val
        }

        FirExpr::Construct { fields, .. } => {
            if let Some(&alloc_ref) = state.runtime_refs.get("aster_class_alloc") {
                let size = fields.len() * 8;
                let size_val = builder.ins().iconst(types::I64, size as i64);
                let call = builder.ins().call(alloc_ref, &[size_val]);
                let ptr = builder.inst_results(call)[0];
                for (i, field_expr) in fields.iter().enumerate() {
                    let field_val = translate_expr(builder, state, field_expr);
                    let off = i32::try_from(i * 8)
                        .expect("codegen: construct field offset exceeds i32::MAX");
                    builder
                        .ins()
                        .store(ir::MemFlags::new(), field_val, ptr, Offset32::new(off));
                }
                ptr
            } else {
                builder.ins().iconst(types::I64, 0)
            }
        }

        FirExpr::ListNew { elements, elem_ty } => {
            if let Some(&new_ref) = state.runtime_refs.get("aster_list_new") {
                let cap = elements.len().max(4) as i64;
                let cap_val = builder.ins().iconst(types::I64, cap);
                let ptr_elems = if matches!(elem_ty, FirType::Ptr | FirType::Struct(_)) {
                    1i64
                } else {
                    0i64
                };
                let ptr_elems_val = builder.ins().iconst(types::I64, ptr_elems);
                let call = builder.ins().call(new_ref, &[cap_val, ptr_elems_val]);
                let list_ptr = builder.inst_results(call)[0];
                if let Some(&push_ref) = state.runtime_refs.get("aster_list_push") {
                    let mut current_ptr = list_ptr;
                    for elem in elements {
                        let val = translate_expr(builder, state, elem);
                        let call = builder.ins().call(push_ref, &[current_ptr, val]);
                        current_ptr = builder.inst_results(call)[0];
                    }
                    current_ptr
                } else {
                    list_ptr
                }
            } else {
                builder.ins().iconst(types::I64, 0)
            }
        }

        FirExpr::ListGet { list, index, .. } => {
            let list_val = translate_expr(builder, state, list);
            let idx_val = translate_expr(builder, state, index);
            if let Some(&get_ref) = state.runtime_refs.get("aster_list_get") {
                let call = builder.ins().call(get_ref, &[list_val, idx_val]);
                builder.inst_results(call)[0]
            } else {
                builder.ins().iconst(types::I64, 0)
            }
        }

        FirExpr::ListSet { list, index, value } => {
            let list_val = translate_expr(builder, state, list);
            let idx_val = translate_expr(builder, state, index);
            let val = translate_expr(builder, state, value);
            if let Some(&set_ref) = state.runtime_refs.get("aster_list_set") {
                builder.ins().call(set_ref, &[list_val, idx_val, val]);
            }
            val
        }

        FirExpr::TagWrap { tag, value, .. } => {
            if *tag == 1 {
                // Nil: return null pointer (0)
                builder.ins().iconst(types::I64, 0)
            } else {
                // Some(value): box the value — allocate 8 bytes, store, return ptr
                let inner_val = translate_expr(builder, state, value);
                if let Some(&alloc_ref) = state.runtime_refs.get("aster_class_alloc") {
                    let size = builder.ins().iconst(types::I64, 8);
                    let ptr = builder.ins().call(alloc_ref, &[size]);
                    let ptr_val = builder.inst_results(ptr)[0];
                    builder
                        .ins()
                        .store(ir::MemFlags::new(), inner_val, ptr_val, Offset32::new(0));
                    ptr_val
                } else {
                    // Fallback if runtime not available: just pass through
                    inner_val
                }
            }
        }
        FirExpr::TagUnwrap { value, .. } => {
            // Unwrap: load the boxed value from ptr+0
            let ptr_val = translate_expr(builder, state, value);
            builder
                .ins()
                .load(types::I64, ir::MemFlags::new(), ptr_val, Offset32::new(0))
        }
        FirExpr::TagCheck { value, tag } => {
            // Check if nullable is nil (tag=1) or has value (tag=0)
            let ptr_val = translate_expr(builder, state, value);
            let zero = builder.ins().iconst(types::I64, 0);
            if *tag == 1 {
                // IsNil: ptr == 0
                builder.ins().icmp(IntCC::Equal, ptr_val, zero)
            } else {
                // IsSome: ptr != 0
                builder.ins().icmp(IntCC::NotEqual, ptr_val, zero)
            }
        }

        FirExpr::ClosureCreate { func, env, .. } => {
            // Allocate closure struct: [func_ptr: i64][env_ptr: i64]
            if let Some(&alloc_ref) = state.runtime_refs.get("aster_class_alloc") {
                let size = builder.ins().iconst(types::I64, 16);
                let call = builder.ins().call(alloc_ref, &[size]);
                let closure_ptr = builder.inst_results(call)[0];

                // Store the actual function pointer (for indirect calls)
                let func_addr = if let Some(&func_ref) = state.func_refs.get(func) {
                    builder.ins().func_addr(types::I64, func_ref)
                } else {
                    builder.ins().iconst(types::I64, 0)
                };
                builder.ins().store(
                    ir::MemFlags::new(),
                    func_addr,
                    closure_ptr,
                    Offset32::new(0),
                );

                // Store env pointer
                let env_val = translate_expr(builder, state, env);
                builder
                    .ins()
                    .store(ir::MemFlags::new(), env_val, closure_ptr, Offset32::new(8));

                closure_ptr
            } else {
                builder.ins().iconst(types::I64, 0)
            }
        }

        FirExpr::ClosureCall {
            closure,
            args,
            ret_ty,
        } => {
            let closure_ptr = translate_expr(builder, state, closure);

            // Load function pointer from closure[0]
            let func_ptr = builder.ins().load(
                types::I64,
                ir::MemFlags::new(),
                closure_ptr,
                Offset32::new(0),
            );

            // Load env pointer from closure[8]
            let env_ptr = builder.ins().load(
                types::I64,
                ir::MemFlags::new(),
                closure_ptr,
                Offset32::new(8),
            );

            // Build signature for the indirect call: (env_ptr: i64, args...) -> ret_ty
            let call_conv = builder.func.signature.call_conv;
            let sig = builder.func.import_signature(ir::Signature {
                params: {
                    let mut params = vec![AbiParam::new(types::I64)]; // env ptr
                    for arg in args {
                        let arg_ty = fir_type_to_clif(&infer_operand_type(state, arg));
                        params.push(AbiParam::new(arg_ty));
                    }
                    params
                },
                returns: vec![AbiParam::new(fir_type_to_clif(ret_ty))],
                call_conv,
            });

            // Build args: env_ptr first, then explicit args
            let mut call_args = vec![env_ptr];
            for arg in args {
                call_args.push(translate_expr(builder, state, arg));
            }

            let call = builder.ins().call_indirect(sig, func_ptr, &call_args);
            builder.inst_results(call)[0]
        }

        FirExpr::EnvLoad { env, offset, ty } => {
            let env_ptr = translate_expr(builder, state, env);
            let clif_ty = fir_type_to_clif(ty);
            let off = i32::try_from(*offset).expect("codegen: env load offset exceeds i32::MAX");
            builder
                .ins()
                .load(clif_ty, ir::MemFlags::new(), env_ptr, Offset32::new(off))
        }

        FirExpr::GlobalFunc(func) => {
            // Return the function ID as an integer (for closure creation)
            builder.ins().iconst(types::I64, func.0 as i64)
        }
    }
}

fn translate_binop(
    builder: &mut FunctionBuilder,
    state: &TranslationState,
    op: &BinOp,
    lhs: Value,
    rhs: Value,
    left_expr: &FirExpr,
) -> Value {
    let is_f = is_float(&infer_operand_type(state, left_expr));

    match op {
        BinOp::Add if is_f => builder.ins().fadd(lhs, rhs),
        BinOp::Add => builder.ins().iadd(lhs, rhs),
        BinOp::Sub if is_f => builder.ins().fsub(lhs, rhs),
        BinOp::Sub => builder.ins().isub(lhs, rhs),
        BinOp::Mul if is_f => builder.ins().fmul(lhs, rhs),
        BinOp::Mul => builder.ins().imul(lhs, rhs),
        BinOp::Div if is_f => builder.ins().fdiv(lhs, rhs),
        BinOp::Div => {
            // Guard against division by zero and i64::MIN / -1 overflow.
            // Both cause hardware traps (SIGFPE) with no recovery path.
            let zero = builder.ins().iconst(types::I64, 0);
            let is_zero = builder.ins().icmp(IntCC::Equal, rhs, zero);
            let safe_block = builder.create_block();
            let trap_block = builder.create_block();
            let merge_block = builder.create_block();
            builder.append_block_param(merge_block, types::I64);

            builder
                .ins()
                .brif(is_zero, trap_block, &[], safe_block, &[]);

            builder.switch_to_block(trap_block);
            builder.seal_block(trap_block);
            let zero_result = builder.ins().iconst(types::I64, 0);
            builder
                .ins()
                .jump(merge_block, &[BlockArg::Value(zero_result)]);

            builder.switch_to_block(safe_block);
            builder.seal_block(safe_block);
            let result = builder.ins().sdiv(lhs, rhs);
            builder.ins().jump(merge_block, &[BlockArg::Value(result)]);

            builder.switch_to_block(merge_block);
            builder.seal_block(merge_block);
            builder.block_params(merge_block)[0]
        }
        BinOp::Mod if is_f => {
            // Float modulo: a - floor(a/b) * b
            let div = builder.ins().fdiv(lhs, rhs);
            let floored = builder.ins().floor(div);
            let prod = builder.ins().fmul(floored, rhs);
            builder.ins().fsub(lhs, prod)
        }
        BinOp::Mod => {
            // Guard against modulo by zero (same trap as division)
            let zero = builder.ins().iconst(types::I64, 0);
            let is_zero = builder.ins().icmp(IntCC::Equal, rhs, zero);
            let safe_block = builder.create_block();
            let trap_block = builder.create_block();
            let merge_block = builder.create_block();
            builder.append_block_param(merge_block, types::I64);

            builder
                .ins()
                .brif(is_zero, trap_block, &[], safe_block, &[]);

            builder.switch_to_block(trap_block);
            builder.seal_block(trap_block);
            let zero_result = builder.ins().iconst(types::I64, 0);
            builder
                .ins()
                .jump(merge_block, &[BlockArg::Value(zero_result)]);

            builder.switch_to_block(safe_block);
            builder.seal_block(safe_block);
            let result = builder.ins().srem(lhs, rhs);
            builder.ins().jump(merge_block, &[BlockArg::Value(result)]);

            builder.switch_to_block(merge_block);
            builder.seal_block(merge_block);
            builder.block_params(merge_block)[0]
        }
        BinOp::Eq if is_f => builder.ins().fcmp(FloatCC::Equal, lhs, rhs),
        BinOp::Eq => builder.ins().icmp(IntCC::Equal, lhs, rhs),
        BinOp::Neq if is_f => builder.ins().fcmp(FloatCC::NotEqual, lhs, rhs),
        BinOp::Neq => builder.ins().icmp(IntCC::NotEqual, lhs, rhs),
        BinOp::Lt if is_f => builder.ins().fcmp(FloatCC::LessThan, lhs, rhs),
        BinOp::Lt => builder.ins().icmp(IntCC::SignedLessThan, lhs, rhs),
        BinOp::Gt if is_f => builder.ins().fcmp(FloatCC::GreaterThan, lhs, rhs),
        BinOp::Gt => builder.ins().icmp(IntCC::SignedGreaterThan, lhs, rhs),
        BinOp::Lte if is_f => builder.ins().fcmp(FloatCC::LessThanOrEqual, lhs, rhs),
        BinOp::Lte => builder.ins().icmp(IntCC::SignedLessThanOrEqual, lhs, rhs),
        BinOp::Gte if is_f => builder.ins().fcmp(FloatCC::GreaterThanOrEqual, lhs, rhs),
        BinOp::Gte => builder
            .ins()
            .icmp(IntCC::SignedGreaterThanOrEqual, lhs, rhs),
        // And/Or are handled by translate_short_circuit before reaching here
        BinOp::And | BinOp::Or => unreachable!("And/Or handled by short-circuit path"),
    }
}

fn translate_unaryop(
    builder: &mut FunctionBuilder,
    state: &TranslationState,
    op: &UnaryOp,
    val: Value,
    operand: &FirExpr,
) -> Value {
    let is_f = is_float(&infer_operand_type(state, operand));
    match op {
        UnaryOp::Neg if is_f => builder.ins().fneg(val),
        UnaryOp::Neg => builder.ins().ineg(val),
        UnaryOp::Not => {
            let one = builder.ins().iconst(types::I8, 1);
            builder.ins().bxor(val, one)
        }
    }
}

fn translate_short_circuit(
    builder: &mut FunctionBuilder,
    state: &mut TranslationState,
    op: &BinOp,
    left: &FirExpr,
    right: &FirExpr,
) -> Value {
    // Evaluate left side
    let lhs = translate_expr(builder, state, left);

    // Create blocks: rhs_block (evaluate right), merge_block (join point)
    let rhs_block = builder.create_block();
    let merge_block = builder.create_block();
    // merge_block has one i8 parameter for the result
    builder.append_block_param(merge_block, types::I8);

    match op {
        BinOp::And => {
            // If lhs is false, short-circuit to merge with false; otherwise evaluate rhs
            builder
                .ins()
                .brif(lhs, rhs_block, &[], merge_block, &[BlockArg::Value(lhs)]);
        }
        BinOp::Or => {
            // If lhs is true, short-circuit to merge with true; otherwise evaluate rhs
            builder
                .ins()
                .brif(lhs, merge_block, &[BlockArg::Value(lhs)], rhs_block, &[]);
        }
        _ => unreachable!(),
    }

    // rhs_block: evaluate right side, jump to merge with result
    builder.switch_to_block(rhs_block);
    builder.seal_block(rhs_block);
    let rhs = translate_expr(builder, state, right);
    builder.ins().jump(merge_block, &[BlockArg::Value(rhs)]);

    // merge_block: result is the block parameter
    builder.switch_to_block(merge_block);
    builder.seal_block(merge_block);
    builder.block_params(merge_block)[0]
}

fn translate_runtime_call(
    builder: &mut FunctionBuilder,
    state: &mut TranslationState,
    name: &str,
    args: &[FirExpr],
) -> Value {
    match name {
        "log" | "say" => {
            if let Some(first_arg) = args.first() {
                let val = translate_expr(builder, state, first_arg);
                let say_fn = match infer_operand_type(state, first_arg) {
                    FirType::I64 => "aster_say_int",
                    FirType::F64 => "aster_say_float",
                    FirType::Bool => "aster_say_bool",
                    _ => "aster_say_str",
                };
                if let Some(&func_ref) = state.runtime_refs.get(say_fn) {
                    builder.ins().call(func_ref, &[val]);
                }
            }
            builder.ins().iconst(types::I64, 0)
        }
        other => {
            if let Some(&func_ref) = state.runtime_refs.get(other) {
                let arg_vals: Vec<Value> = args
                    .iter()
                    .map(|a| translate_expr(builder, state, a))
                    .collect();
                let call = builder.ins().call(func_ref, &arg_vals);
                let results = builder.inst_results(call);
                if !results.is_empty() {
                    return results[0];
                }
            }
            builder.ins().iconst(types::I64, 0)
        }
    }
}

fn normalize_branch_condition(builder: &mut FunctionBuilder, cond: Value) -> Value {
    let ty = builder.func.dfg.value_type(cond);
    if ty == types::I8 || ty == types::I16 || ty == types::I32 || ty == types::I64 {
        let zero = builder.ins().iconst(ty, 0);
        builder.ins().icmp(IntCC::NotEqual, cond, zero)
    } else {
        cond
    }
}

fn infer_operand_type(_state: &TranslationState, expr: &FirExpr) -> FirType {
    match expr {
        FirExpr::IntLit(_) => FirType::I64,
        FirExpr::FloatLit(_) => FirType::F64,
        FirExpr::BoolLit(_) => FirType::Bool,
        FirExpr::StringLit(_) => FirType::Ptr,
        FirExpr::NilLit => FirType::Void,
        FirExpr::LocalVar(_id, ty) => ty.clone(),
        FirExpr::BinaryOp { result_ty, .. } => result_ty.clone(),
        FirExpr::UnaryOp { result_ty, .. } => result_ty.clone(),
        FirExpr::Call { ret_ty, .. } => ret_ty.clone(),
        FirExpr::Spawn { ret_ty, .. } => ret_ty.clone(),
        FirExpr::BlockOn { ret_ty, .. } => ret_ty.clone(),
        FirExpr::ResolveTask { ret_ty, .. } => ret_ty.clone(),
        FirExpr::CancelTask { .. } | FirExpr::WaitCancel { .. } | FirExpr::Safepoint => {
            FirType::Void
        }
        FirExpr::RuntimeCall { ret_ty, .. } => ret_ty.clone(),
        FirExpr::FieldGet { ty, .. } => ty.clone(),
        FirExpr::Construct { ty, .. } => ty.clone(),
        FirExpr::ListNew { .. } => FirType::Ptr,
        FirExpr::ListGet { elem_ty, .. } => elem_ty.clone(),
        FirExpr::ClosureCreate { .. } => FirType::Ptr,
        FirExpr::ClosureCall { ret_ty, .. } => ret_ty.clone(),
        FirExpr::EnvLoad { ty, .. } => ty.clone(),
        FirExpr::GlobalFunc(_) => FirType::Ptr,
        FirExpr::FieldSet { .. } | FirExpr::ListSet { .. } => FirType::Void,
        FirExpr::TagWrap { ty, .. } => ty.clone(),
        FirExpr::TagUnwrap { ty, .. } => ty.clone(),
        FirExpr::TagCheck { .. } => FirType::Bool,
    }
}

fn task_resolve_runtime_name(result_ty: &FirType) -> &'static str {
    match result_ty {
        FirType::F64 => "aster_task_resolve_f64",
        FirType::Bool => "aster_task_resolve_i8",
        _ => "aster_task_resolve_i64",
    }
}
