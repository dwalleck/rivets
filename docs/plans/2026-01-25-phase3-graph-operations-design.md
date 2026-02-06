# Phase 3: Graph Operations - Implementation Plan

**Status:** Ready for Implementation
**Created:** 2026-01-25
**Design Doc:** [tethys-code-intelligence.md](../design/tethys-code-intelligence.md)

## Overview

Phase 3 adds graph operations to Tethys for impact analysis and path finding. The implementation uses a trait-based abstraction with SQL-driven implementations initially, allowing future swap to petgraph if needed.

**Priority:** Symbol-level operations (function impact analysis, call path finding) over file-level operations.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                         Tethys                              │
│                    (public API holder)                      │
└─────────────────────────┬───────────────────────────────────┘
                          │ delegates to
          ┌───────────────┴───────────────┐
          ▼                               ▼
┌─────────────────────┐       ┌─────────────────────┐
│   SymbolGraphOps    │       │    FileGraphOps     │
│      (trait)        │       │      (trait)        │
├─────────────────────┤       ├─────────────────────┤
│ • get_callers()     │       │ • get_impact()      │
│ • get_callees()     │       │ • detect_cycles()   │
│ • get_impact()      │       │ • find_path()       │
│ • find_path()       │       └──────────┬──────────┘
└──────────┬──────────┘                  │
           │                             │
           ▼                             ▼
┌─────────────────────┐       ┌─────────────────────┐
│  SqlSymbolGraph     │       │  SqlFileGraph       │
│  (SQL + recursive   │       │  (initial impl)     │
│   CTEs)             │       │                     │
└─────────────────────┘       └─────────────────────┘
                                         │
                              ┌──────────┴──────────┐
                              │  PetgraphFileGraph  │
                              │  (future: cycles)   │
                              └─────────────────────┘
```

## Module Structure

```
crates/tethys/src/
├── lib.rs              # Public API, Tethys struct (modify)
├── db.rs               # SQLite storage layer (exists)
├── error.rs            # Error types (modify if needed)
├── types.rs            # Public types (exists)
└── graph/              # NEW: Graph operations
    ├── mod.rs          # Trait definitions, re-exports
    ├── sql.rs          # SqlSymbolGraph, SqlFileGraph implementations
    └── types.rs        # CallerInfo, CalleeInfo, SymbolImpact, etc.
```

## Trait Definitions

### SymbolGraphOps

```rust
/// Operations on the symbol-level dependency graph.
pub trait SymbolGraphOps: Send + Sync {
    /// Get symbols that directly call/reference the given symbol.
    fn get_callers(&self, symbol_id: i64) -> Result<Vec<CallerInfo>>;

    /// Get symbols that the given symbol directly calls/references.
    fn get_callees(&self, symbol_id: i64) -> Result<Vec<CalleeInfo>>;

    /// Get transitive callers (impact analysis).
    fn get_transitive_callers(&self, symbol_id: i64, max_depth: Option<u32>) -> Result<SymbolImpact>;

    /// Find the shortest call path between two symbols.
    fn find_call_path(&self, from_symbol_id: i64, to_symbol_id: i64) -> Result<Option<CallPath>>;
}
```

### FileGraphOps

```rust
/// Operations on the file-level dependency graph.
pub trait FileGraphOps: Send + Sync {
    /// Get files that directly depend on the given file.
    fn get_dependents(&self, file_id: i64) -> Result<Vec<FileDepInfo>>;

    /// Get files that the given file directly depends on.
    fn get_dependencies(&self, file_id: i64) -> Result<Vec<FileDepInfo>>;

    /// Get transitive dependents (file-level impact analysis).
    fn get_transitive_dependents(&self, file_id: i64, max_depth: Option<u32>) -> Result<FileImpact>;

    /// Find the shortest dependency path between two files.
    fn find_dependency_path(&self, from_file_id: i64, to_file_id: i64) -> Result<Option<FilePath>>;

    /// Detect circular dependencies in the codebase.
    fn detect_cycles(&self) -> Result<Vec<Cycle>>;

    /// Detect cycles that involve a specific file.
    fn detect_cycles_involving(&self, file_id: i64) -> Result<Vec<Cycle>>;
}
```

## Types

### graph/types.rs

```rust
use crate::types::{ReferenceKind, Symbol, IndexedFile};

/// Information about a caller of a symbol.
#[derive(Debug, Clone)]
pub struct CallerInfo {
    pub symbol: Symbol,
    pub reference_count: usize,
    pub reference_kinds: Vec<ReferenceKind>,
}

/// Information about a callee of a symbol.
#[derive(Debug, Clone)]
pub struct CalleeInfo {
    pub symbol: Symbol,
    pub reference_count: usize,
    pub reference_kinds: Vec<ReferenceKind>,
}

/// Result of transitive caller analysis.
#[derive(Debug, Clone)]
pub struct SymbolImpact {
    pub target: Symbol,
    pub direct_callers: Vec<CallerInfo>,
    pub transitive_callers: Vec<CallerInfo>,
    pub total_caller_count: usize,
    pub max_depth_reached: u32,
}

/// A path through the call graph.
#[derive(Debug, Clone)]
pub struct CallPath {
    pub symbols: Vec<Symbol>,
    pub edges: Vec<ReferenceKind>,
}

/// Information about a file dependency.
#[derive(Debug, Clone)]
pub struct FileDepInfo {
    pub file: IndexedFile,
    pub ref_count: usize,
}

/// Result of file-level impact analysis.
#[derive(Debug, Clone)]
pub struct FileImpact {
    pub target: IndexedFile,
    pub direct_dependents: Vec<FileDepInfo>,
    pub transitive_dependents: Vec<FileDepInfo>,
    pub total_dependent_count: usize,
}

/// A path through the file dependency graph.
#[derive(Debug, Clone)]
pub struct FilePath {
    pub files: Vec<IndexedFile>,
}
```

## SQL Implementation

### Recursive CTE for Transitive Callers

```sql
WITH RECURSIVE caller_tree(symbol_id, depth) AS (
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
      AND ct.depth < ?2  -- max_depth limit
)
SELECT DISTINCT s.*, ct.depth
FROM caller_tree ct
JOIN symbols s ON s.id = ct.symbol_id
ORDER BY ct.depth, s.name;
```

### Bidirectional BFS for Path Finding

```sql
WITH RECURSIVE
forward(symbol_id, path, depth) AS (
    SELECT ?1, CAST(?1 AS TEXT), 0
    UNION
    SELECT r.symbol_id,
           f.path || ',' || r.symbol_id,
           f.depth + 1
    FROM refs r
    JOIN forward f ON r.in_symbol_id = f.symbol_id
    WHERE f.depth < ?3
),
backward(symbol_id, path, depth) AS (
    SELECT ?2, CAST(?2 AS TEXT), 0
    UNION
    SELECT r.in_symbol_id,
           r.in_symbol_id || ',' || b.path,
           b.depth + 1
    FROM refs r
    JOIN backward b ON r.symbol_id = b.symbol_id
    WHERE r.in_symbol_id IS NOT NULL
      AND b.depth < ?3
)
SELECT f.path || ',' || SUBSTR(b.path, INSTR(b.path, ',') + 1) AS full_path
FROM forward f
JOIN backward b ON f.symbol_id = b.symbol_id
ORDER BY f.depth + b.depth
LIMIT 1;
```

## Tethys Integration

```rust
pub struct Tethys {
    workspace_root: PathBuf,
    db_path: PathBuf,
    db: Index,
    parser: tree_sitter::Parser,

    // Phase 3: Graph operation delegates
    symbol_graph: Box<dyn SymbolGraphOps>,
    file_graph: Box<dyn FileGraphOps>,
}
```

Graph implementations get their own DB connection to avoid borrow conflicts. Public API uses `qualified_name` and `Path`; internal uses `i64` IDs.

## Implementation Steps

### Step 1: Types and Traits
**Files:** `graph/mod.rs`, `graph/types.rs`

- [ ] Create `graph/` directory
- [ ] Define types in `graph/types.rs`: `CallerInfo`, `CalleeInfo`, `SymbolImpact`, `FileImpact`, `CallPath`, `FilePath`, `FileDepInfo`
- [ ] Define `SymbolGraphOps` trait in `graph/mod.rs`
- [ ] Define `FileGraphOps` trait in `graph/mod.rs`
- [ ] Add `mod graph;` to `lib.rs`
- [ ] Verify compilation

### Step 2: SqlSymbolGraph - Direct Queries
**Files:** `graph/sql.rs`

- [ ] Create `SqlSymbolGraph` struct with DB connection
- [ ] Implement `get_callers()` - single SQL query joining refs and symbols
- [ ] Implement `get_callees()` - single SQL query for outgoing refs
- [ ] Write unit tests for direct caller/callee queries
- [ ] Test with empty results case

### Step 3: SqlSymbolGraph - Transitive Queries
**Files:** `graph/sql.rs`

- [ ] Implement `get_transitive_callers()` with recursive CTE
- [ ] Add depth limiting parameter
- [ ] Handle cycles (recursive CTE naturally deduplicates)
- [ ] Write tests with known graph structures
- [ ] Test depth limiting behavior

### Step 4: SqlSymbolGraph - Path Finding
**Files:** `graph/sql.rs`

- [ ] Implement `find_call_path()` with bidirectional BFS in SQL
- [ ] Parse comma-separated path back to symbol IDs
- [ ] Fetch full Symbol objects for the path
- [ ] Handle no-path case (return None)
- [ ] Write tests for connected and unconnected scenarios

### Step 5: SqlFileGraph Implementation
**Files:** `graph/sql.rs`

- [ ] Create `SqlFileGraph` struct with DB connection
- [ ] Implement `get_dependents()` - query file_deps table
- [ ] Implement `get_dependencies()` - query file_deps table
- [ ] Implement `get_transitive_dependents()` with recursive CTE
- [ ] Implement `find_dependency_path()`
- [ ] Stub `detect_cycles()` and `detect_cycles_involving()` with `todo!()`
- [ ] Write unit tests

### Step 6: Tethys Integration
**Files:** `lib.rs`

- [ ] Add `symbol_graph: Box<dyn SymbolGraphOps>` field
- [ ] Add `file_graph: Box<dyn FileGraphOps>` field
- [ ] Initialize both in `Tethys::new()`
- [ ] Implement `get_callers()` - delegate to symbol_graph
- [ ] Implement `get_symbol_impact()` - delegate to symbol_graph
- [ ] Implement `get_symbol_dependencies()` - use get_callees
- [ ] Implement `get_impact()` - delegate to file_graph
- [ ] Implement `get_dependency_chain()` - delegate to file_graph
- [ ] Update `detect_cycles()` to delegate to file_graph
- [ ] Add `Error::NotFound` variant if not present

### Step 7: Integration Testing
**Files:** `tests/graph.rs`

- [ ] Create test workspace helper with realistic Rust project
- [ ] Test `get_symbol_impact()` through public API
- [ ] Test `get_callers()` through public API
- [ ] Test `get_impact()` for file-level analysis
- [ ] Test `get_dependency_chain()` for file paths
- [ ] Test error cases (symbol not found, file not found)

## Testing Strategy

### Unit Test Graph Structure

```
main::run → auth::validate → db::query
         ↘ cache::get ↗
```

### Test Cases

| Test | Description |
|------|-------------|
| Direct callers | `db::query` has 2 callers: `auth::validate`, `cache::get` |
| Transitive callers | `db::query` has 3 total: above + `main::run` |
| Path finding | `main::run` → `db::query` returns length-3 path |
| No path | Unconnected symbols return `None` |
| Depth limit | `max_depth=1` stops at direct callers |
| Cycles | Graph with cycle doesn't infinite loop |
| Empty results | Symbol with no callers returns empty vec |

## Success Criteria

- [ ] All `todo!()` stubs in lib.rs replaced with working implementations
- [ ] Symbol-level impact analysis works for the rivets codebase itself
- [ ] Path finding returns sensible results
- [ ] All tests pass
- [ ] No performance regression on indexing (graph ops are query-time only)

## Future Work (Not in Scope)

- `PetgraphFileGraph` for Tarjan's SCC cycle detection
- Caching of graph query results
- Incremental graph updates (currently requires re-query after index changes)
- Symbol-level cycle detection
