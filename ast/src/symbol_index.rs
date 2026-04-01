use std::collections::HashMap;

use crate::span::Span;
use crate::types::Type;

/// The kind of symbol a name refers to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SymbolKind {
    Variable,
    Function,
    Class,
    Trait,
    Field,
    Method,
    Parameter,
    EnumVariant,
}

/// Information about a resolved symbol at a use site.
#[derive(Debug, Clone, PartialEq)]
pub struct SymbolInfo {
    /// The identifier text at the use site.
    pub name: String,
    /// The resolved type of the symbol.
    pub ty: Type,
    /// The span where this symbol was defined (if known).
    pub def_span: Option<Span>,
    /// What kind of symbol this is.
    pub kind: SymbolKind,
}

/// Maps use-site spans to their resolved symbol information.
///
/// Populated by the typechecker during type checking, consumed by the
/// future LSP server for go-to-definition, hover, and other features.
#[derive(Debug, Clone, Default)]
pub struct SymbolIndex {
    map: HashMap<Span, SymbolInfo>,
}

impl SymbolIndex {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    /// Record symbol information for the identifier at the given use-site span.
    pub fn insert(&mut self, use_span: Span, info: SymbolInfo) {
        self.map.insert(use_span, info);
    }

    /// Look up symbol information for a use-site span.
    pub fn get(&self, use_span: &Span) -> Option<&SymbolInfo> {
        self.map.get(use_span)
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Merge all entries from another SymbolIndex into this one.
    pub fn extend(&mut self, other: SymbolIndex) {
        self.map.extend(other.map);
    }

    /// Iterate over all (use_span, symbol_info) pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&Span, &SymbolInfo)> {
        self.map.iter()
    }

    /// Find all use sites that refer to a definition at the given span.
    pub fn find_references(&self, def_span: &Span) -> Vec<Span> {
        self.map
            .iter()
            .filter(|(_, info)| info.def_span.as_ref() == Some(def_span))
            .map(|(use_span, _)| *use_span)
            .collect()
    }

    /// Find all symbols with the given name.
    pub fn find_by_name(&self, name: &str) -> Vec<(&Span, &SymbolInfo)> {
        self.map
            .iter()
            .filter(|(_, info)| info.name == name)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_info(name: &str, ty: Type, def_span: Option<Span>, kind: SymbolKind) -> SymbolInfo {
        SymbolInfo {
            name: name.to_string(),
            ty,
            def_span,
            kind,
        }
    }

    // ── Contract tests ──────────────────────────────────────────────

    #[test]
    fn empty_index() {
        let idx = SymbolIndex::new();
        assert!(idx.is_empty());
        assert_eq!(idx.len(), 0);
        assert_eq!(idx.get(&Span::new(0, 5)), None);
    }

    #[test]
    fn default_is_empty() {
        let idx = SymbolIndex::default();
        assert!(idx.is_empty());
    }

    // ── Happy path tests ────────────────────────────────────────────

    #[test]
    fn insert_and_get() {
        let mut idx = SymbolIndex::new();
        let use_span = Span::new(10, 11);
        let def_span = Span::new(0, 5);
        let info = make_info("x", Type::Int, Some(def_span), SymbolKind::Variable);
        idx.insert(use_span, info.clone());
        assert_eq!(idx.get(&use_span), Some(&info));
        assert_eq!(idx.len(), 1);
    }

    #[test]
    fn multiple_use_sites() {
        let mut idx = SymbolIndex::new();
        let def = Span::new(0, 5);
        idx.insert(
            Span::new(10, 11),
            make_info("x", Type::Int, Some(def), SymbolKind::Variable),
        );
        idx.insert(
            Span::new(20, 21),
            make_info("x", Type::Int, Some(def), SymbolKind::Variable),
        );
        idx.insert(
            Span::new(30, 33),
            make_info("foo", Type::Bool, None, SymbolKind::Function),
        );
        assert_eq!(idx.len(), 3);
    }

    #[test]
    fn overwrite_existing_use_site() {
        let mut idx = SymbolIndex::new();
        let use_span = Span::new(10, 11);
        idx.insert(
            use_span,
            make_info("x", Type::Int, None, SymbolKind::Variable),
        );
        let updated = make_info("x", Type::Float, None, SymbolKind::Variable);
        idx.insert(use_span, updated.clone());
        assert_eq!(idx.get(&use_span), Some(&updated));
        assert_eq!(idx.len(), 1);
    }

    #[test]
    fn all_symbol_kinds() {
        let mut idx = SymbolIndex::new();
        let kinds = vec![
            SymbolKind::Variable,
            SymbolKind::Function,
            SymbolKind::Class,
            SymbolKind::Trait,
            SymbolKind::Field,
            SymbolKind::Method,
            SymbolKind::Parameter,
            SymbolKind::EnumVariant,
        ];
        for (i, kind) in kinds.into_iter().enumerate() {
            let span = Span::new(i * 10, i * 10 + 3);
            idx.insert(span, make_info("sym", Type::Int, None, kind));
        }
        assert_eq!(idx.len(), 8);
    }

    // ── find_references tests ───────────────────────────────────────

    #[test]
    fn find_references_for_definition() {
        let mut idx = SymbolIndex::new();
        let def = Span::new(0, 5);
        idx.insert(
            Span::new(10, 11),
            make_info("x", Type::Int, Some(def), SymbolKind::Variable),
        );
        idx.insert(
            Span::new(20, 21),
            make_info("x", Type::Int, Some(def), SymbolKind::Variable),
        );
        idx.insert(
            Span::new(30, 31),
            make_info(
                "y",
                Type::Bool,
                Some(Span::new(5, 10)),
                SymbolKind::Variable,
            ),
        );
        let refs = idx.find_references(&def);
        assert_eq!(refs.len(), 2);
        assert!(refs.contains(&Span::new(10, 11)));
        assert!(refs.contains(&Span::new(20, 21)));
    }

    #[test]
    fn find_references_returns_empty_for_unknown_def() {
        let idx = SymbolIndex::new();
        let refs = idx.find_references(&Span::new(999, 1000));
        assert!(refs.is_empty());
    }

    // ── find_by_name tests ──────────────────────────────────────────

    #[test]
    fn find_by_name_returns_matching_symbols() {
        let mut idx = SymbolIndex::new();
        idx.insert(
            Span::new(10, 11),
            make_info("foo", Type::Int, None, SymbolKind::Variable),
        );
        idx.insert(
            Span::new(20, 23),
            make_info("foo", Type::Int, None, SymbolKind::Function),
        );
        idx.insert(
            Span::new(30, 33),
            make_info("bar", Type::Bool, None, SymbolKind::Variable),
        );
        let results = idx.find_by_name("foo");
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|(_, info)| info.name == "foo"));
    }

    #[test]
    fn find_by_name_returns_empty_for_unknown() {
        let idx = SymbolIndex::new();
        assert!(idx.find_by_name("nonexistent").is_empty());
    }

    // ── extend tests ────────────────────────────────────────────────

    #[test]
    fn extend_merges_two_indices() {
        let mut a = SymbolIndex::new();
        a.insert(
            Span::new(0, 1),
            make_info("x", Type::Int, None, SymbolKind::Variable),
        );
        let mut b = SymbolIndex::new();
        b.insert(
            Span::new(10, 11),
            make_info("y", Type::Bool, None, SymbolKind::Variable),
        );
        a.extend(b);
        assert_eq!(a.len(), 2);
        assert!(a.get(&Span::new(0, 1)).is_some());
        assert!(a.get(&Span::new(10, 11)).is_some());
    }

    // ── iter tests ──────────────────────────────────────────────────

    #[test]
    fn iter_visits_all_entries() {
        let mut idx = SymbolIndex::new();
        idx.insert(
            Span::new(0, 1),
            make_info("a", Type::Int, None, SymbolKind::Variable),
        );
        idx.insert(
            Span::new(10, 11),
            make_info("b", Type::Bool, None, SymbolKind::Function),
        );
        let entries: Vec<_> = idx.iter().collect();
        assert_eq!(entries.len(), 2);
    }

    // ── Clone and Debug ─────────────────────────────────────────────

    #[test]
    fn clone_preserves_entries() {
        let mut idx = SymbolIndex::new();
        idx.insert(
            Span::new(5, 10),
            make_info("z", Type::String, None, SymbolKind::Parameter),
        );
        let cloned = idx.clone();
        assert_eq!(cloned.len(), 1);
        assert_eq!(cloned.get(&Span::new(5, 10)), idx.get(&Span::new(5, 10)));
    }

    #[test]
    fn symbol_info_debug_format() {
        let info = make_info("x", Type::Int, Some(Span::new(0, 1)), SymbolKind::Variable);
        let debug = format!("{:?}", info);
        assert!(debug.contains("Variable"));
        assert!(debug.contains("Int"));
    }
}
