//! Call edge CRUD operations for the Tethys index.
//!
//! Call edges are pre-computed from the refs table for fast graph queries.
//! They represent "who calls what" at the symbol level.

use tracing::trace;

use super::Index;
use crate::error::Result;

impl Index {
    /// Clear all call edges (for full rebuild).
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

    /// Populate file-level dependencies from call edges.
    ///
    /// Derives file dependencies by aggregating call edges - if symbol A in file X
    /// calls symbol B in file Y, then file X depends on file Y.
    ///
    /// This captures actual function calls, not just explicit imports. Uses upsert
    /// semantics to merge with any existing file deps from import statements.
    ///
    /// Returns the number of file dependency edges inserted or updated.
    pub fn populate_file_deps_from_call_edges(&self) -> Result<usize> {
        trace!("Populating file deps from call edges");
        let conn = self.connection()?;

        // Aggregate call edges into file-level dependencies
        // JOIN symbols twice to get file_id for both caller and callee
        // Exclude same-file calls (s1.file_id != s2.file_id)
        let inserted = conn.execute(
            "INSERT INTO file_deps (from_file_id, to_file_id, ref_count)
             SELECT s1.file_id, s2.file_id, SUM(ce.call_count)
             FROM call_edges ce
             JOIN symbols s1 ON ce.caller_symbol_id = s1.id
             JOIN symbols s2 ON ce.callee_symbol_id = s2.id
             WHERE s1.file_id != s2.file_id
             GROUP BY s1.file_id, s2.file_id
             ON CONFLICT(from_file_id, to_file_id) DO UPDATE SET
                 ref_count = file_deps.ref_count + excluded.ref_count",
            [],
        )?;

        trace!(
            file_deps_inserted = inserted,
            "Populated file deps from call edges"
        );

        Ok(inserted)
    }
}
