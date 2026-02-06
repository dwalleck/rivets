//! File CRUD operations for the Tethys index.

use std::path::Path;

use rusqlite::params;
use rusqlite::OptionalExtension;

use super::{row_to_indexed_file, Index, SymbolData, FILES_COLUMNS};
use crate::error::Result;
use crate::types::{FileId, IndexedFile, Language, SymbolId};

/// Normalize a file path to use forward slashes for consistent DB storage.
///
/// On Windows, `Path::to_string_lossy()` preserves backslashes from OS APIs,
/// but tests and cross-platform code use forward slashes. Normalizing to `/`
/// ensures lookups match regardless of how the path was constructed.
fn normalize_path(path: &Path) -> String {
    let s = path.to_string_lossy();
    if cfg!(windows) {
        s.replace('\\', "/")
    } else {
        s.into_owned()
    }
}

impl Index {
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
        let (file_id, _symbol_ids) =
            self.index_file_atomic(path, language, mtime_ns, size_bytes, content_hash, &[])?;
        Ok(file_id)
    }

    /// Get a file by path.
    pub fn get_file(&self, path: &Path) -> Result<Option<IndexedFile>> {
        let path_str = normalize_path(path);
        let conn = self.connection()?;

        conn.query_row(
            &format!("SELECT {FILES_COLUMNS} FROM files WHERE path = ?1"),
            [&path_str],
            row_to_indexed_file,
        )
        .optional()
        .map_err(Into::into)
    }

    /// Get file ID by path.
    pub fn get_file_id(&self, path: &Path) -> Result<Option<FileId>> {
        let path_str = normalize_path(path);
        let conn = self.connection()?;

        conn.query_row("SELECT id FROM files WHERE path = ?1", [&path_str], |row| {
            row.get::<_, i64>(0).map(FileId::from)
        })
        .optional()
        .map_err(Into::into)
    }

    /// Get a file by its database ID.
    pub fn get_file_by_id(&self, id: FileId) -> Result<Option<IndexedFile>> {
        let conn = self.connection()?;

        conn.query_row(
            &format!("SELECT {FILES_COLUMNS} FROM files WHERE id = ?1"),
            [id.as_i64()],
            row_to_indexed_file,
        )
        .optional()
        .map_err(Into::into)
    }

    /// Atomically index a file with all its symbols in a transaction.
    ///
    /// Returns the file ID and the generated `SymbolId` for each inserted symbol,
    /// in the same order as the input `symbols` slice. This avoids a separate
    /// query to retrieve symbol IDs after insertion.
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
    ) -> Result<(FileId, Vec<SymbolId>)> {
        let mut conn = self.connection()?;
        let tx = conn.transaction()?;

        let path_str = normalize_path(path);
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

        // Insert all symbols, capturing generated IDs
        let mut symbol_ids = Vec::with_capacity(symbols.len());
        for sym in symbols {
            tx.execute(
                "INSERT INTO symbols (file_id, name, module_path, qualified_name, kind, line, column,
                 end_line, end_column, signature, visibility, parent_symbol_id, is_test)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
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
                    sym.parent_symbol_id.map(SymbolId::as_i64),
                    sym.is_test
                ],
            )?;
            symbol_ids.push(SymbolId::from(tx.last_insert_rowid()));
        }

        tx.commit()?;
        Ok((FileId::from(file_id), symbol_ids))
    }

    /// Get all files of a specific language.
    ///
    /// Used for language-specific dependency resolution passes.
    pub fn get_files_by_language(&self, language: Language) -> Result<Vec<IndexedFile>> {
        let lang_str = language.as_str();
        let conn = self.connection()?;

        let mut stmt = conn.prepare(&format!(
            "SELECT {FILES_COLUMNS} FROM files WHERE language = ?1"
        ))?;

        let files = stmt
            .query_map([lang_str], row_to_indexed_file)?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(files)
    }

    /// Get all indexed files.
    ///
    /// Used for dependency computation after streaming writes.
    pub fn list_all_files(&self) -> Result<Vec<IndexedFile>> {
        let conn = self.connection()?;

        let mut stmt = conn.prepare(&format!("SELECT {FILES_COLUMNS} FROM files ORDER BY path"))?;

        let files = stmt
            .query_map([], row_to_indexed_file)?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(files)
    }
}
