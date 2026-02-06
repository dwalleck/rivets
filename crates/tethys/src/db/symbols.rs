//! Symbol CRUD operations for the Tethys index.

use rusqlite::params;
use rusqlite::OptionalExtension;
use tracing::trace;

use super::{row_to_symbol, Index, SYMBOLS_COLUMNS};
use crate::error::Result;
use crate::types::{FileId, Span, Symbol, SymbolId, SymbolKind, Visibility};

impl Index {
    /// Insert a symbol, returning the symbol ID.
    #[allow(dead_code)] // Public API, not yet used internally
    #[allow(clippy::too_many_arguments)] // Database row has many columns
    pub fn insert_symbol(
        &self,
        file_id: FileId,
        name: &str,
        module_path: &str,
        qualified_name: &str,
        kind: SymbolKind,
        line: u32,
        column: u32,
        span: Option<Span>,
        signature: Option<&str>,
        visibility: Visibility,
        parent_symbol_id: Option<SymbolId>,
        is_test: bool,
    ) -> Result<SymbolId> {
        let conn = self.connection()?;

        conn.execute(
            "INSERT INTO symbols (file_id, name, module_path, qualified_name, kind, line, column,
             end_line, end_column, signature, visibility, parent_symbol_id, is_test)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                file_id.as_i64(),
                name,
                module_path,
                qualified_name,
                kind.as_str(),
                line,
                column,
                span.map(|s| s.end_line()),
                span.map(|s| s.end_column()),
                signature,
                visibility.as_str(),
                parent_symbol_id.map(SymbolId::as_i64),
                is_test
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
        // usize limit fits in i64 on all supported platforms
        #[allow(clippy::cast_possible_wrap)]
        let limit_i64 = limit as i64;
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
        // usize limit fits in i64 on all supported platforms
        #[allow(clippy::cast_possible_wrap)]
        let limit_i64 = limit as i64;
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

    /// Get total counts for stats.
    #[allow(dead_code)] // Public API, not yet used internally
    pub fn get_counts(&self) -> Result<(usize, usize, usize)> {
        let conn = self.connection()?;

        let files: i64 = conn.query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))?;
        let symbols: i64 = conn.query_row("SELECT COUNT(*) FROM symbols", [], |row| row.get(0))?;
        let refs: i64 = conn.query_row("SELECT COUNT(*) FROM refs", [], |row| row.get(0))?;

        // Safety: COUNT(*) returns non-negative values
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        Ok((files as usize, symbols as usize, refs as usize))
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
