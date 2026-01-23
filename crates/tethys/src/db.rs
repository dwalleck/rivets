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
use tracing::warn;

use crate::error::Result;
use crate::types::{IndexedFile, Language, Span, Symbol, SymbolKind, Visibility};

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
    /// Falls back to 0 (Unix epoch) if system time is unavailable, with a warning logged.
    fn now_ns() -> i64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map_or_else(
            |e| {
                warn!(error = %e, "System time before Unix epoch, using 0");
                0
            },
            |d| d.as_nanos() as i64,
        )
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
        let lang_str = match language {
            Language::Rust => "rust",
            Language::CSharp => "csharp",
        };
        let indexed_at = Self::now_ns();

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
                |row| {
                    Ok(IndexedFile {
                        id: row.get(0)?,
                        path: PathBuf::from(row.get::<_, String>(1)?),
                        language: parse_language(row.get::<_, String>(2)?.as_str()),
                        mtime_ns: row.get(3)?,
                        size_bytes: row.get::<_, i64>(4)? as u64,
                        content_hash: row.get::<_, Option<i64>>(5)?.map(|h| h as u64),
                        indexed_at: row.get(6)?,
                    })
                },
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
        let lang_str = match language {
            Language::Rust => "rust",
            Language::CSharp => "csharp",
        };
        let indexed_at = Self::now_ns();

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
                    sym.span.map(|s| s.end_line),
                    sym.span.map(|s| s.end_column),
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
                span.map(|s| s.end_line),
                span.map(|s| s.end_column),
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
/// Falls back to `Language::Rust` for unrecognized values, with a warning logged.
fn parse_language(s: &str) -> Language {
    match s {
        "rust" => Language::Rust,
        "csharp" => Language::CSharp,
        unknown => {
            warn!(
                language = unknown,
                "Unknown language in database, defaulting to Rust"
            );
            Language::Rust
        }
    }
}

/// Parse a symbol kind string from the database.
///
/// Falls back to `SymbolKind::Function` for unrecognized values, with a warning logged.
fn parse_symbol_kind(s: &str) -> SymbolKind {
    match s {
        "function" => SymbolKind::Function,
        "method" => SymbolKind::Method,
        "struct" => SymbolKind::Struct,
        "class" => SymbolKind::Class,
        "enum" => SymbolKind::Enum,
        "trait" => SymbolKind::Trait,
        "interface" => SymbolKind::Interface,
        "const" => SymbolKind::Const,
        "static" => SymbolKind::Static,
        "module" => SymbolKind::Module,
        "type_alias" => SymbolKind::TypeAlias,
        "macro" => SymbolKind::Macro,
        unknown => {
            warn!(
                kind = unknown,
                "Unknown symbol kind in database, defaulting to Function"
            );
            SymbolKind::Function
        }
    }
}

/// Parse a visibility string from the database.
///
/// Falls back to `Visibility::Private` for unrecognized values, with a warning logged.
fn parse_visibility(s: &str) -> Visibility {
    match s {
        "public" => Visibility::Public,
        "crate" => Visibility::Crate,
        "module" => Visibility::Module,
        "private" => Visibility::Private,
        unknown => {
            warn!(
                visibility = unknown,
                "Unknown visibility in database, defaulting to Private"
            );
            Visibility::Private
        }
    }
}

/// Convert a database row to a Symbol.
fn row_to_symbol(row: &rusqlite::Row) -> rusqlite::Result<Symbol> {
    let end_line: Option<u32> = row.get(8)?;
    let end_column: Option<u32> = row.get(9)?;
    let line: u32 = row.get(6)?;
    let column: u32 = row.get(7)?;

    let span = match (end_line, end_column) {
        (Some(el), Some(ec)) => Some(Span {
            start_line: line,
            start_column: column,
            end_line: el,
            end_column: ec,
        }),
        _ => None,
    };

    Ok(Symbol {
        id: row.get(0)?,
        file_id: row.get(1)?,
        name: row.get(2)?,
        module_path: row.get(3)?,
        qualified_name: row.get(4)?,
        kind: parse_symbol_kind(&row.get::<_, String>(5)?),
        line,
        column,
        span,
        signature: row.get(10)?,
        signature_details: None, // TODO: Parse from JSON column when stored
        visibility: parse_visibility(&row.get::<_, String>(11)?),
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

    // === Parsing fallback tests ===

    #[test]
    fn parse_language_known_values() {
        assert_eq!(parse_language("rust"), Language::Rust);
        assert_eq!(parse_language("csharp"), Language::CSharp);
    }

    #[test]
    fn parse_language_unknown_defaults_to_rust() {
        // Unknown values should default to Rust (with a warning logged)
        assert_eq!(parse_language("python"), Language::Rust);
        assert_eq!(parse_language(""), Language::Rust);
        assert_eq!(parse_language("unknown"), Language::Rust);
    }

    #[test]
    fn parse_symbol_kind_known_values() {
        assert_eq!(parse_symbol_kind("function"), SymbolKind::Function);
        assert_eq!(parse_symbol_kind("method"), SymbolKind::Method);
        assert_eq!(parse_symbol_kind("struct"), SymbolKind::Struct);
        assert_eq!(parse_symbol_kind("class"), SymbolKind::Class);
        assert_eq!(parse_symbol_kind("enum"), SymbolKind::Enum);
        assert_eq!(parse_symbol_kind("trait"), SymbolKind::Trait);
        assert_eq!(parse_symbol_kind("interface"), SymbolKind::Interface);
        assert_eq!(parse_symbol_kind("const"), SymbolKind::Const);
        assert_eq!(parse_symbol_kind("static"), SymbolKind::Static);
        assert_eq!(parse_symbol_kind("module"), SymbolKind::Module);
        assert_eq!(parse_symbol_kind("type_alias"), SymbolKind::TypeAlias);
        assert_eq!(parse_symbol_kind("macro"), SymbolKind::Macro);
    }

    #[test]
    fn parse_symbol_kind_unknown_defaults_to_function() {
        // Unknown values should default to Function (with a warning logged)
        assert_eq!(parse_symbol_kind("unknown"), SymbolKind::Function);
        assert_eq!(parse_symbol_kind(""), SymbolKind::Function);
    }

    #[test]
    fn parse_visibility_known_values() {
        assert_eq!(parse_visibility("public"), Visibility::Public);
        assert_eq!(parse_visibility("crate"), Visibility::Crate);
        assert_eq!(parse_visibility("module"), Visibility::Module);
        assert_eq!(parse_visibility("private"), Visibility::Private);
    }

    #[test]
    fn parse_visibility_unknown_defaults_to_private() {
        // Unknown values should default to Private (with a warning logged)
        assert_eq!(parse_visibility("protected"), Visibility::Private);
        assert_eq!(parse_visibility(""), Visibility::Private);
        assert_eq!(parse_visibility("internal"), Visibility::Private);
    }
}
