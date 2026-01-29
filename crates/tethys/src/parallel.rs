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
use crate::languages::common::{ExtractedReference, ImportStatement};
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

impl ParsedFileData {
    /// Create a new `ParsedFileData` with validated inputs.
    ///
    /// # Arguments
    /// * `relative_path` - Path relative to workspace root (should not be absolute)
    /// * `language` - The detected programming language
    /// * `mtime_ns` - File modification time in nanoseconds since Unix epoch
    /// * `size_bytes` - File size in bytes
    /// * `symbols` - Extracted symbols from the file
    /// * `references` - Extracted references from the file
    /// * `imports` - Extracted import statements
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        relative_path: PathBuf,
        language: Language,
        mtime_ns: i64,
        size_bytes: u64,
        symbols: Vec<OwnedSymbolData>,
        references: Vec<ExtractedReference>,
        imports: Vec<ImportStatement>,
    ) -> Self {
        debug_assert!(
            !relative_path.is_absolute(),
            "relative_path should not be absolute: {}",
            relative_path.display()
        );
        Self {
            relative_path,
            language,
            mtime_ns,
            size_bytes,
            symbols,
            references,
            imports,
        }
    }
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
}

impl OwnedSymbolData {
    /// Create a new `OwnedSymbolData` from extracted symbol information.
    ///
    /// # Arguments
    /// * `name` - The symbol's name (must not be empty)
    /// * `module_path` - The module path (e.g., `crate::module`)
    /// * `qualified_name` - The fully qualified name
    /// * `kind` - The kind of symbol (function, struct, etc.)
    /// * `line` - Line number (1-indexed)
    /// * `column` - Column number (0-indexed)
    /// * `span` - Optional span covering the entire symbol
    /// * `signature` - Optional type signature
    /// * `visibility` - The symbol's visibility
    /// * `parent_symbol_id` - Optional parent symbol ID for nested symbols
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        name: String,
        module_path: String,
        qualified_name: String,
        kind: SymbolKind,
        line: u32,
        column: u32,
        span: Option<Span>,
        signature: Option<String>,
        visibility: Visibility,
        parent_symbol_id: Option<SymbolId>,
    ) -> Self {
        debug_assert!(!name.is_empty(), "symbol name must not be empty");
        debug_assert!(line > 0, "line numbers should be 1-indexed");
        Self {
            name,
            module_path,
            qualified_name,
            kind,
            line,
            column,
            span,
            signature,
            visibility,
            parent_symbol_id,
        }
    }

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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owned_symbol_data_converts_to_symbol_data() {
        let owned = OwnedSymbolData::new(
            "foo".to_string(),
            "crate::bar".to_string(),
            "crate::bar::foo".to_string(),
            SymbolKind::Function,
            10,
            4,
            Span::new(10, 4, 15, 1), // Span::new returns Option<Span>
            Some("fn foo() -> i32".to_string()),
            Visibility::Public,
            None,
        );

        let borrowed = owned.as_symbol_data();

        assert_eq!(borrowed.name, "foo");
        assert_eq!(borrowed.module_path, "crate::bar");
        assert_eq!(borrowed.qualified_name, "crate::bar::foo");
        assert_eq!(borrowed.kind, SymbolKind::Function);
        assert_eq!(borrowed.line, 10);
    }

    #[test]
    fn parsed_file_data_can_be_created() {
        let data = ParsedFileData::new(
            PathBuf::from("src/main.rs"),
            Language::Rust,
            1_234_567_890,
            100,
            vec![],
            vec![],
            vec![],
        );

        assert_eq!(data.relative_path, PathBuf::from("src/main.rs"));
        assert_eq!(data.language, Language::Rust);
    }

    #[test]
    fn owned_symbol_data_new_constructor() {
        let owned = OwnedSymbolData::new(
            "test_fn".to_string(),
            "crate::module".to_string(),
            "crate::module::test_fn".to_string(),
            SymbolKind::Function,
            5,
            0,
            None,
            Some("fn test_fn()".to_string()),
            Visibility::Public,
            None,
        );

        assert_eq!(owned.name, "test_fn");
        assert_eq!(owned.line, 5);
        assert_eq!(owned.kind, SymbolKind::Function);
    }
}
