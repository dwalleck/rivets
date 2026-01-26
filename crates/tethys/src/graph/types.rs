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
    /// Total number of unique callers.
    pub total_caller_count: usize,
    /// Maximum depth reached during traversal.
    pub max_depth_reached: u32,
}

/// A path through the call graph.
#[derive(Debug, Clone)]
pub struct CallPath {
    /// Symbols from source to target.
    pub symbols: Vec<Symbol>,
    /// The relationship at each step.
    pub edges: Vec<ReferenceKind>,
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
    /// Total number of dependent files.
    pub total_dependent_count: usize,
}

/// A path through the file dependency graph.
#[derive(Debug, Clone)]
pub struct FilePath {
    /// Files from source to target.
    pub files: Vec<IndexedFile>,
}
