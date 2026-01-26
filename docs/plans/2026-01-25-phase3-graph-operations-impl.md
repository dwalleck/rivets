# Phase 3: Graph Operations - Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add symbol-level and file-level graph operations to Tethys for impact analysis and path finding.

**Architecture:** Trait-based abstraction (`SymbolGraphOps`, `FileGraphOps`) with SQL-driven implementations using recursive CTEs. Symbol-level operations prioritized for function impact analysis.

**Tech Stack:** Rust, SQLite recursive CTEs, rusqlite, existing Tethys infrastructure

---

## Task 1: Create Graph Module Structure

**Files:**
- Create: `crates/tethys/src/graph/mod.rs`
- Create: `crates/tethys/src/graph/types.rs`
- Modify: `crates/tethys/src/lib.rs:36` (add mod declaration)

**Step 1: Create the graph directory**

Run: `mkdir -p crates/tethys/src/graph`

**Step 2: Create graph/types.rs with core types**

```rust
//! Types for graph operations.

use crate::types::{IndexedFile, ReferenceKind, Symbol};

/// Information about a caller of a symbol.
#[derive(Debug, Clone)]
pub struct CallerInfo {
    /// The symbol that calls the target.
    pub symbol: Symbol,
    /// How many times it references the target.
    pub reference_count: usize,
    /// The kinds of references (Call, Type, Construct, etc.).
    pub reference_kinds: Vec<ReferenceKind>,
}

/// Information about a callee of a symbol.
#[derive(Debug, Clone)]
pub struct CalleeInfo {
    /// The symbol being called/referenced.
    pub symbol: Symbol,
    /// How many times it is referenced.
    pub reference_count: usize,
    /// The kinds of references.
    pub reference_kinds: Vec<ReferenceKind>,
}

/// Result of transitive caller analysis (symbol-level impact).
#[derive(Debug, Clone)]
pub struct SymbolImpact {
    /// The target symbol being analyzed.
    pub target: Symbol,
    /// Symbols that directly call/reference the target.
    pub direct_callers: Vec<CallerInfo>,
    /// Symbols that transitively call the target (excludes direct).
    pub transitive_callers: Vec<CallerInfo>,
    /// Total number of unique callers.
    pub total_caller_count: usize,
    /// Maximum depth reached during traversal.
    pub max_depth_reached: u32,
}

/// A path through the call graph.
#[derive(Debug, Clone)]
pub struct CallPath {
    /// Symbols from source to target.
    pub symbols: Vec<Symbol>,
    /// The relationship at each step.
    pub edges: Vec<ReferenceKind>,
}

/// Information about a file dependency.
#[derive(Debug, Clone)]
pub struct FileDepInfo {
    /// The dependent/dependency file.
    pub file: IndexedFile,
    /// Number of references between the files.
    pub ref_count: usize,
}

/// Result of file-level impact analysis.
#[derive(Debug, Clone)]
pub struct FileImpact {
    /// The target file being analyzed.
    pub target: IndexedFile,
    /// Files that directly depend on the target.
    pub direct_dependents: Vec<FileDepInfo>,
    /// Files that transitively depend on the target.
    pub transitive_dependents: Vec<FileDepInfo>,
    /// Total number of dependent files.
    pub total_dependent_count: usize,
}

/// A path through the file dependency graph.
#[derive(Debug, Clone)]
pub struct FilePath {
    /// Files from source to target.
    pub files: Vec<IndexedFile>,
}
```

**Step 3: Create graph/mod.rs with trait definitions**

```rust
//! Graph operations for dependency analysis.
//!
//! This module provides traits and implementations for:
//! - Symbol-level queries (who calls this function?)
//! - File-level queries (what files depend on this?)
//! - Impact analysis (transitive dependents)
//! - Path finding (how does A relate to B?)
//!
//! ## Design
//!
//! - Traits define the operations (`SymbolGraphOps`, `FileGraphOps`)
//! - SQL implementations use recursive CTEs for traversal
//! - Petgraph can be swapped in later for specific algorithms

mod types;

pub use types::{
    CallPath, CalleeInfo, CallerInfo, FileDepInfo, FileImpact, FilePath, SymbolImpact,
};

use crate::error::Result;
use crate::types::Cycle;

/// Operations on the symbol-level dependency graph.
///
/// Symbol graphs track "who calls what" at function/method granularity.
/// This enables precise impact analysis and execution flow understanding.
pub trait SymbolGraphOps: Send + Sync {
    /// Get symbols that directly call/reference the given symbol.
    fn get_callers(&self, symbol_id: i64) -> Result<Vec<CallerInfo>>;

    /// Get symbols that the given symbol directly calls/references.
    fn get_callees(&self, symbol_id: i64) -> Result<Vec<CalleeInfo>>;

    /// Get transitive callers (impact analysis).
    ///
    /// Returns all symbols that directly or indirectly call the target.
    fn get_transitive_callers(
        &self,
        symbol_id: i64,
        max_depth: Option<u32>,
    ) -> Result<SymbolImpact>;

    /// Find the shortest call path between two symbols.
    ///
    /// Returns `None` if no path exists.
    fn find_call_path(
        &self,
        from_symbol_id: i64,
        to_symbol_id: i64,
    ) -> Result<Option<CallPath>>;
}

/// Operations on the file-level dependency graph.
///
/// File graphs are coarser than symbol graphs but faster to traverse.
pub trait FileGraphOps: Send + Sync {
    /// Get files that directly depend on the given file.
    fn get_dependents(&self, file_id: i64) -> Result<Vec<FileDepInfo>>;

    /// Get files that the given file directly depends on.
    fn get_dependencies(&self, file_id: i64) -> Result<Vec<FileDepInfo>>;

    /// Get transitive dependents (file-level impact analysis).
    fn get_transitive_dependents(
        &self,
        file_id: i64,
        max_depth: Option<u32>,
    ) -> Result<FileImpact>;

    /// Find the shortest dependency path between two files.
    fn find_dependency_path(
        &self,
        from_file_id: i64,
        to_file_id: i64,
    ) -> Result<Option<FilePath>>;

    /// Detect circular dependencies in the codebase.
    fn detect_cycles(&self) -> Result<Vec<Cycle>>;

    /// Detect cycles involving a specific file.
    fn detect_cycles_involving(&self, file_id: i64) -> Result<Vec<Cycle>>;
}
```

**Step 4: Add mod declaration to lib.rs**

In `crates/tethys/src/lib.rs`, after line 42 (`mod types;`), add:

```rust
mod graph;
```

**Step 5: Verify compilation**

Run: `cargo check -p tethys`
Expected: Compiles with no errors (warnings about unused are OK)

**Step 6: Commit**

```bash
git add crates/tethys/src/graph/
git add crates/tethys/src/lib.rs
git commit -m "feat(tethys): add graph module with trait definitions

Add SymbolGraphOps and FileGraphOps traits for Phase 3 graph operations.
Types defined: CallerInfo, CalleeInfo, SymbolImpact, FileImpact, CallPath, FilePath.

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>"
```

---

## Task 2: Add NotFound Error Variant

**Files:**
- Modify: `crates/tethys/src/error.rs:32-48`
- Modify: `crates/tethys/src/lib.rs:44` (add to pub use)

**Step 1: Add NotFound variant to Error enum**

In `crates/tethys/src/error.rs`, add a new variant to the `Error` enum after `Config`:

```rust
    /// Requested resource was not found
    #[error("not found: {0}")]
    NotFound(String),
```

**Step 2: Export Error in lib.rs if not already**

Verify `Error` is already exported in `crates/tethys/src/lib.rs:44`. It should be:
```rust
pub use error::{Error, IndexError, IndexErrorKind, Result};
```

**Step 3: Verify compilation**

Run: `cargo check -p tethys`
Expected: Compiles with no errors

**Step 4: Commit**

```bash
git add crates/tethys/src/error.rs
git commit -m "feat(tethys): add NotFound error variant

For graph operations that require looking up symbols/files by ID or name.

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>"
```

---

## Task 3: Create SqlSymbolGraph - Direct Callers

**Files:**
- Create: `crates/tethys/src/graph/sql.rs`
- Modify: `crates/tethys/src/graph/mod.rs` (add mod sql)

**Step 1: Write the failing test**

Add to the end of `crates/tethys/src/graph/sql.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Index;
    use crate::types::{Language, SymbolKind, Visibility};
    use tempfile::TempDir;

    /// Create a test database with a known call graph:
    ///
    /// ```text
    ///   main::run ──► auth::validate ──► db::query
    ///              └► cache::get ────────┘
    /// ```
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

        let db_file = index
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
        let main_run = index.get_symbol_by_qualified_name("main::run").unwrap().unwrap();
        let auth_validate = index.get_symbol_by_qualified_name("auth::validate").unwrap().unwrap();
        let db_query = index.get_symbol_by_qualified_name("db::query").unwrap().unwrap();
        let cache_get = index.get_symbol_by_qualified_name("cache::get").unwrap().unwrap();

        // Create references: main::run -> auth::validate
        index.insert_reference(auth_validate.id, main_file, "call", 5, 1, Some(main_run.id)).unwrap();
        // main::run -> cache::get
        index.insert_reference(cache_get.id, main_file, "call", 6, 1, Some(main_run.id)).unwrap();
        // auth::validate -> db::query
        index.insert_reference(db_query.id, auth_file, "call", 3, 1, Some(auth_validate.id)).unwrap();
        // cache::get -> db::query
        index.insert_reference(db_query.id, cache_file, "call", 3, 1, Some(cache_get.id)).unwrap();

        (dir, db_path)
    }

    #[test]
    fn get_callers_returns_direct_callers() {
        let (_dir, db_path) = setup_test_graph();
        let graph = SqlSymbolGraph::new(&db_path).unwrap();
        let index = Index::open(&db_path).unwrap();

        let db_query = index.get_symbol_by_qualified_name("db::query").unwrap().unwrap();
        let callers = graph.get_callers(db_query.id).unwrap();

        // db::query is called by auth::validate and cache::get
        assert_eq!(callers.len(), 2, "expected 2 callers, got: {callers:?}");

        let caller_names: Vec<&str> = callers.iter().map(|c| c.symbol.name.as_str()).collect();
        assert!(caller_names.contains(&"validate"), "should include auth::validate");
        assert!(caller_names.contains(&"get"), "should include cache::get");
    }

    #[test]
    fn get_callers_returns_empty_for_uncalled_symbol() {
        let (_dir, db_path) = setup_test_graph();
        let graph = SqlSymbolGraph::new(&db_path).unwrap();
        let index = Index::open(&db_path).unwrap();

        let main_run = index.get_symbol_by_qualified_name("main::run").unwrap().unwrap();
        let callers = graph.get_callers(main_run.id).unwrap();

        // main::run is not called by anything
        assert!(callers.is_empty(), "main::run should have no callers");
    }
}
```

**Step 2: Create sql.rs with SqlSymbolGraph struct and get_callers**

Create `crates/tethys/src/graph/sql.rs`:

```rust
//! SQL-based implementations of graph operations.
//!
//! Uses recursive CTEs for graph traversal, keeping all data in SQLite.

use std::collections::HashMap;
use std::path::Path;

use rusqlite::{params, Connection};

use super::{CallPath, CalleeInfo, CallerInfo, FileDepInfo, FileGraphOps, FileImpact, FilePath, SymbolGraphOps, SymbolImpact};
use crate::error::{Error, Result};
use crate::types::{Cycle, IndexedFile, Language, ReferenceKind, Span, Symbol, SymbolKind, Visibility};

/// SQL-based implementation of symbol graph operations.
pub struct SqlSymbolGraph {
    conn: Connection,
}

impl SqlSymbolGraph {
    /// Create a new SQL symbol graph connected to the given database.
    pub fn new(db_path: &Path) -> Result<Self> {
        let conn = Connection::open(db_path)?;
        Ok(Self { conn })
    }
}

impl SymbolGraphOps for SqlSymbolGraph {
    fn get_callers(&self, symbol_id: i64) -> Result<Vec<CallerInfo>> {
        // Find all symbols that contain references to the target symbol
        let mut stmt = self.conn.prepare(
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
             ORDER BY s.qualified_name"
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
        // Find all symbols that the given symbol references
        let mut stmt = self.conn.prepare(
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
             ORDER BY s.qualified_name"
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
        todo!("Task 4: Implement get_transitive_callers")
    }

    fn find_call_path(
        &self,
        from_symbol_id: i64,
        to_symbol_id: i64,
    ) -> Result<Option<CallPath>> {
        todo!("Task 5: Implement find_call_path")
    }
}

/// Parse a symbol from a database row.
/// Expected columns: id, file_id, name, module_path, qualified_name, kind, line, column,
///                   end_line, end_column, signature, visibility, parent_symbol_id
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

// Tests at the bottom of file (added in Step 1)
```

**Step 3: Add mod sql to graph/mod.rs**

Add after the `pub use` statements in `crates/tethys/src/graph/mod.rs`:

```rust
mod sql;

pub use sql::{SqlSymbolGraph, SqlFileGraph};
```

Note: `SqlFileGraph` doesn't exist yet, so this will cause a compile error until Task 6.
For now, only add:

```rust
mod sql;

pub use sql::SqlSymbolGraph;
```

**Step 4: Run the tests**

Run: `cargo test -p tethys graph::sql::tests --no-fail-fast`
Expected: Tests pass

**Step 5: Commit**

```bash
git add crates/tethys/src/graph/
git commit -m "feat(tethys): implement SqlSymbolGraph with get_callers/get_callees

Direct caller/callee queries using SQL joins on refs table.
Includes comprehensive test setup with known call graph.

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>"
```

---

## Task 4: SqlSymbolGraph - Transitive Callers

**Files:**
- Modify: `crates/tethys/src/graph/sql.rs`

**Step 1: Write the failing test**

Add to the tests module in `crates/tethys/src/graph/sql.rs`:

```rust
    #[test]
    fn get_transitive_callers_finds_all_ancestors() {
        let (_dir, db_path) = setup_test_graph();
        let graph = SqlSymbolGraph::new(&db_path).unwrap();
        let index = Index::open(&db_path).unwrap();

        let db_query = index.get_symbol_by_qualified_name("db::query").unwrap().unwrap();
        let impact = graph.get_transitive_callers(db_query.id, None).unwrap();

        // db::query's transitive callers: auth::validate, cache::get (direct), main::run (transitive)
        assert_eq!(impact.total_caller_count, 3, "expected 3 total callers");
        assert_eq!(impact.direct_callers.len(), 2, "expected 2 direct callers");
        assert_eq!(impact.transitive_callers.len(), 1, "expected 1 transitive caller");

        let transitive_names: Vec<&str> = impact.transitive_callers.iter().map(|c| c.symbol.name.as_str()).collect();
        assert!(transitive_names.contains(&"run"), "main::run should be transitive caller");
    }

    #[test]
    fn get_transitive_callers_respects_max_depth() {
        let (_dir, db_path) = setup_test_graph();
        let graph = SqlSymbolGraph::new(&db_path).unwrap();
        let index = Index::open(&db_path).unwrap();

        let db_query = index.get_symbol_by_qualified_name("db::query").unwrap().unwrap();
        let impact = graph.get_transitive_callers(db_query.id, Some(1)).unwrap();

        // With max_depth=1, should only get direct callers
        assert_eq!(impact.direct_callers.len(), 2);
        assert!(impact.transitive_callers.is_empty(), "should have no transitive with depth=1");
        assert_eq!(impact.max_depth_reached, 1);
    }

    #[test]
    fn get_transitive_callers_handles_no_callers() {
        let (_dir, db_path) = setup_test_graph();
        let graph = SqlSymbolGraph::new(&db_path).unwrap();
        let index = Index::open(&db_path).unwrap();

        let main_run = index.get_symbol_by_qualified_name("main::run").unwrap().unwrap();
        let impact = graph.get_transitive_callers(main_run.id, None).unwrap();

        assert_eq!(impact.total_caller_count, 0);
        assert!(impact.direct_callers.is_empty());
        assert!(impact.transitive_callers.is_empty());
    }
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p tethys get_transitive_callers --no-fail-fast`
Expected: FAIL with "not yet implemented"

**Step 3: Implement get_transitive_callers**

Replace the `todo!()` in `get_transitive_callers` with:

```rust
    fn get_transitive_callers(
        &self,
        symbol_id: i64,
        max_depth: Option<u32>,
    ) -> Result<SymbolImpact> {
        let max_depth = max_depth.unwrap_or(50);

        // First, get the target symbol
        let target = self.get_symbol_by_id(symbol_id)?
            .ok_or_else(|| Error::NotFound(format!("symbol id: {}", symbol_id)))?;

        // Use recursive CTE to find all callers with their depth
        let mut stmt = self.conn.prepare(
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
            ORDER BY min_depth, s.qualified_name"
        )?;

        let mut direct_callers = Vec::new();
        let mut transitive_callers = Vec::new();
        let mut max_depth_reached: u32 = 0;

        let rows = stmt.query_map(params![symbol_id, max_depth], |row| {
            let symbol = row_to_symbol(row)?;
            let depth: u32 = row.get::<_, i64>(13)? as u32;
            Ok((symbol, depth))
        })?;

        for row in rows {
            let (symbol, depth) = row?;
            max_depth_reached = max_depth_reached.max(depth);

            let caller_info = CallerInfo {
                symbol,
                reference_count: 1, // Simplified; could query for exact count
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
```

**Step 4: Add helper method to get symbol by ID**

Add this method to `impl SqlSymbolGraph` (before the `SymbolGraphOps` impl):

```rust
    /// Get a symbol by its database ID.
    fn get_symbol_by_id(&self, id: i64) -> Result<Option<Symbol>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, file_id, name, module_path, qualified_name, kind, line, column,
             end_line, end_column, signature, visibility, parent_symbol_id
             FROM symbols WHERE id = ?1"
        )?;

        let symbol = stmt
            .query_row([id], row_to_symbol)
            .optional()?;

        Ok(symbol)
    }
```

Also add at the top of sql.rs:

```rust
use rusqlite::OptionalExtension;
```

**Step 5: Run tests**

Run: `cargo test -p tethys get_transitive_callers --no-fail-fast`
Expected: All tests pass

**Step 6: Commit**

```bash
git add crates/tethys/src/graph/sql.rs
git commit -m "feat(tethys): implement transitive caller analysis with recursive CTE

Uses WITH RECURSIVE to find all direct and transitive callers.
Supports max_depth limiting to prevent runaway traversal.

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>"
```

---

## Task 5: SqlSymbolGraph - Path Finding

**Files:**
- Modify: `crates/tethys/src/graph/sql.rs`

**Step 1: Write the failing tests**

Add to tests module:

```rust
    #[test]
    fn find_call_path_returns_shortest_path() {
        let (_dir, db_path) = setup_test_graph();
        let graph = SqlSymbolGraph::new(&db_path).unwrap();
        let index = Index::open(&db_path).unwrap();

        let main_run = index.get_symbol_by_qualified_name("main::run").unwrap().unwrap();
        let db_query = index.get_symbol_by_qualified_name("db::query").unwrap().unwrap();

        let path = graph.find_call_path(main_run.id, db_query.id).unwrap();

        assert!(path.is_some(), "should find path from main::run to db::query");
        let path = path.unwrap();

        // Path should be: main::run -> (auth::validate OR cache::get) -> db::query
        assert_eq!(path.symbols.len(), 3, "path should have 3 symbols");
        assert_eq!(path.symbols[0].qualified_name, "main::run");
        assert_eq!(path.symbols[2].qualified_name, "db::query");
    }

    #[test]
    fn find_call_path_returns_none_for_unconnected() {
        let (_dir, db_path) = setup_test_graph();
        let graph = SqlSymbolGraph::new(&db_path).unwrap();
        let index = Index::open(&db_path).unwrap();

        // db::query doesn't call main::run (reverse direction)
        let db_query = index.get_symbol_by_qualified_name("db::query").unwrap().unwrap();
        let main_run = index.get_symbol_by_qualified_name("main::run").unwrap().unwrap();

        let path = graph.find_call_path(db_query.id, main_run.id).unwrap();

        assert!(path.is_none(), "should not find path in reverse direction");
    }

    #[test]
    fn find_call_path_same_symbol_returns_single_node() {
        let (_dir, db_path) = setup_test_graph();
        let graph = SqlSymbolGraph::new(&db_path).unwrap();
        let index = Index::open(&db_path).unwrap();

        let main_run = index.get_symbol_by_qualified_name("main::run").unwrap().unwrap();

        let path = graph.find_call_path(main_run.id, main_run.id).unwrap();

        assert!(path.is_some());
        let path = path.unwrap();
        assert_eq!(path.symbols.len(), 1);
        assert_eq!(path.symbols[0].qualified_name, "main::run");
    }
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p tethys find_call_path --no-fail-fast`
Expected: FAIL with "not yet implemented"

**Step 3: Implement find_call_path**

Replace the `todo!()` in `find_call_path` with:

```rust
    fn find_call_path(
        &self,
        from_symbol_id: i64,
        to_symbol_id: i64,
    ) -> Result<Option<CallPath>> {
        // Same symbol - trivial path
        if from_symbol_id == to_symbol_id {
            let symbol = self.get_symbol_by_id(from_symbol_id)?
                .ok_or_else(|| Error::NotFound(format!("symbol id: {}", from_symbol_id)))?;
            return Ok(Some(CallPath {
                symbols: vec![symbol],
                edges: vec![],
            }));
        }

        // BFS to find shortest path
        // We search forward from `from` through callees (what does `from` call?)
        let max_depth = 50;

        let mut stmt = self.conn.prepare(
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
            LIMIT 1"
        )?;

        let path_str: Option<String> = stmt
            .query_row(params![from_symbol_id, to_symbol_id, max_depth], |row| row.get(0))
            .optional()?;

        let Some(path_str) = path_str else {
            return Ok(None);
        };

        // Parse path and fetch symbols
        let symbol_ids: Vec<i64> = path_str
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();

        let mut symbols = Vec::with_capacity(symbol_ids.len());
        for id in symbol_ids {
            let symbol = self.get_symbol_by_id(id)?
                .ok_or_else(|| Error::NotFound(format!("symbol id: {}", id)))?;
            symbols.push(symbol);
        }

        // Create edges (all Call for simplicity)
        let edges = vec![ReferenceKind::Call; symbols.len().saturating_sub(1)];

        Ok(Some(CallPath { symbols, edges }))
    }
```

**Step 4: Run tests**

Run: `cargo test -p tethys find_call_path --no-fail-fast`
Expected: All tests pass

**Step 5: Commit**

```bash
git add crates/tethys/src/graph/sql.rs
git commit -m "feat(tethys): implement call path finding with BFS

Finds shortest path between two symbols using recursive CTE.
Returns None if symbols are unconnected.

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>"
```

---

## Task 6: Create SqlFileGraph

**Files:**
- Modify: `crates/tethys/src/graph/sql.rs`
- Modify: `crates/tethys/src/graph/mod.rs`

**Step 1: Write the failing tests**

Add to tests module in sql.rs:

```rust
    // === FileGraphOps Tests ===

    fn setup_file_deps_graph() -> (TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let mut index = Index::open(&db_path).unwrap();

        // Create files: main.rs -> auth.rs -> db.rs
        //                       -> cache.rs -> db.rs
        let main_id = index.index_file_atomic(
            std::path::Path::new("src/main.rs"), Language::Rust, 1000, 100, None, &[]
        ).unwrap();
        let auth_id = index.index_file_atomic(
            std::path::Path::new("src/auth.rs"), Language::Rust, 1000, 100, None, &[]
        ).unwrap();
        let cache_id = index.index_file_atomic(
            std::path::Path::new("src/cache.rs"), Language::Rust, 1000, 100, None, &[]
        ).unwrap();
        let db_id = index.index_file_atomic(
            std::path::Path::new("src/db.rs"), Language::Rust, 1000, 100, None, &[]
        ).unwrap();

        // Set up dependencies
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

        let db_id = index.get_file_id(std::path::Path::new("src/db.rs")).unwrap().unwrap();
        let dependents = graph.get_dependents(db_id).unwrap();

        // db.rs is depended on by auth.rs and cache.rs
        assert_eq!(dependents.len(), 2);
        let paths: Vec<_> = dependents.iter().map(|d| d.file.path.to_string_lossy().to_string()).collect();
        assert!(paths.iter().any(|p| p.contains("auth")));
        assert!(paths.iter().any(|p| p.contains("cache")));
    }

    #[test]
    fn file_graph_get_transitive_dependents() {
        let (_dir, db_path) = setup_file_deps_graph();
        let graph = SqlFileGraph::new(&db_path).unwrap();
        let index = Index::open(&db_path).unwrap();

        let db_id = index.get_file_id(std::path::Path::new("src/db.rs")).unwrap().unwrap();
        let impact = graph.get_transitive_dependents(db_id, None).unwrap();

        // db.rs: direct deps = auth.rs, cache.rs; transitive = main.rs
        assert_eq!(impact.direct_dependents.len(), 2);
        assert_eq!(impact.transitive_dependents.len(), 1);
        assert_eq!(impact.total_dependent_count, 3);
    }
```

**Step 2: Implement SqlFileGraph**

Add to `crates/tethys/src/graph/sql.rs`:

```rust
/// SQL-based implementation of file graph operations.
pub struct SqlFileGraph {
    conn: Connection,
}

impl SqlFileGraph {
    /// Create a new SQL file graph connected to the given database.
    pub fn new(db_path: &Path) -> Result<Self> {
        let conn = Connection::open(db_path)?;
        Ok(Self { conn })
    }

    /// Get a file by its database ID.
    fn get_file_by_id(&self, id: i64) -> Result<Option<IndexedFile>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, path, language, mtime_ns, size_bytes, content_hash, indexed_at
             FROM files WHERE id = ?1"
        )?;

        let file = stmt
            .query_row([id], row_to_indexed_file)
            .optional()?;

        Ok(file)
    }
}

impl FileGraphOps for SqlFileGraph {
    fn get_dependents(&self, file_id: i64) -> Result<Vec<FileDepInfo>> {
        let mut stmt = self.conn.prepare(
            "SELECT f.id, f.path, f.language, f.mtime_ns, f.size_bytes, f.content_hash, f.indexed_at,
                    fd.ref_count
             FROM file_deps fd
             JOIN files f ON f.id = fd.from_file_id
             WHERE fd.to_file_id = ?1
             ORDER BY f.path"
        )?;

        let deps = stmt
            .query_map([file_id], |row| {
                let file = row_to_indexed_file(row)?;
                let ref_count: usize = row.get::<_, i64>(7)? as usize;
                Ok(FileDepInfo { file, ref_count })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(deps)
    }

    fn get_dependencies(&self, file_id: i64) -> Result<Vec<FileDepInfo>> {
        let mut stmt = self.conn.prepare(
            "SELECT f.id, f.path, f.language, f.mtime_ns, f.size_bytes, f.content_hash, f.indexed_at,
                    fd.ref_count
             FROM file_deps fd
             JOIN files f ON f.id = fd.to_file_id
             WHERE fd.from_file_id = ?1
             ORDER BY f.path"
        )?;

        let deps = stmt
            .query_map([file_id], |row| {
                let file = row_to_indexed_file(row)?;
                let ref_count: usize = row.get::<_, i64>(7)? as usize;
                Ok(FileDepInfo { file, ref_count })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(deps)
    }

    fn get_transitive_dependents(
        &self,
        file_id: i64,
        max_depth: Option<u32>,
    ) -> Result<FileImpact> {
        let max_depth = max_depth.unwrap_or(50);

        let target = self.get_file_by_id(file_id)?
            .ok_or_else(|| Error::NotFound(format!("file id: {}", file_id)))?;

        let mut stmt = self.conn.prepare(
            "WITH RECURSIVE dep_tree(file_id, depth) AS (
                SELECT DISTINCT fd.from_file_id, 1
                FROM file_deps fd
                WHERE fd.to_file_id = ?1

                UNION

                SELECT DISTINCT fd.from_file_id, dt.depth + 1
                FROM file_deps fd
                JOIN dep_tree dt ON fd.to_file_id = dt.file_id
                WHERE dt.depth < ?2
            )
            SELECT DISTINCT f.id, f.path, f.language, f.mtime_ns, f.size_bytes,
                   f.content_hash, f.indexed_at, MIN(dt.depth) as min_depth
            FROM dep_tree dt
            JOIN files f ON f.id = dt.file_id
            GROUP BY f.id
            ORDER BY min_depth, f.path"
        )?;

        let mut direct_dependents = Vec::new();
        let mut transitive_dependents = Vec::new();

        let rows = stmt.query_map(params![file_id, max_depth], |row| {
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

        let total_dependent_count = direct_dependents.len() + transitive_dependents.len();

        Ok(FileImpact {
            target,
            direct_dependents,
            transitive_dependents,
            total_dependent_count,
        })
    }

    fn find_dependency_path(
        &self,
        from_file_id: i64,
        to_file_id: i64,
    ) -> Result<Option<FilePath>> {
        if from_file_id == to_file_id {
            let file = self.get_file_by_id(from_file_id)?
                .ok_or_else(|| Error::NotFound(format!("file id: {}", from_file_id)))?;
            return Ok(Some(FilePath { files: vec![file] }));
        }

        let max_depth = 50;
        let mut stmt = self.conn.prepare(
            "WITH RECURSIVE path_search(file_id, path, depth) AS (
                SELECT ?1, CAST(?1 AS TEXT), 0

                UNION

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
            LIMIT 1"
        )?;

        let path_str: Option<String> = stmt
            .query_row(params![from_file_id, to_file_id, max_depth], |row| row.get(0))
            .optional()?;

        let Some(path_str) = path_str else {
            return Ok(None);
        };

        let file_ids: Vec<i64> = path_str
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();

        let mut files = Vec::with_capacity(file_ids.len());
        for id in file_ids {
            let file = self.get_file_by_id(id)?
                .ok_or_else(|| Error::NotFound(format!("file id: {}", id)))?;
            files.push(file);
        }

        Ok(Some(FilePath { files }))
    }

    fn detect_cycles(&self) -> Result<Vec<Cycle>> {
        // Stub implementation - full cycle detection deferred
        Ok(vec![])
    }

    fn detect_cycles_involving(&self, _file_id: i64) -> Result<Vec<Cycle>> {
        // Stub implementation - full cycle detection deferred
        Ok(vec![])
    }
}

fn row_to_indexed_file(row: &rusqlite::Row) -> rusqlite::Result<IndexedFile> {
    Ok(IndexedFile {
        id: row.get(0)?,
        path: std::path::PathBuf::from(row.get::<_, String>(1)?),
        language: parse_language(&row.get::<_, String>(2)?)?,
        mtime_ns: row.get(3)?,
        size_bytes: row.get::<_, i64>(4)? as u64,
        content_hash: row.get::<_, Option<i64>>(5)?.map(|h| h as u64),
        indexed_at: row.get(6)?,
    })
}

fn parse_language(s: &str) -> rusqlite::Result<Language> {
    match s {
        "rust" => Ok(Language::Rust),
        "csharp" => Ok(Language::CSharp),
        unknown => Err(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            format!("Unknown language: {unknown}").into(),
        )),
    }
}
```

**Step 3: Update graph/mod.rs exports**

Update the exports in `crates/tethys/src/graph/mod.rs`:

```rust
pub use sql::{SqlFileGraph, SqlSymbolGraph};
```

**Step 4: Run tests**

Run: `cargo test -p tethys graph::sql::tests --no-fail-fast`
Expected: All tests pass

**Step 5: Commit**

```bash
git add crates/tethys/src/graph/
git commit -m "feat(tethys): implement SqlFileGraph for file-level operations

File-level dependents, dependencies, transitive impact, and path finding.
Cycle detection stubbed for future implementation.

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>"
```

---

## Task 7: Integrate Graph Operations into Tethys

**Files:**
- Modify: `crates/tethys/src/lib.rs`

**Step 1: Add graph fields to Tethys struct**

In `crates/tethys/src/lib.rs`, modify the `Tethys` struct (around line 77):

```rust
use graph::{
    CallPath, CallerInfo, FileDepInfo, FileGraphOps, FileImpact, FilePath,
    SqlFileGraph, SqlSymbolGraph, SymbolGraphOps, SymbolImpact,
};

pub struct Tethys {
    workspace_root: PathBuf,
    db_path: PathBuf,
    db: Index,
    parser: tree_sitter::Parser,
    symbol_graph: Box<dyn SymbolGraphOps>,
    file_graph: Box<dyn FileGraphOps>,
}
```

**Step 2: Initialize graph operations in Tethys::new()**

Modify the `new()` function to initialize the graph operations:

```rust
    pub fn new(workspace_root: &Path) -> Result<Self> {
        let workspace_root = workspace_root.canonicalize().map_err(|e| {
            Error::Io(std::io::Error::new(
                e.kind(),
                format!("workspace root not found: {}", workspace_root.display()),
            ))
        })?;

        let db_path = workspace_root
            .join(".rivets")
            .join("index")
            .join("tethys.db");
        let db = Index::open(&db_path)?;

        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .map_err(|e| Error::Parser(e.to_string()))?;

        // Initialize graph operations with their own DB connections
        let symbol_graph = Box::new(SqlSymbolGraph::new(&db_path)?);
        let file_graph = Box::new(SqlFileGraph::new(&db_path)?);

        Ok(Self {
            workspace_root,
            db_path,
            db,
            parser,
            symbol_graph,
            file_graph,
        })
    }
```

**Step 3: Implement public graph methods**

Replace the `todo!()` implementations with real ones. Update these methods:

```rust
    /// Get impact analysis: direct and transitive dependents of a file.
    pub fn get_impact(&self, path: &Path) -> Result<Impact> {
        let file_id = self.db.get_file_id(self.relative_path(path))?
            .ok_or_else(|| Error::NotFound(format!("file: {}", path.display())))?;

        let file_impact = self.file_graph.get_transitive_dependents(file_id, Some(50))?;

        // Convert FileImpact to public Impact type
        Ok(Impact {
            target: file_impact.target.path,
            direct_dependents: file_impact.direct_dependents.into_iter().map(|d| Dependent {
                file: d.file.path,
                symbols_used: vec![],
                line_count: d.ref_count,
            }).collect(),
            transitive_dependents: file_impact.transitive_dependents.into_iter().map(|d| Dependent {
                file: d.file.path,
                symbols_used: vec![],
                line_count: d.ref_count,
            }).collect(),
        })
    }

    /// Get symbols that call/use the given symbol.
    pub fn get_callers(&self, qualified_name: &str) -> Result<Vec<Dependent>> {
        let symbol = self.db.get_symbol_by_qualified_name(qualified_name)?
            .ok_or_else(|| Error::NotFound(format!("symbol: {}", qualified_name)))?;

        let callers = self.symbol_graph.get_callers(symbol.id)?;

        // Convert CallerInfo to Dependent
        callers.into_iter().map(|c| {
            let file = self.db.get_file_by_id(c.symbol.file_id)?
                .ok_or_else(|| Error::NotFound(format!("file id: {}", c.symbol.file_id)))?;
            Ok(Dependent {
                file: file.path,
                symbols_used: vec![c.symbol.qualified_name],
                line_count: c.reference_count,
            })
        }).collect()
    }

    /// Get symbols that the given symbol calls/uses.
    pub fn get_symbol_dependencies(&self, qualified_name: &str) -> Result<Vec<Symbol>> {
        let symbol = self.db.get_symbol_by_qualified_name(qualified_name)?
            .ok_or_else(|| Error::NotFound(format!("symbol: {}", qualified_name)))?;

        let callees = self.symbol_graph.get_callees(symbol.id)?;

        Ok(callees.into_iter().map(|c| c.symbol).collect())
    }

    /// Get impact analysis: direct and transitive callers of a symbol.
    pub fn get_symbol_impact(&self, qualified_name: &str) -> Result<Impact> {
        let symbol = self.db.get_symbol_by_qualified_name(qualified_name)?
            .ok_or_else(|| Error::NotFound(format!("symbol: {}", qualified_name)))?;

        let impact = self.symbol_graph.get_transitive_callers(symbol.id, Some(50))?;

        // Convert SymbolImpact to public Impact type
        let mut direct_dependents = Vec::new();
        for caller in impact.direct_callers {
            let file = self.db.get_file_by_id(caller.symbol.file_id)?
                .ok_or_else(|| Error::NotFound(format!("file id: {}", caller.symbol.file_id)))?;
            direct_dependents.push(Dependent {
                file: file.path,
                symbols_used: vec![caller.symbol.qualified_name],
                line_count: caller.reference_count,
            });
        }

        let mut transitive_dependents = Vec::new();
        for caller in impact.transitive_callers {
            let file = self.db.get_file_by_id(caller.symbol.file_id)?
                .ok_or_else(|| Error::NotFound(format!("file id: {}", caller.symbol.file_id)))?;
            transitive_dependents.push(Dependent {
                file: file.path,
                symbols_used: vec![caller.symbol.qualified_name],
                line_count: caller.reference_count,
            });
        }

        Ok(Impact {
            target: self.db.get_file_by_id(symbol.file_id)?
                .ok_or_else(|| Error::NotFound(format!("file id: {}", symbol.file_id)))?.path,
            direct_dependents,
            transitive_dependents,
        })
    }

    /// Detect circular dependencies in the codebase.
    pub fn detect_cycles(&self) -> Result<Vec<Cycle>> {
        self.file_graph.detect_cycles()
    }

    /// Get the shortest dependency path between two files.
    pub fn get_dependency_chain(&self, from: &Path, to: &Path) -> Result<Option<Vec<PathBuf>>> {
        let from_id = self.db.get_file_id(self.relative_path(from))?
            .ok_or_else(|| Error::NotFound(format!("file: {}", from.display())))?;
        let to_id = self.db.get_file_id(self.relative_path(to))?
            .ok_or_else(|| Error::NotFound(format!("file: {}", to.display())))?;

        let path = self.file_graph.find_dependency_path(from_id, to_id)?;

        Ok(path.map(|p| p.files.into_iter().map(|f| f.path).collect()))
    }
```

**Step 4: Remove #[allow(unused_variables)] attributes**

Remove the `#[allow(unused_variables)]` attributes from the methods you just implemented.

**Step 5: Verify compilation**

Run: `cargo check -p tethys`
Expected: Compiles (may have some warnings)

**Step 6: Run all tethys tests**

Run: `cargo test -p tethys`
Expected: All tests pass

**Step 7: Commit**

```bash
git add crates/tethys/src/lib.rs
git commit -m "feat(tethys): integrate graph operations into Tethys API

Replace todo!() stubs with real implementations delegating to
SqlSymbolGraph and SqlFileGraph.

Public API now provides:
- get_impact() for file-level impact analysis
- get_callers() for symbol-level callers
- get_symbol_dependencies() for callees
- get_symbol_impact() for transitive symbol callers
- get_dependency_chain() for file path finding
- detect_cycles() stub for cycle detection

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>"
```

---

## Task 8: Integration Tests

**Files:**
- Create: `crates/tethys/tests/graph.rs`

**Step 1: Create integration test file**

Create `crates/tethys/tests/graph.rs`:

```rust
//! Integration tests for Phase 3 graph operations.

use std::fs;
use tempfile::TempDir;
use tethys::Tethys;

/// Create a workspace with a known dependency structure for testing.
fn workspace_with_call_graph() -> (TempDir, Tethys) {
    let dir = tempfile::tempdir().expect("failed to create temp dir");

    // Create src directory
    fs::create_dir_all(dir.path().join("src")).unwrap();

    // main.rs calls auth::login and cache::get
    fs::write(
        dir.path().join("src/main.rs"),
        r#"
mod auth;
mod cache;

fn main() {
    auth::login();
    cache::get();
}
"#,
    )
    .unwrap();

    // auth.rs calls db::query
    fs::write(
        dir.path().join("src/auth.rs"),
        r#"
use crate::db;

pub fn login() -> bool {
    db::query();
    true
}
"#,
    )
    .unwrap();

    // cache.rs calls db::query
    fs::write(
        dir.path().join("src/cache.rs"),
        r#"
use crate::db;

pub fn get() -> Option<String> {
    db::query();
    None
}
"#,
    )
    .unwrap();

    // db.rs is the leaf
    fs::write(
        dir.path().join("src/db.rs"),
        r#"
pub fn query() -> Vec<u8> {
    vec![]
}
"#,
    )
    .unwrap();

    // lib.rs declares all modules
    fs::write(
        dir.path().join("src/lib.rs"),
        r#"
mod auth;
mod cache;
mod db;
"#,
    )
    .unwrap();

    let tethys = Tethys::new(dir.path()).expect("failed to create Tethys");
    (dir, tethys)
}

#[test]
fn get_impact_returns_file_dependents() {
    let (_dir, mut tethys) = workspace_with_call_graph();
    tethys.index().expect("index failed");

    let impact = tethys
        .get_impact(std::path::Path::new("src/db.rs"))
        .expect("get_impact failed");

    // db.rs should have auth.rs and cache.rs as direct dependents
    assert!(
        !impact.direct_dependents.is_empty(),
        "db.rs should have dependents"
    );
}

#[test]
fn get_dependency_chain_finds_path() {
    let (_dir, mut tethys) = workspace_with_call_graph();
    tethys.index().expect("index failed");

    let chain = tethys
        .get_dependency_chain(
            std::path::Path::new("src/auth.rs"),
            std::path::Path::new("src/db.rs"),
        )
        .expect("get_dependency_chain failed");

    assert!(chain.is_some(), "should find path from auth.rs to db.rs");
    let chain = chain.unwrap();
    assert!(chain.len() >= 2, "path should have at least 2 files");
}

#[test]
fn get_dependency_chain_returns_none_for_unconnected() {
    let (_dir, mut tethys) = workspace_with_call_graph();
    tethys.index().expect("index failed");

    // db.rs doesn't depend on main.rs (reverse direction)
    let chain = tethys
        .get_dependency_chain(
            std::path::Path::new("src/db.rs"),
            std::path::Path::new("src/main.rs"),
        )
        .expect("get_dependency_chain failed");

    assert!(chain.is_none(), "should not find path in reverse direction");
}

#[test]
fn detect_cycles_returns_empty_for_acyclic() {
    let (_dir, mut tethys) = workspace_with_call_graph();
    tethys.index().expect("index failed");

    let cycles = tethys.detect_cycles().expect("detect_cycles failed");

    // Our test graph is acyclic
    assert!(cycles.is_empty(), "acyclic graph should have no cycles");
}
```

**Step 2: Run integration tests**

Run: `cargo test -p tethys --test graph`
Expected: Tests pass (some may be skipped if symbols aren't being linked correctly - this is OK for Phase 3)

**Step 3: Commit**

```bash
git add crates/tethys/tests/graph.rs
git commit -m "test(tethys): add integration tests for graph operations

Tests file-level impact analysis, dependency chains, and cycle detection
through the public Tethys API.

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>"
```

---

## Task 9: Final Verification

**Step 1: Run all tests**

Run: `cargo test -p tethys`
Expected: All tests pass

**Step 2: Run clippy**

Run: `cargo clippy -p tethys -- -D warnings`
Expected: No warnings

**Step 3: Verify documentation builds**

Run: `cargo doc -p tethys --no-deps`
Expected: Documentation builds without errors

**Step 4: Final commit if any cleanup needed**

If any cleanup was needed:
```bash
git add -A
git commit -m "chore(tethys): cleanup and polish Phase 3 implementation

Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>"
```

---

## Summary

After completing all tasks, Phase 3 provides:

| Feature | Status |
|---------|--------|
| `SymbolGraphOps` trait | Complete |
| `FileGraphOps` trait | Complete |
| `SqlSymbolGraph.get_callers()` | Complete |
| `SqlSymbolGraph.get_callees()` | Complete |
| `SqlSymbolGraph.get_transitive_callers()` | Complete |
| `SqlSymbolGraph.find_call_path()` | Complete |
| `SqlFileGraph.get_dependents()` | Complete |
| `SqlFileGraph.get_dependencies()` | Complete |
| `SqlFileGraph.get_transitive_dependents()` | Complete |
| `SqlFileGraph.find_dependency_path()` | Complete |
| `SqlFileGraph.detect_cycles()` | Stubbed |
| Tethys integration | Complete |
| Integration tests | Complete |

Total estimated: ~9 tasks, ~1200 lines of new code
