//! Call edge bulk operations for the Tethys index.
//!
//! Call edges are pre-computed from the refs table for fast graph queries.
//! They represent "who calls what" at the symbol level.

use std::collections::{HashMap, HashSet};

use rusqlite::params;
use tracing::{trace, warn};

use super::Index;
use crate::error::Result;
use crate::types::FileId;

/// Pseudo-crate prefix used to bucket orphan files (those outside any
/// Cargo-known crate) for the K-hybrid filter in
/// [`Index::populate_file_deps_from_call_edges`]. The full pseudo-crate
/// name is `orphan:<top-level-directory>`. Exposed as a `pub(crate)`
/// constant so the indexer's `file_crate_map` builder and the K-hybrid
/// unit tests agree on the format without duplicating the literal.
pub(crate) const ORPHAN_PSEUDO_CRATE_PREFIX: &str = "orphan:";

impl Index {
    /// Clear all call edges before a full rebuild.
    ///
    /// Must run *after* all resolution passes because
    /// [`Index::populate_call_edges`] derives edges from the
    /// post-resolution `refs` table. Counterpart to
    /// [`Index::clear_all_file_deps`], which runs *before* per-file
    /// processing — the two clears are not interchangeable in ordering.
    pub fn clear_all_call_edges(&self) -> Result<()> {
        trace!("Clearing all call edges");
        let conn = self.connection()?;

        conn.execute("DELETE FROM call_edges", [])?;
        Ok(())
    }

    /// Populate call edges from the refs table.
    ///
    /// Scans all references where both `in_symbol_id` (caller) and `symbol_id` (callee)
    /// are resolved, and populates the `call_edges` table. This should be called after
    /// all reference resolution passes (Pass 1, Pass 2, and optionally Pass 3) are complete.
    ///
    /// Returns the number of edges inserted.
    pub fn populate_call_edges(&self) -> Result<usize> {
        trace!("Populating call edges from refs table");
        let conn = self.connection()?;

        // Insert aggregated edges from refs table
        // ON CONFLICT handles duplicates by adding to call_count
        let inserted = conn.execute(
            "INSERT INTO call_edges (caller_symbol_id, callee_symbol_id, call_count)
             SELECT in_symbol_id, symbol_id, COUNT(*) as call_count
             FROM refs
             WHERE in_symbol_id IS NOT NULL AND symbol_id IS NOT NULL
             GROUP BY in_symbol_id, symbol_id
             ON CONFLICT(caller_symbol_id, callee_symbol_id) DO UPDATE SET
                 call_count = call_edges.call_count + excluded.call_count",
            [],
        )?;

        trace!(edges_inserted = inserted, "Populated call edges");

        Ok(inserted)
    }

    /// Populate file-level dependencies from call edges, filtered by import
    /// corroboration for cross-crate edges (rivets-3d0s K-hybrid).
    ///
    /// Aggregates `call_edges` into `file_deps` with these rules:
    /// - **Intra-crate** call edges (caller file's crate == callee file's
    ///   crate) ALWAYS contribute a `file_deps` row.
    /// - **Cross-crate** call edges contribute IFF the caller file has at
    ///   least one import whose first segment matches the callee file's
    ///   crate's Rust-namespace name (e.g., `use crate_b::Foo` corroborates
    ///   a cross-edge into `crate_b`). This filters out phantom resolutions
    ///   where the resolver collapsed a method call (e.g., `.len()`) to
    ///   a workspace-named symbol the caller never explicitly imported.
    ///
    /// `file_crate_map` maps each `FileId` to its crate name. Cargo-known
    /// files use the canonical crate name; orphan files (outside any
    /// `Cargo.toml`-known crate) use the pseudo-crate name prefixed with
    /// [`ORPHAN_PSEUDO_CRATE_PREFIX`] so they participate in the filter
    /// consistently. Callers MUST populate every `FileId` referenced by
    /// `call_edges`; missing entries fall into a conservative
    /// keep-the-edge branch with a `warn!` log, not silent acceptance.
    ///
    /// Uses upsert semantics to merge with `file_deps` already inserted from
    /// import statements during the per-file processing phase.
    ///
    /// Returns the count of `file_deps` rows inserted or updated.
    ///
    /// **API stability:** the `file_crate_map` parameter is a workspace-
    /// internal contract. The function is `pub` (rather than `pub(crate)`)
    /// because tethys's integration tests live in a separate test binary
    /// per Rust convention, but external consumers outside the tethys
    /// crate should treat the signature as unstable.
    pub fn populate_file_deps_from_call_edges(
        &self,
        file_crate_map: &HashMap<FileId, String>,
    ) -> Result<usize> {
        trace!("Populating file deps from call edges (K-hybrid filter)");

        // Aggregate call_edges into (caller_file_id, callee_file_id, ref_count).
        // Scoped so the connection guard releases before the helper below
        // calls `self.connection()` again — `std::sync::Mutex` is not
        // re-entrant on the same thread.
        let aggregated: Vec<(i64, i64, i64)> = {
            let conn = self.connection()?;
            conn.prepare(
                "SELECT s1.file_id, s2.file_id, SUM(ce.call_count)
                 FROM call_edges ce
                 JOIN symbols s1 ON ce.caller_symbol_id = s1.id
                 JOIN symbols s2 ON ce.callee_symbol_id = s2.id
                 WHERE s1.file_id != s2.file_id
                 GROUP BY s1.file_id, s2.file_id",
            )?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?
        };

        // Build per-file set of workspace-crate names the file imports from.
        // Acquires and releases the connection guard internally.
        let imports_per_file = self.build_imports_per_file_crate(file_crate_map)?;

        // Re-acquire the connection for the inserts.
        let conn = self.connection()?;
        let mut inserted = 0usize;
        let mut dropped = 0usize;
        for (from_fid_i64, to_fid_i64, ref_count) in aggregated {
            let from_file = FileId::from(from_fid_i64);
            let to_file = FileId::from(to_fid_i64);
            let from_crate = file_crate_map.get(&from_file);
            let to_crate = file_crate_map.get(&to_file);

            let keep = match (from_crate, to_crate) {
                (Some(a), Some(b)) if a == b => true,
                (Some(_), Some(b)) => imports_per_file
                    .get(&from_file)
                    .is_some_and(|imports| imports.contains(b)),
                _ => {
                    warn!(
                        from_file_id = from_fid_i64,
                        to_file_id = to_fid_i64,
                        from_crate_missing = from_crate.is_none(),
                        to_crate_missing = to_crate.is_none(),
                        "K-hybrid filter: file missing from crate map, keeping edge conservatively"
                    );
                    true
                }
            };
            if keep {
                conn.execute(
                    "INSERT INTO file_deps (from_file_id, to_file_id, ref_count)
                     VALUES (?1, ?2, ?3)
                     ON CONFLICT(from_file_id, to_file_id) DO UPDATE SET
                         ref_count = file_deps.ref_count + excluded.ref_count",
                    params![from_fid_i64, to_fid_i64, ref_count],
                )?;
                inserted += 1;
            } else {
                dropped += 1;
            }
        }

        trace!(
            file_deps_inserted = inserted,
            file_deps_dropped_by_k_hybrid = dropped,
            "Populated file deps from call edges (K-hybrid filter applied)"
        );

        Ok(inserted)
    }

    /// Build a map of `FileId` -> set of workspace crate names the file imports from.
    ///
    /// Parses the first path segment of each row's `source_module` in the
    /// `imports` table and matches it against known crate names from
    /// `file_crate_map`'s values, with `_` -> `-` normalization (Rust uses
    /// underscores in module paths while Cargo crate names often use
    /// hyphens — e.g., `use rivets_jsonl::*` corroborates an edge into
    /// the `rivets-jsonl` crate). Imports whose first segment doesn't match
    /// any workspace crate (e.g., `std::*`, `serde::*`) are ignored — they
    /// cannot corroborate any cross-workspace-crate edge.
    ///
    /// First-segment extraction handles both Rust (`::`-separated, e.g.
    /// `tethys::db::Index`) and C# (`.`-separated, e.g. `MyApp.Services`)
    /// import path syntax. Today tethys treats all C# files as one
    /// pseudo-crate per top-level directory (no `.csproj` discovery), so
    /// the C# handling is mainly future-proofing for when multi-project
    /// C# workspace support lands.
    fn build_imports_per_file_crate(
        &self,
        file_crate_map: &HashMap<FileId, String>,
    ) -> Result<HashMap<FileId, HashSet<String>>> {
        let known_crates: HashSet<&str> = file_crate_map.values().map(String::as_str).collect();
        let conn = self.connection()?;
        let rows = conn
            .prepare("SELECT file_id, source_module FROM imports")?
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut result: HashMap<FileId, HashSet<String>> = HashMap::new();
        for (file_id_i64, source_module) in rows {
            let first = first_path_segment(&source_module);
            if first.is_empty() {
                continue;
            }
            let crate_name = if known_crates.contains(first) {
                first.to_string()
            } else {
                let dashed = first.replace('_', "-");
                if known_crates.contains(dashed.as_str()) {
                    dashed
                } else {
                    continue;
                }
            };
            result
                .entry(FileId::from(file_id_i64))
                .or_default()
                .insert(crate_name);
        }
        Ok(result)
    }
}

/// Extract the first path segment from an import's `source_module`.
///
/// Handles both Rust (`::`-separated, e.g. `tethys::db::Index`) and C#
/// (`.`-separated, e.g. `MyApp.Services`) syntax by splitting at whichever
/// separator appears first. Returns the whole string when neither
/// separator is present.
fn first_path_segment(s: &str) -> &str {
    let pos_colons = s.find("::");
    let pos_dot = s.find('.');
    let pos = match (pos_colons, pos_dot) {
        (None, None) => return s,
        (Some(c), None) => c,
        (None, Some(d)) => d,
        (Some(c), Some(d)) => c.min(d),
    };
    &s[..pos]
}

#[cfg(test)]
mod k_hybrid_filter_tests {
    use super::*;
    use crate::db::Index;
    use crate::db::symbols::InsertSymbolParams;
    use crate::types::{Language, SymbolId, SymbolKind, Visibility};
    use std::path::Path;
    use tempfile::TempDir;

    fn fresh_index() -> (TempDir, Index) {
        let dir = tempfile::tempdir().expect("temp");
        let index = Index::open(&dir.path().join("idx.db")).expect("open");
        (dir, index)
    }

    fn upsert(index: &mut Index, p: &str) -> FileId {
        index
            .upsert_file(Path::new(p), Language::Rust, 0, 0, None)
            .expect("file")
    }

    fn insert_sym(index: &mut Index, file_id: FileId, name: &str, kind: SymbolKind) -> SymbolId {
        index
            .insert_symbol(&InsertSymbolParams {
                file_id,
                name,
                module_path: "",
                qualified_name: name,
                kind,
                line: 1,
                column: 1,
                span: None,
                signature: None,
                visibility: Visibility::Public,
                parent_symbol_id: None,
                is_test: false,
            })
            .expect("symbol")
    }

    // Takes `&Index` (not `&mut`) because `connection()` is `&self` —
    // call_edges has no high-level insert helper, so the test inserts
    // directly via the raw SQLite connection. This intentional asymmetry
    // with `upsert`/`insert_sym` reflects the underlying API surface.
    fn insert_call_edge(index: &Index, from_sym: SymbolId, to_sym: SymbolId) {
        index
            .connection()
            .expect("conn")
            .execute(
                "INSERT INTO call_edges (caller_symbol_id, callee_symbol_id, call_count)
                 VALUES (?1, ?2, 1)",
                params![from_sym.as_i64(), to_sym.as_i64()],
            )
            .expect("insert call edge");
    }

    fn count_file_deps(index: &Index, from: FileId, to: FileId) -> i64 {
        index
            .connection()
            .expect("conn")
            .query_row(
                "SELECT COUNT(*) FROM file_deps WHERE from_file_id = ?1 AND to_file_id = ?2",
                params![from.as_i64(), to.as_i64()],
                |row| row.get(0),
            )
            .expect("count")
    }

    /// Stress fixture from `plan-v3-k-hybrid.md` slice 1. Caller in
    /// `crate_a` makes 5 call edges:
    ///
    /// - intra-crate (kept)
    /// - cross-crate to `crate_b` with corroborating import (kept)
    /// - cross-crate to `crate_c` WITHOUT import (DROPPED — the rivets-3d0s
    ///   phantom shape)
    /// - cross-pseudo-crate to `orphan:examples` (DROPPED — no import
    ///   possible)
    /// - cross-pseudo-crate to `orphan:bruno-examples` (DROPPED)
    ///
    /// Expect: exactly 2 `file_deps` rows survive (intra + corroborated
    /// cross).
    #[test]
    fn k_hybrid_keeps_intra_and_corroborated_drops_uncorroborated_and_orphan() {
        let (_dir, mut index) = fresh_index();

        let f1 = upsert(&mut index, "crates/crate_a/src/lib.rs");
        let f2 = upsert(&mut index, "crates/crate_a/src/utils.rs");
        let f3 = upsert(&mut index, "crates/crate_b/src/lib.rs");
        let f4 = upsert(&mut index, "crates/crate_c/src/lib.rs");
        let f5 = upsert(&mut index, "examples/oddball.rs");
        let f6 = upsert(&mut index, "bruno-examples/types.rs");

        let caller_fn = insert_sym(&mut index, f1, "caller_fn", SymbolKind::Function);
        let helper = insert_sym(&mut index, f2, "helper", SymbolKind::Function);
        let legit_thing = insert_sym(&mut index, f3, "legit_thing", SymbolKind::Function);
        let phantom_len = insert_sym(&mut index, f4, "len", SymbolKind::Method);
        let extract = insert_sym(&mut index, f5, "extract", SymbolKind::Function);
        let encode = insert_sym(&mut index, f6, "encode", SymbolKind::Function);

        insert_call_edge(&index, caller_fn, helper);
        insert_call_edge(&index, caller_fn, legit_thing);
        insert_call_edge(&index, caller_fn, phantom_len);
        insert_call_edge(&index, caller_fn, extract);
        insert_call_edge(&index, caller_fn, encode);

        index
            .insert_import(f1, "legit_thing", "crate_b", None)
            .expect("import");

        let mut file_crate_map: HashMap<FileId, String> = HashMap::new();
        file_crate_map.insert(f1, "crate_a".to_string());
        file_crate_map.insert(f2, "crate_a".to_string());
        file_crate_map.insert(f3, "crate_b".to_string());
        file_crate_map.insert(f4, "crate_c".to_string());
        file_crate_map.insert(f5, "orphan:examples".to_string());
        file_crate_map.insert(f6, "orphan:bruno-examples".to_string());

        let inserted = index
            .populate_file_deps_from_call_edges(&file_crate_map)
            .expect("populate");
        assert_eq!(
            inserted, 2,
            "expected 2 file_deps rows (intra-crate + cross-crate-corroborated); got {inserted}"
        );

        assert_eq!(
            count_file_deps(&index, f1, f2),
            1,
            "intra-crate must be kept"
        );
        assert_eq!(
            count_file_deps(&index, f1, f3),
            1,
            "cross-crate with corroborating import must be kept"
        );
        assert_eq!(
            count_file_deps(&index, f1, f4),
            0,
            "cross-crate WITHOUT import must be DROPPED (rivets-3d0s phantom shape)"
        );
        assert_eq!(
            count_file_deps(&index, f1, f5),
            0,
            "cross-pseudo-crate (orphan target) must be DROPPED"
        );
        assert_eq!(
            count_file_deps(&index, f1, f6),
            0,
            "cross-pseudo-crate (different orphan top-dir) must be DROPPED"
        );
    }

    /// Verifies the Rust-name to Cargo-name normalization: `use rivets_jsonl::*`
    /// (Rust path syntax with underscore) corroborates an edge into the
    /// `rivets-jsonl` crate (Cargo name with hyphen). A bug forgetting this
    /// conversion would silently drop legitimate cross-crate edges into
    /// any hyphenated crate name.
    #[test]
    fn k_hybrid_normalizes_underscore_to_hyphen_in_import_first_segment() {
        let (_dir, mut index) = fresh_index();

        let caller_file = upsert(&mut index, "crates/rivets/src/lib.rs");
        let target_file = upsert(&mut index, "crates/rivets-jsonl/src/writer.rs");

        let caller_sym = insert_sym(&mut index, caller_file, "caller_fn", SymbolKind::Function);
        let target_sym = insert_sym(&mut index, target_file, "write_jsonl", SymbolKind::Function);

        insert_call_edge(&index, caller_sym, target_sym);

        // Import uses Rust syntax: `use rivets_jsonl::writer::write_jsonl`
        index
            .insert_import(caller_file, "write_jsonl", "rivets_jsonl::writer", None)
            .expect("import");

        let mut file_crate_map: HashMap<FileId, String> = HashMap::new();
        file_crate_map.insert(caller_file, "rivets".to_string());
        file_crate_map.insert(target_file, "rivets-jsonl".to_string());

        let inserted = index
            .populate_file_deps_from_call_edges(&file_crate_map)
            .expect("populate");
        assert_eq!(
            inserted, 1,
            "hyphenated crate name must be corroborated by underscored import"
        );
        assert_eq!(count_file_deps(&index, caller_file, target_file), 1);
    }

    /// External-crate imports (`use std::collections::HashMap`, `use serde::Serialize`)
    /// do NOT corroborate workspace cross-crate edges. Verifies that a caller
    /// importing only external crates cannot satisfy the corroboration check
    /// for a workspace-crate target.
    #[test]
    fn k_hybrid_external_imports_do_not_corroborate_workspace_edges() {
        let (_dir, mut index) = fresh_index();

        let caller_file = upsert(&mut index, "crates/crate_a/src/lib.rs");
        let target_file = upsert(&mut index, "crates/crate_b/src/lib.rs");

        let caller_sym = insert_sym(&mut index, caller_file, "caller_fn", SymbolKind::Function);
        let target_sym = insert_sym(&mut index, target_file, "thing", SymbolKind::Function);

        insert_call_edge(&index, caller_sym, target_sym);

        // Only external-crate imports — no workspace corroboration
        index
            .insert_import(caller_file, "HashMap", "std::collections", None)
            .expect("import");
        index
            .insert_import(caller_file, "Serialize", "serde", None)
            .expect("import");

        let mut file_crate_map: HashMap<FileId, String> = HashMap::new();
        file_crate_map.insert(caller_file, "crate_a".to_string());
        file_crate_map.insert(target_file, "crate_b".to_string());

        let inserted = index
            .populate_file_deps_from_call_edges(&file_crate_map)
            .expect("populate");
        assert_eq!(
            inserted, 0,
            "external imports must not corroborate workspace cross-crate edges"
        );
    }

    /// Defensive branch: when a `FileId` referenced by `call_edges` is
    /// missing from `file_crate_map`, the filter falls into a conservative
    /// keep-the-edge path with a `warn!` log. This documents production
    /// behavior if the caller ever constructs an incomplete map (which
    /// shouldn't happen in normal operation since the map is built from
    /// `list_all_files`). Test the fallback explicitly so a future
    /// refactor doesn't silently flip the conservative behavior to a
    /// silent drop.
    #[test]
    fn k_hybrid_keeps_edge_conservatively_when_file_missing_from_crate_map() {
        let (_dir, mut index) = fresh_index();

        let caller_file = upsert(&mut index, "crates/crate_a/src/lib.rs");
        let target_file = upsert(&mut index, "crates/crate_b/src/lib.rs");

        let caller_sym = insert_sym(&mut index, caller_file, "caller_fn", SymbolKind::Function);
        let target_sym = insert_sym(&mut index, target_file, "thing", SymbolKind::Function);

        insert_call_edge(&index, caller_sym, target_sym);

        // Deliberately incomplete file_crate_map: target_file omitted.
        let mut file_crate_map: HashMap<FileId, String> = HashMap::new();
        file_crate_map.insert(caller_file, "crate_a".to_string());

        let inserted = index
            .populate_file_deps_from_call_edges(&file_crate_map)
            .expect("populate");
        assert_eq!(
            inserted, 1,
            "missing-entry must take the conservative-keep branch, not silently drop"
        );
        assert_eq!(
            count_file_deps(&index, caller_file, target_file),
            1,
            "the kept edge must actually appear in file_deps"
        );
    }

    /// Edge case: when the `imports` table has zero rows (e.g., a workspace
    /// of pure data crates with no `use` statements), every cross-crate
    /// call edge must be dropped (no import can corroborate). Intra-crate
    /// edges are unaffected.
    #[test]
    fn k_hybrid_empty_imports_table_drops_all_cross_crate_keeps_intra() {
        let (_dir, mut index) = fresh_index();

        let crate_a_caller = upsert(&mut index, "crates/crate_a/src/lib.rs");
        let crate_a_target = upsert(&mut index, "crates/crate_a/src/helper.rs");
        let crate_b_target = upsert(&mut index, "crates/crate_b/src/lib.rs");

        let caller_sym = insert_sym(
            &mut index,
            crate_a_caller,
            "caller_fn",
            SymbolKind::Function,
        );
        let helper_sym = insert_sym(&mut index, crate_a_target, "helper", SymbolKind::Function);
        let cross_sym = insert_sym(&mut index, crate_b_target, "cross", SymbolKind::Function);

        insert_call_edge(&index, caller_sym, helper_sym);
        insert_call_edge(&index, caller_sym, cross_sym);

        // Note: no insert_import calls. imports table stays empty.

        let mut file_crate_map: HashMap<FileId, String> = HashMap::new();
        file_crate_map.insert(crate_a_caller, "crate_a".to_string());
        file_crate_map.insert(crate_a_target, "crate_a".to_string());
        file_crate_map.insert(crate_b_target, "crate_b".to_string());

        let inserted = index
            .populate_file_deps_from_call_edges(&file_crate_map)
            .expect("populate");
        assert_eq!(
            inserted, 1,
            "with no imports, only the intra-crate edge must survive"
        );
        assert_eq!(
            count_file_deps(&index, crate_a_caller, crate_a_target),
            1,
            "intra-crate edge must be kept regardless of imports table state"
        );
        assert_eq!(
            count_file_deps(&index, crate_a_caller, crate_b_target),
            0,
            "cross-crate edge must be dropped when no import can corroborate it"
        );
    }
}
