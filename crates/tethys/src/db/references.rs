//! Reference CRUD operations for the Tethys index.
//!
//! These operations support symbol-level "who calls X?" queries.
//! See graph module for higher-level graph traversal using these primitives.

use std::path::PathBuf;

use rusqlite::params;
use tracing::trace;

use super::{row_to_reference, Index, REFS_COLUMNS};
use crate::error::Result;
use crate::types::{FileId, RefId, Reference, SymbolId};

impl Index {
    /// Insert a reference to a symbol.
    ///
    /// If `symbol_id` is `None`, the reference is unresolved and `reference_name`
    /// should be provided for later resolution in Pass 2.
    #[allow(clippy::too_many_arguments)]
    pub fn insert_reference(
        &self,
        symbol_id: Option<SymbolId>,
        file_id: FileId,
        kind: &str,
        line: u32,
        column: u32,
        in_symbol_id: Option<SymbolId>,
        reference_name: Option<&str>,
    ) -> Result<i64> {
        let conn = self.connection()?;

        conn.execute(
            "INSERT INTO refs (symbol_id, file_id, kind, line, column, in_symbol_id, reference_name)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                symbol_id.map(SymbolId::as_i64),
                file_id.as_i64(),
                kind,
                line,
                column,
                in_symbol_id.map(SymbolId::as_i64),
                reference_name
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Get all unresolved references (where `symbol_id` is NULL).
    ///
    /// These references need to be resolved in Pass 2 by matching their
    /// `reference_name` to symbols discovered in other files.
    #[allow(dead_code)] // Public API, not yet used internally
    pub fn get_unresolved_references(&self) -> Result<Vec<Reference>> {
        trace!("Getting unresolved references");
        let conn = self.connection()?;

        let mut stmt = conn.prepare(&format!(
            "SELECT {REFS_COLUMNS} FROM refs WHERE symbol_id IS NULL ORDER BY file_id, line"
        ))?;

        let refs = stmt
            .query_map([], row_to_reference)?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(refs)
    }

    /// Get unresolved references with file path information for LSP queries.
    ///
    /// Returns references where `symbol_id` is NULL, including the file path
    /// from the files table. LSP clients need file paths (not database IDs) to
    /// issue `goto_definition` requests.
    ///
    /// Positions are returned as 1-indexed (as stored in DB). Convert to 0-indexed
    /// before passing to LSP clients.
    pub fn get_unresolved_references_for_lsp(
        &self,
    ) -> Result<Vec<crate::types::UnresolvedRefForLsp>> {
        trace!("Getting unresolved references for LSP resolution");
        let conn = self.connection()?;

        let mut stmt = conn.prepare(
            "SELECT r.id, r.file_id, f.path, r.line, r.column, r.reference_name
             FROM refs r
             JOIN files f ON r.file_id = f.id
             WHERE r.symbol_id IS NULL AND r.reference_name IS NOT NULL
             ORDER BY f.path, r.line, r.column",
        )?;

        let refs = stmt
            .query_map([], |row| {
                Ok(crate::types::UnresolvedRefForLsp {
                    ref_id: RefId::from(row.get::<_, i64>(0)?),
                    file_id: FileId::from(row.get::<_, i64>(1)?),
                    file_path: PathBuf::from(row.get::<_, String>(2)?),
                    line: row.get(3)?,
                    column: row.get(4)?,
                    reference_name: row.get(5)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        trace!(
            unresolved_count = refs.len(),
            "Found unresolved references for LSP"
        );

        Ok(refs)
    }

    /// Resolve a reference by setting its `symbol_id`.
    ///
    /// This is used in Pass 2 to link unresolved references to their target symbols
    /// after cross-file symbol resolution.
    #[allow(dead_code)] // Public API, not yet used internally
    pub fn resolve_reference(&self, ref_id: i64, symbol_id: SymbolId) -> Result<()> {
        trace!(
            ref_id = ref_id,
            symbol_id = %symbol_id,
            "Resolving reference"
        );
        let conn = self.connection()?;

        conn.execute(
            "UPDATE refs SET symbol_id = ?2, reference_name = NULL WHERE id = ?1",
            params![ref_id, symbol_id.as_i64()],
        )?;
        Ok(())
    }

    /// Get all references to a symbol.
    pub fn get_references_to_symbol(&self, symbol_id: SymbolId) -> Result<Vec<Reference>> {
        trace!(symbol_id = %symbol_id, "Getting references to symbol");
        let conn = self.connection()?;

        let mut stmt = conn.prepare(&format!(
            "SELECT {REFS_COLUMNS} FROM refs WHERE symbol_id = ?1 ORDER BY file_id, line"
        ))?;

        let refs = stmt
            .query_map([symbol_id.as_i64()], row_to_reference)?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(refs)
    }

    /// List all outgoing references from a file.
    pub fn list_references_in_file(&self, file_id: FileId) -> Result<Vec<Reference>> {
        trace!(file_id = %file_id, "Listing references in file");
        let conn = self.connection()?;

        let mut stmt = conn.prepare(&format!(
            "SELECT {REFS_COLUMNS} FROM refs WHERE file_id = ?1 ORDER BY line, column"
        ))?;

        let refs = stmt
            .query_map([file_id.as_i64()], row_to_reference)?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(refs)
    }
}
