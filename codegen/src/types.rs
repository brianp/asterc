use cranelift_codegen::ir::Type as ClifType;
use cranelift_codegen::ir::types;
use fir::FirType;

/// Map FIR types to Cranelift IR types.
pub fn fir_type_to_clif(ty: &FirType) -> ClifType {
    match ty {
        FirType::I64 => types::I64,
        FirType::F64 => types::F64,
        FirType::Bool => types::I8,
        FirType::Ptr => types::I64,  // pointers are 64-bit
        FirType::Void => types::I64, // void represented as i64(0) for simplicity
        FirType::Never => types::I64,
        FirType::TaggedUnion { .. } => types::I64, // TODO: proper tagged union layout
        FirType::Struct(_) => types::I64,          // heap pointer
        FirType::FnPtr(_) => types::I64,           // function pointer
    }
}

/// Returns true if this FIR type should be treated as a float in Cranelift.
pub fn is_float(ty: &FirType) -> bool {
    matches!(ty, FirType::F64)
}

/// Returns true if this FIR type is a heap pointer.
pub fn is_ptr(ty: &FirType) -> bool {
    matches!(ty, FirType::Ptr)
}
