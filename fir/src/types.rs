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

impl Eq for FirType {}
