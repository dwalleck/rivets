//! Types for graph operations.
//!
//! Some fields are intentionally kept for API completeness and future use,
//! even if not currently consumed by the public Tethys API.

#![allow(dead_code)]

use crate::types::{IndexedFile, ReferenceKind, Symbol};

/// Information about a caller of a symbol.
#[derive(Debug, Clone)]
pub struct CallerInfo {
    /// The symbol that calls the target.
    pub symbol: Symbol,
    /// How many times it references the target.
    pub reference_count: usize,
    /// The kinds of references (Call, Type, Construct, etc.).
    pub reference_kinds: Vec<ReferenceKind>,
}

/// Information about a callee of a symbol.
#[derive(Debug, Clone)]
pub struct CalleeInfo {
    /// The symbol being called/referenced.
    pub symbol: Symbol,
    /// How many times it is referenced.
    pub reference_count: usize,
    /// The kinds of references.
    pub reference_kinds: Vec<ReferenceKind>,
}

/// Result of transitive caller analysis (symbol-level impact).
#[derive(Debug, Clone)]
pub struct SymbolImpact {
    /// The target symbol being analyzed.
    pub target: Symbol,
    /// Symbols that directly call/reference the target.
    pub direct_callers: Vec<CallerInfo>,
    /// Symbols that transitively call the target (excludes direct).
    pub transitive_callers: Vec<CallerInfo>,
    /// Maximum depth reached during traversal.
    pub max_depth_reached: u32,
}

impl SymbolImpact {
    /// Total number of unique callers (direct + transitive).
    pub fn total_caller_count(&self) -> usize {
        self.direct_callers.len() + self.transitive_callers.len()
    }
}

/// A path through the call graph.
#[derive(Debug, Clone)]
pub struct CallPath {
    /// Symbols from source to target.
    pub symbols: Vec<Symbol>,
    /// The relationship at each step.
    pub edges: Vec<ReferenceKind>,
}

impl CallPath {
    /// Create a new call path, validating invariants.
    ///
    /// Returns `None` if:
    /// - `symbols` is empty
    /// - `edges.len()` does not equal `symbols.len() - 1`
    pub fn new(symbols: Vec<Symbol>, edges: Vec<ReferenceKind>) -> Option<Self> {
        if symbols.is_empty() {
            return None;
        }
        if edges.len() != symbols.len().saturating_sub(1) {
            return None;
        }
        Some(Self { symbols, edges })
    }

    /// Create a trivial path with a single symbol.
    pub fn single(symbol: Symbol) -> Self {
        Self {
            symbols: vec![symbol],
            edges: vec![],
        }
    }
}

/// Information about a file dependency.
#[derive(Debug, Clone)]
pub struct FileDepInfo {
    /// The dependent/dependency file.
    pub file: IndexedFile,
    /// Number of references between the files.
    pub ref_count: usize,
}

/// Result of file-level impact analysis.
#[derive(Debug, Clone)]
pub struct FileImpact {
    /// The target file being analyzed.
    pub target: IndexedFile,
    /// Files that directly depend on the target.
    pub direct_dependents: Vec<FileDepInfo>,
    /// Files that transitively depend on the target.
    pub transitive_dependents: Vec<FileDepInfo>,
}

impl FileImpact {
    /// Total number of dependent files (direct + transitive).
    pub fn total_dependent_count(&self) -> usize {
        self.direct_dependents.len() + self.transitive_dependents.len()
    }
}

/// A path through the file dependency graph.
#[derive(Debug, Clone)]
pub struct FilePath {
    /// Files from source to target.
    pub files: Vec<IndexedFile>,
}

impl FilePath {
    /// Create a new file path, validating invariants.
    ///
    /// Returns `None` if `files` is empty.
    pub fn new(files: Vec<IndexedFile>) -> Option<Self> {
        if files.is_empty() {
            return None;
        }
        Some(Self { files })
    }

    /// Create a trivial path with a single file.
    pub fn single(file: IndexedFile) -> Self {
        Self { files: vec![file] }
    }
}
