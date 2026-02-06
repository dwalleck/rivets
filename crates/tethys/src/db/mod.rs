//! `SQLite` storage layer for Tethys.
//!
//! This module manages the `SQLite` database that stores indexed symbols and references.
//! `SQLite` is the source of truth for all persistent data. See `graph` module for
//! graph traversal queries built on top of this storage layer.
//!
//! ## Module Structure
//!
//! - `schema` - Database schema (DDL)
//! - `helpers` - Row conversion and parsing utilities
//! - `files` - File CRUD operations
//! - `symbols` - Symbol CRUD operations
//! - `references` - Reference CRUD operations
//! - `imports` - Import CRUD operations
//! - `call_edges` - Call edge CRUD operations
//! - `file_deps` - File dependency CRUD operations
//! - `graph` - Graph traversal operations (`SymbolGraphOps`, `FileGraphOps`)

mod call_edges;
mod file_deps;
mod files;
mod graph;
mod helpers;
mod imports;
mod panic_points;
mod references;
mod schema;
mod symbols;

// Re-export helper functions and SQL constants used by other modules
pub(crate) use helpers::{
    parse_language, parse_symbol_kind, row_to_import, row_to_indexed_file, row_to_reference,
    row_to_symbol, FILES_COLUMNS, REFS_COLUMNS, SYMBOLS_COLUMNS,
};
pub(crate) use schema::SCHEMA;

// Re-export parse_visibility for tests in types.rs
#[cfg(test)]
pub(crate) use helpers::parse_visibility;

use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;

use crate::error::{Error, Result};
use crate::types::{Span, SymbolKind, Visibility};

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
    pub parent_symbol_id: Option<crate::types::SymbolId>,
    /// Whether this symbol is a test function.
    pub is_test: bool,
}

/// `SQLite` database wrapper for Tethys index.
///
/// The connection is wrapped in a `Mutex` to allow sharing across graph operations
/// while maintaining thread safety.
pub struct Index {
    conn: Mutex<Connection>,
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

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Acquire the connection lock.
    ///
    /// Returns a `MutexGuard` providing exclusive access to the underlying connection.
    /// Used internally by all database operations.
    pub(crate) fn connection(&self) -> Result<MutexGuard<'_, Connection>> {
        self.conn.lock().map_err(|e| {
            Error::Internal(format!(
                "database connection mutex poisoned (a thread panicked while holding the lock): {e}"
            ))
        })
    }

    /// Get the current unix timestamp in nanoseconds.
    ///
    /// Returns an error if the system time is before the Unix epoch, which would
    /// break timestamp comparison logic for incremental indexing.
    // u128 nanoseconds won't exceed i64::MAX until year 2262
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
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

    /// Clear all data from the database.
    pub fn clear(&self) -> Result<()> {
        let conn = self.connection()?;

        conn.execute_batch(
            "DELETE FROM call_edges; DELETE FROM refs; DELETE FROM symbols; DELETE FROM file_deps; DELETE FROM imports; DELETE FROM files;",
        )?;
        Ok(())
    }

    /// Get statistics about the database contents.
    pub fn get_stats(&self) -> Result<crate::types::DatabaseStats> {
        use std::collections::HashMap;

        let conn = self.connection()?;
        let mut stats = crate::types::DatabaseStats::default();

        // File counts by language
        let mut stmt = conn.prepare("SELECT language, COUNT(*) FROM files GROUP BY language")?;
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
        let mut stmt = conn.prepare("SELECT kind, COUNT(*) FROM symbols GROUP BY kind")?;
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
        let ref_count: usize = conn.query_row("SELECT COUNT(*) FROM refs", [], |row| row.get(0))?;
        stats.reference_count = ref_count;

        // File dependency count
        let dep_count: usize =
            conn.query_row("SELECT COUNT(*) FROM file_deps", [], |row| row.get(0))?;
        stats.file_dependency_count = dep_count;

        Ok(stats)
    }

    /// Update `SQLite` query planner statistics.
    ///
    /// Should be called after bulk data changes (full re-index) so the query
    /// planner can make better index-selection decisions. Not needed after
    /// small incremental updates.
    pub fn analyze(&self) -> Result<()> {
        let conn = self.connection()?;

        conn.execute_batch("ANALYZE")?;
        Ok(())
    }

    /// Vacuum the database.
    pub fn vacuum(&self) -> Result<()> {
        let conn = self.connection()?;

        conn.execute_batch("VACUUM")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Language, SymbolKind, Visibility};
    use std::path::PathBuf;
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
        let conn = index.connection().expect("should get connection");

        let tables: Vec<String> = conn
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
        assert!(tables.contains(&"call_edges".to_string()));
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
                false,
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
                false,
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
                false,
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
                false,
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
}
