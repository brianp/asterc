use std::collections::HashMap;

use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::immediates::Offset32;
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
    pub next_var: usize,
    pub func_refs: HashMap<FunctionId, ir::FuncRef>,
    pub runtime_refs: HashMap<String, ir::FuncRef>,
    pub string_data: HashMap<String, (ir::GlobalValue, usize)>,
    pub loop_exit: Option<ir::Block>,
    pub loop_header: Option<ir::Block>,
    /// True if the current block has been terminated (return/break/continue/jump).
    pub terminated: bool,
}

impl TranslationState {
    pub fn new(
        func_refs: HashMap<FunctionId, ir::FuncRef>,
        runtime_refs: HashMap<String, ir::FuncRef>,
        string_data: HashMap<String, (ir::GlobalValue, usize)>,
    ) -> Self {
        Self {
            locals: HashMap::new(),
            local_types: HashMap::new(),
            next_var: 0,
            func_refs,
            runtime_refs,
            string_data,
            loop_exit: None,
            loop_header: None,
            terminated: false,
        }
    }

    fn new_variable(&mut self) -> Variable {
        let v = Variable::from_u32(self.next_var as u32);
        self.next_var += 1;
        v
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
        let var = state.new_variable();
        let clif_ty = fir_type_to_clif(ty);
        builder.declare_var(var, clif_ty);
        builder.def_var(var, params[i]);
        state.locals.insert(LocalId(i as u32), var);
        state.local_types.insert(LocalId(i as u32), ty.clone());
    }
    state.next_var = func.params.len();
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
            let var = state.new_variable();
            let clif_ty = fir_type_to_clif(ty);
            builder.declare_var(var, clif_ty);
            builder.def_var(var, val);
            state.locals.insert(*name, var);
            state.local_types.insert(*name, ty.clone());
        }

        FirStmt::Assign { target, value } => {
            let val = translate_expr(builder, state, value);
            match target {
                FirPlace::Local(id) => {
                    let var = state.locals[id];
                    builder.def_var(var, val);
                }
                FirPlace::Field { object, offset } => {
                    let obj_ptr = translate_expr(builder, state, object);
                    builder.ins().store(
                        ir::MemFlags::new(),
                        val,
                        obj_ptr,
                        Offset32::new(*offset as i32),
                    );
                }
                FirPlace::Index { list, index } => {
                    let list_val = translate_expr(builder, state, list);
                    let idx_val = translate_expr(builder, state, index);
                    if let Some(&func_ref) = state.runtime_refs.get("aster_list_set") {
                        builder.ins().call(func_ref, &[list_val, idx_val, val]);
                    }
                }
            }
        }

        FirStmt::Return(expr) => {
            let val = translate_expr(builder, state, expr);
            builder.ins().return_(&[val]);
            state.terminated = true;
        }

        FirStmt::If {
            cond,
            then_body,
            else_body,
        } => {
            let cond_val = translate_expr(builder, state, cond);
            let then_block = builder.create_block();
            let else_block = builder.create_block();
            let merge_block = builder.create_block();

            builder
                .ins()
                .brif(cond_val, then_block, &[], else_block, &[]);

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

        FirStmt::While { cond, body } => {
            let header_block = builder.create_block();
            let body_block = builder.create_block();
            let exit_block = builder.create_block();

            let saved_exit = state.loop_exit.replace(exit_block);
            let saved_header = state.loop_header.replace(header_block);

            builder.ins().jump(header_block, &[]);

            builder.switch_to_block(header_block);
            let cond_val = translate_expr(builder, state, cond);
            builder
                .ins()
                .brif(cond_val, body_block, &[], exit_block, &[]);

            builder.switch_to_block(body_block);
            builder.seal_block(body_block);
            state.terminated = false;
            translate_body(builder, state, body);
            if !state.terminated {
                builder.ins().jump(header_block, &[]);
            }

            builder.seal_block(header_block);
            builder.switch_to_block(exit_block);
            builder.seal_block(exit_block);

            state.loop_exit = saved_exit;
            state.loop_header = saved_header;
            state.terminated = false; // while exit is always reachable
        }

        FirStmt::Break => {
            if let Some(exit) = state.loop_exit {
                builder.ins().jump(exit, &[]);
                state.terminated = true;
            }
        }

        FirStmt::Continue => {
            if let Some(header) = state.loop_header {
                builder.ins().jump(header, &[]);
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
            let var = state.locals[id];
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

        FirExpr::RuntimeCall { name, args, .. } => {
            translate_runtime_call(builder, state, name, args)
        }

        FirExpr::FieldGet { object, offset, ty } => {
            let obj_ptr = translate_expr(builder, state, object);
            let clif_ty = fir_type_to_clif(ty);
            builder.ins().load(
                clif_ty,
                ir::MemFlags::new(),
                obj_ptr,
                Offset32::new(*offset as i32),
            )
        }

        FirExpr::FieldSet {
            object,
            offset,
            value,
        } => {
            let obj_ptr = translate_expr(builder, state, object);
            let val = translate_expr(builder, state, value);
            builder.ins().store(
                ir::MemFlags::new(),
                val,
                obj_ptr,
                Offset32::new(*offset as i32),
            );
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
                    builder.ins().store(
                        ir::MemFlags::new(),
                        field_val,
                        ptr,
                        Offset32::new((i * 8) as i32),
                    );
                }
                ptr
            } else {
                builder.ins().iconst(types::I64, 0)
            }
        }

        FirExpr::ListNew { elements, .. } => {
            if let Some(&new_ref) = state.runtime_refs.get("aster_list_new") {
                let cap = elements.len().max(4) as i64;
                let cap_val = builder.ins().iconst(types::I64, cap);
                let call = builder.ins().call(new_ref, &[cap_val]);
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

        FirExpr::TagWrap { value, .. } => translate_expr(builder, state, value),
        FirExpr::TagUnwrap { value, .. } => translate_expr(builder, state, value),
        FirExpr::TagCheck { .. } => builder.ins().iconst(types::I8, 1),

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
                builder
                    .ins()
                    .store(ir::MemFlags::new(), func_addr, closure_ptr, Offset32::new(0));

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
                call_conv: cranelift_codegen::isa::CallConv::Fast,
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
            builder.ins().load(
                clif_ty,
                ir::MemFlags::new(),
                env_ptr,
                Offset32::new(*offset as i32),
            )
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
        BinOp::Div => builder.ins().sdiv(lhs, rhs),
        BinOp::Mod if is_f => {
            // Float modulo: a - floor(a/b) * b
            let div = builder.ins().fdiv(lhs, rhs);
            let floored = builder.ins().floor(div);
            let prod = builder.ins().fmul(floored, rhs);
            builder.ins().fsub(lhs, prod)
        }
        BinOp::Mod => builder.ins().srem(lhs, rhs),
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
            builder.ins().brif(lhs, rhs_block, &[], merge_block, &[lhs]);
        }
        BinOp::Or => {
            // If lhs is true, short-circuit to merge with true; otherwise evaluate rhs
            builder.ins().brif(lhs, merge_block, &[lhs], rhs_block, &[]);
        }
        _ => unreachable!(),
    }

    // rhs_block: evaluate right side, jump to merge with result
    builder.switch_to_block(rhs_block);
    builder.seal_block(rhs_block);
    let rhs = translate_expr(builder, state, right);
    builder.ins().jump(merge_block, &[rhs]);

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
        "log" | "print" => {
            if let Some(first_arg) = args.first() {
                let val = translate_expr(builder, state, first_arg);
                let print_fn = match infer_operand_type(state, first_arg) {
                    FirType::I64 => "aster_print_int",
                    FirType::F64 => "aster_print_float",
                    FirType::Bool => "aster_print_bool",
                    _ => "aster_print_str",
                };
                if let Some(&func_ref) = state.runtime_refs.get(print_fn) {
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
