//! File dependency CRUD operations for the Tethys index.

use rusqlite::params;

use super::Index;
use crate::error::Result;
use crate::types::FileId;

impl Index {
    /// Insert or update a file-level dependency.
    ///
    /// Records that `from_file_id` depends on `to_file_id`.
    pub fn insert_file_dependency(&self, from_file_id: FileId, to_file_id: FileId) -> Result<()> {
        let conn = self.connection()?;

        // Use upsert (ON CONFLICT) to handle duplicates (increments ref_count)
        conn.execute(
            "INSERT INTO file_deps (from_file_id, to_file_id, ref_count)
             VALUES (?1, ?2, 1)
             ON CONFLICT(from_file_id, to_file_id) DO UPDATE SET ref_count = ref_count + 1",
            params![from_file_id.as_i64(), to_file_id.as_i64()],
        )?;
        Ok(())
    }

    /// Get files that the given file depends on.
    pub fn get_file_dependencies(&self, file_id: FileId) -> Result<Vec<FileId>> {
        let conn = self.connection()?;

        let mut stmt = conn.prepare("SELECT to_file_id FROM file_deps WHERE from_file_id = ?1")?;

        let deps = stmt
            .query_map([file_id.as_i64()], |row| {
                row.get::<_, i64>(0).map(FileId::from)
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(deps)
    }

    /// Get files that depend on the given file.
    pub fn get_file_dependents(&self, file_id: FileId) -> Result<Vec<FileId>> {
        let conn = self.connection()?;

        let mut stmt = conn.prepare("SELECT from_file_id FROM file_deps WHERE to_file_id = ?1")?;

        let deps = stmt
            .query_map([file_id.as_i64()], |row| {
                row.get::<_, i64>(0).map(FileId::from)
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(deps)
    }
}
