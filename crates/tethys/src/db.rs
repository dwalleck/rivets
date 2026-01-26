//! `SQLite` storage layer for Tethys.
//!
//! This module manages the `SQLite` database that stores indexed symbols and references.
//! `SQLite` is the source of truth; petgraph is used for graph algorithms with subgraphs
//! loaded on-demand.

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
    IndexedFile, Language, Reference, ReferenceKind, Span, Symbol, SymbolKind, Visibility,
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
    pub parent_symbol_id: Option<i64>,
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
    #[allow(dead_code)] // Public API, not yet used internally
    pub fn upsert_file(
        &self,
        path: &Path,
        language: Language,
        mtime_ns: i64,
        size_bytes: u64,
        content_hash: Option<u64>,
    ) -> Result<i64> {
        let path_str = path.to_string_lossy();
        let lang_str = language.as_str();
        let indexed_at = Self::now_ns()?;

        // Try to update first
        let updated = self.conn.execute(
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

        if updated > 0 {
            // Get the existing ID
            let id: i64 = self.conn.query_row(
                "SELECT id FROM files WHERE path = ?1",
                [&path_str],
                |row| row.get(0),
            )?;

            // Clear old symbols and refs for this file (they'll be re-added)
            self.conn
                .execute("DELETE FROM symbols WHERE file_id = ?1", [id])?;

            Ok(id)
        } else {
            // Insert new
            self.conn.execute(
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
            Ok(self.conn.last_insert_rowid())
        }
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
    pub fn get_file_id(&self, path: &Path) -> Result<Option<i64>> {
        let path_str = path.to_string_lossy();

        self.conn
            .query_row("SELECT id FROM files WHERE path = ?1", [&path_str], |row| {
                row.get(0)
            })
            .optional()
            .map_err(Into::into)
    }

    /// Get a file by its database ID.
    pub fn get_file_by_id(&self, id: i64) -> Result<Option<IndexedFile>> {
        self.conn
            .query_row(
                "SELECT id, path, language, mtime_ns, size_bytes, content_hash, indexed_at
                 FROM files WHERE id = ?1",
                [id],
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
    ) -> Result<i64> {
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

            // Clear old symbols for this file
            tx.execute("DELETE FROM symbols WHERE file_id = ?1", [id])?;
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
                    sym.parent_symbol_id
                ],
            )?;
        }

        tx.commit()?;
        Ok(file_id)
    }

    // === Symbol Operations ===

    /// Insert a symbol, returning the symbol ID.
    #[allow(dead_code)] // Public API, not yet used internally
    #[allow(clippy::too_many_arguments)] // Database row has many columns
    pub fn insert_symbol(
        &self,
        file_id: i64,
        name: &str,
        module_path: &str,
        qualified_name: &str,
        kind: SymbolKind,
        line: u32,
        column: u32,
        span: Option<Span>,
        signature: Option<&str>,
        visibility: Visibility,
        parent_symbol_id: Option<i64>,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO symbols (file_id, name, module_path, qualified_name, kind, line, column,
             end_line, end_column, signature, visibility, parent_symbol_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                file_id,
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
                parent_symbol_id
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// List symbols in a file.
    pub fn list_symbols_in_file(&self, file_id: i64) -> Result<Vec<Symbol>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, file_id, name, module_path, qualified_name, kind, line, column,
             end_line, end_column, signature, visibility, parent_symbol_id
             FROM symbols WHERE file_id = ?1 ORDER BY line",
        )?;

        let symbols = stmt
            .query_map([file_id], row_to_symbol)?
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
    pub fn get_symbol_by_id(&self, id: i64) -> Result<Option<Symbol>> {
        trace!(symbol_id = id, "Looking up symbol by ID");
        let mut stmt = self.conn.prepare(
            "SELECT id, file_id, name, module_path, qualified_name, kind, line, column,
             end_line, end_column, signature, visibility, parent_symbol_id
             FROM symbols WHERE id = ?1",
        )?;

        let mut rows = stmt.query([id])?;
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

    // === Reference Operations ===
    // These operations support symbol-level "who calls X?" queries.
    // See graph/sql.rs for higher-level graph traversal using these primitives.

    /// Insert a reference to a symbol.
    #[allow(clippy::too_many_arguments)]
    pub fn insert_reference(
        &self,
        symbol_id: i64,
        file_id: i64,
        kind: &str,
        line: u32,
        column: u32,
        in_symbol_id: Option<i64>,
    ) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO refs (symbol_id, file_id, kind, line, column, in_symbol_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![symbol_id, file_id, kind, line, column, in_symbol_id],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Get all references to a symbol.
    pub fn get_references_to_symbol(&self, symbol_id: i64) -> Result<Vec<Reference>> {
        trace!(symbol_id, "Getting references to symbol");
        let mut stmt = self.conn.prepare(
            "SELECT id, symbol_id, file_id, kind, line, column, end_line, end_column, in_symbol_id
             FROM refs WHERE symbol_id = ?1 ORDER BY file_id, line",
        )?;

        let refs = stmt
            .query_map([symbol_id], row_to_reference)?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(refs)
    }

    /// List all outgoing references from a file.
    pub fn list_references_in_file(&self, file_id: i64) -> Result<Vec<Reference>> {
        trace!(file_id, "Listing references in file");
        let mut stmt = self.conn.prepare(
            "SELECT id, symbol_id, file_id, kind, line, column, end_line, end_column, in_symbol_id
             FROM refs WHERE file_id = ?1 ORDER BY line, column",
        )?;

        let refs = stmt
            .query_map([file_id], row_to_reference)?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(refs)
    }

    // === File Dependency Operations ===

    /// Insert or update a file-level dependency.
    ///
    /// Records that `from_file_id` depends on `to_file_id`.
    pub fn insert_file_dependency(&self, from_file_id: i64, to_file_id: i64) -> Result<()> {
        // Use upsert (ON CONFLICT) to handle duplicates (increments ref_count)
        self.conn.execute(
            "INSERT INTO file_deps (from_file_id, to_file_id, ref_count)
             VALUES (?1, ?2, 1)
             ON CONFLICT(from_file_id, to_file_id) DO UPDATE SET ref_count = ref_count + 1",
            params![from_file_id, to_file_id],
        )?;
        Ok(())
    }

    /// Get files that the given file depends on.
    pub fn get_file_dependencies(&self, file_id: i64) -> Result<Vec<i64>> {
        let mut stmt = self
            .conn
            .prepare("SELECT to_file_id FROM file_deps WHERE from_file_id = ?1")?;

        let deps = stmt
            .query_map([file_id], |row| row.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(deps)
    }

    /// Get files that depend on the given file.
    pub fn get_file_dependents(&self, file_id: i64) -> Result<Vec<i64>> {
        let mut stmt = self
            .conn
            .prepare("SELECT from_file_id FROM file_deps WHERE to_file_id = ?1")?;

        let deps = stmt
            .query_map([file_id], |row| row.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(deps)
    }

    /// Clear all data from the database.
    pub fn clear(&self) -> Result<()> {
        self.conn.execute_batch(
            "DELETE FROM refs; DELETE FROM symbols; DELETE FROM file_deps; DELETE FROM files;",
        )?;
        Ok(())
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
        id: row.get(0)?,
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
        symbol_id: row.get(1)?,
        file_id: row.get(2)?,
        kind: parse_reference_kind(&row.get::<_, String>(3)?)?,
        line,
        column,
        span: build_span(line, column, end_line, end_column),
        in_symbol_id: row.get(8)?,
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

/// Convert a database row to a Symbol.
pub(crate) fn row_to_symbol(row: &rusqlite::Row) -> rusqlite::Result<Symbol> {
    let line: u32 = row.get(6)?;
    let column: u32 = row.get(7)?;
    let end_line: Option<u32> = row.get(8)?;
    let end_column: Option<u32> = row.get(9)?;

    Ok(Symbol {
        id: row.get(0)?,
        file_id: row.get(1)?,
        name: row.get(2)?,
        module_path: row.get(3)?,
        qualified_name: row.get(4)?,
        kind: parse_symbol_kind(&row.get::<_, String>(5)?)?,
        line,
        column,
        span: build_span(line, column, end_line, end_column),
        signature: row.get(10)?,
        signature_details: None, // TODO: Parse from JSON column when stored
        visibility: parse_visibility(&row.get::<_, String>(11)?)?,
        parent_symbol_id: row.get(12)?,
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
CREATE TABLE IF NOT EXISTS refs (
    id INTEGER PRIMARY KEY,
    symbol_id INTEGER NOT NULL REFERENCES symbols(id) ON DELETE CASCADE,
    file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    line INTEGER NOT NULL,
    column INTEGER NOT NULL,
    end_line INTEGER,
    end_column INTEGER,
    in_symbol_id INTEGER REFERENCES symbols(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_refs_symbol ON refs(symbol_id);
CREATE INDEX IF NOT EXISTS idx_refs_file ON refs(file_id);
CREATE INDEX IF NOT EXISTS idx_refs_in_symbol ON refs(in_symbol_id);

-- File-level dependencies (denormalized for fast queries)
CREATE TABLE IF NOT EXISTS file_deps (
    from_file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    to_file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    ref_count INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (from_file_id, to_file_id)
);

CREATE INDEX IF NOT EXISTS idx_file_deps_to ON file_deps(to_file_id);
";

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn temp_db() -> (TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
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
    }

    #[test]
    fn upsert_file_inserts_new_file() {
        let (_dir, path) = temp_db();
        let index = Index::open(&path).unwrap();

        let file_id = index
            .upsert_file(
                Path::new("src/main.rs"),
                Language::Rust,
                1_234_567_890,
                100,
                Some(0xDEAD_BEEF),
            )
            .unwrap();

        assert!(file_id > 0);

        let file = index.get_file(Path::new("src/main.rs")).unwrap();
        assert!(file.is_some());
        let file = file.unwrap();
        assert_eq!(file.language, Language::Rust);
        assert_eq!(file.size_bytes, 100);
    }

    #[test]
    fn upsert_file_updates_existing() {
        let (_dir, path) = temp_db();
        let index = Index::open(&path).unwrap();

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
        let index = Index::open(&path).unwrap();

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
        let index = Index::open(&path).unwrap();

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
        let index = Index::open(&path).unwrap();

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
            .insert_reference(symbol_id, ref_file_id, "call", 5, 10, None)
            .unwrap();

        // Query references
        let refs = index.get_references_to_symbol(symbol_id).unwrap();
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].symbol_id, symbol_id);
        assert_eq!(refs[0].file_id, ref_file_id);
        assert_eq!(refs[0].line, 5);
    }

    #[test]
    fn insert_file_dependency() {
        let (_dir, path) = temp_db();
        let index = Index::open(&path).unwrap();

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
        let index = Index::open(&path).unwrap();

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
}
