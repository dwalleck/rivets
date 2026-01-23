# Tethys Storage Spike: SQLite + petgraph

**Date**: 2026-01-22
**Status**: Spike
**Goal**: Validate SQLite + petgraph hybrid approach for Tethys code intelligence

## Architecture Overview

```
┌─────────────────────────────────────────────────────────┐
│                      Tethys API                         │
├─────────────────────────────────────────────────────────┤
│  Simple Queries          │  Graph Operations            │
│  (SQL)                   │  (petgraph)                  │
│                          │                              │
│  - Find symbol by name   │  - Impact analysis (transitive) │
│  - List references       │  - Cycle detection           │
│  - File dependencies     │  - Shortest path             │
│  - Symbol lookup         │  - Connected components      │
├──────────────────────────┴──────────────────────────────┤
│                     SQLite Storage                      │
│              (.rivets/index/tethys.db)                  │
└─────────────────────────────────────────────────────────┘
```

**Strategy**: SQLite is the source of truth. For graph operations, load relevant subgraph into petgraph, compute, return results.

## SQLite Schema

```sql
-- Indexed source files
CREATE TABLE files (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    language TEXT NOT NULL,          -- 'rust', 'csharp'
    mtime_ns INTEGER NOT NULL,       -- nanoseconds since epoch
    size_bytes INTEGER NOT NULL,
    content_hash INTEGER,            -- xxhash64 for change detection
    indexed_at INTEGER NOT NULL      -- when we last parsed this file
);

CREATE INDEX idx_files_path ON files(path);
CREATE INDEX idx_files_language ON files(language);

-- Symbol definitions (functions, structs, traits, etc.)
CREATE TABLE symbols (
    id INTEGER PRIMARY KEY,
    file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    name TEXT NOT NULL,              -- simple name: "save"
    module_path TEXT NOT NULL,       -- module location: "crate::storage::issue"
    qualified_name TEXT NOT NULL,    -- symbol hierarchy: "IssueStorage::save"
    kind TEXT NOT NULL,              -- 'function', 'struct', 'trait', 'method', etc.
    line INTEGER NOT NULL,
    column INTEGER NOT NULL,
    end_line INTEGER,
    end_column INTEGER,
    signature TEXT,                  -- "fn save(&self, issue: &Issue) -> Result<()>"
    visibility TEXT NOT NULL,        -- 'public', 'private', 'crate', 'super'
    parent_symbol_id INTEGER REFERENCES symbols(id) ON DELETE CASCADE
);

CREATE INDEX idx_symbols_name ON symbols(name);
CREATE INDEX idx_symbols_module_path ON symbols(module_path);
CREATE INDEX idx_symbols_qualified ON symbols(qualified_name);
CREATE INDEX idx_symbols_file ON symbols(file_id);
CREATE INDEX idx_symbols_kind ON symbols(kind);

-- References (usages of symbols)
CREATE TABLE refs (
    id INTEGER PRIMARY KEY,
    symbol_id INTEGER NOT NULL REFERENCES symbols(id) ON DELETE CASCADE,
    file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,              -- 'call', 'import', 'type_ref', 'field_access'
    line INTEGER NOT NULL,
    column INTEGER NOT NULL,
    in_symbol_id INTEGER REFERENCES symbols(id) ON DELETE CASCADE  -- which symbol contains this ref
);

CREATE INDEX idx_refs_symbol ON refs(symbol_id);
CREATE INDEX idx_refs_file ON refs(file_id);
CREATE INDEX idx_refs_in_symbol ON refs(in_symbol_id);

-- File-level dependencies (derived from refs, denormalized for fast queries)
CREATE TABLE file_deps (
    from_file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    to_file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    ref_count INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (from_file_id, to_file_id)
);

CREATE INDEX idx_file_deps_to ON file_deps(to_file_id);
```

## Rust Types

```rust
use rusqlite::{Connection, params};
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::algo::{has_path_connecting, toposort};
use petgraph::visit::Bfs;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// Symbol kinds we track
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    Method,
    Struct,
    Enum,
    Trait,
    Impl,
    Const,
    Static,
    Module,
    TypeAlias,
    Macro,
}

/// Reference kinds
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    Call,           // function/method invocation
    Import,         // use statement
    TypeRef,        // type annotation
    FieldAccess,    // struct field access
    TraitImpl,      // impl Trait for Type
}

/// A symbol definition
#[derive(Debug, Clone)]
pub struct Symbol {
    pub id: i64,
    pub file_path: PathBuf,
    pub name: String,
    pub qualified_name: String,
    pub kind: SymbolKind,
    pub line: u32,
    pub column: u32,
    pub signature: Option<String>,
    pub visibility: Visibility,
}

/// A reference to a symbol
#[derive(Debug, Clone)]
pub struct Reference {
    pub id: i64,
    pub symbol_id: i64,
    pub file_path: PathBuf,
    pub kind: RefKind,
    pub line: u32,
    pub column: u32,
    pub in_symbol_id: Option<i64>,  // containing symbol
}
```

## Query Patterns

### Simple Queries (Pure SQL)

```rust
impl TethysDb {
    /// Find symbols by name (fuzzy)
    pub fn search_symbols(&self, query: &str, limit: usize) -> Result<Vec<Symbol>> {
        let sql = r#"
            SELECT s.id, f.path, s.name, s.qualified_name, s.kind,
                   s.line, s.column, s.signature, s.visibility
            FROM symbols s
            JOIN files f ON s.file_id = f.id
            WHERE s.name LIKE ?1 OR s.qualified_name LIKE ?1
            ORDER BY
                CASE WHEN s.name = ?2 THEN 0 ELSE 1 END,  -- exact match first
                length(s.qualified_name)                   -- shorter paths first
            LIMIT ?3
        "#;

        let pattern = format!("%{}%", query);
        // ... execute and map results
    }

    /// Find all references to a symbol
    pub fn find_references(&self, symbol_id: i64) -> Result<Vec<Reference>> {
        let sql = r#"
            SELECT r.id, r.symbol_id, f.path, r.kind, r.line, r.column, r.in_symbol_id
            FROM refs r
            JOIN files f ON r.file_id = f.id
            WHERE r.symbol_id = ?1
            ORDER BY f.path, r.line
        "#;
        // ... execute and map results
    }

    /// Find direct callers of a symbol (symbols that reference it)
    pub fn find_direct_callers(&self, symbol_id: i64) -> Result<Vec<Symbol>> {
        let sql = r#"
            SELECT DISTINCT s.id, f.path, s.name, s.qualified_name, s.kind,
                   s.line, s.column, s.signature, s.visibility
            FROM refs r
            JOIN symbols s ON r.in_symbol_id = s.id
            JOIN files f ON s.file_id = f.id
            WHERE r.symbol_id = ?1
            ORDER BY s.qualified_name
        "#;
        // ... execute and map results
    }

    /// Get files that depend on a given file
    pub fn get_dependent_files(&self, file_path: &Path) -> Result<Vec<PathBuf>> {
        let sql = r#"
            SELECT f2.path
            FROM file_deps fd
            JOIN files f1 ON fd.to_file_id = f1.id
            JOIN files f2 ON fd.from_file_id = f2.id
            WHERE f1.path = ?1
            ORDER BY f2.path
        "#;
        // ... execute and map results
    }
}
```

### Graph Operations (SQLite → petgraph)

```rust
impl TethysDb {
    /// Build a symbol dependency graph for graph operations
    fn build_symbol_graph(&self, root_symbol_id: i64) -> Result<SymbolGraph> {
        // Load all symbols and their call relationships
        let sql = r#"
            WITH RECURSIVE reachable(id) AS (
                -- Start with root symbol
                SELECT ?1
                UNION
                -- Add symbols that reference reachable symbols
                SELECT DISTINCT r.in_symbol_id
                FROM refs r
                JOIN reachable reach ON r.symbol_id = reach.id
                WHERE r.in_symbol_id IS NOT NULL
            )
            SELECT s.id, s.qualified_name, r.in_symbol_id as caller_id
            FROM reachable reach
            JOIN symbols s ON reach.id = s.id
            LEFT JOIN refs r ON r.symbol_id = s.id AND r.in_symbol_id IS NOT NULL
        "#;

        let mut graph: DiGraph<i64, ()> = DiGraph::new();
        let mut node_map: HashMap<i64, NodeIndex> = HashMap::new();

        // Build graph from query results...

        Ok(SymbolGraph { graph, node_map })
    }

    /// Get impact: all symbols transitively affected by changing target
    pub fn get_impact(&self, symbol_id: i64) -> Result<Impact> {
        let symbol_graph = self.build_symbol_graph(symbol_id)?;

        let root_idx = symbol_graph.node_map.get(&symbol_id)
            .ok_or_else(|| Error::SymbolNotFound(symbol_id))?;

        // BFS to find all reachable nodes (dependents)
        let mut dependents = Vec::new();
        let mut bfs = Bfs::new(&symbol_graph.graph, *root_idx);

        while let Some(node_idx) = bfs.next(&symbol_graph.graph) {
            if node_idx != *root_idx {
                let symbol_id = symbol_graph.graph[node_idx];
                dependents.push(symbol_id);
            }
        }

        // Compute depth for each dependent
        let depths = petgraph::algo::dijkstra(
            &symbol_graph.graph,
            *root_idx,
            None,
            |_| 1u32
        );

        let max_depth = depths.values().max().copied().unwrap_or(0);

        Ok(Impact {
            root_symbol_id: symbol_id,
            direct_dependents: self.find_direct_callers(symbol_id)?,
            transitive_count: dependents.len(),
            max_depth,
            // Load full symbol details for dependents...
        })
    }

    /// Detect circular dependencies
    pub fn detect_cycles(&self) -> Result<Vec<Cycle>> {
        // Load file dependency graph
        let sql = "SELECT from_file_id, to_file_id FROM file_deps";

        let mut graph: DiGraph<i64, ()> = DiGraph::new();
        let mut node_map: HashMap<i64, NodeIndex> = HashMap::new();

        // ... build graph from results

        // Try topological sort - failure means cycles exist
        match toposort(&graph, None) {
            Ok(_) => Ok(vec![]),  // No cycles
            Err(cycle_node) => {
                // Find all cycles using Tarjan's algorithm
                let sccs = petgraph::algo::tarjan_scc(&graph);
                let cycles: Vec<Cycle> = sccs
                    .into_iter()
                    .filter(|scc| scc.len() > 1)  // SCCs with >1 node are cycles
                    .map(|scc| {
                        // Convert node indices back to file IDs
                        Cycle {
                            file_ids: scc.iter()
                                .map(|idx| graph[*idx])
                                .collect()
                        }
                    })
                    .collect();
                Ok(cycles)
            }
        }
    }
}
```

## Impact Result Type

```rust
/// Result of impact analysis
#[derive(Debug)]
pub struct Impact {
    pub root_symbol_id: i64,
    pub root_symbol: Symbol,

    /// Symbols that directly call/reference the root
    pub direct_dependents: Vec<Symbol>,

    /// Total count of transitively affected symbols
    pub transitive_count: usize,

    /// Maximum depth in the dependency chain
    pub max_depth: u32,

    /// Breakdown by file
    pub affected_files: Vec<AffectedFile>,
}

#[derive(Debug)]
pub struct AffectedFile {
    pub path: PathBuf,
    pub direct_refs: usize,
    pub transitive_refs: usize,
}
```

## Performance Considerations

### When to Use SQL vs petgraph

| Operation | Approach | Reason |
|-----------|----------|--------|
| Symbol search | SQL | Simple filtering, indices handle it |
| List references | SQL | Direct lookup by foreign key |
| Direct callers | SQL | Single JOIN operation |
| File dependencies | SQL | Denormalized table, fast |
| Impact analysis | petgraph | Requires transitive closure |
| Cycle detection | petgraph | Requires graph algorithms |
| Shortest path | petgraph | Graph algorithm |

### Graph Loading Strategy

For large codebases, loading the entire graph is expensive. Options:

1. **Bounded loading**: Only load N levels deep from the target symbol
2. **Lazy expansion**: Start with direct deps, expand on demand
3. **Cached subgraphs**: Keep hot subgraphs in memory between queries

```rust
/// Load graph with depth limit
fn build_bounded_graph(&self, root: i64, max_depth: u32) -> Result<SymbolGraph> {
    let sql = r#"
        WITH RECURSIVE reachable(id, depth) AS (
            SELECT ?1, 0
            UNION
            SELECT DISTINCT r.in_symbol_id, reach.depth + 1
            FROM refs r
            JOIN reachable reach ON r.symbol_id = reach.id
            WHERE r.in_symbol_id IS NOT NULL
              AND reach.depth < ?2
        )
        SELECT DISTINCT id FROM reachable
    "#;
    // ...
}
```

## Index Lifecycle

```rust
impl TethysDb {
    /// Open or create the index database
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    /// Check if a file needs re-indexing
    pub fn is_stale(&self, path: &Path, current_mtime: i64) -> Result<bool> {
        let sql = "SELECT mtime_ns FROM files WHERE path = ?1";
        match self.conn.query_row(sql, [path.to_string_lossy()], |row| {
            row.get::<_, i64>(0)
        }) {
            Ok(indexed_mtime) => Ok(current_mtime > indexed_mtime),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(true),  // Not indexed
            Err(e) => Err(e.into()),
        }
    }

    /// Re-index a file (delete old data, insert new)
    pub fn reindex_file(&self, file: &IndexedFile) -> Result<()> {
        let tx = self.conn.transaction()?;

        // Delete old data (CASCADE handles symbols and refs)
        tx.execute("DELETE FROM files WHERE path = ?1", [&file.path])?;

        // Insert file
        tx.execute(
            "INSERT INTO files (path, language, mtime_ns, size_bytes, content_hash, indexed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![file.path, file.language, file.mtime_ns, file.size, file.hash, now()]
        )?;

        let file_id = tx.last_insert_rowid();

        // Insert symbols and refs...

        tx.commit()
    }
}
```

## Dependencies

```toml
[dependencies]
rusqlite = { version = "0.32", features = ["bundled"] }
petgraph = "0.8"
```

## Summary

This hybrid approach gives us:
- **Simplicity**: SQL for 80% of queries (search, lookup, direct deps)
- **Power**: petgraph for complex graph algorithms when needed
- **Reliability**: Both libraries are actively maintained and battle-tested
- **Performance**: Load subgraphs on-demand, not the entire codebase

The main complexity is the SQL↔petgraph bridge, but it's straightforward:
1. Query relevant nodes/edges from SQLite
2. Build in-memory DiGraph
3. Run algorithm
4. Map results back to domain types
