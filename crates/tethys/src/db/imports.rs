//! Import CRUD operations for the Tethys index.

use rusqlite::params;
use tracing::trace;

use super::{row_to_import, Index};
use crate::error::Result;
use crate::types::{FileId, Import};

impl Index {
    /// Insert an import record for cross-file reference resolution.
    ///
    /// Records that `file_id` imports `symbol_name` from `source_module`.
    /// Uses upsert semantics: if the import already exists, this is a no-op.
    pub fn insert_import(
        &self,
        file_id: FileId,
        symbol_name: &str,
        source_module: &str,
        alias: Option<&str>,
    ) -> Result<()> {
        trace!(
            file_id = %file_id,
            symbol_name = %symbol_name,
            source_module = %source_module,
            alias = ?alias,
            "Inserting import"
        );
        let conn = self.connection()?;

        conn.execute(
            "INSERT OR REPLACE INTO imports (file_id, symbol_name, source_module, alias)
             VALUES (?1, ?2, ?3, ?4)",
            params![file_id.as_i64(), symbol_name, source_module, alias],
        )?;
        Ok(())
    }

    /// Get all imports for a file.
    ///
    /// Returns a list of all symbols imported by the given file.
    pub fn get_imports_for_file(&self, file_id: FileId) -> Result<Vec<Import>> {
        trace!(file_id = %file_id, "Getting imports for file");
        let conn = self.connection()?;

        let mut stmt = conn.prepare(
            "SELECT file_id, symbol_name, source_module, alias
             FROM imports WHERE file_id = ?1 ORDER BY source_module, symbol_name",
        )?;

        let imports = stmt
            .query_map([file_id.as_i64()], row_to_import)?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(imports)
    }

    /// Clear all imports for a file (for re-indexing).
    ///
    /// Call this before re-indexing a file to remove stale imports.
    pub fn clear_imports_for_file(&self, file_id: FileId) -> Result<()> {
        trace!(file_id = %file_id, "Clearing imports for file");
        let conn = self.connection()?;

        conn.execute("DELETE FROM imports WHERE file_id = ?1", [file_id.as_i64()])?;
        Ok(())
    }
}
