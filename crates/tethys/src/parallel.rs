//! Parallel file parsing infrastructure.
//!
//! This module provides types for parallelizing file indexing:
//!
//! - **`ParsedFileData`**: Holds parsed file results for transfer across threads
//! - **`OwnedSymbolData`**: Owned version of `SymbolData` for thread-safe transfer
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                     index_with_options                       │
//! ├─────────────────────────────────────────────────────────────┤
//! │  Phase 1a (Parallel):    rayon::par_iter() file parsing     │
//! │  Phase 1b (Sequential):  Database writes + dependency calc   │
//! │  Phase 2  (Sequential):  Cross-file reference resolution     │
//! │  Phase 3  (Sequential):  LSP-based resolution (optional)     │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Usage
//!
//! ```ignore
//! let parsed_files: Vec<ParsedFileData> = source_files
//!     .par_iter()
//!     .filter_map(|(path, lang)| {
//!         Tethys::parse_file_static(&workspace_root, path, *lang).ok()
//!     })
//!     .collect();
//!
//! for data in parsed_files {
//!     tethys.write_parsed_file(&data, &mut pending)?;
//! }
//! ```

use std::path::PathBuf;

use crate::db::SymbolData;
use crate::languages::common::{ExtractedAttribute, ExtractedReference, ImportStatement};
use crate::types::{Language, Span, SymbolId, SymbolKind, Visibility};

/// Parsed file data ready for database insertion.
///
/// This struct holds all the extracted information from a single file,
/// allowing it to be passed from worker threads to the main thread for
/// sequential database writes.
#[derive(Debug)]
pub struct ParsedFileData {
    /// Path relative to workspace root
    pub relative_path: PathBuf,
    /// Detected language
    pub language: Language,
    /// File modification time in nanoseconds since Unix epoch
    pub mtime_ns: i64,
    /// File size in bytes
    pub size_bytes: u64,
    /// Extracted symbols
    pub symbols: Vec<OwnedSymbolData>,
    /// Extracted references
    pub references: Vec<ExtractedReference>,
    /// Extracted imports
    pub imports: Vec<ImportStatement>,
}

/// Owned version of `SymbolData` for thread-safe transfer.
///
/// `SymbolData` uses borrowed strings for efficiency during single-threaded
/// indexing. This owned version is used when transferring parsed data across
/// threads via the parallel parsing infrastructure.
#[derive(Debug, Clone)]
pub struct OwnedSymbolData {
    pub name: String,
    pub module_path: String,
    pub qualified_name: String,
    pub kind: SymbolKind,
    pub line: u32,
    pub column: u32,
    pub span: Option<Span>,
    pub signature: Option<String>,
    pub visibility: Visibility,
    pub parent_symbol_id: Option<SymbolId>,
    /// Whether this symbol is a test function.
    pub is_test: bool,
    /// Attributes attached to this symbol (e.g. `#[derive(Clone)]`, `#[source]`).
    pub attributes: Vec<ExtractedAttribute>,
}

impl ParsedFileData {
    /// Asserts struct invariants in debug builds.
    ///
    /// - `relative_path` must not be absolute.
    #[cfg(debug_assertions)]
    pub fn debug_assert_valid(&self) {
        debug_assert!(
            !self.relative_path.is_absolute(),
            "relative_path should not be absolute: {}",
            self.relative_path.display()
        );
    }

    /// No-op in release builds.
    #[cfg(not(debug_assertions))]
    #[inline]
    pub fn debug_assert_valid(&self) {}
}

impl OwnedSymbolData {
    /// Asserts struct invariants in debug builds.
    ///
    /// - `name` must not be empty.
    /// - `line` must be >= 1.
    #[cfg(debug_assertions)]
    pub fn debug_assert_valid(&self) {
        debug_assert!(!self.name.is_empty(), "symbol name should not be empty");
        debug_assert!(
            self.line >= 1,
            "symbol line should be >= 1, got {} for '{}'",
            self.line,
            self.name
        );
    }

    /// No-op in release builds.
    #[cfg(not(debug_assertions))]
    #[inline]
    pub fn debug_assert_valid(&self) {}

    /// Convert to borrowed `SymbolData` for database insertion.
    pub fn as_symbol_data(&self) -> SymbolData<'_> {
        SymbolData {
            name: &self.name,
            module_path: &self.module_path,
            qualified_name: &self.qualified_name,
            kind: self.kind,
            line: self.line,
            column: self.column,
            span: self.span,
            signature: self.signature.as_deref(),
            visibility: self.visibility,
            parent_symbol_id: self.parent_symbol_id,
            is_test: self.is_test,
            attributes: &self.attributes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owned_symbol_data_converts_to_symbol_data() {
        let owned = OwnedSymbolData {
            name: "foo".to_string(),
            module_path: "crate::bar".to_string(),
            qualified_name: "crate::bar::foo".to_string(),
            kind: SymbolKind::Function,
            line: 10,
            column: 4,
            span: Span::new(10, 4, 15, 1), // Span::new returns Option<Span>
            signature: Some("fn foo() -> i32".to_string()),
            visibility: Visibility::Public,
            parent_symbol_id: None,
            is_test: false,
            attributes: Vec::new(),
        };

        let borrowed = owned.as_symbol_data();

        assert_eq!(borrowed.name, "foo");
        assert_eq!(borrowed.module_path, "crate::bar");
        assert_eq!(borrowed.qualified_name, "crate::bar::foo");
        assert_eq!(borrowed.kind, SymbolKind::Function);
        assert_eq!(borrowed.line, 10);
        assert!(!borrowed.is_test);
    }

    #[test]
    fn parsed_file_data_can_be_created() {
        let data = ParsedFileData {
            relative_path: PathBuf::from("src/main.rs"),
            language: Language::Rust,
            mtime_ns: 1_234_567_890,
            size_bytes: 100,
            symbols: vec![],
            references: vec![],
            imports: vec![],
        };

        assert_eq!(data.relative_path, PathBuf::from("src/main.rs"));
        assert_eq!(data.language, Language::Rust);
    }

    #[test]
    fn owned_symbol_data_struct_literal_construction() {
        let owned = OwnedSymbolData {
            name: "test_fn".to_string(),
            module_path: "crate::module".to_string(),
            qualified_name: "crate::module::test_fn".to_string(),
            kind: SymbolKind::Function,
            line: 5,
            column: 0,
            span: None,
            signature: Some("fn test_fn()".to_string()),
            visibility: Visibility::Public,
            parent_symbol_id: None,
            is_test: true,
            attributes: Vec::new(),
        };

        assert_eq!(owned.name, "test_fn");
        assert_eq!(owned.line, 5);
        assert_eq!(owned.kind, SymbolKind::Function);
        assert!(owned.is_test);
    }
}
