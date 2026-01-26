//! SQL-based implementations of graph operations.
//!
//! Uses recursive CTEs for graph traversal, keeping all data in `SQLite`.

// SQLite uses i64 for all integer storage. These casts are intentional and safe for
// practical values (reference counts within reasonable bounds).
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use rusqlite::{Connection, OptionalExtension};

use super::{
    CallPath, CalleeInfo, CallerInfo, FileDepInfo, FileGraphOps, FileImpact, FilePath,
    SymbolGraphOps, SymbolImpact,
};
use crate::db::{row_to_indexed_file, row_to_symbol};
use crate::error::{Error, Result};
use crate::types::{FileId, IndexedFile, ReferenceKind, Symbol, SymbolId};

/// Default maximum depth for recursive graph traversals.
///
/// Prevents runaway recursion in deeply nested or cyclic dependency graphs.
/// Can be overridden by passing an explicit `max_depth` parameter.
const DEFAULT_MAX_DEPTH: u32 = 50;

/// Thread-safe wrapper around a `SQLite` connection.
///
/// Provides common functionality for graph operation implementations.
struct DbConnection {
    conn: Mutex<Connection>,
}

impl DbConnection {
    /// Open a new connection to the database.
    fn open(db_path: &Path) -> Result<Self> {
        let conn = Connection::open(db_path)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Acquire the connection lock, converting poison errors to our error type.
    fn lock(&self) -> Result<MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|e| Error::Internal(format!("mutex poisoned: {e}")))
    }
}

/// SQL-based implementation of symbol graph operations.
///
/// Wraps a `Connection` in a `Mutex` to satisfy the `Send + Sync` bounds
/// required by the `SymbolGraphOps` trait.
pub struct SqlSymbolGraph {
    db: DbConnection,
}

impl SqlSymbolGraph {
    /// Create a new SQL symbol graph connected to the given database.
    pub fn new(db_path: &Path) -> Result<Self> {
        Ok(Self {
            db: DbConnection::open(db_path)?,
        })
    }

    /// Get a symbol by its database ID.
    fn get_symbol_by_id(&self, id: i64) -> Result<Option<Symbol>> {
        let conn = self.db.lock()?;

        let mut stmt = conn.prepare(
            "SELECT id, file_id, name, module_path, qualified_name, kind, line, column,
             end_line, end_column, signature, visibility, parent_symbol_id
             FROM symbols WHERE id = ?1",
        )?;

        let symbol = stmt.query_row([id], row_to_symbol).optional()?;

        Ok(symbol)
    }
}

/// SQL-based implementation of file graph operations.
///
/// Wraps a `Connection` in a `Mutex` to satisfy the `Send + Sync` bounds
/// required by the `FileGraphOps` trait.
pub struct SqlFileGraph {
    db: DbConnection,
}

impl SqlFileGraph {
    /// Create a new SQL file graph connected to the given database.
    pub fn new(db_path: &Path) -> Result<Self> {
        Ok(Self {
            db: DbConnection::open(db_path)?,
        })
    }

    /// Get a file by its database ID.
    fn get_file_by_id(&self, id: i64) -> Result<Option<IndexedFile>> {
        let conn = self.db.lock()?;

        let mut stmt = conn.prepare(
            "SELECT id, path, language, mtime_ns, size_bytes, content_hash, indexed_at
             FROM files WHERE id = ?1",
        )?;

        let file = stmt.query_row([id], row_to_indexed_file).optional()?;

        Ok(file)
    }
}

impl FileGraphOps for SqlFileGraph {
    fn get_dependents(&self, file_id: FileId) -> Result<Vec<FileDepInfo>> {
        let conn = self.db.lock()?;

        // Find all files that depend on the target file
        // file_deps has (from_file_id, to_file_id) where from depends on to
        // So we need files where to_file_id = file_id (files that depend ON it)
        let mut stmt = conn.prepare(
            "SELECT
                f.id, f.path, f.language, f.mtime_ns, f.size_bytes, f.content_hash, f.indexed_at,
                fd.ref_count
             FROM file_deps fd
             JOIN files f ON f.id = fd.from_file_id
             WHERE fd.to_file_id = ?1
             ORDER BY f.path",
        )?;

        let dependents = stmt
            .query_map([file_id.as_i64()], |row| {
                let file = row_to_indexed_file(row)?;
                let ref_count: usize = row.get::<_, i64>(7)? as usize;

                Ok(FileDepInfo { file, ref_count })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(dependents)
    }

    fn get_dependencies(&self, file_id: FileId) -> Result<Vec<FileDepInfo>> {
        let conn = self.db.lock()?;

        // Find all files that the given file depends on
        // file_deps has (from_file_id, to_file_id) where from depends on to
        // So we need files where from_file_id = file_id (files it depends ON)
        let mut stmt = conn.prepare(
            "SELECT
                f.id, f.path, f.language, f.mtime_ns, f.size_bytes, f.content_hash, f.indexed_at,
                fd.ref_count
             FROM file_deps fd
             JOIN files f ON f.id = fd.to_file_id
             WHERE fd.from_file_id = ?1
             ORDER BY f.path",
        )?;

        let dependencies = stmt
            .query_map([file_id.as_i64()], |row| {
                let file = row_to_indexed_file(row)?;
                let ref_count: usize = row.get::<_, i64>(7)? as usize;

                Ok(FileDepInfo { file, ref_count })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(dependencies)
    }

    fn get_transitive_dependents(
        &self,
        file_id: FileId,
        max_depth: Option<u32>,
    ) -> Result<FileImpact> {
        let max_depth = max_depth.unwrap_or(DEFAULT_MAX_DEPTH);
        let target = self
            .get_file_by_id(file_id.as_i64())?
            .ok_or_else(|| Error::NotFound(format!("file id: {}", file_id.as_i64())))?;

        let conn = self.db.lock()?;

        // Use recursive CTE to find all dependents with their depth
        let mut stmt = conn.prepare(
            "WITH RECURSIVE dependent_tree(file_id, depth) AS (
                -- Base case: direct dependents
                SELECT DISTINCT fd.from_file_id, 1
                FROM file_deps fd
                WHERE fd.to_file_id = ?1

                UNION

                -- Recursive case: dependents of dependents
                SELECT DISTINCT fd.from_file_id, dt.depth + 1
                FROM file_deps fd
                JOIN dependent_tree dt ON fd.to_file_id = dt.file_id
                WHERE dt.depth < ?2
            )
            SELECT DISTINCT
                f.id, f.path, f.language, f.mtime_ns, f.size_bytes, f.content_hash, f.indexed_at,
                MIN(dt.depth) as min_depth
            FROM dependent_tree dt
            JOIN files f ON f.id = dt.file_id
            GROUP BY f.id
            ORDER BY min_depth, f.path",
        )?;

        let mut direct_dependents = Vec::new();
        let mut transitive_dependents = Vec::new();

        let rows = stmt.query_map(rusqlite::params![file_id.as_i64(), max_depth], |row| {
            let file = row_to_indexed_file(row)?;
            let depth: u32 = row.get::<_, i64>(7)? as u32;
            Ok((file, depth))
        })?;

        for row in rows {
            let (file, depth) = row?;

            let dep_info = FileDepInfo { file, ref_count: 1 };

            if depth == 1 {
                direct_dependents.push(dep_info);
            } else {
                transitive_dependents.push(dep_info);
            }
        }

        Ok(FileImpact {
            target,
            direct_dependents,
            transitive_dependents,
        })
    }

    fn find_dependency_path(
        &self,
        from_file_id: FileId,
        to_file_id: FileId,
    ) -> Result<Option<FilePath>> {
        // Same file - trivial path
        if from_file_id == to_file_id {
            let file = self
                .get_file_by_id(from_file_id.as_i64())?
                .ok_or_else(|| Error::NotFound(format!("file id: {}", from_file_id.as_i64())))?;
            return Ok(Some(FilePath::single(file)));
        }

        // BFS to find shortest path using recursive CTE
        // We search forward from `from` through dependencies (what does `from` depend on?)
        let max_depth = DEFAULT_MAX_DEPTH;

        // Scope the connection lock to just the query execution
        let file_ids: Option<Vec<i64>> = {
            let conn = self.db.lock()?;

            let mut stmt = conn.prepare(
                "WITH RECURSIVE path_search(file_id, path, depth) AS (
                    -- Start from the source file
                    SELECT ?1, CAST(?1 AS TEXT), 0

                    UNION

                    -- Follow dependencies (files that the current file depends on)
                    SELECT fd.to_file_id,
                           ps.path || ',' || fd.to_file_id,
                           ps.depth + 1
                    FROM file_deps fd
                    JOIN path_search ps ON fd.from_file_id = ps.file_id
                    WHERE ps.depth < ?3
                )
                SELECT path
                FROM path_search
                WHERE file_id = ?2
                ORDER BY depth
                LIMIT 1",
            )?;

            let path_str: Option<String> = stmt
                .query_row(
                    rusqlite::params![from_file_id.as_i64(), to_file_id.as_i64(), max_depth],
                    |row| row.get(0),
                )
                .optional()?;

            path_str.map(|s| parse_path_ids(&s))
        };

        let Some(file_ids) = file_ids else {
            return Ok(None);
        };

        // Fetch files for each ID in the path
        let mut files = Vec::with_capacity(file_ids.len());
        for id in file_ids {
            let file = self
                .get_file_by_id(id)?
                .ok_or_else(|| Error::NotFound(format!("file id: {id}")))?;
            files.push(file);
        }

        // Use validated constructor - invariants guaranteed by construction
        Ok(FilePath::new(files))
    }

    fn detect_cycles(&self) -> Result<Vec<crate::types::Cycle>> {
        tracing::warn!("Cycle detection not yet implemented, returning empty result");
        Ok(vec![])
    }

    fn detect_cycles_involving(&self, file_id: FileId) -> Result<Vec<crate::types::Cycle>> {
        tracing::warn!(
            file_id = %file_id.as_i64(),
            "Cycle detection not yet implemented, returning empty result"
        );
        Ok(vec![])
    }
}

impl SymbolGraphOps for SqlSymbolGraph {
    fn get_callers(&self, symbol_id: SymbolId) -> Result<Vec<CallerInfo>> {
        let conn = self.db.lock()?;

        // Find all symbols that contain references to the target symbol
        let mut stmt = conn.prepare(
            "SELECT
                s.id, s.file_id, s.name, s.module_path, s.qualified_name,
                s.kind, s.line, s.column, s.end_line, s.end_column,
                s.signature, s.visibility, s.parent_symbol_id,
                COUNT(*) as ref_count,
                GROUP_CONCAT(DISTINCT r.kind) as ref_kinds
             FROM refs r
             JOIN symbols s ON s.id = r.in_symbol_id
             WHERE r.symbol_id = ?1 AND r.in_symbol_id IS NOT NULL
             GROUP BY s.id
             ORDER BY s.qualified_name",
        )?;

        let callers = stmt
            .query_map([symbol_id.as_i64()], |row| {
                let symbol = row_to_symbol(row)?;
                let ref_count: usize = row.get::<_, i64>(13)? as usize;
                let ref_kinds_str: String = row.get(14)?;
                let reference_kinds = parse_reference_kinds(&ref_kinds_str);

                Ok(CallerInfo {
                    symbol,
                    reference_count: ref_count,
                    reference_kinds,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(callers)
    }

    fn get_callees(&self, symbol_id: SymbolId) -> Result<Vec<CalleeInfo>> {
        let conn = self.db.lock()?;

        // Find all symbols that the given symbol references
        let mut stmt = conn.prepare(
            "SELECT
                s.id, s.file_id, s.name, s.module_path, s.qualified_name,
                s.kind, s.line, s.column, s.end_line, s.end_column,
                s.signature, s.visibility, s.parent_symbol_id,
                COUNT(*) as ref_count,
                GROUP_CONCAT(DISTINCT r.kind) as ref_kinds
             FROM refs r
             JOIN symbols s ON s.id = r.symbol_id
             WHERE r.in_symbol_id = ?1
             GROUP BY s.id
             ORDER BY s.qualified_name",
        )?;

        let callees = stmt
            .query_map([symbol_id.as_i64()], |row| {
                let symbol = row_to_symbol(row)?;
                let ref_count: usize = row.get::<_, i64>(13)? as usize;
                let ref_kinds_str: String = row.get(14)?;
                let reference_kinds = parse_reference_kinds(&ref_kinds_str);

                Ok(CalleeInfo {
                    symbol,
                    reference_count: ref_count,
                    reference_kinds,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(callees)
    }

    fn get_transitive_callers(
        &self,
        symbol_id: SymbolId,
        max_depth: Option<u32>,
    ) -> Result<SymbolImpact> {
        let max_depth = max_depth.unwrap_or(DEFAULT_MAX_DEPTH);
        let target = self
            .get_symbol_by_id(symbol_id.as_i64())?
            .ok_or_else(|| Error::NotFound(format!("symbol id: {}", symbol_id.as_i64())))?;

        let conn = self.db.lock()?;

        // Use recursive CTE to find all callers with their depth
        let mut stmt = conn.prepare(
            "WITH RECURSIVE caller_tree(symbol_id, depth) AS (
                -- Base case: direct callers
                SELECT DISTINCT r.in_symbol_id, 1
                FROM refs r
                WHERE r.symbol_id = ?1
                  AND r.in_symbol_id IS NOT NULL

                UNION

                -- Recursive case: callers of callers
                SELECT DISTINCT r.in_symbol_id, ct.depth + 1
                FROM refs r
                JOIN caller_tree ct ON r.symbol_id = ct.symbol_id
                WHERE r.in_symbol_id IS NOT NULL
                  AND ct.depth < ?2
            )
            SELECT DISTINCT
                s.id, s.file_id, s.name, s.module_path, s.qualified_name,
                s.kind, s.line, s.column, s.end_line, s.end_column,
                s.signature, s.visibility, s.parent_symbol_id,
                MIN(ct.depth) as min_depth
            FROM caller_tree ct
            JOIN symbols s ON s.id = ct.symbol_id
            GROUP BY s.id
            ORDER BY min_depth, s.qualified_name",
        )?;

        let mut direct_callers = Vec::new();
        let mut transitive_callers = Vec::new();
        let mut max_depth_reached: u32 = 0;

        let rows = stmt.query_map(rusqlite::params![symbol_id.as_i64(), max_depth], |row| {
            let symbol = row_to_symbol(row)?;
            let depth: u32 = row.get::<_, i64>(13)? as u32;
            Ok((symbol, depth))
        })?;

        for row in rows {
            let (symbol, depth) = row?;
            max_depth_reached = max_depth_reached.max(depth);

            let caller_info = CallerInfo {
                symbol,
                reference_count: 1,
                reference_kinds: vec![ReferenceKind::Call],
            };

            if depth == 1 {
                direct_callers.push(caller_info);
            } else {
                transitive_callers.push(caller_info);
            }
        }

        Ok(SymbolImpact {
            target,
            direct_callers,
            transitive_callers,
            max_depth_reached,
        })
    }

    fn find_call_path(
        &self,
        from_symbol_id: SymbolId,
        to_symbol_id: SymbolId,
    ) -> Result<Option<CallPath>> {
        // Same symbol - trivial path
        if from_symbol_id == to_symbol_id {
            let symbol = self
                .get_symbol_by_id(from_symbol_id.as_i64())?
                .ok_or_else(|| {
                    Error::NotFound(format!("symbol id: {}", from_symbol_id.as_i64()))
                })?;
            return Ok(Some(CallPath::single(symbol)));
        }

        // BFS to find shortest path using recursive CTE
        // We search forward from `from` through callees (what does `from` call?)
        let max_depth = DEFAULT_MAX_DEPTH;

        // Scope the connection lock to just the query execution
        let symbol_ids: Option<Vec<i64>> = {
            let conn = self.db.lock()?;

            let mut stmt = conn.prepare(
                "WITH RECURSIVE path_search(symbol_id, path, depth) AS (
                    -- Start from the source symbol
                    SELECT ?1, CAST(?1 AS TEXT), 0

                    UNION

                    -- Follow callees (symbols that the current symbol calls)
                    SELECT r.symbol_id,
                           ps.path || ',' || r.symbol_id,
                           ps.depth + 1
                    FROM refs r
                    JOIN path_search ps ON r.in_symbol_id = ps.symbol_id
                    WHERE ps.depth < ?3
                      AND r.symbol_id IS NOT NULL
                )
                SELECT path
                FROM path_search
                WHERE symbol_id = ?2
                ORDER BY depth
                LIMIT 1",
            )?;

            let path_str: Option<String> = stmt
                .query_row(
                    rusqlite::params![from_symbol_id.as_i64(), to_symbol_id.as_i64(), max_depth],
                    |row| row.get(0),
                )
                .optional()?;

            path_str.map(|s| parse_path_ids(&s))
        };

        let Some(symbol_ids) = symbol_ids else {
            return Ok(None);
        };

        // Fetch symbols for each ID in the path
        let mut symbols = Vec::with_capacity(symbol_ids.len());
        for id in symbol_ids {
            let symbol = self
                .get_symbol_by_id(id)?
                .ok_or_else(|| Error::NotFound(format!("symbol id: {id}")))?;
            symbols.push(symbol);
        }

        // Create edges (all Call for simplicity)
        let edges = vec![ReferenceKind::Call; symbols.len().saturating_sub(1)];

        // Use validated constructor - invariants guaranteed by construction
        Ok(CallPath::new(symbols, edges))
    }
}

/// Parse a comma-separated path string into a vector of i64 IDs.
///
/// Used by path-finding queries that store traversal paths as comma-separated strings
/// in SQL. Invalid IDs are logged as warnings and skipped.
fn parse_path_ids(path_str: &str) -> Vec<i64> {
    path_str
        .split(',')
        .filter_map(|id| {
            let trimmed = id.trim();
            if trimmed.is_empty() {
                return None;
            }
            match trimmed.parse() {
                Ok(id) => Some(id),
                Err(e) => {
                    tracing::warn!(
                        raw_id = %trimmed,
                        error = %e,
                        raw_path = %path_str,
                        "Failed to parse ID in path, possible database corruption"
                    );
                    None
                }
            }
        })
        .collect()
}

fn parse_reference_kinds(s: &str) -> Vec<ReferenceKind> {
    s.split(',')
        .filter_map(|kind| match kind.trim() {
            "import" => Some(ReferenceKind::Import),
            "call" => Some(ReferenceKind::Call),
            "type" => Some(ReferenceKind::Type),
            "inherit" => Some(ReferenceKind::Inherit),
            "construct" => Some(ReferenceKind::Construct),
            "field_access" => Some(ReferenceKind::FieldAccess),
            "" => None, // Empty strings from split are expected
            unknown => {
                tracing::warn!(
                    unknown_kind = %unknown,
                    raw_input = %s,
                    "Unknown reference kind in database, possible corruption or version mismatch"
                );
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Index;
    use crate::types::{FileId, Language, SymbolId, SymbolKind, Visibility};
    use tempfile::TempDir;

    /// Create a test database with a known call graph:
    ///
    /// ```text
    ///   main::run --> auth::validate --> db::query
    ///              \-> cache::get -------/
    /// ```
    #[allow(clippy::too_many_lines)]
    fn setup_test_graph() -> (TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let mut index = Index::open(&db_path).unwrap();

        // Create files
        let main_file = index
            .index_file_atomic(
                std::path::Path::new("src/main.rs"),
                Language::Rust,
                1000,
                100,
                None,
                &[crate::db::SymbolData {
                    name: "run",
                    module_path: "main",
                    qualified_name: "main::run",
                    kind: SymbolKind::Function,
                    line: 1,
                    column: 1,
                    span: None,
                    signature: Some("fn run()"),
                    visibility: Visibility::Public,
                    parent_symbol_id: None,
                }],
            )
            .unwrap();

        let auth_file = index
            .index_file_atomic(
                std::path::Path::new("src/auth.rs"),
                Language::Rust,
                1000,
                100,
                None,
                &[crate::db::SymbolData {
                    name: "validate",
                    module_path: "auth",
                    qualified_name: "auth::validate",
                    kind: SymbolKind::Function,
                    line: 1,
                    column: 1,
                    span: None,
                    signature: Some("fn validate()"),
                    visibility: Visibility::Public,
                    parent_symbol_id: None,
                }],
            )
            .unwrap();

        let _db_file = index
            .index_file_atomic(
                std::path::Path::new("src/db.rs"),
                Language::Rust,
                1000,
                100,
                None,
                &[crate::db::SymbolData {
                    name: "query",
                    module_path: "db",
                    qualified_name: "db::query",
                    kind: SymbolKind::Function,
                    line: 1,
                    column: 1,
                    span: None,
                    signature: Some("fn query()"),
                    visibility: Visibility::Public,
                    parent_symbol_id: None,
                }],
            )
            .unwrap();

        let cache_file = index
            .index_file_atomic(
                std::path::Path::new("src/cache.rs"),
                Language::Rust,
                1000,
                100,
                None,
                &[crate::db::SymbolData {
                    name: "get",
                    module_path: "cache",
                    qualified_name: "cache::get",
                    kind: SymbolKind::Function,
                    line: 1,
                    column: 1,
                    span: None,
                    signature: Some("fn get()"),
                    visibility: Visibility::Public,
                    parent_symbol_id: None,
                }],
            )
            .unwrap();

        // Get symbol IDs
        let main_run = index
            .get_symbol_by_qualified_name("main::run")
            .unwrap()
            .unwrap();
        let auth_validate = index
            .get_symbol_by_qualified_name("auth::validate")
            .unwrap()
            .unwrap();
        let db_query = index
            .get_symbol_by_qualified_name("db::query")
            .unwrap()
            .unwrap();
        let cache_get = index
            .get_symbol_by_qualified_name("cache::get")
            .unwrap()
            .unwrap();

        // Create references: main::run -> auth::validate
        index
            .insert_reference(auth_validate.id, main_file, "call", 5, 1, Some(main_run.id))
            .unwrap();
        // main::run -> cache::get
        index
            .insert_reference(cache_get.id, main_file, "call", 6, 1, Some(main_run.id))
            .unwrap();
        // auth::validate -> db::query
        index
            .insert_reference(db_query.id, auth_file, "call", 3, 1, Some(auth_validate.id))
            .unwrap();
        // cache::get -> db::query
        index
            .insert_reference(db_query.id, cache_file, "call", 3, 1, Some(cache_get.id))
            .unwrap();

        (dir, db_path)
    }

    #[test]
    fn get_callers_returns_direct_callers() {
        let (_dir, db_path) = setup_test_graph();
        let graph = SqlSymbolGraph::new(&db_path).unwrap();
        let index = Index::open(&db_path).unwrap();

        let db_query = index
            .get_symbol_by_qualified_name("db::query")
            .unwrap()
            .unwrap();
        let callers = graph.get_callers(SymbolId::from(db_query.id)).unwrap();

        // db::query is called by auth::validate and cache::get
        assert_eq!(callers.len(), 2, "expected 2 callers, got: {callers:?}");

        let caller_names: Vec<&str> = callers.iter().map(|c| c.symbol.name.as_str()).collect();
        assert!(
            caller_names.contains(&"validate"),
            "should include auth::validate"
        );
        assert!(caller_names.contains(&"get"), "should include cache::get");
    }

    #[test]
    fn get_callers_returns_empty_for_uncalled_symbol() {
        let (_dir, db_path) = setup_test_graph();
        let graph = SqlSymbolGraph::new(&db_path).unwrap();
        let index = Index::open(&db_path).unwrap();

        let main_run = index
            .get_symbol_by_qualified_name("main::run")
            .unwrap()
            .unwrap();
        let callers = graph.get_callers(SymbolId::from(main_run.id)).unwrap();

        // main::run is not called by anything
        assert!(callers.is_empty(), "main::run should have no callers");
    }

    #[test]
    fn get_callees_returns_direct_callees() {
        let (_dir, db_path) = setup_test_graph();
        let graph = SqlSymbolGraph::new(&db_path).unwrap();
        let index = Index::open(&db_path).unwrap();

        let main_run = index
            .get_symbol_by_qualified_name("main::run")
            .unwrap()
            .unwrap();
        let callees = graph.get_callees(SymbolId::from(main_run.id)).unwrap();

        // main::run calls auth::validate and cache::get
        assert_eq!(callees.len(), 2, "expected 2 callees, got: {callees:?}");

        let callee_names: Vec<&str> = callees.iter().map(|c| c.symbol.name.as_str()).collect();
        assert!(
            callee_names.contains(&"validate"),
            "should include auth::validate"
        );
        assert!(callee_names.contains(&"get"), "should include cache::get");
    }

    #[test]
    fn get_callees_returns_empty_for_leaf_symbol() {
        let (_dir, db_path) = setup_test_graph();
        let graph = SqlSymbolGraph::new(&db_path).unwrap();
        let index = Index::open(&db_path).unwrap();

        let db_query = index
            .get_symbol_by_qualified_name("db::query")
            .unwrap()
            .unwrap();
        let callees = graph.get_callees(SymbolId::from(db_query.id)).unwrap();

        // db::query doesn't call anything
        assert!(callees.is_empty(), "db::query should have no callees");
    }

    #[test]
    fn get_callers_includes_reference_kinds() {
        let (_dir, db_path) = setup_test_graph();
        let graph = SqlSymbolGraph::new(&db_path).unwrap();
        let index = Index::open(&db_path).unwrap();

        let db_query = index
            .get_symbol_by_qualified_name("db::query")
            .unwrap()
            .unwrap();
        let callers = graph.get_callers(SymbolId::from(db_query.id)).unwrap();

        // All references should be "call" type
        for caller in &callers {
            assert!(
                caller.reference_kinds.contains(&ReferenceKind::Call),
                "expected call reference kind for {:?}",
                caller.symbol.name
            );
        }
    }

    #[test]
    fn get_callers_includes_reference_count() {
        let (_dir, db_path) = setup_test_graph();
        let graph = SqlSymbolGraph::new(&db_path).unwrap();
        let index = Index::open(&db_path).unwrap();

        let db_query = index
            .get_symbol_by_qualified_name("db::query")
            .unwrap()
            .unwrap();
        let callers = graph.get_callers(SymbolId::from(db_query.id)).unwrap();

        // Each caller should have exactly 1 reference
        for caller in &callers {
            assert_eq!(
                caller.reference_count, 1,
                "expected 1 reference for {:?}",
                caller.symbol.name
            );
        }
    }

    #[test]
    fn get_transitive_callers_finds_all_ancestors() {
        let (_dir, db_path) = setup_test_graph();
        let graph = SqlSymbolGraph::new(&db_path).unwrap();
        let index = Index::open(&db_path).unwrap();

        let db_query = index
            .get_symbol_by_qualified_name("db::query")
            .unwrap()
            .unwrap();
        let impact = graph
            .get_transitive_callers(SymbolId::from(db_query.id), None)
            .unwrap();

        // db::query's transitive callers: auth::validate, cache::get (direct), main::run (transitive)
        assert_eq!(impact.total_caller_count(), 3, "expected 3 total callers");
        assert_eq!(impact.direct_callers.len(), 2, "expected 2 direct callers");
        assert_eq!(
            impact.transitive_callers.len(),
            1,
            "expected 1 transitive caller"
        );

        let transitive_names: Vec<&str> = impact
            .transitive_callers
            .iter()
            .map(|c| c.symbol.name.as_str())
            .collect();
        assert!(
            transitive_names.contains(&"run"),
            "main::run should be transitive caller"
        );
    }

    #[test]
    fn get_transitive_callers_respects_max_depth() {
        let (_dir, db_path) = setup_test_graph();
        let graph = SqlSymbolGraph::new(&db_path).unwrap();
        let index = Index::open(&db_path).unwrap();

        let db_query = index
            .get_symbol_by_qualified_name("db::query")
            .unwrap()
            .unwrap();
        let impact = graph
            .get_transitive_callers(SymbolId::from(db_query.id), Some(1))
            .unwrap();

        // With max_depth=1, should only get direct callers
        assert_eq!(impact.direct_callers.len(), 2);
        assert!(
            impact.transitive_callers.is_empty(),
            "should have no transitive with depth=1"
        );
        assert_eq!(impact.max_depth_reached, 1);
    }

    #[test]
    fn get_transitive_callers_handles_no_callers() {
        let (_dir, db_path) = setup_test_graph();
        let graph = SqlSymbolGraph::new(&db_path).unwrap();
        let index = Index::open(&db_path).unwrap();

        let main_run = index
            .get_symbol_by_qualified_name("main::run")
            .unwrap()
            .unwrap();
        let impact = graph
            .get_transitive_callers(SymbolId::from(main_run.id), None)
            .unwrap();

        assert_eq!(impact.total_caller_count(), 0);
        assert!(impact.direct_callers.is_empty());
        assert!(impact.transitive_callers.is_empty());
    }

    #[test]
    fn find_call_path_returns_shortest_path() {
        let (_dir, db_path) = setup_test_graph();
        let graph = SqlSymbolGraph::new(&db_path).unwrap();
        let index = Index::open(&db_path).unwrap();

        let main_run = index
            .get_symbol_by_qualified_name("main::run")
            .unwrap()
            .unwrap();
        let db_query = index
            .get_symbol_by_qualified_name("db::query")
            .unwrap()
            .unwrap();

        let path = graph
            .find_call_path(SymbolId::from(main_run.id), SymbolId::from(db_query.id))
            .unwrap();

        assert!(
            path.is_some(),
            "should find path from main::run to db::query"
        );
        let path = path.unwrap();

        // Path should be: main::run -> (auth::validate OR cache::get) -> db::query
        assert_eq!(path.symbols().len(), 3, "path should have 3 symbols");
        assert_eq!(path.symbols()[0].qualified_name, "main::run");
        assert_eq!(path.symbols()[2].qualified_name, "db::query");
    }

    #[test]
    fn find_call_path_returns_none_for_unconnected() {
        let (_dir, db_path) = setup_test_graph();
        let graph = SqlSymbolGraph::new(&db_path).unwrap();
        let index = Index::open(&db_path).unwrap();

        // db::query doesn't call main::run (reverse direction)
        let db_query = index
            .get_symbol_by_qualified_name("db::query")
            .unwrap()
            .unwrap();
        let main_run = index
            .get_symbol_by_qualified_name("main::run")
            .unwrap()
            .unwrap();

        let path = graph
            .find_call_path(SymbolId::from(db_query.id), SymbolId::from(main_run.id))
            .unwrap();

        assert!(path.is_none(), "should not find path in reverse direction");
    }

    #[test]
    fn find_call_path_same_symbol_returns_single_node() {
        let (_dir, db_path) = setup_test_graph();
        let graph = SqlSymbolGraph::new(&db_path).unwrap();
        let index = Index::open(&db_path).unwrap();

        let main_run = index
            .get_symbol_by_qualified_name("main::run")
            .unwrap()
            .unwrap();

        let path = graph
            .find_call_path(SymbolId::from(main_run.id), SymbolId::from(main_run.id))
            .unwrap();

        assert!(path.is_some());
        let path = path.unwrap();
        assert_eq!(path.symbols().len(), 1);
        assert_eq!(path.symbols()[0].qualified_name, "main::run");
    }

    // === FileGraphOps Tests ===

    /// Create a test database with a known file dependency graph:
    ///
    /// ```text
    /// main.rs -> auth.rs -> db.rs
    ///         -> cache.rs -> db.rs
    /// ```
    fn setup_file_deps_graph() -> (TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let mut index = Index::open(&db_path).unwrap();

        // Create files: main.rs -> auth.rs -> db.rs
        //                       -> cache.rs -> db.rs
        let main_id = index
            .index_file_atomic(
                std::path::Path::new("src/main.rs"),
                Language::Rust,
                1000,
                100,
                None,
                &[],
            )
            .unwrap();
        let auth_id = index
            .index_file_atomic(
                std::path::Path::new("src/auth.rs"),
                Language::Rust,
                1000,
                100,
                None,
                &[],
            )
            .unwrap();
        let cache_id = index
            .index_file_atomic(
                std::path::Path::new("src/cache.rs"),
                Language::Rust,
                1000,
                100,
                None,
                &[],
            )
            .unwrap();
        let db_id = index
            .index_file_atomic(
                std::path::Path::new("src/db.rs"),
                Language::Rust,
                1000,
                100,
                None,
                &[],
            )
            .unwrap();

        // Set up dependencies (from_file depends on to_file)
        index.insert_file_dependency(main_id, auth_id).unwrap();
        index.insert_file_dependency(main_id, cache_id).unwrap();
        index.insert_file_dependency(auth_id, db_id).unwrap();
        index.insert_file_dependency(cache_id, db_id).unwrap();

        (dir, db_path)
    }

    #[test]
    fn file_graph_get_dependents_returns_direct() {
        let (_dir, db_path) = setup_file_deps_graph();
        let graph = SqlFileGraph::new(&db_path).unwrap();
        let index = Index::open(&db_path).unwrap();

        let db_id = index
            .get_file_id(std::path::Path::new("src/db.rs"))
            .unwrap()
            .unwrap();
        let dependents = graph.get_dependents(FileId::from(db_id)).unwrap();

        // db.rs is depended on by auth.rs and cache.rs
        assert_eq!(dependents.len(), 2);
        let paths: Vec<_> = dependents
            .iter()
            .map(|d| d.file.path.to_string_lossy().to_string())
            .collect();
        assert!(paths.iter().any(|p| p.contains("auth")));
        assert!(paths.iter().any(|p| p.contains("cache")));
    }

    #[test]
    fn file_graph_get_dependencies_returns_direct() {
        let (_dir, db_path) = setup_file_deps_graph();
        let graph = SqlFileGraph::new(&db_path).unwrap();
        let index = Index::open(&db_path).unwrap();

        let main_id = index
            .get_file_id(std::path::Path::new("src/main.rs"))
            .unwrap()
            .unwrap();
        let dependencies = graph.get_dependencies(FileId::from(main_id)).unwrap();

        // main.rs depends on auth.rs and cache.rs
        assert_eq!(dependencies.len(), 2);
        let paths: Vec<_> = dependencies
            .iter()
            .map(|d| d.file.path.to_string_lossy().to_string())
            .collect();
        assert!(paths.iter().any(|p| p.contains("auth")));
        assert!(paths.iter().any(|p| p.contains("cache")));
    }

    #[test]
    fn file_graph_get_transitive_dependents() {
        let (_dir, db_path) = setup_file_deps_graph();
        let graph = SqlFileGraph::new(&db_path).unwrap();
        let index = Index::open(&db_path).unwrap();

        let db_id = index
            .get_file_id(std::path::Path::new("src/db.rs"))
            .unwrap()
            .unwrap();
        let impact = graph
            .get_transitive_dependents(FileId::from(db_id), None)
            .unwrap();

        // db.rs: direct deps = auth.rs, cache.rs; transitive = main.rs
        assert_eq!(impact.direct_dependents.len(), 2);
        assert_eq!(impact.transitive_dependents.len(), 1);
        assert_eq!(impact.total_dependent_count(), 3);
    }

    #[test]
    fn file_graph_get_transitive_dependents_respects_max_depth() {
        let (_dir, db_path) = setup_file_deps_graph();
        let graph = SqlFileGraph::new(&db_path).unwrap();
        let index = Index::open(&db_path).unwrap();

        let db_id = index
            .get_file_id(std::path::Path::new("src/db.rs"))
            .unwrap()
            .unwrap();
        let impact = graph
            .get_transitive_dependents(FileId::from(db_id), Some(1))
            .unwrap();

        // With max_depth=1, should only get direct dependents
        assert_eq!(impact.direct_dependents.len(), 2);
        assert!(
            impact.transitive_dependents.is_empty(),
            "should have no transitive with depth=1"
        );
    }

    #[test]
    fn file_graph_find_dependency_path_returns_shortest() {
        let (_dir, db_path) = setup_file_deps_graph();
        let graph = SqlFileGraph::new(&db_path).unwrap();
        let index = Index::open(&db_path).unwrap();

        let main_id = index
            .get_file_id(std::path::Path::new("src/main.rs"))
            .unwrap()
            .unwrap();
        let db_id = index
            .get_file_id(std::path::Path::new("src/db.rs"))
            .unwrap()
            .unwrap();

        let path = graph
            .find_dependency_path(FileId::from(main_id), FileId::from(db_id))
            .unwrap();

        assert!(path.is_some(), "should find path from main.rs to db.rs");
        let path = path.unwrap();

        // Path should be: main.rs -> (auth.rs OR cache.rs) -> db.rs
        assert_eq!(path.files().len(), 3, "path should have 3 files");
        assert!(path.files()[0].path.to_string_lossy().contains("main"));
        assert!(path.files()[2].path.to_string_lossy().contains("db"));
    }

    #[test]
    fn file_graph_find_dependency_path_returns_none_for_unconnected() {
        let (_dir, db_path) = setup_file_deps_graph();
        let graph = SqlFileGraph::new(&db_path).unwrap();
        let index = Index::open(&db_path).unwrap();

        // db.rs doesn't depend on main.rs (reverse direction)
        let db_id = index
            .get_file_id(std::path::Path::new("src/db.rs"))
            .unwrap()
            .unwrap();
        let main_id = index
            .get_file_id(std::path::Path::new("src/main.rs"))
            .unwrap()
            .unwrap();

        let path = graph
            .find_dependency_path(FileId::from(db_id), FileId::from(main_id))
            .unwrap();

        assert!(path.is_none(), "should not find path in reverse direction");
    }

    #[test]
    fn file_graph_find_dependency_path_same_file_returns_single_node() {
        let (_dir, db_path) = setup_file_deps_graph();
        let graph = SqlFileGraph::new(&db_path).unwrap();
        let index = Index::open(&db_path).unwrap();

        let main_id = index
            .get_file_id(std::path::Path::new("src/main.rs"))
            .unwrap()
            .unwrap();

        let path = graph
            .find_dependency_path(FileId::from(main_id), FileId::from(main_id))
            .unwrap();

        assert!(path.is_some());
        let path = path.unwrap();
        assert_eq!(path.files().len(), 1);
        assert!(path.files()[0].path.to_string_lossy().contains("main"));
    }

    #[test]
    fn file_graph_detect_cycles_returns_empty_for_acyclic() {
        let (_dir, db_path) = setup_file_deps_graph();
        let graph = SqlFileGraph::new(&db_path).unwrap();

        let cycles = graph.detect_cycles().unwrap();
        assert!(cycles.is_empty(), "acyclic graph should have no cycles");
    }

    #[test]
    fn file_graph_detect_cycles_involving_returns_empty_for_acyclic() {
        let (_dir, db_path) = setup_file_deps_graph();
        let graph = SqlFileGraph::new(&db_path).unwrap();
        let index = Index::open(&db_path).unwrap();

        let main_id = index
            .get_file_id(std::path::Path::new("src/main.rs"))
            .unwrap()
            .unwrap();

        let cycles = graph
            .detect_cycles_involving(FileId::from(main_id))
            .unwrap();
        assert!(
            cycles.is_empty(),
            "acyclic graph should have no cycles involving main.rs"
        );
    }
}
