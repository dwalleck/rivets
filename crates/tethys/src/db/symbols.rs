//! Symbol CRUD operations for the Tethys index.

use rusqlite::OptionalExtension;
use rusqlite::params;
use tracing::trace;

use super::{Index, SYMBOLS_COLUMNS, row_to_symbol};
use crate::error::Result;
use crate::types::{FileId, Symbol, SymbolId, SymbolKind};

/// Parameters for inserting a symbol into the index (test-only).
#[cfg(test)]
pub(crate) struct InsertSymbolParams<'a> {
    pub file_id: FileId,
    pub name: &'a str,
    pub module_path: &'a str,
    pub qualified_name: &'a str,
    pub kind: SymbolKind,
    pub line: u32,
    pub column: u32,
    pub span: Option<crate::types::Span>,
    pub signature: Option<&'a str>,
    pub visibility: crate::types::Visibility,
    pub parent_symbol_id: Option<SymbolId>,
    pub is_test: bool,
}

impl Index {
    /// Insert a symbol, returning the symbol ID.
    #[cfg(test)]
    pub fn insert_symbol(&self, params: &InsertSymbolParams<'_>) -> Result<SymbolId> {
        let conn = self.connection()?;

        conn.execute(
            "INSERT INTO symbols (file_id, name, module_path, qualified_name, kind, line, column,
             end_line, end_column, signature, visibility, parent_symbol_id, is_test)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                params.file_id.as_i64(),
                params.name,
                params.module_path,
                params.qualified_name,
                params.kind.as_str(),
                params.line,
                params.column,
                params.span.map(|s| s.end_line()),
                params.span.map(|s| s.end_column()),
                params.signature,
                params.visibility.as_str(),
                params.parent_symbol_id.map(SymbolId::as_i64),
                params.is_test
            ],
        )?;
        Ok(SymbolId::from(conn.last_insert_rowid()))
    }

    /// List symbols in a file.
    pub fn list_symbols_in_file(&self, file_id: FileId) -> Result<Vec<Symbol>> {
        let conn = self.connection()?;

        let mut stmt = conn.prepare(&format!(
            "SELECT {SYMBOLS_COLUMNS} FROM symbols WHERE file_id = ?1 ORDER BY line"
        ))?;

        let symbols = stmt
            .query_map([file_id.as_i64()], row_to_symbol)?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(symbols)
    }

    /// Search symbols by name pattern.
    pub fn search_symbols(&self, query: &str, limit: usize) -> Result<Vec<Symbol>> {
        if query.is_empty() {
            return Ok(vec![]);
        }

        let pattern = format!("%{query}%");
        let limit_i64 = i64::try_from(limit).unwrap_or(i64::MAX);
        let conn = self.connection()?;

        let mut stmt = conn.prepare(&format!(
            "SELECT {SYMBOLS_COLUMNS} FROM symbols \
             WHERE name LIKE ?1 OR qualified_name LIKE ?1 \
             ORDER BY CASE WHEN name = ?2 THEN 0 ELSE 1 END, length(qualified_name) \
             LIMIT ?3"
        ))?;

        let symbols = stmt
            .query_map(params![pattern, query, limit_i64], row_to_symbol)?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(symbols)
    }

    /// Get a symbol by its database ID.
    pub fn get_symbol_by_id(&self, id: SymbolId) -> Result<Option<Symbol>> {
        trace!(symbol_id = %id, "Looking up symbol by ID");
        let conn = self.connection()?;

        conn.query_row(
            &format!("SELECT {SYMBOLS_COLUMNS} FROM symbols WHERE id = ?1"),
            [id.as_i64()],
            row_to_symbol,
        )
        .optional()
        .map_err(Into::into)
    }

    /// Get a symbol by its qualified name (exact match).
    pub fn get_symbol_by_qualified_name(&self, qualified_name: &str) -> Result<Option<Symbol>> {
        trace!(qualified_name = %qualified_name, "Looking up symbol by qualified name");
        let conn = self.connection()?;

        conn.query_row(
            &format!("SELECT {SYMBOLS_COLUMNS} FROM symbols WHERE qualified_name = ?1"),
            [qualified_name],
            row_to_symbol,
        )
        .optional()
        .map_err(Into::into)
    }

    /// Search symbols by their kind (e.g., `SymbolKind::Module` for namespaces).
    ///
    /// This is used to build namespace-to-file maps for C# dependency resolution.
    pub fn search_symbols_by_kind(&self, kind: SymbolKind, limit: usize) -> Result<Vec<Symbol>> {
        let limit_i64 = i64::try_from(limit).unwrap_or(i64::MAX);
        let conn = self.connection()?;

        let mut stmt = conn.prepare(&format!(
            "SELECT {SYMBOLS_COLUMNS} FROM symbols WHERE kind = ?1 LIMIT ?2"
        ))?;

        let symbols = stmt
            .query_map(params![kind.as_str(), limit_i64], row_to_symbol)?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(symbols)
    }

    /// Get all test symbols in the index.
    ///
    /// Returns all symbols where `is_test = true`, useful for test topology
    /// analysis and "affected tests" queries.
    pub fn get_test_symbols(&self) -> Result<Vec<Symbol>> {
        let conn = self.connection()?;

        let mut stmt = conn.prepare(&format!(
            "SELECT {SYMBOLS_COLUMNS} FROM symbols WHERE is_test = 1 ORDER BY file_id, line"
        ))?;

        let symbols = stmt
            .query_map([], row_to_symbol)?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(symbols)
    }

    /// Search for a symbol by name within a specific file.
    ///
    /// This is used in Pass 2 for cross-file reference resolution. Given a symbol
    /// name and the file ID it should be defined in, find the matching symbol.
    pub fn search_symbol_in_file(&self, name: &str, file_id: FileId) -> Result<Option<Symbol>> {
        trace!(
            symbol_name = %name,
            file_id = %file_id,
            "Searching for symbol in file"
        );

        let conn = self.connection()?;

        // Try exact name match in the specified file
        let result = conn
            .query_row(
                &format!(
                    "SELECT {SYMBOLS_COLUMNS} FROM symbols WHERE name = ?1 AND file_id = ?2 LIMIT 1"
                ),
                params![name, file_id.as_i64()],
                row_to_symbol,
            )
            .optional()?;

        if result.is_some() {
            return Ok(result);
        }

        // Also try matching by qualified_name for nested symbols (e.g., "Struct::method")
        // where the reference might be to the nested name
        let qualified_pattern = format!("%::{name}");
        conn.query_row(
            &format!(
                "SELECT {SYMBOLS_COLUMNS} FROM symbols \
                 WHERE qualified_name LIKE ?1 AND file_id = ?2 LIMIT 1"
            ),
            params![qualified_pattern, file_id.as_i64()],
            row_to_symbol,
        )
        .optional()
        .map_err(Into::into)
    }

    /// Search for a symbol by qualified name in a specific file.
    ///
    /// This is used for resolving qualified references like `Index::open` where
    /// we know the module (file) the type is imported from.
    pub fn search_symbol_by_qualified_name_in_file(
        &self,
        qualified_name: &str,
        file_id: FileId,
    ) -> Result<Option<Symbol>> {
        trace!(
            qualified_name = %qualified_name,
            file_id = %file_id,
            "Searching for symbol by qualified name in file"
        );

        let conn = self.connection()?;

        conn.query_row(
            &format!(
                "SELECT {SYMBOLS_COLUMNS} FROM symbols \
                 WHERE qualified_name = ?1 AND file_id = ?2 LIMIT 1"
            ),
            params![qualified_name, file_id.as_i64()],
            row_to_symbol,
        )
        .optional()
        .map_err(Into::into)
    }

    /// Search for a symbol by name, restricted to files whose path begins
    /// with `path_prefix`.
    ///
    /// Used by `fallback_symbol_search` to prefer same-crate matches before
    /// falling back to the unscoped [`Self::search_symbol_by_name`]. The
    /// prefix is typically the caller's containing crate path with a
    /// trailing separator (e.g. `"crates/tethys/"`).
    ///
    /// Returns `None` if no symbol with that name exists under the prefix.
    pub fn search_symbol_by_name_in_path_prefix(
        &self,
        name: &str,
        path_prefix: &str,
    ) -> Result<Option<Symbol>> {
        debug_assert!(!path_prefix.is_empty(), "path_prefix must not be empty");
        let conn = self.connection()?;
        let like_pattern = format!("{path_prefix}%");
        conn.query_row(
            &format!(
                "SELECT {SYMBOLS_COLUMNS} FROM symbols
                 WHERE name = ?1
                   AND file_id IN (SELECT id FROM files WHERE path LIKE ?2)
                 LIMIT 1"
            ),
            params![name, like_pattern],
            row_to_symbol,
        )
        .optional()
        .map_err(Into::into)
    }

    /// Search for a symbol by name across all files.
    ///
    /// This is a fallback for glob imports where we don't know which specific
    /// file the symbol comes from.
    pub fn search_symbol_by_name(&self, name: &str) -> Result<Option<Symbol>> {
        trace!(
            symbol_name = %name,
            "Searching for symbol by name (any file)"
        );

        let conn = self.connection()?;

        conn.query_row(
            &format!("SELECT {SYMBOLS_COLUMNS} FROM symbols WHERE name = ?1 LIMIT 1"),
            [name],
            row_to_symbol,
        )
        .optional()
        .map_err(Into::into)
    }

    /// Find a symbol at a specific file and line.
    ///
    /// This is used to match LSP `goto_definition` results to our indexed symbols.
    /// The LSP returns file path and line number; we find the symbol defined at that line.
    ///
    /// Returns the symbol whose definition starts at the given line. If multiple symbols
    /// are defined on the same line, returns the one with the lowest column number.
    pub fn find_symbol_at_line(&self, file_id: FileId, line: u32) -> Result<Option<Symbol>> {
        trace!(
            file_id = %file_id,
            line = line,
            "Finding symbol at line"
        );

        let conn = self.connection()?;

        conn.query_row(
            &format!(
                "SELECT {SYMBOLS_COLUMNS} FROM symbols \
                 WHERE file_id = ?1 AND line = ?2 ORDER BY column ASC LIMIT 1"
            ),
            params![file_id.as_i64(), line],
            row_to_symbol,
        )
        .optional()
        .map_err(Into::into)
    }
}

#[cfg(test)]
mod search_in_prefix_tests {
    use super::*;
    use crate::db::Index;
    use crate::types::{Language, Visibility};
    use tempfile::TempDir;

    /// Set up an index with two synthetic crates, each defining a symbol `Foo`.
    /// Returns `(dir, index, foo_in_a_id, foo_in_b_id)`. Dir kept alive by caller.
    fn two_crate_workspace_with_shared_foo() -> (TempDir, Index, SymbolId, SymbolId) {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut index = Index::open(&dir.path().join("idx.db")).expect("open");

        let file_a = index
            .upsert_file(
                std::path::Path::new("crate_a/src/lib.rs"),
                Language::Rust,
                0,
                0,
                None,
            )
            .expect("file a");
        let file_b = index
            .upsert_file(
                std::path::Path::new("crate_b/src/lib.rs"),
                Language::Rust,
                0,
                0,
                None,
            )
            .expect("file b");

        let foo_in_a = index
            .insert_symbol(&InsertSymbolParams {
                file_id: file_a,
                name: "Foo",
                module_path: "crate",
                qualified_name: "crate_a::Foo",
                kind: SymbolKind::Struct,
                line: 1,
                column: 1,
                span: None,
                signature: Some("pub struct Foo"),
                visibility: Visibility::Public,
                parent_symbol_id: None,
                is_test: false,
            })
            .expect("foo in a");
        let foo_in_b = index
            .insert_symbol(&InsertSymbolParams {
                file_id: file_b,
                name: "Foo",
                module_path: "crate",
                qualified_name: "crate_b::Foo",
                kind: SymbolKind::Struct,
                line: 1,
                column: 1,
                span: None,
                signature: Some("pub struct Foo"),
                visibility: Visibility::Public,
                parent_symbol_id: None,
                is_test: false,
            })
            .expect("foo in b");

        (dir, index, foo_in_a, foo_in_b)
    }

    #[test]
    fn returns_same_crate_match_when_caller_in_crate_a() {
        let (_dir, index, foo_in_a, _) = two_crate_workspace_with_shared_foo();
        let result = index
            .search_symbol_by_name_in_path_prefix("Foo", "crate_a/")
            .expect("query")
            .expect("found");
        assert_eq!(
            result.id, foo_in_a,
            "must return crate_a's Foo, not crate_b's"
        );
    }

    #[test]
    fn returns_same_crate_match_when_caller_in_crate_b() {
        let (_dir, index, _, foo_in_b) = two_crate_workspace_with_shared_foo();
        let result = index
            .search_symbol_by_name_in_path_prefix("Foo", "crate_b/")
            .expect("query")
            .expect("found");
        assert_eq!(
            result.id, foo_in_b,
            "must return crate_b's Foo, not crate_a's"
        );
    }

    #[test]
    fn returns_none_when_prefix_matches_no_files() {
        let (_dir, index, _, _) = two_crate_workspace_with_shared_foo();
        let result = index
            .search_symbol_by_name_in_path_prefix("Foo", "crate_c/")
            .expect("query");
        assert!(
            result.is_none(),
            "must return None when no file's path begins with the prefix, got {result:?}"
        );
    }
}
