use std::collections::HashMap;

use crate::span::Span;
use crate::types::Type;

/// Maps AST expression spans to their resolved types.
///
/// Populated by the typechecker during inference; consumed by the FIR lowerer
/// so that `Type::Inferred` and `Type::TypeVar` are resolved to concrete types
/// rather than defaulting to I64.
#[derive(Debug, Clone, Default)]
pub struct TypeTable {
    map: HashMap<Span, Type>,
}

impl TypeTable {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    /// Record a resolved type for the expression at the given span.
    pub fn insert(&mut self, span: Span, ty: Type) {
        self.map.insert(span, ty);
    }

    /// Look up the resolved type for an expression span.
    pub fn get(&self, span: &Span) -> Option<&Type> {
        self.map.get(span)
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Merge all entries from another TypeTable into this one.
    pub fn extend(&mut self, other: TypeTable) {
        self.map.extend(other.map);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_table() {
        let table = TypeTable::new();
        assert!(table.is_empty());
        assert_eq!(table.len(), 0);
        assert_eq!(table.get(&Span::new(0, 5)), None);
    }

    #[test]
    fn insert_and_get() {
        let mut table = TypeTable::new();
        let span = Span::new(10, 20);
        table.insert(span, Type::Int);
        assert_eq!(table.get(&span), Some(&Type::Int));
        assert_eq!(table.len(), 1);
    }

    #[test]
    fn overwrite_existing() {
        let mut table = TypeTable::new();
        let span = Span::new(5, 15);
        table.insert(span, Type::Inferred);
        table.insert(span, Type::Float);
        assert_eq!(table.get(&span), Some(&Type::Float));
        assert_eq!(table.len(), 1);
    }

    #[test]
    fn multiple_spans() {
        let mut table = TypeTable::new();
        table.insert(Span::new(0, 5), Type::Int);
        table.insert(Span::new(10, 15), Type::Bool);
        table.insert(Span::new(20, 30), Type::Float);
        assert_eq!(table.len(), 3);
        assert_eq!(table.get(&Span::new(0, 5)), Some(&Type::Int));
        assert_eq!(table.get(&Span::new(10, 15)), Some(&Type::Bool));
        assert_eq!(table.get(&Span::new(20, 30)), Some(&Type::Float));
    }

    #[test]
    fn default_is_empty() {
        let table = TypeTable::default();
        assert!(table.is_empty());
    }
}
