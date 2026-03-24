use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FunctionId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ClassId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LocalId(pub u32);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FirType {
    I64,
    F64,
    Bool,
    /// Pointer to heap object (String, List, Class instance).
    Ptr,
    Void,
    Never,
    /// Tagged union for nullable T?, Result<T, E>.
    TaggedUnion {
        tag_bits: u8,
        variants: Vec<FirType>,
    },
    Struct(ClassId),
    FnPtr(FunctionId),
}

impl FirType {
    /// Whether this type can hold a GC-managed heap pointer and therefore
    /// needs a shadow stack root slot to stay alive across allocations.
    pub fn needs_gc_root(&self) -> bool {
        match self {
            FirType::Ptr | FirType::Struct(_) => true,
            FirType::TaggedUnion { variants, .. } => variants.iter().any(|v| v.needs_gc_root()),
            _ => false,
        }
    }
}

impl Eq for FirType {}
