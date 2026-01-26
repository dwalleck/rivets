//! Graph operations for dependency analysis.
//!
//! This module provides traits and implementations for:
//! - Symbol-level queries (who calls this function?)
//! - File-level queries (what files depend on this?)
//! - Impact analysis (transitive dependents)
//! - Path finding (how does A relate to B?)
//!
//! ## Design
//!
//! - Traits define the operations (`SymbolGraphOps`, `FileGraphOps`)
//! - SQL implementations use recursive CTEs for traversal
//! - Petgraph can be swapped in later for specific algorithms

// TODO: Remove when implementations are integrated in Phase 3 Task 7
#![allow(dead_code)]
#![allow(unused_imports)]

mod types;

pub use types::{
    CallPath, CalleeInfo, CallerInfo, FileDepInfo, FileImpact, FilePath, SymbolImpact,
};

mod sql;

pub use sql::{SqlFileGraph, SqlSymbolGraph};

use crate::error::Result;
use crate::types::Cycle;

/// Operations on the symbol-level dependency graph.
///
/// Symbol graphs track "who calls what" at function/method granularity.
/// This enables precise impact analysis and execution flow understanding.
pub trait SymbolGraphOps: Send + Sync {
    /// Get symbols that directly call/reference the given symbol.
    fn get_callers(&self, symbol_id: i64) -> Result<Vec<CallerInfo>>;

    /// Get symbols that the given symbol directly calls/references.
    fn get_callees(&self, symbol_id: i64) -> Result<Vec<CalleeInfo>>;

    /// Get transitive callers (impact analysis).
    ///
    /// Returns all symbols that directly or indirectly call the target.
    fn get_transitive_callers(
        &self,
        symbol_id: i64,
        max_depth: Option<u32>,
    ) -> Result<SymbolImpact>;

    /// Find the shortest call path between two symbols.
    ///
    /// Returns `None` if no path exists.
    fn find_call_path(&self, from_symbol_id: i64, to_symbol_id: i64) -> Result<Option<CallPath>>;
}

/// Operations on the file-level dependency graph.
///
/// File graphs are coarser than symbol graphs but faster to traverse.
pub trait FileGraphOps: Send + Sync {
    /// Get files that directly depend on the given file.
    fn get_dependents(&self, file_id: i64) -> Result<Vec<FileDepInfo>>;

    /// Get files that the given file directly depends on.
    fn get_dependencies(&self, file_id: i64) -> Result<Vec<FileDepInfo>>;

    /// Get transitive dependents (file-level impact analysis).
    fn get_transitive_dependents(&self, file_id: i64, max_depth: Option<u32>)
        -> Result<FileImpact>;

    /// Find the shortest dependency path between two files.
    fn find_dependency_path(&self, from_file_id: i64, to_file_id: i64) -> Result<Option<FilePath>>;

    /// Detect circular dependencies in the codebase.
    fn detect_cycles(&self) -> Result<Vec<Cycle>>;

    /// Detect cycles involving a specific file.
    fn detect_cycles_involving(&self, file_id: i64) -> Result<Vec<Cycle>>;
}
