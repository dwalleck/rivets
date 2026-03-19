//! Reference CRUD operations for the Tethys index.
//!
//! These operations support symbol-level "who calls X?" queries.
//! See graph module for higher-level graph traversal using these primitives.

use std::path::PathBuf;

use rusqlite::params;
use tracing::trace;

use super::{Index, REFS_COLUMNS, row_to_reference};
use crate::error::Result;
use crate::types::{FileId, RefId, Reference, SymbolId};

/// Parameters for inserting a reference into the index.
pub(crate) struct InsertReferenceParams<'a> {
    /// The symbol this reference points to, or `None` for unresolved references.
    pub symbol_id: Option<SymbolId>,
    /// The file containing this reference.
    pub file_id: FileId,
    /// The kind of reference (e.g., "call", "use").
    pub kind: &'a str,
    /// Line number of the reference (1-indexed).
    pub line: u32,
    /// Column number of the reference (0-indexed).
    pub column: u32,
    /// The symbol that contains this reference, if any.
    pub in_symbol_id: Option<SymbolId>,
    /// The name used in the reference, for later resolution in Pass 2.
    pub reference_name: Option<&'a str>,
}

impl InsertReferenceParams<'_> {
    /// Asserts struct invariants in debug builds.
    ///
    /// - `line` must be >= 1 (1-indexed).
    /// - If `symbol_id` is `None`, `reference_name` must be `Some` (unresolved refs need a name for Pass 2).
    #[cfg(debug_assertions)]
    pub(crate) fn debug_assert_valid(&self) {
        debug_assert!(
            self.line >= 1,
            "reference line should be >= 1, got {}",
            self.line
        );
        debug_assert!(
            self.symbol_id.is_some() || self.reference_name.is_some(),
            "unresolved reference (symbol_id is None) must have a reference_name for Pass 2"
        );
    }

    /// No-op in release builds.
    #[cfg(not(debug_assertions))]
    #[inline]
    pub(crate) fn debug_assert_valid(&self) {}
}

impl Index {
    /// Insert a reference to a symbol.
    ///
    /// If `symbol_id` is `None`, the reference is unresolved and `reference_name`
    /// should be provided for later resolution in Pass 2.
    pub fn insert_reference(&self, params: &InsertReferenceParams<'_>) -> Result<i64> {
        params.debug_assert_valid();
        let conn = self.connection()?;

        conn.execute(
            "INSERT INTO refs (symbol_id, file_id, kind, line, column, in_symbol_id, reference_name)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                params.symbol_id.map(SymbolId::as_i64),
                params.file_id.as_i64(),
                params.kind,
                params.line,
                params.column,
                params.in_symbol_id.map(SymbolId::as_i64),
                params.reference_name
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Get all unresolved references (where `symbol_id` is NULL).
    ///
    /// These references need to be resolved in Pass 2 by matching their
    /// `reference_name` to symbols discovered in other files.
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
