use serde::{Deserialize, Serialize};

/// Byte-offset span in source code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Span { start, end }
    }

    /// Merge two spans into one covering both.
    pub fn merge(self, other: Span) -> Span {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }

    /// Zero-width span for synthetic/test nodes.
    pub fn dummy() -> Self {
        Span { start: 0, end: 0 }
    }

    /// Returns true if this span is the sentinel dummy span (start == 0, end == 0).
    /// A dummy span indicates that no real source location is available.
    pub fn is_dummy(self) -> bool {
        self.start == 0 && self.end == 0
    }
}
