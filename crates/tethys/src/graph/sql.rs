//! SQL-based implementations of graph operations.
//!
//! Uses recursive CTEs for graph traversal, keeping all data in `SQLite`.

// SQLite uses i64 for all integer storage. These casts are intentional and safe for
// practical values (reference counts within reasonable bounds).
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{Connection, OptionalExtension};

use super::{CallPath, CalleeInfo, CallerInfo, SymbolGraphOps, SymbolImpact};
use crate::error::Result;
use crate::types::{ReferenceKind, Span, Symbol, SymbolKind, Visibility};

/// SQL-based implementation of symbol graph operations.
///
/// Wraps a `Connection` in a `Mutex` to satisfy the `Send + Sync` bounds
/// required by the `SymbolGraphOps` trait.
pub struct SqlSymbolGraph {
    conn: Mutex<Connection>,
}

impl SqlSymbolGraph {
    /// Create a new SQL symbol graph connected to the given database.
    pub fn new(db_path: &Path) -> Result<Self> {
        let conn = Connection::open(db_path)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Get a symbol by its database ID.
    fn get_symbol_by_id(&self, id: i64) -> Result<Option<Symbol>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| crate::error::Error::Internal(format!("mutex poisoned: {e}")))?;

        let mut stmt = conn.prepare(
            "SELECT id, file_id, name, module_path, qualified_name, kind, line, column,
             end_line, end_column, signature, visibility, parent_symbol_id
             FROM symbols WHERE id = ?1",
        )?;

        let symbol = stmt.query_row([id], row_to_symbol).optional()?;

        Ok(symbol)
    }
}

impl SymbolGraphOps for SqlSymbolGraph {
    fn get_callers(&self, symbol_id: i64) -> Result<Vec<CallerInfo>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| crate::error::Error::Internal(format!("mutex poisoned: {e}")))?;

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
            .query_map([symbol_id], |row| {
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

    fn get_callees(&self, symbol_id: i64) -> Result<Vec<CalleeInfo>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| crate::error::Error::Internal(format!("mutex poisoned: {e}")))?;

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
            .query_map([symbol_id], |row| {
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
        symbol_id: i64,
        max_depth: Option<u32>,
    ) -> Result<SymbolImpact> {
        let max_depth = max_depth.unwrap_or(50);

        // First, get the target symbol
        let target = self
            .get_symbol_by_id(symbol_id)?
            .ok_or_else(|| crate::error::Error::NotFound(format!("symbol id: {symbol_id}")))?;

        let conn = self
            .conn
            .lock()
            .map_err(|e| crate::error::Error::Internal(format!("mutex poisoned: {e}")))?;

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

        let rows = stmt.query_map(rusqlite::params![symbol_id, max_depth], |row| {
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

        let total_caller_count = direct_callers.len() + transitive_callers.len();

        Ok(SymbolImpact {
            target,
            direct_callers,
            transitive_callers,
            total_caller_count,
            max_depth_reached,
        })
    }

    fn find_call_path(&self, _from_symbol_id: i64, _to_symbol_id: i64) -> Result<Option<CallPath>> {
        todo!("Task 5: Implement find_call_path")
    }
}

/// Parse a symbol from a database row.
///
/// Expects 13 columns matching the symbols table schema.
fn row_to_symbol(row: &rusqlite::Row) -> rusqlite::Result<Symbol> {
    let line: u32 = row.get(6)?;
    let column: u32 = row.get(7)?;
    let end_line: Option<u32> = row.get(8)?;
    let end_column: Option<u32> = row.get(9)?;

    let span = end_line.zip(end_column).map(|(el, ec)| Span {
        start_line: line,
        start_column: column,
        end_line: el,
        end_column: ec,
    });

    Ok(Symbol {
        id: row.get(0)?,
        file_id: row.get(1)?,
        name: row.get(2)?,
        module_path: row.get(3)?,
        qualified_name: row.get(4)?,
        kind: parse_symbol_kind(&row.get::<_, String>(5)?)?,
        line,
        column,
        span,
        signature: row.get(10)?,
        signature_details: None,
        visibility: parse_visibility(&row.get::<_, String>(11)?)?,
        parent_symbol_id: row.get(12)?,
    })
}

fn parse_symbol_kind(s: &str) -> rusqlite::Result<SymbolKind> {
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
            format!("Unknown symbol kind: {unknown}").into(),
        )),
    }
}

fn parse_visibility(s: &str) -> rusqlite::Result<Visibility> {
    match s {
        "public" => Ok(Visibility::Public),
        "crate" => Ok(Visibility::Crate),
        "module" => Ok(Visibility::Module),
        "private" => Ok(Visibility::Private),
        unknown => Err(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            format!("Unknown visibility: {unknown}").into(),
        )),
    }
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
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Index;
    use crate::types::{Language, SymbolKind, Visibility};
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
        let callers = graph.get_callers(db_query.id).unwrap();

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
        let callers = graph.get_callers(main_run.id).unwrap();

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
        let callees = graph.get_callees(main_run.id).unwrap();

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
        let callees = graph.get_callees(db_query.id).unwrap();

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
        let callers = graph.get_callers(db_query.id).unwrap();

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
        let callers = graph.get_callers(db_query.id).unwrap();

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
        let impact = graph.get_transitive_callers(db_query.id, None).unwrap();

        // db::query's transitive callers: auth::validate, cache::get (direct), main::run (transitive)
        assert_eq!(impact.total_caller_count, 3, "expected 3 total callers");
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
        let impact = graph.get_transitive_callers(db_query.id, Some(1)).unwrap();

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
        let impact = graph.get_transitive_callers(main_run.id, None).unwrap();

        assert_eq!(impact.total_caller_count, 0);
        assert!(impact.direct_callers.is_empty());
        assert!(impact.transitive_callers.is_empty());
    }
}
