use serde::{Deserialize, Serialize};

use crate::stmts::FirStmt;
use crate::types::{ClassId, FirType, FunctionId};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirModule {
    pub functions: Vec<FirFunction>,
    pub classes: Vec<FirClass>,
    pub entry: Option<FunctionId>,
}

impl FirModule {
    pub fn new() -> Self {
        Self {
            functions: Vec::new(),
            classes: Vec::new(),
            entry: None,
        }
    }

    /// Insert a function at its declared ID position. Grows the vector
    /// with placeholders if needed, allowing out-of-order insertion
    /// (e.g. a lambda lifted inside a function that hasn't been added yet).
    pub fn add_function(&mut self, func: FirFunction) -> FunctionId {
        let id = func.id;
        let idx = id.0 as usize;
        // Grow vector with placeholders if needed
        while self.functions.len() <= idx {
            let placeholder_id = FunctionId(self.functions.len() as u32);
            self.functions.push(FirFunction {
                id: placeholder_id,
                name: String::new(),
                params: vec![],
                ret_type: FirType::Void,
                body: vec![],
                is_entry: false,
                suspendable: false,
            });
        }
        self.functions[idx] = func;
        id
    }

    /// Append a class layout. Returns its ClassId.
    pub fn add_class(&mut self, class: FirClass) -> ClassId {
        let id = ClassId(self.classes.len() as u32);
        debug_assert_eq!(class.id, id);
        self.classes.push(class);
        id
    }

    /// Look up a function by ID. O(1).
    pub fn get_function(&self, id: FunctionId) -> &FirFunction {
        &self.functions[id.0 as usize]
    }

    /// Look up a class by ID. O(1).
    pub fn get_class(&self, id: ClassId) -> &FirClass {
        &self.classes[id.0 as usize]
    }

    /// Snapshot the current size for incremental tracking.
    pub fn mark(&self) -> usize {
        self.functions.len()
    }

    /// All functions added since a given mark. Used by REPL to compile
    /// only new definitions without recompiling the whole module.
    pub fn functions_since(&self, mark: usize) -> &[FirFunction] {
        &self.functions[mark..]
    }
}

impl Default for FirModule {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirFunction {
    pub id: FunctionId,
    pub name: String,
    pub params: Vec<(String, FirType)>,
    pub ret_type: FirType,
    pub body: Vec<FirStmt>,
    pub is_entry: bool,
    pub suspendable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirClass {
    pub id: ClassId,
    pub name: String,
    /// (field_name, type, byte_offset)
    pub fields: Vec<(String, FirType, usize)>,
    pub methods: Vec<FunctionId>,
    /// (method_name, implementing_function_id)
    pub vtable: Vec<(String, FunctionId)>,
    pub size: usize,
    pub alignment: usize,
    pub parent: Option<ClassId>,
}
