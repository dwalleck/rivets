//! SQL-based implementations of graph operations.
//!
//! Uses recursive CTEs for graph traversal, keeping all data in `SQLite`.

// SQLite uses i64 for all integer storage. These casts are intentional and safe for
// practical values (reference counts within reasonable bounds).
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use rusqlite::{Connection, OptionalExtension};

use super::{
    CallPath, CalleeInfo, CallerInfo, FileDepInfo, FileGraphOps, FileImpact, FilePath,
    SymbolGraphOps, SymbolImpact,
};
use crate::db::{row_to_indexed_file, row_to_symbol};
use crate::error::{Error, Result};
use crate::types::{Cycle, FileId, IndexedFile, ReferenceKind, Symbol, SymbolId};

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
    /// Open a new connection to the database with standard pragmas.
    ///
    /// Configures WAL mode for better concurrency and enables foreign key enforcement.
    fn open(db_path: &Path) -> Result<Self> {
        let conn = Connection::open(db_path)?;

        // Apply same pragmas as Index::open for consistency
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;

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

    /// Get all unique file IDs that participate in dependencies.
    ///
    /// Returns file IDs that appear in the `file_deps` table (either as source or target).
    fn get_all_file_ids_in_deps(&self) -> Result<Vec<FileId>> {
        let conn = self.db.lock()?;

        let mut stmt = conn.prepare(
            "SELECT DISTINCT id FROM (
                SELECT from_file_id AS id FROM file_deps
                UNION
                SELECT to_file_id AS id FROM file_deps
             )",
        )?;

        let ids = stmt
            .query_map([], |row| row.get::<_, i64>(0).map(FileId::from))?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(ids)
    }

    /// Get file IDs that the given file depends on.
    fn get_dependencies_as_ids(&self, file_id: FileId) -> Result<Vec<FileId>> {
        let conn = self.db.lock()?;

        let mut stmt = conn.prepare("SELECT to_file_id FROM file_deps WHERE from_file_id = ?1")?;

        let deps = stmt
            .query_map([file_id.as_i64()], |row| {
                row.get::<_, i64>(0).map(FileId::from)
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(deps)
    }

    /// Build an adjacency list representation of the dependency graph.
    ///
    /// Returns a map from file ID to list of files it depends on (outgoing edges).
    fn build_adjacency_list(&self) -> Result<HashMap<FileId, Vec<FileId>>> {
        let conn = self.db.lock()?;

        let mut stmt = conn.prepare("SELECT from_file_id, to_file_id FROM file_deps")?;

        let rows = stmt.query_map([], |row| {
            let from: i64 = row.get(0)?;
            let to: i64 = row.get(1)?;
            Ok((FileId::from(from), FileId::from(to)))
        })?;

        let mut adj: HashMap<FileId, Vec<FileId>> = HashMap::new();
        for result in rows {
            let (from, to) = result?;
            adj.entry(from).or_default().push(to);
        }

        Ok(adj)
    }

    /// DFS-based cycle detection.
    ///
    /// Uses standard cycle detection with visited set and recursion stack.
    /// When a back edge is found, reconstructs the cycle path.
    ///
    /// # Complexity
    ///
    /// - **Time**: O(V + E) for the DFS traversal where V = nodes, E = edges.
    ///   Deduplication adds O(C × L × log C) where C = cycle count, L = avg cycle length.
    /// - **Space**: O(V) for visited/recursion sets, plus O(C × L) for storing cycles.
    fn find_cycles_dfs(&self, adj: &HashMap<FileId, Vec<FileId>>) -> Result<Vec<Cycle>> {
        let mut visited: HashSet<FileId> = HashSet::new();
        let mut rec_stack: HashSet<FileId> = HashSet::new();
        let mut path: Vec<FileId> = Vec::new();
        let mut cycles: Vec<Vec<FileId>> = Vec::new();

        // Get all nodes that participate in the graph
        let all_nodes: HashSet<FileId> = adj
            .iter()
            .flat_map(|(from, tos)| std::iter::once(*from).chain(tos.iter().copied()))
            .collect();

        let edge_count: usize = adj.values().map(Vec::len).sum();
        tracing::debug!(
            node_count = all_nodes.len(),
            edge_count = edge_count,
            "Starting cycle detection with DFS"
        );

        for &start in &all_nodes {
            if !visited.contains(&start) {
                dfs_visit_for_cycles(
                    start,
                    adj,
                    &mut visited,
                    &mut rec_stack,
                    &mut path,
                    &mut cycles,
                );
            }
        }

        let raw_cycle_count = cycles.len();

        // Deduplicate cycles (same cycle can be discovered from different starting nodes)
        let unique_cycles = deduplicate_cycles(cycles);

        tracing::debug!(
            raw_cycles = raw_cycle_count,
            unique_cycles = unique_cycles.len(),
            "DFS traversal complete, deduplicating cycles"
        );

        // Convert file IDs to Cycle structs with paths
        let result: Result<Vec<Cycle>> = unique_cycles
            .into_iter()
            .map(|ids| self.ids_to_cycle(&ids))
            .collect();

        if let Ok(ref cycles) = result {
            tracing::info!(cycle_count = cycles.len(), "Cycle detection complete");
        }

        result
    }

    /// Convert a list of file IDs to a `Cycle` struct with file paths.
    fn ids_to_cycle(&self, ids: &[FileId]) -> Result<Cycle> {
        let mut files = Vec::with_capacity(ids.len());

        for (idx, &id) in ids.iter().enumerate() {
            let file = self
                .get_file_by_id(id.as_i64())
                .map_err(|e| {
                    tracing::error!(
                        error = %e,
                        file_id = id.as_i64(),
                        cycle_position = idx,
                        cycle_length = ids.len(),
                        "Database error while resolving file for cycle"
                    );
                    e
                })?
                .ok_or_else(|| {
                    tracing::error!(
                        file_id = id.as_i64(),
                        cycle_position = idx,
                        cycle_length = ids.len(),
                        "File not found in database but referenced in dependency cycle \
                         (possible data integrity issue)"
                    );
                    Error::NotFound(format!(
                        "file id: {} (position {} in cycle of length {})",
                        id.as_i64(),
                        idx,
                        ids.len()
                    ))
                })?;
            files.push(file.path);
        }

        Ok(Cycle { files })
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

            match path_str {
                Some(s) => Some(parse_path_ids(&s)?),
                None => None,
            }
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

    fn detect_cycles(&self) -> Result<Vec<Cycle>> {
        let adj = self.build_adjacency_list()?;
        self.find_cycles_dfs(&adj)
    }

    fn detect_cycles_involving(&self, file_id: FileId) -> Result<Vec<Cycle>> {
        let all_cycles = self.detect_cycles()?;

        // Get the target file path once, propagating errors instead of swallowing them
        let target_file = self
            .get_file_by_id(file_id.as_i64())?
            .ok_or_else(|| Error::NotFound(format!("file id: {}", file_id.as_i64())))?;

        // Filter to cycles that contain the target file
        Ok(all_cycles
            .into_iter()
            .filter(|cycle| cycle.files.contains(&target_file.path))
            .collect())
    }
}

// === Cycle Detection Helper Functions ===

/// Recursive DFS visitor for cycle detection.
///
/// Traverses the graph marking nodes as visited. When a back edge is found
/// (an edge to a node still in the current DFS path/recursion stack), a cycle
/// is recorded. Back edges indicate cycles because we've reached a node we're
/// still in the process of exploring.
fn dfs_visit_for_cycles(
    node: FileId,
    adj: &HashMap<FileId, Vec<FileId>>,
    visited: &mut HashSet<FileId>,
    rec_stack: &mut HashSet<FileId>,
    path: &mut Vec<FileId>,
    cycles: &mut Vec<Vec<FileId>>,
) {
    visited.insert(node);
    rec_stack.insert(node);
    path.push(node);

    if let Some(neighbors) = adj.get(&node) {
        for &neighbor in neighbors {
            if !visited.contains(&neighbor) {
                dfs_visit_for_cycles(neighbor, adj, visited, rec_stack, path, cycles);
            } else if rec_stack.contains(&neighbor) {
                // Back edge found - extract the cycle
                if let Some(cycle_start_idx) = path.iter().position(|&id| id == neighbor) {
                    let cycle: Vec<FileId> = path[cycle_start_idx..].to_vec();
                    cycles.push(cycle);
                }
            }
        }
    }

    path.pop();
    rec_stack.remove(&node);
}

/// Deduplicate cycles by normalizing their representation.
///
/// Two cycles are considered the same if they contain the same nodes in the same
/// circular order, regardless of which node they start with.
///
/// We only normalize the starting point, not direction, because the DFS discovers
/// cycles by following directed edges. In a directed graph, A→B→C→A and C→B→A→C
/// are topologically distinct, so direction is semantically meaningful.
fn deduplicate_cycles(cycles: Vec<Vec<FileId>>) -> Vec<Vec<FileId>> {
    let mut seen: HashSet<Vec<FileId>> = HashSet::new();
    let mut unique: Vec<Vec<FileId>> = Vec::new();

    for cycle in cycles {
        if cycle.is_empty() {
            continue;
        }

        // Normalize: rotate so the smallest ID is first
        let normalized = normalize_cycle(&cycle);

        if seen.insert(normalized.clone()) {
            unique.push(normalized);
        }
    }

    unique
}

/// Normalize a cycle by rotating it so the smallest ID is first.
fn normalize_cycle(cycle: &[FileId]) -> Vec<FileId> {
    if cycle.is_empty() {
        return Vec::new();
    }

    // Find the index of the minimum element
    let min_idx = cycle
        .iter()
        .enumerate()
        .min_by_key(|(_, id)| id.as_i64())
        .map_or(0, |(idx, _)| idx);

    // Rotate so minimum is first
    let mut normalized = Vec::with_capacity(cycle.len());
    normalized.extend_from_slice(&cycle[min_idx..]);
    normalized.extend_from_slice(&cycle[..min_idx]);

    normalized
}

impl SymbolGraphOps for SqlSymbolGraph {
    fn get_callers(&self, symbol_id: SymbolId) -> Result<Vec<CallerInfo>> {
        let conn = self.db.lock()?;

        // Use pre-computed call_edges table for efficient indexed lookup
        let mut stmt = conn.prepare(
            "SELECT
                s.id, s.file_id, s.name, s.module_path, s.qualified_name,
                s.kind, s.line, s.column, s.end_line, s.end_column,
                s.signature, s.visibility, s.parent_symbol_id,
                ce.call_count
             FROM call_edges ce
             JOIN symbols s ON s.id = ce.caller_symbol_id
             WHERE ce.callee_symbol_id = ?1
             ORDER BY s.qualified_name",
        )?;

        let callers = stmt
            .query_map([symbol_id.as_i64()], |row| {
                let symbol = row_to_symbol(row)?;
                let ref_count: usize = row.get::<_, i64>(13)? as usize;

                Ok(CallerInfo {
                    symbol,
                    reference_count: ref_count,
                    // call_edges doesn't track reference kinds; default to Call
                    reference_kinds: vec![ReferenceKind::Call],
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(callers)
    }

    fn get_callees(&self, symbol_id: SymbolId) -> Result<Vec<CalleeInfo>> {
        let conn = self.db.lock()?;

        // Use pre-computed call_edges table for efficient indexed lookup
        let mut stmt = conn.prepare(
            "SELECT
                s.id, s.file_id, s.name, s.module_path, s.qualified_name,
                s.kind, s.line, s.column, s.end_line, s.end_column,
                s.signature, s.visibility, s.parent_symbol_id,
                ce.call_count
             FROM call_edges ce
             JOIN symbols s ON s.id = ce.callee_symbol_id
             WHERE ce.caller_symbol_id = ?1
             ORDER BY s.qualified_name",
        )?;

        let callees = stmt
            .query_map([symbol_id.as_i64()], |row| {
                let symbol = row_to_symbol(row)?;
                let ref_count: usize = row.get::<_, i64>(13)? as usize;

                Ok(CalleeInfo {
                    symbol,
                    reference_count: ref_count,
                    // call_edges doesn't track reference kinds; default to Call
                    reference_kinds: vec![ReferenceKind::Call],
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

        // Use recursive CTE with call_edges table for efficient traversal
        let mut stmt = conn.prepare(
            "WITH RECURSIVE caller_tree(symbol_id, depth) AS (
                -- Base case: direct callers from call_edges
                SELECT caller_symbol_id, 1
                FROM call_edges
                WHERE callee_symbol_id = ?1

                UNION

                -- Recursive case: callers of callers
                SELECT ce.caller_symbol_id, ct.depth + 1
                FROM call_edges ce
                JOIN caller_tree ct ON ce.callee_symbol_id = ct.symbol_id
                WHERE ct.depth < ?2
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

        // BFS to find shortest path using recursive CTE with call_edges table
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

                    -- Follow callees via call_edges
                    SELECT ce.callee_symbol_id,
                           ps.path || ',' || ce.callee_symbol_id,
                           ps.depth + 1
                    FROM call_edges ce
                    JOIN path_search ps ON ce.caller_symbol_id = ps.symbol_id
                    WHERE ps.depth < ?3
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

            match path_str {
                Some(s) => Some(parse_path_ids(&s)?),
                None => None,
            }
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
/// in SQL.
///
/// # Errors
///
/// Returns `Error::Internal` if any ID in the path cannot be parsed as an integer,
/// which indicates database corruption or a version mismatch.
fn parse_path_ids(path_str: &str) -> Result<Vec<i64>> {
    path_str
        .split(',')
        .filter(|id| !id.trim().is_empty())
        .map(|id| {
            let trimmed = id.trim();
            trimmed.parse().map_err(|e| {
                Error::Internal(format!(
                    "failed to parse ID '{trimmed}' in path '{path_str}': {e} (possible database corruption)"
                ))
            })
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
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let db_path = dir.path().join("test.db");
        let mut index = Index::open(&db_path).expect("failed to open test database");

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
            .expect("failed to index main.rs");

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
            .expect("failed to index auth.rs");

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
            .expect("failed to index db.rs");

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
            .expect("failed to index cache.rs");

        // Get symbol IDs
        let main_run = index
            .get_symbol_by_qualified_name("main::run")
            .expect("failed to query main::run")
            .expect("main::run not found");
        let auth_validate = index
            .get_symbol_by_qualified_name("auth::validate")
            .expect("failed to query auth::validate")
            .expect("auth::validate not found");
        let db_query = index
            .get_symbol_by_qualified_name("db::query")
            .expect("failed to query db::query")
            .expect("db::query not found");
        let cache_get = index
            .get_symbol_by_qualified_name("cache::get")
            .expect("failed to query cache::get")
            .expect("cache::get not found");

        // Create references: main::run -> auth::validate
        index
            .insert_reference(
                Some(auth_validate.id),
                main_file,
                ReferenceKind::Call.as_str(),
                5,
                1,
                Some(main_run.id),
                None,
            )
            .expect("failed to insert auth::validate reference");
        // main::run -> cache::get
        index
            .insert_reference(
                Some(cache_get.id),
                main_file,
                ReferenceKind::Call.as_str(),
                6,
                1,
                Some(main_run.id),
                None,
            )
            .expect("failed to insert cache::get reference");
        // auth::validate -> db::query
        index
            .insert_reference(
                Some(db_query.id),
                auth_file,
                ReferenceKind::Call.as_str(),
                3,
                1,
                Some(auth_validate.id),
                None,
            )
            .expect("failed to insert db::query reference from auth");
        // cache::get -> db::query
        index
            .insert_reference(
                Some(db_query.id),
                cache_file,
                ReferenceKind::Call.as_str(),
                3,
                1,
                Some(cache_get.id),
                None,
            )
            .expect("failed to insert db::query reference from cache");

        // Populate call_edges table from refs
        index
            .populate_call_edges()
            .expect("failed to populate call edges");

        (dir, db_path)
    }

    #[test]
    fn get_callers_returns_direct_callers() {
        let (_dir, db_path) = setup_test_graph();
        let graph = SqlSymbolGraph::new(&db_path).expect("failed to create symbol graph");
        let index = Index::open(&db_path).expect("failed to open index");

        let db_query = index
            .get_symbol_by_qualified_name("db::query")
            .expect("failed to query db::query")
            .expect("db::query not found");
        let callers = graph
            .get_callers(db_query.id)
            .expect("failed to get callers");

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
        let graph = SqlSymbolGraph::new(&db_path).expect("failed to create symbol graph");
        let index = Index::open(&db_path).expect("failed to open index");

        let main_run = index
            .get_symbol_by_qualified_name("main::run")
            .expect("failed to query main::run")
            .expect("main::run not found");
        let callers = graph
            .get_callers(main_run.id)
            .expect("failed to get callers");

        // main::run is not called by anything
        assert!(callers.is_empty(), "main::run should have no callers");
    }

    #[test]
    fn get_callees_returns_direct_callees() {
        let (_dir, db_path) = setup_test_graph();
        let graph = SqlSymbolGraph::new(&db_path).expect("failed to create symbol graph");
        let index = Index::open(&db_path).expect("failed to open index");

        let main_run = index
            .get_symbol_by_qualified_name("main::run")
            .expect("failed to query main::run")
            .expect("main::run not found");
        let callees = graph
            .get_callees(main_run.id)
            .expect("failed to get callees");

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
        let graph = SqlSymbolGraph::new(&db_path).expect("failed to create symbol graph");
        let index = Index::open(&db_path).expect("failed to open index");

        let db_query = index
            .get_symbol_by_qualified_name("db::query")
            .expect("failed to query db::query")
            .expect("db::query not found");
        let callees = graph
            .get_callees(db_query.id)
            .expect("failed to get callees");

        // db::query doesn't call anything
        assert!(callees.is_empty(), "db::query should have no callees");
    }

    #[test]
    fn get_callers_includes_reference_kinds() {
        let (_dir, db_path) = setup_test_graph();
        let graph = SqlSymbolGraph::new(&db_path).expect("failed to create symbol graph");
        let index = Index::open(&db_path).expect("failed to open index");

        let db_query = index
            .get_symbol_by_qualified_name("db::query")
            .expect("failed to query db::query")
            .expect("db::query not found");
        let callers = graph
            .get_callers(db_query.id)
            .expect("failed to get callers");

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
        let graph = SqlSymbolGraph::new(&db_path).expect("failed to create symbol graph");
        let index = Index::open(&db_path).expect("failed to open index");

        let db_query = index
            .get_symbol_by_qualified_name("db::query")
            .expect("failed to query db::query")
            .expect("db::query not found");
        let callers = graph
            .get_callers(db_query.id)
            .expect("failed to get callers");

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
        let graph = SqlSymbolGraph::new(&db_path).expect("failed to create symbol graph");
        let index = Index::open(&db_path).expect("failed to open index");

        let db_query = index
            .get_symbol_by_qualified_name("db::query")
            .expect("failed to query db::query")
            .expect("db::query not found");
        let impact = graph
            .get_transitive_callers(db_query.id, None)
            .expect("failed to get transitive callers");

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
        let graph = SqlSymbolGraph::new(&db_path).expect("failed to create symbol graph");
        let index = Index::open(&db_path).expect("failed to open index");

        let db_query = index
            .get_symbol_by_qualified_name("db::query")
            .expect("failed to query db::query")
            .expect("db::query not found");
        let impact = graph
            .get_transitive_callers(db_query.id, Some(1))
            .expect("failed to get transitive callers");

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
        let graph = SqlSymbolGraph::new(&db_path).expect("failed to create symbol graph");
        let index = Index::open(&db_path).expect("failed to open index");

        let main_run = index
            .get_symbol_by_qualified_name("main::run")
            .expect("failed to query main::run")
            .expect("main::run not found");
        let impact = graph
            .get_transitive_callers(main_run.id, None)
            .expect("failed to get transitive callers");

        assert_eq!(impact.total_caller_count(), 0);
        assert!(impact.direct_callers.is_empty());
        assert!(impact.transitive_callers.is_empty());
    }

    #[test]
    fn find_call_path_returns_shortest_path() {
        let (_dir, db_path) = setup_test_graph();
        let graph = SqlSymbolGraph::new(&db_path).expect("failed to create symbol graph");
        let index = Index::open(&db_path).expect("failed to open index");

        let main_run = index
            .get_symbol_by_qualified_name("main::run")
            .expect("failed to query main::run")
            .expect("main::run not found");
        let db_query = index
            .get_symbol_by_qualified_name("db::query")
            .expect("failed to query db::query")
            .expect("db::query not found");

        let path = graph
            .find_call_path(main_run.id, db_query.id)
            .expect("failed to find call path");

        assert!(
            path.is_some(),
            "should find path from main::run to db::query"
        );
        let path = path.expect("path should exist");

        // Path should be: main::run -> (auth::validate OR cache::get) -> db::query
        assert_eq!(path.symbols().len(), 3, "path should have 3 symbols");
        assert_eq!(path.symbols()[0].qualified_name, "main::run");
        assert_eq!(path.symbols()[2].qualified_name, "db::query");
    }

    #[test]
    fn find_call_path_returns_none_for_unconnected() {
        let (_dir, db_path) = setup_test_graph();
        let graph = SqlSymbolGraph::new(&db_path).expect("failed to create symbol graph");
        let index = Index::open(&db_path).expect("failed to open index");

        // db::query doesn't call main::run (reverse direction)
        let db_query = index
            .get_symbol_by_qualified_name("db::query")
            .expect("failed to query db::query")
            .expect("db::query not found");
        let main_run = index
            .get_symbol_by_qualified_name("main::run")
            .expect("failed to query main::run")
            .expect("main::run not found");

        let path = graph
            .find_call_path(db_query.id, main_run.id)
            .expect("failed to find call path");

        assert!(path.is_none(), "should not find path in reverse direction");
    }

    #[test]
    fn find_call_path_same_symbol_returns_single_node() {
        let (_dir, db_path) = setup_test_graph();
        let graph = SqlSymbolGraph::new(&db_path).expect("failed to create symbol graph");
        let index = Index::open(&db_path).expect("failed to open index");

        let main_run = index
            .get_symbol_by_qualified_name("main::run")
            .expect("failed to query main::run")
            .expect("main::run not found");

        let path = graph
            .find_call_path(main_run.id, main_run.id)
            .expect("failed to find call path");

        assert!(path.is_some());
        let path = path.expect("path should exist");
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
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let db_path = dir.path().join("test.db");
        let mut index = Index::open(&db_path).expect("failed to open index");

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
            .expect("failed to index main.rs");
        let auth_id = index
            .index_file_atomic(
                std::path::Path::new("src/auth.rs"),
                Language::Rust,
                1000,
                100,
                None,
                &[],
            )
            .expect("failed to index auth.rs");
        let cache_id = index
            .index_file_atomic(
                std::path::Path::new("src/cache.rs"),
                Language::Rust,
                1000,
                100,
                None,
                &[],
            )
            .expect("failed to index cache.rs");
        let db_id = index
            .index_file_atomic(
                std::path::Path::new("src/db.rs"),
                Language::Rust,
                1000,
                100,
                None,
                &[],
            )
            .expect("failed to index db.rs");

        // Set up dependencies (from_file depends on to_file)
        index
            .insert_file_dependency(main_id, auth_id)
            .expect("failed to insert main->auth dep");
        index
            .insert_file_dependency(main_id, cache_id)
            .expect("failed to insert main->cache dep");
        index
            .insert_file_dependency(auth_id, db_id)
            .expect("failed to insert auth->db dep");
        index
            .insert_file_dependency(cache_id, db_id)
            .expect("failed to insert cache->db dep");

        (dir, db_path)
    }

    #[test]
    fn file_graph_get_dependents_returns_direct() {
        let (_dir, db_path) = setup_file_deps_graph();
        let graph = SqlFileGraph::new(&db_path).expect("failed to create file graph");
        let index = Index::open(&db_path).expect("failed to open index");

        let db_id = index
            .get_file_id(std::path::Path::new("src/db.rs"))
            .expect("failed to query db.rs")
            .expect("db.rs not found");
        let dependents = graph
            .get_dependents(db_id)
            .expect("failed to get dependents");

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
        let graph = SqlFileGraph::new(&db_path).expect("failed to create file graph");
        let index = Index::open(&db_path).expect("failed to open index");

        let main_id = index
            .get_file_id(std::path::Path::new("src/main.rs"))
            .expect("failed to query main.rs")
            .expect("main.rs not found");
        let dependencies = graph
            .get_dependencies(main_id)
            .expect("failed to get dependencies");

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
        let graph = SqlFileGraph::new(&db_path).expect("failed to create file graph");
        let index = Index::open(&db_path).expect("failed to open index");

        let db_id = index
            .get_file_id(std::path::Path::new("src/db.rs"))
            .expect("failed to query db.rs")
            .expect("db.rs not found");
        let impact = graph
            .get_transitive_dependents(db_id, None)
            .expect("failed to get transitive dependents");

        // db.rs: direct deps = auth.rs, cache.rs; transitive = main.rs
        assert_eq!(impact.direct_dependents.len(), 2);
        assert_eq!(impact.transitive_dependents.len(), 1);
        assert_eq!(impact.total_dependent_count(), 3);
    }

    #[test]
    fn file_graph_get_transitive_dependents_respects_max_depth() {
        let (_dir, db_path) = setup_file_deps_graph();
        let graph = SqlFileGraph::new(&db_path).expect("failed to create file graph");
        let index = Index::open(&db_path).expect("failed to open index");

        let db_id = index
            .get_file_id(std::path::Path::new("src/db.rs"))
            .expect("failed to query db.rs")
            .expect("db.rs not found");
        let impact = graph
            .get_transitive_dependents(db_id, Some(1))
            .expect("failed to get transitive dependents");

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
        let graph = SqlFileGraph::new(&db_path).expect("failed to create file graph");
        let index = Index::open(&db_path).expect("failed to open index");

        let main_id = index
            .get_file_id(std::path::Path::new("src/main.rs"))
            .expect("failed to query main.rs")
            .expect("main.rs not found");
        let db_id = index
            .get_file_id(std::path::Path::new("src/db.rs"))
            .expect("failed to query db.rs")
            .expect("db.rs not found");

        let path = graph
            .find_dependency_path(main_id, db_id)
            .expect("failed to find dependency path");

        assert!(path.is_some(), "should find path from main.rs to db.rs");
        let path = path.expect("path should exist");

        // Path should be: main.rs -> (auth.rs OR cache.rs) -> db.rs
        assert_eq!(path.files().len(), 3, "path should have 3 files");
        assert!(path.files()[0].path.to_string_lossy().contains("main"));
        assert!(path.files()[2].path.to_string_lossy().contains("db"));
    }

    #[test]
    fn file_graph_find_dependency_path_returns_none_for_unconnected() {
        let (_dir, db_path) = setup_file_deps_graph();
        let graph = SqlFileGraph::new(&db_path).expect("failed to create file graph");
        let index = Index::open(&db_path).expect("failed to open index");

        // db.rs doesn't depend on main.rs (reverse direction)
        let db_id = index
            .get_file_id(std::path::Path::new("src/db.rs"))
            .expect("failed to query db.rs")
            .expect("db.rs not found");
        let main_id = index
            .get_file_id(std::path::Path::new("src/main.rs"))
            .expect("failed to query main.rs")
            .expect("main.rs not found");

        let path = graph
            .find_dependency_path(db_id, main_id)
            .expect("failed to find dependency path");

        assert!(path.is_none(), "should not find path in reverse direction");
    }

    #[test]
    fn file_graph_find_dependency_path_same_file_returns_single_node() {
        let (_dir, db_path) = setup_file_deps_graph();
        let graph = SqlFileGraph::new(&db_path).expect("failed to create file graph");
        let index = Index::open(&db_path).expect("failed to open index");

        let main_id = index
            .get_file_id(std::path::Path::new("src/main.rs"))
            .expect("failed to query main.rs")
            .expect("main.rs not found");

        let path = graph
            .find_dependency_path(main_id, main_id)
            .expect("failed to find dependency path");

        assert!(path.is_some());
        let path = path.expect("path should exist");
        assert_eq!(path.files().len(), 1);
        assert!(path.files()[0].path.to_string_lossy().contains("main"));
    }

    // === Cycle Detection Tests ===

    /// Create a test database with a simple A -> B -> A cycle.
    fn setup_simple_cycle_graph() -> (TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let db_path = dir.path().join("test.db");
        let mut index = Index::open(&db_path).expect("failed to open test database");

        // Create files: a.rs <-> b.rs (mutual dependency = cycle)
        let a_id = index
            .index_file_atomic(
                std::path::Path::new("src/a.rs"),
                Language::Rust,
                1000,
                100,
                None,
                &[],
            )
            .expect("failed to index a.rs");
        let b_id = index
            .index_file_atomic(
                std::path::Path::new("src/b.rs"),
                Language::Rust,
                1000,
                100,
                None,
                &[],
            )
            .expect("failed to index b.rs");

        // A depends on B, B depends on A
        index
            .insert_file_dependency(a_id, b_id)
            .expect("failed to insert a->b dep");
        index
            .insert_file_dependency(b_id, a_id)
            .expect("failed to insert b->a dep");

        (dir, db_path)
    }

    /// Create a test database with a longer A -> B -> C -> A cycle.
    fn setup_three_node_cycle_graph() -> (TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let db_path = dir.path().join("test.db");
        let mut index = Index::open(&db_path).expect("failed to open test database");

        // Create files: a.rs -> b.rs -> c.rs -> a.rs
        let a_id = index
            .index_file_atomic(
                std::path::Path::new("src/a.rs"),
                Language::Rust,
                1000,
                100,
                None,
                &[],
            )
            .expect("failed to index a.rs");
        let b_id = index
            .index_file_atomic(
                std::path::Path::new("src/b.rs"),
                Language::Rust,
                1000,
                100,
                None,
                &[],
            )
            .expect("failed to index b.rs");
        let c_id = index
            .index_file_atomic(
                std::path::Path::new("src/c.rs"),
                Language::Rust,
                1000,
                100,
                None,
                &[],
            )
            .expect("failed to index c.rs");

        // A -> B -> C -> A
        index
            .insert_file_dependency(a_id, b_id)
            .expect("failed to insert a->b dep");
        index
            .insert_file_dependency(b_id, c_id)
            .expect("failed to insert b->c dep");
        index
            .insert_file_dependency(c_id, a_id)
            .expect("failed to insert c->a dep");

        (dir, db_path)
    }

    /// Create a test database with a self-referential file (imports itself).
    fn setup_self_loop_graph() -> (TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let db_path = dir.path().join("test.db");
        let mut index = Index::open(&db_path).expect("failed to open test database");

        let a_id = index
            .index_file_atomic(
                std::path::Path::new("src/self_ref.rs"),
                Language::Rust,
                1000,
                100,
                None,
                &[],
            )
            .expect("failed to index self_ref.rs");

        // Self-dependency
        index
            .insert_file_dependency(a_id, a_id)
            .expect("failed to insert self-dependency");

        (dir, db_path)
    }

    #[test]
    fn detect_cycles_finds_simple_two_node_cycle() {
        let (_dir, db_path) = setup_simple_cycle_graph();
        let graph = SqlFileGraph::new(&db_path).expect("failed to create file graph");

        let cycles = graph.detect_cycles().expect("failed to detect cycles");

        assert_eq!(cycles.len(), 1, "should find exactly one cycle");

        let cycle = &cycles[0];
        assert_eq!(cycle.files.len(), 2, "cycle should contain 2 files");

        // Both files should be present
        let paths: Vec<String> = cycle
            .files
            .iter()
            .map(|p| p.display().to_string())
            .collect();
        assert!(
            paths.iter().any(|p| p.contains("a.rs")),
            "cycle should contain a.rs"
        );
        assert!(
            paths.iter().any(|p| p.contains("b.rs")),
            "cycle should contain b.rs"
        );
    }

    #[test]
    fn detect_cycles_finds_three_node_cycle() {
        let (_dir, db_path) = setup_three_node_cycle_graph();
        let graph = SqlFileGraph::new(&db_path).expect("failed to create file graph");

        let cycles = graph.detect_cycles().expect("failed to detect cycles");

        assert_eq!(cycles.len(), 1, "should find exactly one cycle");

        let cycle = &cycles[0];
        assert_eq!(cycle.files.len(), 3, "cycle should contain 3 files");

        let paths: Vec<String> = cycle
            .files
            .iter()
            .map(|p| p.display().to_string())
            .collect();
        assert!(
            paths.iter().any(|p| p.contains("a.rs")),
            "cycle should contain a.rs"
        );
        assert!(
            paths.iter().any(|p| p.contains("b.rs")),
            "cycle should contain b.rs"
        );
        assert!(
            paths.iter().any(|p| p.contains("c.rs")),
            "cycle should contain c.rs"
        );
    }

    #[test]
    fn detect_cycles_finds_self_loop() {
        let (_dir, db_path) = setup_self_loop_graph();
        let graph = SqlFileGraph::new(&db_path).expect("failed to create file graph");

        let cycles = graph.detect_cycles().expect("failed to detect cycles");

        assert_eq!(cycles.len(), 1, "should find exactly one cycle");

        let cycle = &cycles[0];
        assert_eq!(
            cycle.files.len(),
            1,
            "self-loop cycle should contain 1 file"
        );
        assert!(
            cycle.files[0].display().to_string().contains("self_ref.rs"),
            "cycle should contain self_ref.rs"
        );
    }

    #[test]
    fn detect_cycles_returns_empty_for_acyclic_graph() {
        // The setup_file_deps_graph creates an acyclic graph:
        // main.rs -> auth.rs -> db.rs
        //         -> cache.rs -> db.rs
        let (_dir, db_path) = setup_file_deps_graph();
        let graph = SqlFileGraph::new(&db_path).expect("failed to create file graph");

        let cycles = graph.detect_cycles().expect("failed to detect cycles");

        assert!(cycles.is_empty(), "acyclic graph should have no cycles");
    }

    #[test]
    fn detect_cycles_involving_filters_to_specified_file() {
        let (_dir, db_path) = setup_simple_cycle_graph();
        let graph = SqlFileGraph::new(&db_path).expect("failed to create file graph");
        let index = Index::open(&db_path).expect("failed to open index");

        let a_id = index
            .get_file_id(std::path::Path::new("src/a.rs"))
            .expect("failed to query a.rs")
            .expect("a.rs not found");

        let cycles = graph
            .detect_cycles_involving(a_id)
            .expect("failed to detect cycles");

        assert_eq!(cycles.len(), 1, "should find the cycle involving a.rs");
    }

    #[test]
    fn detect_cycles_involving_returns_empty_for_file_not_in_cycle() {
        // Use the acyclic graph - no file is in a cycle
        let (_dir, db_path) = setup_file_deps_graph();
        let graph = SqlFileGraph::new(&db_path).expect("failed to create file graph");
        let index = Index::open(&db_path).expect("failed to open index");

        let main_id = index
            .get_file_id(std::path::Path::new("src/main.rs"))
            .expect("failed to query main.rs")
            .expect("main.rs not found");

        let cycles = graph
            .detect_cycles_involving(main_id)
            .expect("failed to detect cycles");

        assert!(cycles.is_empty(), "file not in a cycle should return empty");
    }

    #[test]
    fn detect_cycles_handles_multiple_cycles() {
        // Create a graph with two independent cycles
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let db_path = dir.path().join("test.db");
        let mut index = Index::open(&db_path).expect("failed to open test database");

        // Cycle 1: a.rs <-> b.rs
        let a_id = index
            .index_file_atomic(
                std::path::Path::new("src/a.rs"),
                Language::Rust,
                1000,
                100,
                None,
                &[],
            )
            .expect("failed to index a.rs");
        let b_id = index
            .index_file_atomic(
                std::path::Path::new("src/b.rs"),
                Language::Rust,
                1000,
                100,
                None,
                &[],
            )
            .expect("failed to index b.rs");

        // Cycle 2: x.rs <-> y.rs
        let x_id = index
            .index_file_atomic(
                std::path::Path::new("src/x.rs"),
                Language::Rust,
                1000,
                100,
                None,
                &[],
            )
            .expect("failed to index x.rs");
        let y_id = index
            .index_file_atomic(
                std::path::Path::new("src/y.rs"),
                Language::Rust,
                1000,
                100,
                None,
                &[],
            )
            .expect("failed to index y.rs");

        // Set up both cycles
        index
            .insert_file_dependency(a_id, b_id)
            .expect("failed to insert a->b dep");
        index
            .insert_file_dependency(b_id, a_id)
            .expect("failed to insert b->a dep");
        index
            .insert_file_dependency(x_id, y_id)
            .expect("failed to insert x->y dep");
        index
            .insert_file_dependency(y_id, x_id)
            .expect("failed to insert y->x dep");

        let graph = SqlFileGraph::new(&db_path).expect("failed to create file graph");
        let cycles = graph.detect_cycles().expect("failed to detect cycles");

        assert_eq!(cycles.len(), 2, "should find both cycles");

        // Verify both cycles are represented
        let all_files: std::collections::HashSet<String> = cycles
            .iter()
            .flat_map(|c| c.files.iter().map(|p| p.display().to_string()))
            .collect();

        assert!(all_files.iter().any(|p| p.contains("a.rs")));
        assert!(all_files.iter().any(|p| p.contains("b.rs")));
        assert!(all_files.iter().any(|p| p.contains("x.rs")));
        assert!(all_files.iter().any(|p| p.contains("y.rs")));
    }

    #[test]
    fn detect_cycles_deduplicates_same_cycle() {
        // A -> B -> A forms one cycle, not two
        let (_dir, db_path) = setup_simple_cycle_graph();
        let graph = SqlFileGraph::new(&db_path).expect("failed to create file graph");

        let cycles = graph.detect_cycles().expect("failed to detect cycles");

        // The A->B->A cycle should only be reported once, not twice
        // (once starting from A, once starting from B)
        assert_eq!(cycles.len(), 1, "same cycle should only be reported once");

        // Verify the cycle contains both files (normalization correctness)
        let cycle = &cycles[0];
        assert_eq!(cycle.files.len(), 2, "cycle should contain 2 files");

        let paths: Vec<String> = cycle
            .files
            .iter()
            .map(|p| p.display().to_string())
            .collect();
        assert!(
            paths.iter().any(|p| p.contains("a.rs")),
            "cycle should contain a.rs"
        );
        assert!(
            paths.iter().any(|p| p.contains("b.rs")),
            "cycle should contain b.rs"
        );
    }

    // === Helper function tests ===

    #[test]
    fn normalize_cycle_rotates_to_smallest() {
        // Test the normalization function directly
        let cycle = vec![FileId::from(5), FileId::from(2), FileId::from(8)];
        let normalized = super::normalize_cycle(&cycle);

        // Should rotate so 2 (smallest) is first
        assert_eq!(normalized[0].as_i64(), 2);
        assert_eq!(normalized[1].as_i64(), 8);
        assert_eq!(normalized[2].as_i64(), 5);
    }

    #[test]
    fn normalize_cycle_handles_empty() {
        let cycle: Vec<FileId> = vec![];
        let normalized = super::normalize_cycle(&cycle);
        assert!(normalized.is_empty());
    }

    #[test]
    fn normalize_cycle_handles_single_element() {
        let cycle = vec![FileId::from(42)];
        let normalized = super::normalize_cycle(&cycle);
        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized[0].as_i64(), 42);
    }

    #[test]
    fn detect_cycles_handles_completely_empty_database() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let db_path = dir.path().join("test.db");

        // Create database but don't index any files
        let _index = Index::open(&db_path).expect("failed to create database");

        let graph = SqlFileGraph::new(&db_path).expect("failed to create file graph");
        let cycles = graph
            .detect_cycles()
            .expect("should succeed on empty database");

        assert!(cycles.is_empty(), "empty database should have no cycles");
    }

    #[test]
    fn detect_cycles_involving_returns_error_for_nonexistent_file() {
        let (_dir, db_path) = setup_simple_cycle_graph();
        let graph = SqlFileGraph::new(&db_path).expect("failed to create file graph");

        // Use a file ID that doesn't exist
        let nonexistent_id = FileId::from(99999);
        let result = graph.detect_cycles_involving(nonexistent_id);

        assert!(
            result.is_err(),
            "should return error for non-existent file, not empty vec"
        );
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("not found") || err.contains("Not found"),
            "error should indicate file not found: {err}"
        );
    }

    #[test]
    fn detect_cycles_handles_large_cycle() {
        const CYCLE_SIZE: usize = 100;

        // Create a cycle with 100 files: file0 -> file1 -> ... -> file99 -> file0
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let db_path = dir.path().join("test.db");
        let mut index = Index::open(&db_path).expect("failed to open test database");

        let mut file_ids = Vec::with_capacity(CYCLE_SIZE);

        for i in 0..CYCLE_SIZE {
            let id = index
                .index_file_atomic(
                    std::path::Path::new(&format!("src/file{i}.rs")),
                    Language::Rust,
                    1000,
                    100,
                    None,
                    &[],
                )
                .expect("failed to index file");
            file_ids.push(id);
        }

        // Create cycle: 0 -> 1 -> 2 -> ... -> 99 -> 0
        for i in 0..CYCLE_SIZE {
            let next = (i + 1) % CYCLE_SIZE;
            index
                .insert_file_dependency(file_ids[i], file_ids[next])
                .expect("failed to insert dependency");
        }

        let graph = SqlFileGraph::new(&db_path).expect("failed to create file graph");
        let cycles = graph
            .detect_cycles()
            .expect("should handle large cycle without stack overflow");

        assert_eq!(cycles.len(), 1, "should find exactly one large cycle");
        assert_eq!(
            cycles[0].files.len(),
            CYCLE_SIZE,
            "cycle should contain all {CYCLE_SIZE} files"
        );
    }
}
