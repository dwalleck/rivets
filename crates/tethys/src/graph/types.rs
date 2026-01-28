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
    #[must_use]
    pub fn total_caller_count(&self) -> usize {
        self.direct_callers.len() + self.transitive_callers.len()
    }
}

/// A path through the call graph.
#[derive(Debug, Clone)]
pub struct CallPath {
    /// Symbols from source to target.
    symbols: Vec<Symbol>,
    /// The relationship at each step.
    edges: Vec<ReferenceKind>,
}

impl CallPath {
    /// Create a new call path, validating invariants.
    ///
    /// Returns `None` if:
    /// - `symbols` is empty
    /// - `edges.len()` does not equal `symbols.len() - 1`
    #[must_use]
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
    #[must_use]
    pub fn single(symbol: Symbol) -> Self {
        Self {
            symbols: vec![symbol],
            edges: vec![],
        }
    }

    /// Get the symbols in this path.
    #[must_use]
    pub fn symbols(&self) -> &[Symbol] {
        &self.symbols
    }

    /// Get the edges (reference kinds) between symbols.
    #[must_use]
    pub fn edges(&self) -> &[ReferenceKind] {
        &self.edges
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
    #[must_use]
    pub fn total_dependent_count(&self) -> usize {
        self.direct_dependents.len() + self.transitive_dependents.len()
    }
}

/// A path through the file dependency graph.
#[derive(Debug, Clone)]
pub struct FilePath {
    /// Files from source to target.
    files: Vec<IndexedFile>,
}

impl FilePath {
    /// Create a new file path, validating invariants.
    ///
    /// Returns `None` if `files` is empty.
    #[must_use]
    pub fn new(files: Vec<IndexedFile>) -> Option<Self> {
        if files.is_empty() {
            return None;
        }
        Some(Self { files })
    }

    /// Create a trivial path with a single file.
    #[must_use]
    pub fn single(file: IndexedFile) -> Self {
        Self { files: vec![file] }
    }

    /// Get the files in this path.
    #[must_use]
    pub fn files(&self) -> &[IndexedFile] {
        &self.files
    }

    /// Consume the path and return the files.
    #[must_use]
    pub fn into_files(self) -> Vec<IndexedFile> {
        self.files
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FileId, Language, SymbolId, SymbolKind, Visibility};
    use std::path::PathBuf;

    /// Create a test symbol with minimal required fields.
    fn make_test_symbol(id: i64, name: &str) -> Symbol {
        Symbol {
            id: SymbolId::from(id),
            file_id: FileId::from(1),
            name: name.to_string(),
            module_path: "test".to_string(),
            qualified_name: name.to_string(),
            kind: SymbolKind::Function,
            line: 1,
            column: 1,
            span: None,
            signature: None,
            signature_details: None,
            visibility: Visibility::Public,
            parent_symbol_id: None,
        }
    }

    /// Create a test indexed file with minimal required fields.
    fn make_test_file(id: i64, path: &str) -> IndexedFile {
        IndexedFile {
            id: FileId::from(id),
            path: PathBuf::from(path),
            language: Language::Rust,
            mtime_ns: 0,
            size_bytes: 0,
            content_hash: None,
            indexed_at: 0,
        }
    }

    // === CallPath invariant tests ===

    #[test]
    fn call_path_new_returns_none_for_empty_symbols() {
        let result = CallPath::new(vec![], vec![]);
        assert!(
            result.is_none(),
            "CallPath::new should return None for empty symbols"
        );
    }

    #[test]
    fn call_path_new_returns_none_for_mismatched_edge_count() {
        let sym1 = make_test_symbol(1, "foo");
        let sym2 = make_test_symbol(2, "bar");

        // Two symbols should have exactly one edge (edges.len() == symbols.len() - 1)
        // Provide zero edges - should fail
        let result = CallPath::new(vec![sym1.clone(), sym2.clone()], vec![]);
        assert!(
            result.is_none(),
            "CallPath::new should return None when edges.len() != symbols.len() - 1"
        );

        // Provide two edges for two symbols - should fail
        let result = CallPath::new(
            vec![sym1.clone(), sym2.clone()],
            vec![ReferenceKind::Call, ReferenceKind::Call],
        );
        assert!(
            result.is_none(),
            "CallPath::new should return None when edges.len() > symbols.len() - 1"
        );
    }

    #[test]
    fn call_path_new_accepts_valid_inputs() {
        // Single symbol, zero edges
        let sym1 = make_test_symbol(1, "foo");
        let result = CallPath::new(vec![sym1.clone()], vec![]);
        assert!(
            result.is_some(),
            "CallPath::new should accept single symbol with no edges"
        );
        let path = result.expect("should be valid");
        assert_eq!(path.symbols().len(), 1);
        assert_eq!(path.edges().len(), 0);

        // Two symbols, one edge
        let sym2 = make_test_symbol(2, "bar");
        let result = CallPath::new(vec![sym1.clone(), sym2.clone()], vec![ReferenceKind::Call]);
        assert!(
            result.is_some(),
            "CallPath::new should accept two symbols with one edge"
        );
        let path = result.expect("should be valid");
        assert_eq!(path.symbols().len(), 2);
        assert_eq!(path.edges().len(), 1);
        assert_eq!(path.edges()[0], ReferenceKind::Call);

        // Three symbols, two edges
        let sym3 = make_test_symbol(3, "baz");
        let result = CallPath::new(
            vec![sym1, sym2, sym3],
            vec![ReferenceKind::Call, ReferenceKind::Type],
        );
        assert!(
            result.is_some(),
            "CallPath::new should accept three symbols with two edges"
        );
        let path = result.expect("should be valid");
        assert_eq!(path.symbols().len(), 3);
        assert_eq!(path.edges().len(), 2);
    }

    #[test]
    fn call_path_single_creates_trivial_path() {
        let sym = make_test_symbol(1, "foo");
        let path = CallPath::single(sym.clone());

        assert_eq!(path.symbols().len(), 1);
        assert_eq!(path.symbols()[0].name, "foo");
        assert!(path.edges().is_empty());
    }

    // === FilePath invariant tests ===

    #[test]
    fn file_path_new_returns_none_for_empty_files() {
        let result = FilePath::new(vec![]);
        assert!(
            result.is_none(),
            "FilePath::new should return None for empty files"
        );
    }

    #[test]
    fn file_path_new_accepts_valid_inputs() {
        // Single file
        let file1 = make_test_file(1, "src/main.rs");
        let result = FilePath::new(vec![file1.clone()]);
        assert!(result.is_some(), "FilePath::new should accept single file");
        let path = result.expect("should be valid");
        assert_eq!(path.files().len(), 1);
        assert_eq!(path.files()[0].path, PathBuf::from("src/main.rs"));

        // Multiple files
        let file2 = make_test_file(2, "src/lib.rs");
        let file3 = make_test_file(3, "src/util.rs");
        let result = FilePath::new(vec![file1, file2, file3]);
        assert!(
            result.is_some(),
            "FilePath::new should accept multiple files"
        );
        let path = result.expect("should be valid");
        assert_eq!(path.files().len(), 3);
    }

    #[test]
    fn file_path_single_creates_trivial_path() {
        let file = make_test_file(1, "src/main.rs");
        let path = FilePath::single(file);

        assert_eq!(path.files().len(), 1);
        assert_eq!(path.files()[0].path, PathBuf::from("src/main.rs"));
    }

    #[test]
    fn file_path_into_files_returns_owned_files() {
        let first_file = make_test_file(1, "src/main.rs");
        let second_file = make_test_file(2, "src/lib.rs");
        let path = FilePath::new(vec![first_file, second_file]).expect("should be valid");

        let files = path.into_files();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, PathBuf::from("src/main.rs"));
        assert_eq!(files[1].path, PathBuf::from("src/lib.rs"));
    }
}
