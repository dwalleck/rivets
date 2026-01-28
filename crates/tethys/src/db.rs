//! `SQLite` storage layer for Tethys.
//!
//! This module manages the `SQLite` database that stores indexed symbols and references.
//! `SQLite` is the source of truth for all persistent data. See `graph::sql` for
//! graph traversal queries built on top of this storage layer.

// SQLite uses i64 for all integer storage. These casts are intentional and safe for
// practical values (file sizes, line numbers, timestamps within reasonable bounds).
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::cast_sign_loss)]

use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::trace;

use crate::error::{Error, Result};
use crate::types::{
    FileId, Import, IndexedFile, Language, Reference, ReferenceKind, Span, Symbol, SymbolId,
    SymbolKind, Visibility,
};

/// Data required to insert a symbol into the database.
///
/// This is used by `index_file_atomic` to insert symbols within a transaction.
#[derive(Debug, Clone)]
pub struct SymbolData<'a> {
    pub name: &'a str,
    pub module_path: &'a str,
    pub qualified_name: &'a str,
    pub kind: SymbolKind,
    pub line: u32,
    pub column: u32,
    pub span: Option<Span>,
    pub signature: Option<&'a str>,
    pub visibility: Visibility,
    pub parent_symbol_id: Option<SymbolId>,
}

/// `SQLite` database wrapper for Tethys index.
pub struct Index {
    conn: Connection,
}

impl Index {
    /// Open or create the index database.
    pub fn open(path: &Path) -> Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(path)?;

        // Enable WAL mode and foreign keys
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;

        // Apply schema
        conn.execute_batch(SCHEMA)?;

        Ok(Self { conn })
    }

    /// Get the current unix timestamp in nanoseconds.
    ///
    /// Returns an error if the system time is before the Unix epoch, which would
    /// break timestamp comparison logic for incremental indexing.
    fn now_ns() -> Result<i64> {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as i64)
            .map_err(|e| {
                Error::Config(format!(
                    "System clock is before Unix epoch: {e}. Fix system time before indexing."
                ))
            })
    }

    // === File Operations ===

    /// Insert or update a file record, returning the file ID.
    ///
    /// Delegates to [`Self::index_file_atomic`] with an empty symbol list.
    #[allow(dead_code)] // Public API, not yet used internally
    pub fn upsert_file(
        &mut self,
        path: &Path,
        language: Language,
        mtime_ns: i64,
        size_bytes: u64,
        content_hash: Option<u64>,
    ) -> Result<FileId> {
        self.index_file_atomic(path, language, mtime_ns, size_bytes, content_hash, &[])
    }

    /// Get a file by path.
    pub fn get_file(&self, path: &Path) -> Result<Option<IndexedFile>> {
        let path_str = path.to_string_lossy();

        self.conn
            .query_row(
                "SELECT id, path, language, mtime_ns, size_bytes, content_hash, indexed_at
                 FROM files WHERE path = ?1",
                [&path_str],
                row_to_indexed_file,
            )
            .optional()
            .map_err(Into::into)
    }

    /// Get file ID by path.
    pub fn get_file_id(&self, path: &Path) -> Result<Option<FileId>> {
        let path_str = path.to_string_lossy();

        self.conn
            .query_row("SELECT id FROM files WHERE path = ?1", [&path_str], |row| {
                row.get::<_, i64>(0).map(FileId::from)
            })
            .optional()
            .map_err(Into::into)
    }

    /// Get a file by its database ID.
    pub fn get_file_by_id(&self, id: FileId) -> Result<Option<IndexedFile>> {
        self.conn
            .query_row(
                "SELECT id, path, language, mtime_ns, size_bytes, content_hash, indexed_at
                 FROM files WHERE id = ?1",
                [id.as_i64()],
                row_to_indexed_file,
            )
            .optional()
            .map_err(Into::into)
    }

    /// Atomically index a file with all its symbols in a transaction.
    ///
    /// This ensures that either the file and all symbols are stored, or nothing is.
    /// If any operation fails, the entire transaction is rolled back.
    pub fn index_file_atomic(
        &mut self,
        path: &Path,
        language: Language,
        mtime_ns: i64,
        size_bytes: u64,
        content_hash: Option<u64>,
        symbols: &[SymbolData],
    ) -> Result<FileId> {
        let tx = self.conn.transaction()?;

        let path_str = path.to_string_lossy();
        let lang_str = language.as_str();
        let indexed_at = Self::now_ns()?;

        // Try to update first
        let updated = tx.execute(
            "UPDATE files SET language = ?2, mtime_ns = ?3, size_bytes = ?4,
             content_hash = ?5, indexed_at = ?6 WHERE path = ?1",
            params![
                path_str,
                lang_str,
                mtime_ns,
                size_bytes as i64,
                content_hash.map(|h| h as i64),
                indexed_at
            ],
        )?;

        let file_id = if updated > 0 {
            // Get the existing ID
            let id: i64 =
                tx.query_row("SELECT id FROM files WHERE path = ?1", [&path_str], |row| {
                    row.get(0)
                })?;

            // Clear old symbols and imports for this file (for re-indexing)
            tx.execute("DELETE FROM symbols WHERE file_id = ?1", [id])?;
            tx.execute("DELETE FROM imports WHERE file_id = ?1", [id])?;
            id
        } else {
            // Insert new
            tx.execute(
                "INSERT INTO files (path, language, mtime_ns, size_bytes, content_hash, indexed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    path_str,
                    lang_str,
                    mtime_ns,
                    size_bytes as i64,
                    content_hash.map(|h| h as i64),
                    indexed_at
                ],
            )?;
            tx.last_insert_rowid()
        };

        // Insert all symbols
        for sym in symbols {
            tx.execute(
                "INSERT INTO symbols (file_id, name, module_path, qualified_name, kind, line, column,
                 end_line, end_column, signature, visibility, parent_symbol_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    file_id,
                    sym.name,
                    sym.module_path,
                    sym.qualified_name,
                    sym.kind.as_str(),
                    sym.line,
                    sym.column,
                    sym.span.map(|s| s.end_line()),
                    sym.span.map(|s| s.end_column()),
                    sym.signature,
                    sym.visibility.as_str(),
                    sym.parent_symbol_id.map(SymbolId::as_i64)
                ],
            )?;
        }

        tx.commit()?;
        Ok(FileId::from(file_id))
    }

    // === Symbol Operations ===

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
    ) -> Result<SymbolId> {
        self.conn.execute(
            "INSERT INTO symbols (file_id, name, module_path, qualified_name, kind, line, column,
             end_line, end_column, signature, visibility, parent_symbol_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
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
                parent_symbol_id.map(SymbolId::as_i64)
            ],
        )?;
        Ok(SymbolId::from(self.conn.last_insert_rowid()))
    }

    /// List symbols in a file.
    pub fn list_symbols_in_file(&self, file_id: FileId) -> Result<Vec<Symbol>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, file_id, name, module_path, qualified_name, kind, line, column,
             end_line, end_column, signature, visibility, parent_symbol_id
             FROM symbols WHERE file_id = ?1 ORDER BY line",
        )?;

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

        let mut stmt = self.conn.prepare(
            "SELECT id, file_id, name, module_path, qualified_name, kind, line, column,
             end_line, end_column, signature, visibility, parent_symbol_id
             FROM symbols
             WHERE name LIKE ?1 OR qualified_name LIKE ?1
             ORDER BY
                 CASE WHEN name = ?2 THEN 0 ELSE 1 END,
                 length(qualified_name)
             LIMIT ?3",
        )?;

        let symbols = stmt
            .query_map(params![pattern, query, limit as i64], row_to_symbol)?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(symbols)
    }

    /// Get a symbol by its database ID.
    pub fn get_symbol_by_id(&self, id: SymbolId) -> Result<Option<Symbol>> {
        trace!(symbol_id = %id, "Looking up symbol by ID");
        let mut stmt = self.conn.prepare(
            "SELECT id, file_id, name, module_path, qualified_name, kind, line, column,
             end_line, end_column, signature, visibility, parent_symbol_id
             FROM symbols WHERE id = ?1",
        )?;

        let mut rows = stmt.query([id.as_i64()])?;
        match rows.next()? {
            Some(row) => Ok(Some(row_to_symbol(row)?)),
            None => Ok(None),
        }
    }

    /// Get a symbol by its qualified name (exact match).
    pub fn get_symbol_by_qualified_name(&self, qualified_name: &str) -> Result<Option<Symbol>> {
        trace!(qualified_name = %qualified_name, "Looking up symbol by qualified name");
        let mut stmt = self.conn.prepare(
            "SELECT id, file_id, name, module_path, qualified_name, kind, line, column,
             end_line, end_column, signature, visibility, parent_symbol_id
             FROM symbols WHERE qualified_name = ?1",
        )?;

        let mut rows = stmt.query([qualified_name])?;
        match rows.next()? {
            Some(row) => Ok(Some(row_to_symbol(row)?)),
            None => Ok(None),
        }
    }

    /// Search symbols by their kind (e.g., `SymbolKind::Module` for namespaces).
    ///
    /// This is used to build namespace-to-file maps for C# dependency resolution.
    pub fn search_symbols_by_kind(&self, kind: SymbolKind, limit: usize) -> Result<Vec<Symbol>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, file_id, name, module_path, qualified_name, kind, line, column,
             end_line, end_column, signature, visibility, parent_symbol_id
             FROM symbols
             WHERE kind = ?1
             LIMIT ?2",
        )?;

        let symbols = stmt
            .query_map(params![kind.as_str(), limit as i64], row_to_symbol)?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(symbols)
    }

    /// Get total counts for stats.
    #[allow(dead_code)] // Public API, not yet used internally
    pub fn get_counts(&self) -> Result<(usize, usize, usize)> {
        let files: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))?;
        let symbols: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM symbols", [], |row| row.get(0))?;
        let refs: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM refs", [], |row| row.get(0))?;

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

        // Try exact name match in the specified file
        let mut stmt = self.conn.prepare(
            "SELECT id, file_id, name, module_path, qualified_name, kind, line, column,
             end_line, end_column, signature, visibility, parent_symbol_id
             FROM symbols
             WHERE name = ?1 AND file_id = ?2
             LIMIT 1",
        )?;

        let result = stmt
            .query_row(params![name, file_id.as_i64()], row_to_symbol)
            .optional()?;

        if result.is_some() {
            return Ok(result);
        }

        // Also try matching by qualified_name for nested symbols (e.g., "Struct::method")
        // where the reference might be to the nested name
        let mut stmt = self.conn.prepare(
            "SELECT id, file_id, name, module_path, qualified_name, kind, line, column,
             end_line, end_column, signature, visibility, parent_symbol_id
             FROM symbols
             WHERE qualified_name LIKE ?1 AND file_id = ?2
             LIMIT 1",
        )?;

        let qualified_pattern = format!("%::{name}");
        stmt.query_row(params![qualified_pattern, file_id.as_i64()], row_to_symbol)
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

        let mut stmt = self.conn.prepare(
            "SELECT id, file_id, name, module_path, qualified_name, kind, line, column,
             end_line, end_column, signature, visibility, parent_symbol_id
             FROM symbols
             WHERE qualified_name = ?1 AND file_id = ?2
             LIMIT 1",
        )?;

        stmt.query_row(params![qualified_name, file_id.as_i64()], row_to_symbol)
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

        let mut stmt = self.conn.prepare(
            "SELECT id, file_id, name, module_path, qualified_name, kind, line, column,
             end_line, end_column, signature, visibility, parent_symbol_id
             FROM symbols
             WHERE name = ?1
             LIMIT 1",
        )?;

        stmt.query_row([name], row_to_symbol)
            .optional()
            .map_err(Into::into)
    }

    // === Reference Operations ===
    // These operations support symbol-level "who calls X?" queries.
    // See graph/sql.rs for higher-level graph traversal using these primitives.

    /// Insert a reference to a symbol.
    ///
    /// If `symbol_id` is `None`, the reference is unresolved and `reference_name`
    /// should be provided for later resolution in Pass 2.
    #[allow(clippy::too_many_arguments)]
    pub fn insert_reference(
        &self,
        symbol_id: Option<SymbolId>,
        file_id: FileId,
        kind: &str,
        line: u32,
        column: u32,
        in_symbol_id: Option<SymbolId>,
        reference_name: Option<&str>,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO refs (symbol_id, file_id, kind, line, column, in_symbol_id, reference_name)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                symbol_id.map(SymbolId::as_i64),
                file_id.as_i64(),
                kind,
                line,
                column,
                in_symbol_id.map(SymbolId::as_i64),
                reference_name
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Get all unresolved references (where `symbol_id` is NULL).
    ///
    /// These references need to be resolved in Pass 2 by matching their
    /// `reference_name` to symbols discovered in other files.
    #[allow(dead_code)] // Public API, not yet used internally
    pub fn get_unresolved_references(&self) -> Result<Vec<Reference>> {
        trace!("Getting unresolved references");
        let mut stmt = self.conn.prepare(
            "SELECT id, symbol_id, file_id, kind, line, column, end_line, end_column, in_symbol_id, reference_name
             FROM refs WHERE symbol_id IS NULL ORDER BY file_id, line",
        )?;

        let refs = stmt
            .query_map([], row_to_reference)?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(refs)
    }

    /// Resolve a reference by setting its `symbol_id`.
    ///
    /// This is used in Pass 2 to link unresolved references to their target symbols
    /// after cross-file symbol resolution.
    #[allow(dead_code)] // Public API, not yet used internally
    pub fn resolve_reference(&self, ref_id: i64, symbol_id: SymbolId) -> Result<()> {
        trace!(
            ref_id = ref_id,
            symbol_id = %symbol_id,
            "Resolving reference"
        );
        self.conn.execute(
            "UPDATE refs SET symbol_id = ?2, reference_name = NULL WHERE id = ?1",
            params![ref_id, symbol_id.as_i64()],
        )?;
        Ok(())
    }

    /// Get all references to a symbol.
    pub fn get_references_to_symbol(&self, symbol_id: SymbolId) -> Result<Vec<Reference>> {
        trace!(symbol_id = %symbol_id, "Getting references to symbol");
        let mut stmt = self.conn.prepare(
            "SELECT id, symbol_id, file_id, kind, line, column, end_line, end_column, in_symbol_id, reference_name
             FROM refs WHERE symbol_id = ?1 ORDER BY file_id, line",
        )?;

        let refs = stmt
            .query_map([symbol_id.as_i64()], row_to_reference)?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(refs)
    }

    /// List all outgoing references from a file.
    pub fn list_references_in_file(&self, file_id: FileId) -> Result<Vec<Reference>> {
        trace!(file_id = %file_id, "Listing references in file");
        let mut stmt = self.conn.prepare(
            "SELECT id, symbol_id, file_id, kind, line, column, end_line, end_column, in_symbol_id, reference_name
             FROM refs WHERE file_id = ?1 ORDER BY line, column",
        )?;

        let refs = stmt
            .query_map([file_id.as_i64()], row_to_reference)?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(refs)
    }

    /// Get all files of a specific language.
    ///
    /// Used for language-specific dependency resolution passes.
    pub fn get_files_by_language(&self, language: Language) -> Result<Vec<IndexedFile>> {
        let lang_str = language.as_str();
        let mut stmt = self.conn.prepare(
            "SELECT id, path, language, mtime_ns, size_bytes, content_hash, indexed_at
             FROM files WHERE language = ?1",
        )?;

        let files = stmt
            .query_map([lang_str], row_to_indexed_file)?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(files)
    }

    // === File Dependency Operations ===

    /// Insert or update a file-level dependency.
    ///
    /// Records that `from_file_id` depends on `to_file_id`.
    pub fn insert_file_dependency(&self, from_file_id: FileId, to_file_id: FileId) -> Result<()> {
        // Use upsert (ON CONFLICT) to handle duplicates (increments ref_count)
        self.conn.execute(
            "INSERT INTO file_deps (from_file_id, to_file_id, ref_count)
             VALUES (?1, ?2, 1)
             ON CONFLICT(from_file_id, to_file_id) DO UPDATE SET ref_count = ref_count + 1",
            params![from_file_id.as_i64(), to_file_id.as_i64()],
        )?;
        Ok(())
    }

    /// Get files that the given file depends on.
    pub fn get_file_dependencies(&self, file_id: FileId) -> Result<Vec<FileId>> {
        let mut stmt = self
            .conn
            .prepare("SELECT to_file_id FROM file_deps WHERE from_file_id = ?1")?;

        let deps = stmt
            .query_map([file_id.as_i64()], |row| {
                row.get::<_, i64>(0).map(FileId::from)
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(deps)
    }

    /// Get files that depend on the given file.
    pub fn get_file_dependents(&self, file_id: FileId) -> Result<Vec<FileId>> {
        let mut stmt = self
            .conn
            .prepare("SELECT from_file_id FROM file_deps WHERE to_file_id = ?1")?;

        let deps = stmt
            .query_map([file_id.as_i64()], |row| {
                row.get::<_, i64>(0).map(FileId::from)
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(deps)
    }

    // === Import Operations ===

    /// Insert an import record for cross-file reference resolution.
    ///
    /// Records that `file_id` imports `symbol_name` from `source_module`.
    /// Uses upsert semantics: if the import already exists, this is a no-op.
    pub fn insert_import(
        &self,
        file_id: FileId,
        symbol_name: &str,
        source_module: &str,
        alias: Option<&str>,
    ) -> Result<()> {
        trace!(
            file_id = %file_id,
            symbol_name = %symbol_name,
            source_module = %source_module,
            alias = ?alias,
            "Inserting import"
        );
        self.conn.execute(
            "INSERT OR REPLACE INTO imports (file_id, symbol_name, source_module, alias)
             VALUES (?1, ?2, ?3, ?4)",
            params![file_id.as_i64(), symbol_name, source_module, alias],
        )?;
        Ok(())
    }

    /// Get all imports for a file.
    ///
    /// Returns a list of all symbols imported by the given file.
    pub fn get_imports_for_file(&self, file_id: FileId) -> Result<Vec<Import>> {
        trace!(file_id = %file_id, "Getting imports for file");
        let mut stmt = self.conn.prepare(
            "SELECT file_id, symbol_name, source_module, alias
             FROM imports WHERE file_id = ?1 ORDER BY source_module, symbol_name",
        )?;

        let imports = stmt
            .query_map([file_id.as_i64()], row_to_import)?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(imports)
    }

    /// Clear all imports for a file (for re-indexing).
    ///
    /// Call this before re-indexing a file to remove stale imports.
    pub fn clear_imports_for_file(&self, file_id: FileId) -> Result<()> {
        trace!(file_id = %file_id, "Clearing imports for file");
        self.conn
            .execute("DELETE FROM imports WHERE file_id = ?1", [file_id.as_i64()])?;
        Ok(())
    }

    /// Clear all data from the database.
    pub fn clear(&self) -> Result<()> {
        self.conn.execute_batch(
            "DELETE FROM refs; DELETE FROM symbols; DELETE FROM file_deps; DELETE FROM imports; DELETE FROM files;",
        )?;
        Ok(())
    }

    /// Get statistics about the database contents.
    pub fn get_stats(&self) -> Result<crate::types::DatabaseStats> {
        use std::collections::HashMap;

        let mut stats = crate::types::DatabaseStats::default();

        // File counts by language
        let mut stmt = self
            .conn
            .prepare("SELECT language, COUNT(*) FROM files GROUP BY language")?;
        let rows = stmt.query_map([], |row| {
            let lang_str: String = row.get(0)?;
            let count: usize = row.get(1)?;
            Ok((lang_str, count))
        })?;

        let mut files_by_language: HashMap<crate::types::Language, usize> = HashMap::new();
        for row in rows {
            let (lang_str, count) = row?;
            if let Ok(lang) = parse_language(&lang_str) {
                files_by_language.insert(lang, count);
                stats.file_count += count;
            } else {
                tracing::warn!(
                    language = %lang_str,
                    count = count,
                    "Unknown language in database, skipping from stats"
                );
                stats.skipped_unknown_languages += count;
            }
        }
        stats.files_by_language = files_by_language;

        // Symbol counts by kind
        let mut stmt = self
            .conn
            .prepare("SELECT kind, COUNT(*) FROM symbols GROUP BY kind")?;
        let rows = stmt.query_map([], |row| {
            let kind_str: String = row.get(0)?;
            let count: usize = row.get(1)?;
            Ok((kind_str, count))
        })?;

        let mut symbols_by_kind: HashMap<crate::types::SymbolKind, usize> = HashMap::new();
        for row in rows {
            let (kind_str, count) = row?;
            if let Ok(kind) = parse_symbol_kind(&kind_str) {
                symbols_by_kind.insert(kind, count);
                stats.symbol_count += count;
            } else {
                tracing::warn!(
                    kind = %kind_str,
                    count = count,
                    "Unknown symbol kind in database, skipping from stats"
                );
                stats.skipped_unknown_kinds += count;
            }
        }
        stats.symbols_by_kind = symbols_by_kind;

        // Reference count
        let ref_count: usize = self
            .conn
            .query_row("SELECT COUNT(*) FROM refs", [], |row| row.get(0))?;
        stats.reference_count = ref_count;

        // File dependency count
        let dep_count: usize =
            self.conn
                .query_row("SELECT COUNT(*) FROM file_deps", [], |row| row.get(0))?;
        stats.file_dependency_count = dep_count;

        Ok(stats)
    }

    /// Vacuum the database.
    pub fn vacuum(&self) -> Result<()> {
        self.conn.execute_batch("VACUUM")?;
        Ok(())
    }
}

/// Parse a language string from the database.
///
/// Returns an error for unrecognized values, indicating possible database corruption.
pub(crate) fn parse_language(s: &str) -> rusqlite::Result<Language> {
    match s {
        "rust" => Ok(Language::Rust),
        "csharp" => Ok(Language::CSharp),
        unknown => Err(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            format!("Unknown language '{unknown}' in database. Database may be corrupted or from a newer version.").into(),
        )),
    }
}

/// Parse a symbol kind string from the database.
///
/// Returns an error for unrecognized values, indicating possible database corruption.
pub(crate) fn parse_symbol_kind(s: &str) -> rusqlite::Result<SymbolKind> {
    match s {
        "function" => Ok(SymbolKind::Function),
        "method" => Ok(SymbolKind::Method),
        "struct" => Ok(SymbolKind::Struct),
        "class" => Ok(SymbolKind::Class),
        "enum" => Ok(SymbolKind::Enum),
        "trait" => Ok(SymbolKind::Trait),
        "interface" => Ok(SymbolKind::Interface),
        "const" => Ok(SymbolKind::Const),
        "static" => Ok(SymbolKind::Static),
        "module" => Ok(SymbolKind::Module),
        "type_alias" => Ok(SymbolKind::TypeAlias),
        "macro" => Ok(SymbolKind::Macro),
        unknown => Err(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            format!("Unknown symbol kind '{unknown}' in database. Database may be corrupted or from a newer version.").into(),
        )),
    }
}

/// Parse a visibility string from the database.
///
/// Returns an error for unrecognized values, indicating possible database corruption.
pub(crate) fn parse_visibility(s: &str) -> rusqlite::Result<Visibility> {
    match s {
        "public" => Ok(Visibility::Public),
        "crate" => Ok(Visibility::Crate),
        "module" => Ok(Visibility::Module),
        "private" => Ok(Visibility::Private),
        unknown => Err(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            format!("Unknown visibility '{unknown}' in database. Database may be corrupted or from a newer version.").into(),
        )),
    }
}

/// Convert a database row to an [`IndexedFile`].
///
/// Expected columns: id, path, language, `mtime_ns`, `size_bytes`, `content_hash`, `indexed_at`
pub(crate) fn row_to_indexed_file(row: &rusqlite::Row) -> rusqlite::Result<IndexedFile> {
    Ok(IndexedFile {
        id: FileId::from(row.get::<_, i64>(0)?),
        path: PathBuf::from(row.get::<_, String>(1)?),
        language: parse_language(row.get::<_, String>(2)?.as_str())?,
        mtime_ns: row.get(3)?,
        size_bytes: row.get::<_, i64>(4)? as u64,
        content_hash: row.get::<_, Option<i64>>(5)?.map(|h| h as u64),
        indexed_at: row.get(6)?,
    })
}

/// Build a span from start and optional end positions.
///
/// Returns `None` if either `end_line` or `end_column` is missing, or if the
/// span would be invalid (end before start).
pub(crate) fn build_span(
    start_line: u32,
    start_column: u32,
    end_line: Option<u32>,
    end_column: Option<u32>,
) -> Option<Span> {
    end_line
        .zip(end_column)
        .and_then(|(el, ec)| Span::new(start_line, start_column, el, ec))
}

/// Convert a database row to a Reference.
fn row_to_reference(row: &rusqlite::Row) -> rusqlite::Result<Reference> {
    let line: u32 = row.get(4)?;
    let column: u32 = row.get(5)?;
    let end_line: Option<u32> = row.get(6)?;
    let end_column: Option<u32> = row.get(7)?;

    Ok(Reference {
        id: row.get(0)?,
        symbol_id: row.get::<_, Option<i64>>(1)?.map(SymbolId::from),
        file_id: FileId::from(row.get::<_, i64>(2)?),
        kind: parse_reference_kind(&row.get::<_, String>(3)?)?,
        line,
        column,
        span: build_span(line, column, end_line, end_column),
        in_symbol_id: row.get::<_, Option<i64>>(8)?.map(SymbolId::from),
        reference_name: row.get(9)?,
    })
}

/// Parse a reference kind string from the database.
///
/// Returns an error for unrecognized values, indicating possible database corruption.
fn parse_reference_kind(s: &str) -> rusqlite::Result<ReferenceKind> {
    match s {
        "import" => Ok(ReferenceKind::Import),
        "call" => Ok(ReferenceKind::Call),
        "type" => Ok(ReferenceKind::Type),
        "inherit" => Ok(ReferenceKind::Inherit),
        "construct" => Ok(ReferenceKind::Construct),
        "field_access" => Ok(ReferenceKind::FieldAccess),
        unknown => Err(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            format!("Unknown reference kind '{unknown}' in database. Database may be corrupted or from a newer version.").into(),
        )),
    }
}

/// Convert a database row to an Import.
fn row_to_import(row: &rusqlite::Row) -> rusqlite::Result<Import> {
    Ok(Import {
        file_id: FileId::from(row.get::<_, i64>(0)?),
        symbol_name: row.get(1)?,
        source_module: row.get(2)?,
        alias: row.get(3)?,
    })
}

/// Convert a database row to a Symbol.
pub(crate) fn row_to_symbol(row: &rusqlite::Row) -> rusqlite::Result<Symbol> {
    let line: u32 = row.get(6)?;
    let column: u32 = row.get(7)?;
    let end_line: Option<u32> = row.get(8)?;
    let end_column: Option<u32> = row.get(9)?;

    Ok(Symbol {
        id: SymbolId::from(row.get::<_, i64>(0)?),
        file_id: FileId::from(row.get::<_, i64>(1)?),
        name: row.get(2)?,
        module_path: row.get(3)?,
        qualified_name: row.get(4)?,
        kind: parse_symbol_kind(&row.get::<_, String>(5)?)?,
        line,
        column,
        span: build_span(line, column, end_line, end_column),
        signature: row.get(10)?,
        signature_details: None, // Not persisted to database; populated by parsers only
        visibility: parse_visibility(&row.get::<_, String>(11)?)?,
        parent_symbol_id: row.get::<_, Option<i64>>(12)?.map(SymbolId::from),
    })
}

/// Database schema definition.
const SCHEMA: &str = r"
-- Indexed source files
CREATE TABLE IF NOT EXISTS files (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    language TEXT NOT NULL,
    mtime_ns INTEGER NOT NULL,
    size_bytes INTEGER NOT NULL,
    content_hash INTEGER,
    indexed_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_files_path ON files(path);
CREATE INDEX IF NOT EXISTS idx_files_language ON files(language);

-- Symbol definitions
CREATE TABLE IF NOT EXISTS symbols (
    id INTEGER PRIMARY KEY,
    file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    module_path TEXT NOT NULL,
    qualified_name TEXT NOT NULL,
    kind TEXT NOT NULL,
    line INTEGER NOT NULL,
    column INTEGER NOT NULL,
    end_line INTEGER,
    end_column INTEGER,
    signature TEXT,
    visibility TEXT NOT NULL,
    parent_symbol_id INTEGER REFERENCES symbols(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name);
CREATE INDEX IF NOT EXISTS idx_symbols_module_path ON symbols(module_path);
CREATE INDEX IF NOT EXISTS idx_symbols_qualified ON symbols(qualified_name);
CREATE INDEX IF NOT EXISTS idx_symbols_file ON symbols(file_id);
CREATE INDEX IF NOT EXISTS idx_symbols_kind ON symbols(kind);

-- References (usages of symbols)
-- symbol_id is NULL for unresolved references (to be resolved in Pass 2)
-- reference_name stores the name for resolution (e.g., Index_open)
CREATE TABLE IF NOT EXISTS refs (
    id INTEGER PRIMARY KEY,
    symbol_id INTEGER REFERENCES symbols(id) ON DELETE CASCADE,
    file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    line INTEGER NOT NULL,
    column INTEGER NOT NULL,
    end_line INTEGER,
    end_column INTEGER,
    in_symbol_id INTEGER REFERENCES symbols(id) ON DELETE CASCADE,
    reference_name TEXT
);

CREATE INDEX IF NOT EXISTS idx_refs_symbol ON refs(symbol_id);
CREATE INDEX IF NOT EXISTS idx_refs_file ON refs(file_id);
CREATE INDEX IF NOT EXISTS idx_refs_in_symbol ON refs(in_symbol_id);
CREATE INDEX IF NOT EXISTS idx_refs_unresolved ON refs(symbol_id) WHERE symbol_id IS NULL;

-- File-level dependencies (denormalized for fast queries)
CREATE TABLE IF NOT EXISTS file_deps (
    from_file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    to_file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    ref_count INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (from_file_id, to_file_id)
);

CREATE INDEX IF NOT EXISTS idx_file_deps_to ON file_deps(to_file_id);

-- Import statements for cross-file reference resolution
CREATE TABLE IF NOT EXISTS imports (
    file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    symbol_name TEXT NOT NULL,      -- e.g. Index or * for globs
    source_module TEXT NOT NULL,    -- e.g. crate::db or MyApp.Services
    alias TEXT,                      -- for use foo as bar
    PRIMARY KEY (file_id, symbol_name, source_module)
);

CREATE INDEX IF NOT EXISTS idx_imports_file ON imports(file_id);
CREATE INDEX IF NOT EXISTS idx_imports_symbol ON imports(symbol_name);
";

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn temp_db() -> (TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("should create temp directory");
        let path = dir.path().join("test.db");
        (dir, path)
    }

    #[test]
    fn open_creates_database_and_schema() {
        let (_dir, path) = temp_db();

        let index = Index::open(&path).expect("failed to open database");

        let tables: Vec<String> = index
            .conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();

        assert!(tables.contains(&"files".to_string()));
        assert!(tables.contains(&"symbols".to_string()));
        assert!(tables.contains(&"refs".to_string()));
        assert!(tables.contains(&"file_deps".to_string()));
        assert!(tables.contains(&"imports".to_string()));
    }

    #[test]
    fn upsert_file_inserts_new_file() {
        let (_dir, path) = temp_db();
        let mut index = Index::open(&path).unwrap();

        let file_id = index
            .upsert_file(
                Path::new("src/main.rs"),
                Language::Rust,
                1_234_567_890,
                100,
                Some(0xDEAD_BEEF),
            )
            .unwrap();

        assert!(file_id.as_i64() > 0);

        let file = index.get_file(Path::new("src/main.rs")).unwrap();
        assert!(file.is_some());
        let file = file.unwrap();
        assert_eq!(file.language, Language::Rust);
        assert_eq!(file.size_bytes, 100);
    }

    #[test]
    fn upsert_file_updates_existing() {
        let (_dir, path) = temp_db();
        let mut index = Index::open(&path).unwrap();

        let id1 = index
            .upsert_file(Path::new("src/main.rs"), Language::Rust, 1000, 100, None)
            .unwrap();

        let id2 = index
            .upsert_file(Path::new("src/main.rs"), Language::Rust, 2000, 200, None)
            .unwrap();

        assert_eq!(id1, id2); // Same file, same ID

        let file = index.get_file(Path::new("src/main.rs")).unwrap().unwrap();
        assert_eq!(file.size_bytes, 200); // Updated
    }

    #[test]
    fn insert_and_list_symbols() {
        let (_dir, path) = temp_db();
        let mut index = Index::open(&path).unwrap();

        let file_id = index
            .upsert_file(Path::new("src/lib.rs"), Language::Rust, 1000, 100, None)
            .unwrap();

        index
            .insert_symbol(
                file_id,
                "foo",
                "crate",
                "foo",
                SymbolKind::Function,
                10,
                1,
                None,
                Some("fn foo()"),
                Visibility::Public,
                None,
            )
            .unwrap();

        index
            .insert_symbol(
                file_id,
                "bar",
                "crate",
                "bar",
                SymbolKind::Function,
                20,
                1,
                None,
                None,
                Visibility::Private,
                None,
            )
            .unwrap();

        let symbols = index.list_symbols_in_file(file_id).unwrap();
        assert_eq!(symbols.len(), 2);
        assert_eq!(symbols[0].name, "foo");
        assert_eq!(symbols[1].name, "bar");
    }

    #[test]
    fn search_symbols_finds_matches() {
        let (_dir, path) = temp_db();
        let mut index = Index::open(&path).unwrap();

        let file_id = index
            .upsert_file(Path::new("src/lib.rs"), Language::Rust, 1000, 100, None)
            .unwrap();

        index
            .insert_symbol(
                file_id,
                "authenticate",
                "crate::auth",
                "authenticate",
                SymbolKind::Function,
                10,
                1,
                None,
                None,
                Visibility::Public,
                None,
            )
            .unwrap();

        index
            .insert_symbol(
                file_id,
                "authorize",
                "crate::auth",
                "authorize",
                SymbolKind::Function,
                20,
                1,
                None,
                None,
                Visibility::Public,
                None,
            )
            .unwrap();

        let results = index.search_symbols("auth", 10).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn search_symbols_empty_query_returns_empty() {
        let (_dir, path) = temp_db();
        let index = Index::open(&path).unwrap();

        let results = index.search_symbols("", 10).unwrap();
        assert!(results.is_empty());
    }

    // === Parsing tests ===

    #[test]
    fn parse_language_known_values() {
        assert_eq!(parse_language("rust").unwrap(), Language::Rust);
        assert_eq!(parse_language("csharp").unwrap(), Language::CSharp);
    }

    #[test]
    fn parse_language_unknown_returns_error() {
        // Unknown values should return an error (database corruption)
        assert!(parse_language("python").is_err());
        assert!(parse_language("").is_err());
        assert!(parse_language("unknown").is_err());

        // Verify error message contains the unknown value
        let err = parse_language("python").unwrap_err();
        assert!(err.to_string().contains("python"));
    }

    #[test]
    fn parse_symbol_kind_known_values() {
        assert_eq!(parse_symbol_kind("function").unwrap(), SymbolKind::Function);
        assert_eq!(parse_symbol_kind("method").unwrap(), SymbolKind::Method);
        assert_eq!(parse_symbol_kind("struct").unwrap(), SymbolKind::Struct);
        assert_eq!(parse_symbol_kind("class").unwrap(), SymbolKind::Class);
        assert_eq!(parse_symbol_kind("enum").unwrap(), SymbolKind::Enum);
        assert_eq!(parse_symbol_kind("trait").unwrap(), SymbolKind::Trait);
        assert_eq!(
            parse_symbol_kind("interface").unwrap(),
            SymbolKind::Interface
        );
        assert_eq!(parse_symbol_kind("const").unwrap(), SymbolKind::Const);
        assert_eq!(parse_symbol_kind("static").unwrap(), SymbolKind::Static);
        assert_eq!(parse_symbol_kind("module").unwrap(), SymbolKind::Module);
        assert_eq!(
            parse_symbol_kind("type_alias").unwrap(),
            SymbolKind::TypeAlias
        );
        assert_eq!(parse_symbol_kind("macro").unwrap(), SymbolKind::Macro);
    }

    #[test]
    fn parse_symbol_kind_unknown_returns_error() {
        // Unknown values should return an error (database corruption)
        assert!(parse_symbol_kind("unknown").is_err());
        assert!(parse_symbol_kind("").is_err());

        // Verify error message contains the unknown value
        let err = parse_symbol_kind("bogus").unwrap_err();
        assert!(err.to_string().contains("bogus"));
    }

    #[test]
    fn parse_visibility_known_values() {
        assert_eq!(parse_visibility("public").unwrap(), Visibility::Public);
        assert_eq!(parse_visibility("crate").unwrap(), Visibility::Crate);
        assert_eq!(parse_visibility("module").unwrap(), Visibility::Module);
        assert_eq!(parse_visibility("private").unwrap(), Visibility::Private);
    }

    #[test]
    fn parse_visibility_unknown_returns_error() {
        // Unknown values should return an error (database corruption)
        assert!(parse_visibility("protected").is_err());
        assert!(parse_visibility("").is_err());
        assert!(parse_visibility("internal").is_err());

        // Verify error message contains the unknown value
        let err = parse_visibility("protected").unwrap_err();
        assert!(err.to_string().contains("protected"));
    }

    // ========================================================================
    // Reference and Dependency Tests (Phase 2: Step 4)
    // ========================================================================

    #[test]
    fn insert_and_query_references() {
        let (_dir, path) = temp_db();
        let mut index = Index::open(&path).unwrap();

        // Create a file and symbol
        let file_id = index
            .upsert_file(Path::new("src/lib.rs"), Language::Rust, 1000, 100, None)
            .unwrap();

        let symbol_id = index
            .insert_symbol(
                file_id,
                "authenticate",
                "crate::auth",
                "authenticate",
                SymbolKind::Function,
                10,
                1,
                None,
                None,
                Visibility::Public,
                None,
            )
            .unwrap();

        // Create another file that references the symbol
        let ref_file_id = index
            .upsert_file(Path::new("src/main.rs"), Language::Rust, 1000, 50, None)
            .unwrap();

        // Insert a reference
        index
            .insert_reference(Some(symbol_id), ref_file_id, "call", 5, 10, None, None)
            .unwrap();

        // Query references
        let refs = index.get_references_to_symbol(symbol_id).unwrap();
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].symbol_id, Some(symbol_id));
        assert_eq!(refs[0].file_id, ref_file_id);
        assert_eq!(refs[0].line, 5);
    }

    #[test]
    fn insert_file_dependency() {
        let (_dir, path) = temp_db();
        let mut index = Index::open(&path).unwrap();

        // Create two files
        let file1_id = index
            .upsert_file(Path::new("src/main.rs"), Language::Rust, 1000, 100, None)
            .unwrap();
        let file2_id = index
            .upsert_file(Path::new("src/auth.rs"), Language::Rust, 1000, 50, None)
            .unwrap();

        // main.rs depends on auth.rs
        index.insert_file_dependency(file1_id, file2_id).unwrap();

        // Verify the dependency
        let deps = index.get_file_dependencies(file1_id).unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0], file2_id);
    }

    #[test]
    fn get_file_dependencies_and_dependents() {
        let (_dir, path) = temp_db();
        let mut index = Index::open(&path).unwrap();

        // Create three files: main.rs -> auth.rs -> db.rs
        let main_id = index
            .upsert_file(Path::new("src/main.rs"), Language::Rust, 1000, 100, None)
            .unwrap();
        let auth_id = index
            .upsert_file(Path::new("src/auth.rs"), Language::Rust, 1000, 50, None)
            .unwrap();
        let db_id = index
            .upsert_file(Path::new("src/db.rs"), Language::Rust, 1000, 75, None)
            .unwrap();

        // Set up dependencies
        index.insert_file_dependency(main_id, auth_id).unwrap();
        index.insert_file_dependency(auth_id, db_id).unwrap();

        // main.rs depends on auth.rs
        let main_deps = index.get_file_dependencies(main_id).unwrap();
        assert_eq!(main_deps.len(), 1);
        assert_eq!(main_deps[0], auth_id);

        // auth.rs is depended on by main.rs
        let auth_dependents = index.get_file_dependents(auth_id).unwrap();
        assert_eq!(auth_dependents.len(), 1);
        assert_eq!(auth_dependents[0], main_id);

        // db.rs is depended on by auth.rs
        let db_dependents = index.get_file_dependents(db_id).unwrap();
        assert_eq!(db_dependents.len(), 1);
        assert_eq!(db_dependents[0], auth_id);
    }

    // ========================================================================
    // search_symbols_by_kind Tests
    // ========================================================================

    #[test]
    fn search_symbols_by_kind_filters_correctly() {
        let (_dir, path) = temp_db();
        let mut index = Index::open(&path).expect("should open database");

        let file_id = index
            .upsert_file(Path::new("src/lib.rs"), Language::Rust, 1000, 100, None)
            .expect("should insert file");

        // Insert symbols of different kinds
        index
            .insert_symbol(
                file_id,
                "my_function",
                "crate",
                "my_function",
                SymbolKind::Function,
                10,
                1,
                None,
                Some("fn my_function()"),
                Visibility::Public,
                None,
            )
            .expect("should insert function symbol");

        index
            .insert_symbol(
                file_id,
                "MyStruct",
                "crate",
                "MyStruct",
                SymbolKind::Struct,
                20,
                1,
                None,
                None,
                Visibility::Public,
                None,
            )
            .expect("should insert struct symbol");

        index
            .insert_symbol(
                file_id,
                "another_fn",
                "crate",
                "another_fn",
                SymbolKind::Function,
                30,
                1,
                None,
                None,
                Visibility::Private,
                None,
            )
            .expect("should insert another function symbol");

        // Search for functions only
        let functions = index
            .search_symbols_by_kind(SymbolKind::Function, 100)
            .expect("should search by kind");
        assert_eq!(functions.len(), 2, "should find exactly 2 functions");
        assert!(
            functions.iter().all(|s| s.kind == SymbolKind::Function),
            "all results should be functions"
        );

        // Search for structs only
        let structs = index
            .search_symbols_by_kind(SymbolKind::Struct, 100)
            .expect("should search by kind");
        assert_eq!(structs.len(), 1, "should find exactly 1 struct");
        assert_eq!(structs[0].name, "MyStruct");
    }

    #[test]
    fn search_symbols_by_kind_respects_limit() {
        let (_dir, path) = temp_db();
        let mut index = Index::open(&path).expect("should open database");

        let file_id = index
            .upsert_file(Path::new("src/lib.rs"), Language::Rust, 1000, 100, None)
            .expect("should insert file");

        // Insert 5 functions
        for i in 0..5 {
            index
                .insert_symbol(
                    file_id,
                    &format!("func_{i}"),
                    "crate",
                    &format!("func_{i}"),
                    SymbolKind::Function,
                    (10 + i * 10) as u32,
                    1,
                    None,
                    None,
                    Visibility::Public,
                    None,
                )
                .expect("should insert function symbol");
        }

        // Search with limit of 2
        let results = index
            .search_symbols_by_kind(SymbolKind::Function, 2)
            .expect("should search by kind");
        assert_eq!(results.len(), 2, "should respect limit of 2");

        // Search with limit of 10 (more than available)
        let results = index
            .search_symbols_by_kind(SymbolKind::Function, 10)
            .expect("should search by kind");
        assert_eq!(results.len(), 5, "should return all 5 functions");
    }

    #[test]
    fn search_symbols_by_kind_returns_empty_when_no_matches() {
        let (_dir, path) = temp_db();
        let mut index = Index::open(&path).expect("should open database");

        let file_id = index
            .upsert_file(Path::new("src/lib.rs"), Language::Rust, 1000, 100, None)
            .expect("should insert file");

        // Insert only functions
        index
            .insert_symbol(
                file_id,
                "my_function",
                "crate",
                "my_function",
                SymbolKind::Function,
                10,
                1,
                None,
                None,
                Visibility::Public,
                None,
            )
            .expect("should insert function symbol");

        // Search for modules (none exist)
        let modules = index
            .search_symbols_by_kind(SymbolKind::Module, 100)
            .expect("should search by kind");
        assert!(
            modules.is_empty(),
            "should return empty vec when no matches"
        );

        // Search for traits (none exist)
        let traits = index
            .search_symbols_by_kind(SymbolKind::Trait, 100)
            .expect("should search by kind");
        assert!(traits.is_empty(), "should return empty vec for traits");
    }

    #[test]
    fn search_symbols_by_kind_with_multiple_kinds_in_database() {
        let (_dir, path) = temp_db();
        let mut index = Index::open(&path).expect("should open database");

        let file_id = index
            .upsert_file(Path::new("src/lib.rs"), Language::Rust, 1000, 100, None)
            .expect("should insert file");

        // Insert various symbol kinds
        let kinds_and_names = [
            (SymbolKind::Function, "my_func"),
            (SymbolKind::Struct, "MyStruct"),
            (SymbolKind::Module, "my_module"),
            (SymbolKind::Trait, "MyTrait"),
            (SymbolKind::Enum, "MyEnum"),
            (SymbolKind::Const, "MY_CONST"),
        ];

        for (i, (kind, name)) in kinds_and_names.iter().enumerate() {
            index
                .insert_symbol(
                    file_id,
                    name,
                    "crate",
                    name,
                    *kind,
                    (10 + i * 10) as u32,
                    1,
                    None,
                    None,
                    Visibility::Public,
                    None,
                )
                .expect("should insert symbol");
        }

        // Each kind should return exactly one result
        for (kind, expected_name) in kinds_and_names {
            let results = index
                .search_symbols_by_kind(kind, 100)
                .expect("should search by kind");
            assert_eq!(results.len(), 1, "should find exactly 1 {kind:?} symbol");
            assert_eq!(
                results[0].name, expected_name,
                "symbol name should match for {kind:?}"
            );
            assert_eq!(results[0].kind, kind, "symbol kind should match");
        }
    }

    #[test]
    fn search_symbols_by_kind_with_zero_limit() {
        let (_dir, path) = temp_db();
        let mut index = Index::open(&path).expect("should open database");

        let file_id = index
            .upsert_file(Path::new("src/lib.rs"), Language::Rust, 1000, 100, None)
            .expect("should insert file");

        // Insert some functions
        for i in 0..3 {
            index
                .insert_symbol(
                    file_id,
                    &format!("func_{i}"),
                    "crate",
                    &format!("func_{i}"),
                    SymbolKind::Function,
                    (10 + i * 10) as u32,
                    1,
                    None,
                    None,
                    Visibility::Public,
                    None,
                )
                .expect("should insert function symbol");
        }

        // Search with limit of 0 should return empty
        let results = index
            .search_symbols_by_kind(SymbolKind::Function, 0)
            .expect("should handle zero limit");
        assert!(results.is_empty(), "limit=0 should return empty vec");
    }

    #[test]
    fn search_symbols_by_kind_returns_complete_symbol_data() {
        let (_dir, path) = temp_db();
        let mut index = Index::open(&path).expect("should open database");

        let file_id = index
            .upsert_file(Path::new("src/lib.rs"), Language::Rust, 1000, 100, None)
            .expect("should insert file");

        // Insert a symbol with all fields populated
        let span = Span::new(42, 8, 50, 1).expect("valid span");
        index
            .insert_symbol(
                file_id,
                "my_func",
                "crate::module",
                "crate::module::my_func",
                SymbolKind::Function,
                42,
                8,
                Some(span),
                Some("fn my_func(x: i32) -> bool"),
                Visibility::Private,
                None,
            )
            .expect("should insert symbol");

        let results = index
            .search_symbols_by_kind(SymbolKind::Function, 10)
            .expect("should search");
        assert_eq!(results.len(), 1, "should find one function");

        let sym = &results[0];
        assert_eq!(sym.name, "my_func", "name should match");
        assert_eq!(sym.module_path, "crate::module", "module_path should match");
        assert_eq!(
            sym.qualified_name, "crate::module::my_func",
            "qualified_name should match"
        );
        assert_eq!(sym.kind, SymbolKind::Function, "kind should match");
        assert_eq!(sym.line, 42, "line should match");
        assert_eq!(sym.column, 8, "column should match");
        assert!(sym.span.is_some(), "span should be present");
        let sym_span = sym.span.expect("span should exist");
        assert_eq!(sym_span.start_line(), 42, "span start_line should match");
        assert_eq!(sym_span.end_line(), 50, "span end_line should match");
        assert_eq!(
            sym.signature,
            Some("fn my_func(x: i32) -> bool".to_string()),
            "signature should match"
        );
        assert_eq!(
            sym.visibility,
            Visibility::Private,
            "visibility should match"
        );
        assert!(sym.parent_symbol_id.is_none(), "parent should be None");
    }

    // ========================================================================
    // get_files_by_language Tests
    // ========================================================================

    #[test]
    fn get_files_by_language_filters_correctly() {
        let (_dir, path) = temp_db();
        let mut index = Index::open(&path).expect("should open database");

        // Insert Rust files
        index
            .upsert_file(Path::new("src/main.rs"), Language::Rust, 1000, 100, None)
            .expect("should insert file");
        index
            .upsert_file(Path::new("src/lib.rs"), Language::Rust, 1000, 200, None)
            .expect("should insert file");

        // Insert C# files
        index
            .upsert_file(
                Path::new("src/Program.cs"),
                Language::CSharp,
                1000,
                150,
                None,
            )
            .expect("should insert file");

        // Get Rust files
        let rust_files = index
            .get_files_by_language(Language::Rust)
            .expect("should get files by language");
        assert_eq!(rust_files.len(), 2, "should find 2 Rust files");
        assert!(
            rust_files.iter().all(|f| f.language == Language::Rust),
            "all files should be Rust"
        );

        // Get C# files
        let csharp_files = index
            .get_files_by_language(Language::CSharp)
            .expect("should get files by language");
        assert_eq!(csharp_files.len(), 1, "should find 1 C# file");
        assert_eq!(csharp_files[0].language, Language::CSharp);
    }

    #[test]
    fn get_files_by_language_returns_empty_when_no_matches() {
        let (_dir, path) = temp_db();
        let mut index = Index::open(&path).expect("should open database");

        // Insert only Rust files
        index
            .upsert_file(Path::new("src/main.rs"), Language::Rust, 1000, 100, None)
            .expect("should insert file");

        // Get C# files (none exist)
        let csharp_files = index
            .get_files_by_language(Language::CSharp)
            .expect("should get files by language");
        assert!(
            csharp_files.is_empty(),
            "should return empty vec when no C# files exist"
        );
    }

    #[test]
    fn get_files_by_language_with_mixed_languages() {
        let (_dir, path) = temp_db();
        let mut index = Index::open(&path).expect("should open database");

        // Insert files of mixed languages
        let rust_paths = ["src/main.rs", "src/lib.rs", "src/utils.rs"];
        let csharp_paths = ["src/Program.cs", "src/Service.cs"];

        for p in rust_paths {
            index
                .upsert_file(Path::new(p), Language::Rust, 1000, 100, None)
                .expect("should insert Rust file");
        }

        for p in csharp_paths {
            index
                .upsert_file(Path::new(p), Language::CSharp, 1000, 100, None)
                .expect("should insert C# file");
        }

        // Verify Rust files
        let rust_files = index
            .get_files_by_language(Language::Rust)
            .expect("should get Rust files");
        assert_eq!(rust_files.len(), 3, "should find 3 Rust files");

        let rust_file_paths: Vec<_> = rust_files
            .iter()
            .map(|f| f.path.to_string_lossy().to_string())
            .collect();
        for expected_path in rust_paths {
            assert!(
                rust_file_paths.contains(&expected_path.to_string()),
                "should contain {expected_path}"
            );
        }

        // Verify C# files
        let csharp_files = index
            .get_files_by_language(Language::CSharp)
            .expect("should get C# files");
        assert_eq!(csharp_files.len(), 2, "should find 2 C# files");

        let csharp_file_paths: Vec<_> = csharp_files
            .iter()
            .map(|f| f.path.to_string_lossy().to_string())
            .collect();
        for expected_path in csharp_paths {
            assert!(
                csharp_file_paths.contains(&expected_path.to_string()),
                "should contain {expected_path}"
            );
        }
    }

    // ========================================================================
    // Import Operations Tests
    // ========================================================================

    #[test]
    fn insert_and_get_imports() {
        let (_dir, path) = temp_db();
        let mut index = Index::open(&path).expect("should open database");

        let file_id = index
            .upsert_file(Path::new("src/main.rs"), Language::Rust, 1000, 100, None)
            .expect("should insert file");

        // Insert some imports
        index
            .insert_import(file_id, "Index", "crate::db", None)
            .expect("should insert import");
        index
            .insert_import(file_id, "Result", "crate::error", None)
            .expect("should insert import");
        index
            .insert_import(file_id, "HashMap", "std::collections", Some("Map"))
            .expect("should insert import with alias");

        // Get imports
        let imports = index
            .get_imports_for_file(file_id)
            .expect("should get imports");

        assert_eq!(imports.len(), 3, "should have 3 imports");

        // Imports are ordered by source_module, symbol_name
        assert_eq!(imports[0].symbol_name, "Index");
        assert_eq!(imports[0].source_module, "crate::db");
        assert_eq!(imports[0].alias, None);

        assert_eq!(imports[1].symbol_name, "Result");
        assert_eq!(imports[1].source_module, "crate::error");
        assert_eq!(imports[1].alias, None);

        assert_eq!(imports[2].symbol_name, "HashMap");
        assert_eq!(imports[2].source_module, "std::collections");
        assert_eq!(imports[2].alias, Some("Map".to_string()));
    }

    #[test]
    fn insert_import_upsert_semantics() {
        let (_dir, path) = temp_db();
        let mut index = Index::open(&path).expect("should open database");

        let file_id = index
            .upsert_file(Path::new("src/main.rs"), Language::Rust, 1000, 100, None)
            .expect("should insert file");

        // Insert same import twice (should be idempotent or update alias)
        index
            .insert_import(file_id, "Index", "crate::db", None)
            .expect("should insert import");
        index
            .insert_import(file_id, "Index", "crate::db", Some("Idx"))
            .expect("should update import with alias");

        let imports = index
            .get_imports_for_file(file_id)
            .expect("should get imports");

        assert_eq!(imports.len(), 1, "should have 1 import (upsert)");
        assert_eq!(
            imports[0].alias,
            Some("Idx".to_string()),
            "alias should be updated"
        );
    }

    #[test]
    fn get_imports_returns_empty_for_file_without_imports() {
        let (_dir, path) = temp_db();
        let mut index = Index::open(&path).expect("should open database");

        let file_id = index
            .upsert_file(Path::new("src/main.rs"), Language::Rust, 1000, 100, None)
            .expect("should insert file");

        let imports = index
            .get_imports_for_file(file_id)
            .expect("should get imports");

        assert!(
            imports.is_empty(),
            "should return empty vec for file without imports"
        );
    }

    #[test]
    fn clear_imports_for_file() {
        let (_dir, path) = temp_db();
        let mut index = Index::open(&path).expect("should open database");

        let file_id = index
            .upsert_file(Path::new("src/main.rs"), Language::Rust, 1000, 100, None)
            .expect("should insert file");

        // Insert imports
        index
            .insert_import(file_id, "Index", "crate::db", None)
            .expect("should insert import");
        index
            .insert_import(file_id, "Result", "crate::error", None)
            .expect("should insert import");

        // Verify imports exist
        let imports = index
            .get_imports_for_file(file_id)
            .expect("should get imports");
        assert_eq!(imports.len(), 2, "should have 2 imports");

        // Clear imports
        index
            .clear_imports_for_file(file_id)
            .expect("should clear imports");

        // Verify imports are gone
        let imports = index
            .get_imports_for_file(file_id)
            .expect("should get imports");
        assert!(imports.is_empty(), "imports should be cleared");
    }

    #[test]
    fn clear_imports_does_not_affect_other_files() {
        let (_dir, path) = temp_db();
        let mut index = Index::open(&path).expect("should open database");

        let file1_id = index
            .upsert_file(Path::new("src/main.rs"), Language::Rust, 1000, 100, None)
            .expect("should insert file");
        let file2_id = index
            .upsert_file(Path::new("src/lib.rs"), Language::Rust, 1000, 100, None)
            .expect("should insert file");

        // Insert imports for both files
        index
            .insert_import(file1_id, "Index", "crate::db", None)
            .expect("should insert import");
        index
            .insert_import(file2_id, "Result", "crate::error", None)
            .expect("should insert import");

        // Clear imports for file1 only
        index
            .clear_imports_for_file(file1_id)
            .expect("should clear imports");

        // file1 should have no imports
        let imports1 = index
            .get_imports_for_file(file1_id)
            .expect("should get imports");
        assert!(imports1.is_empty(), "file1 imports should be cleared");

        // file2 should still have its import
        let imports2 = index
            .get_imports_for_file(file2_id)
            .expect("should get imports");
        assert_eq!(imports2.len(), 1, "file2 should still have 1 import");
        assert_eq!(imports2[0].symbol_name, "Result");
    }

    #[test]
    fn imports_cascade_delete_when_file_deleted() {
        let (_dir, path) = temp_db();
        let mut index = Index::open(&path).expect("should open database");

        let file_id = index
            .upsert_file(Path::new("src/main.rs"), Language::Rust, 1000, 100, None)
            .expect("should insert file");

        // Insert imports
        index
            .insert_import(file_id, "Index", "crate::db", None)
            .expect("should insert import");

        // Delete the file (via clear which deletes files)
        index.clear().expect("should clear database");

        // Re-create database schema (clear() doesn't drop tables)
        // The imports should be gone due to CASCADE DELETE
        let mut index = Index::open(&path).expect("should reopen database");
        let file_id = index
            .upsert_file(Path::new("src/main.rs"), Language::Rust, 1000, 100, None)
            .expect("should insert file");

        let imports = index
            .get_imports_for_file(file_id)
            .expect("should get imports");
        assert!(
            imports.is_empty(),
            "imports should be gone after file deletion"
        );
    }

    #[test]
    fn import_glob_symbol() {
        let (_dir, path) = temp_db();
        let mut index = Index::open(&path).expect("should open database");

        let file_id = index
            .upsert_file(Path::new("src/main.rs"), Language::Rust, 1000, 100, None)
            .expect("should insert file");

        // Insert glob import
        index
            .insert_import(file_id, "*", "crate::prelude", None)
            .expect("should insert glob import");

        let imports = index
            .get_imports_for_file(file_id)
            .expect("should get imports");

        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].symbol_name, "*");
        assert_eq!(imports[0].source_module, "crate::prelude");
    }

    #[test]
    fn import_with_csharp_namespace() {
        let (_dir, path) = temp_db();
        let mut index = Index::open(&path).expect("should open database");

        let file_id = index
            .upsert_file(
                Path::new("src/Program.cs"),
                Language::CSharp,
                1000,
                100,
                None,
            )
            .expect("should insert file");

        // Insert C# style imports
        index
            .insert_import(file_id, "List", "System.Collections.Generic", None)
            .expect("should insert import");
        index
            .insert_import(file_id, "ILogger", "Microsoft.Extensions.Logging", None)
            .expect("should insert import");

        let imports = index
            .get_imports_for_file(file_id)
            .expect("should get imports");

        assert_eq!(imports.len(), 2);
        // Ordered by source_module
        assert_eq!(imports[0].source_module, "Microsoft.Extensions.Logging");
        assert_eq!(imports[1].source_module, "System.Collections.Generic");
    }

    #[test]
    fn import_file_id_matches() {
        let (_dir, path) = temp_db();
        let mut index = Index::open(&path).expect("should open database");

        let file_id = index
            .upsert_file(Path::new("src/main.rs"), Language::Rust, 1000, 100, None)
            .expect("should insert file");

        index
            .insert_import(file_id, "Index", "crate::db", None)
            .expect("should insert import");

        let imports = index
            .get_imports_for_file(file_id)
            .expect("should get imports");

        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].file_id, file_id, "import file_id should match");
    }

    // ========================================================================
    // Unresolved References Tests
    // ========================================================================

    #[test]
    fn insert_unresolved_reference_with_null_symbol_id() {
        let (_dir, path) = temp_db();
        let mut index = Index::open(&path).expect("should open database");

        let file_id = index
            .upsert_file(Path::new("src/main.rs"), Language::Rust, 1000, 100, None)
            .expect("should insert file");

        // Insert an unresolved reference (symbol_id = None)
        let ref_id = index
            .insert_reference(
                None, // Unresolved
                file_id,
                "call",
                10,
                5,
                None,
                Some("Index::open"), // Reference name for later resolution
            )
            .expect("should insert unresolved reference");

        assert!(ref_id > 0, "should return valid reference ID");

        // Verify it appears in unresolved references
        let unresolved = index
            .get_unresolved_references()
            .expect("should get unresolved references");
        assert_eq!(unresolved.len(), 1, "should have 1 unresolved reference");
        assert_eq!(unresolved[0].id, ref_id);
        assert!(
            unresolved[0].symbol_id.is_none(),
            "symbol_id should be None"
        );
        assert_eq!(
            unresolved[0].reference_name,
            Some("Index::open".to_string()),
            "reference_name should be stored"
        );
    }

    #[test]
    fn get_unresolved_references_excludes_resolved() {
        let (_dir, path) = temp_db();
        let mut index = Index::open(&path).expect("should open database");

        let file_id = index
            .upsert_file(Path::new("src/lib.rs"), Language::Rust, 1000, 100, None)
            .expect("should insert file");

        // Create a symbol to reference
        let symbol_id = index
            .insert_symbol(
                file_id,
                "my_func",
                "crate",
                "my_func",
                SymbolKind::Function,
                5,
                1,
                None,
                None,
                Visibility::Public,
                None,
            )
            .expect("should insert symbol");

        // Insert a resolved reference
        index
            .insert_reference(Some(symbol_id), file_id, "call", 10, 5, None, None)
            .expect("should insert resolved reference");

        // Insert an unresolved reference
        index
            .insert_reference(None, file_id, "call", 15, 3, None, Some("Other::func"))
            .expect("should insert unresolved reference");

        // Get unresolved references
        let unresolved = index
            .get_unresolved_references()
            .expect("should get unresolved references");
        assert_eq!(
            unresolved.len(),
            1,
            "should only have 1 unresolved reference"
        );
        assert!(
            unresolved[0].symbol_id.is_none(),
            "should be the unresolved one"
        );
        assert_eq!(unresolved[0].line, 15, "should be the correct reference");
    }

    #[test]
    fn resolve_reference_sets_symbol_id() {
        let (_dir, path) = temp_db();
        let mut index = Index::open(&path).expect("should open database");

        let file_id = index
            .upsert_file(Path::new("src/main.rs"), Language::Rust, 1000, 100, None)
            .expect("should insert file");

        // Insert an unresolved reference
        let ref_id = index
            .insert_reference(None, file_id, "call", 10, 5, None, Some("Index::open"))
            .expect("should insert unresolved reference");

        // Create the target symbol (simulating it being discovered later)
        let symbol_id = index
            .insert_symbol(
                file_id,
                "open",
                "crate::db::Index",
                "Index::open",
                SymbolKind::Function,
                20,
                1,
                None,
                Some("fn open(&self) -> Result<()>"),
                Visibility::Public,
                None,
            )
            .expect("should insert symbol");

        // Resolve the reference
        index
            .resolve_reference(ref_id, symbol_id)
            .expect("should resolve reference");

        // Verify the reference is now resolved
        let unresolved = index
            .get_unresolved_references()
            .expect("should get unresolved references");
        assert!(
            unresolved.is_empty(),
            "should have no unresolved references after resolution"
        );

        // Verify the reference points to the correct symbol
        let refs = index
            .get_references_to_symbol(symbol_id)
            .expect("should get references");
        assert_eq!(refs.len(), 1, "symbol should have 1 reference");
        assert_eq!(refs[0].id, ref_id, "should be the resolved reference");
        assert_eq!(refs[0].symbol_id, Some(symbol_id), "symbol_id should match");
        assert!(
            refs[0].reference_name.is_none(),
            "reference_name should be cleared after resolution"
        );
    }

    #[test]
    fn list_references_in_file_includes_unresolved() {
        let (_dir, path) = temp_db();
        let mut index = Index::open(&path).expect("should open database");

        let file_id = index
            .upsert_file(Path::new("src/main.rs"), Language::Rust, 1000, 100, None)
            .expect("should insert file");

        // Create a symbol
        let symbol_id = index
            .insert_symbol(
                file_id,
                "local_func",
                "crate",
                "local_func",
                SymbolKind::Function,
                5,
                1,
                None,
                None,
                Visibility::Private,
                None,
            )
            .expect("should insert symbol");

        // Insert both resolved and unresolved references
        index
            .insert_reference(Some(symbol_id), file_id, "call", 10, 5, None, None)
            .expect("should insert resolved reference");
        index
            .insert_reference(None, file_id, "call", 15, 3, None, Some("External::func"))
            .expect("should insert unresolved reference");

        // List all references in file
        let refs = index
            .list_references_in_file(file_id)
            .expect("should list references");
        assert_eq!(refs.len(), 2, "should include both resolved and unresolved");

        // Check they're ordered by line
        assert_eq!(refs[0].line, 10, "first ref should be at line 10");
        assert!(refs[0].symbol_id.is_some(), "first ref should be resolved");
        assert_eq!(refs[1].line, 15, "second ref should be at line 15");
        assert!(
            refs[1].symbol_id.is_none(),
            "second ref should be unresolved"
        );
    }

    #[test]
    fn unresolved_references_ordered_by_file_and_line() {
        let (_dir, path) = temp_db();
        let mut index = Index::open(&path).expect("should open database");

        let file1_id = index
            .upsert_file(Path::new("src/a.rs"), Language::Rust, 1000, 100, None)
            .expect("should insert file");
        let file2_id = index
            .upsert_file(Path::new("src/b.rs"), Language::Rust, 1000, 100, None)
            .expect("should insert file");

        // Insert unresolved references out of order
        index
            .insert_reference(None, file2_id, "call", 20, 1, None, Some("B::func"))
            .expect("insert ref");
        index
            .insert_reference(None, file1_id, "call", 15, 1, None, Some("A::func2"))
            .expect("insert ref");
        index
            .insert_reference(None, file1_id, "call", 5, 1, None, Some("A::func1"))
            .expect("insert ref");
        index
            .insert_reference(None, file2_id, "call", 10, 1, None, Some("B::func2"))
            .expect("insert ref");

        let unresolved = index
            .get_unresolved_references()
            .expect("should get unresolved references");
        assert_eq!(unresolved.len(), 4, "should have 4 unresolved references");

        // Should be ordered by file_id, then line
        assert_eq!(unresolved[0].file_id, file1_id);
        assert_eq!(unresolved[0].line, 5);
        assert_eq!(unresolved[1].file_id, file1_id);
        assert_eq!(unresolved[1].line, 15);
        assert_eq!(unresolved[2].file_id, file2_id);
        assert_eq!(unresolved[2].line, 10);
        assert_eq!(unresolved[3].file_id, file2_id);
        assert_eq!(unresolved[3].line, 20);
    }

    #[test]
    fn insert_reference_with_in_symbol_id() {
        let (_dir, path) = temp_db();
        let mut index = Index::open(&path).expect("should open database");

        let file_id = index
            .upsert_file(Path::new("src/main.rs"), Language::Rust, 1000, 100, None)
            .expect("should insert file");

        // Create containing symbol
        let container_id = index
            .insert_symbol(
                file_id,
                "caller_func",
                "crate",
                "caller_func",
                SymbolKind::Function,
                5,
                1,
                None,
                None,
                Visibility::Public,
                None,
            )
            .expect("should insert symbol");

        // Insert unresolved reference with in_symbol_id
        index
            .insert_reference(
                None,
                file_id,
                "call",
                10,
                5,
                Some(container_id), // Reference is inside caller_func
                Some("External::target"),
            )
            .expect("should insert reference with in_symbol_id");

        let unresolved = index
            .get_unresolved_references()
            .expect("should get unresolved references");
        assert_eq!(unresolved.len(), 1);
        assert_eq!(
            unresolved[0].in_symbol_id,
            Some(container_id),
            "in_symbol_id should be preserved"
        );
    }

    #[test]
    fn search_symbol_by_qualified_name_in_file_finds_exact_match() {
        let (_dir, path) = temp_db();
        let mut index = Index::open(&path).expect("should open database");

        let file1_id = index
            .upsert_file(Path::new("src/a.rs"), Language::Rust, 1000, 100, None)
            .expect("should insert file");
        let file2_id = index
            .upsert_file(Path::new("src/b.rs"), Language::Rust, 1000, 100, None)
            .expect("should insert file");

        // Insert symbols with same qualified name in different files
        let sym1_id = index
            .insert_symbol(
                file1_id,
                "open",
                "crate::a",
                "Index::open",
                SymbolKind::Method,
                10,
                1,
                None,
                None,
                Visibility::Public,
                None,
            )
            .expect("should insert symbol in file1");

        let sym2_id = index
            .insert_symbol(
                file2_id,
                "open",
                "crate::b",
                "Index::open",
                SymbolKind::Method,
                10,
                1,
                None,
                None,
                Visibility::Public,
                None,
            )
            .expect("should insert symbol in file2");

        // Search in file1 should find file1's symbol
        let result = index
            .search_symbol_by_qualified_name_in_file("Index::open", file1_id)
            .expect("search should succeed");
        assert!(result.is_some(), "should find symbol in file1");
        assert_eq!(result.unwrap().id, sym1_id, "should find correct symbol");

        // Search in file2 should find file2's symbol
        let result = index
            .search_symbol_by_qualified_name_in_file("Index::open", file2_id)
            .expect("search should succeed");
        assert!(result.is_some(), "should find symbol in file2");
        assert_eq!(result.unwrap().id, sym2_id, "should find correct symbol");

        // Search with wrong qualified name should return None
        let result = index
            .search_symbol_by_qualified_name_in_file("Other::open", file1_id)
            .expect("search should succeed");
        assert!(result.is_none(), "should not find non-existent symbol");

        // Search with wrong file_id should return None
        let file3_id = index
            .upsert_file(Path::new("src/c.rs"), Language::Rust, 1000, 100, None)
            .expect("should insert file");
        let result = index
            .search_symbol_by_qualified_name_in_file("Index::open", file3_id)
            .expect("search should succeed");
        assert!(result.is_none(), "should not find symbol in wrong file");
    }
}
