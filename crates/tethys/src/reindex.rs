//! Update and rebuild operations for the Tethys index.
//!
//! Thin wrappers around the core indexing pipeline for incremental updates
//! and full rebuilds.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use tracing::{debug, warn};

use crate::Tethys;
use crate::error::Result;
use crate::types::{IndexOptions, IndexStats, IndexUpdate, StalenessReport};

#[expect(
    clippy::missing_errors_doc,
    reason = "error docs deferred to avoid churn during active development"
)]
impl Tethys {
    /// Incrementally update index for changed files.
    ///
    /// **Note:** Currently performs a full re-index. Incremental update is tracked as a future enhancement.
    pub fn update(&mut self) -> Result<IndexUpdate> {
        // For now, just re-index everything
        let stats = self.index()?;
        Ok(IndexUpdate {
            files_changed: stats.files_indexed,
            files_unchanged: 0, // Always 0 until incremental change detection is implemented
            duration: stats.duration,
            errors: stats.errors,
        })
    }

    /// Check if any indexed files have changed since last update.
    ///
    /// Stops at the first detected change rather than allocating the full
    /// [`StalenessReport`] returned by [`get_stale_files`](Self::get_stale_files).
    pub fn needs_update(&self) -> Result<bool> {
        let mut indexed_map = self.load_indexed_map()?;

        // discover_files already emits warn! for each skipped directory; the Vec
        // is unused here because needs_update has no recovery action for skips.
        let disk_files = self.discover_files(&mut Vec::new())?;

        for file_path in disk_files {
            let relative = self.relative_path(&file_path);
            match indexed_map.remove(relative.as_ref()) {
                None => return Ok(true),
                Some((indexed_mtime, indexed_size)) => {
                    if file_changed(&file_path, indexed_mtime, indexed_size) {
                        return Ok(true);
                    }
                }
            }
        }

        // Anything still in the map is on disk no longer — i.e. deleted.
        Ok(!indexed_map.is_empty())
    }

    /// Compare indexed files against the filesystem to find what needs re-indexing.
    ///
    /// Detects three categories of staleness:
    /// - **Modified**: files on disk whose mtime or size differ from the index
    /// - **Added**: source files on disk not yet in the index
    /// - **Deleted**: files in the index no longer present on disk
    ///
    /// Note: there is a small TOCTOU window where files could change between
    /// this staleness check and actual re-indexing. This is acceptable for
    /// development workflows; use [`rebuild`](Self::rebuild) when full
    /// consistency is required.
    pub fn get_stale_files(&self) -> Result<StalenessReport> {
        let mut indexed_map = self.load_indexed_map()?;

        let mut modified = Vec::new();
        let mut added = Vec::new();
        let mut deleted = Vec::new();

        // discover_files already emits warn! for each skipped directory; the Vec
        // is unused here because the staleness report does not surface skips.
        let disk_files = self.discover_files(&mut Vec::new())?;

        for file_path in disk_files {
            let relative = self.relative_path(&file_path);

            if let Some((indexed_mtime, indexed_size)) = indexed_map.remove(relative.as_ref()) {
                match std::fs::metadata(&file_path) {
                    Ok(metadata) => {
                        let size = metadata.len();
                        let mtime = mtime_ns(&metadata, &file_path);
                        if mtime != indexed_mtime || size != indexed_size {
                            modified.push(relative.into_owned());
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        // File was deleted between discovery and metadata check
                        deleted.push(relative.into_owned());
                    }
                    Err(e) => {
                        warn!(
                            path = %file_path.display(),
                            error = %e,
                            "Failed to read metadata during staleness check, treating as modified"
                        );
                        modified.push(relative.into_owned());
                    }
                }
            } else {
                added.push(relative.into_owned());
            }
        }

        deleted.extend(indexed_map.into_keys());

        debug!(
            modified = modified.len(),
            added = added.len(),
            deleted = deleted.len(),
            "Staleness check complete"
        );

        Ok(StalenessReport {
            modified,
            added,
            deleted,
        })
    }

    fn load_indexed_map(&self) -> Result<HashMap<PathBuf, (i64, u64)>> {
        Ok(self
            .db
            .list_all_files()?
            .into_iter()
            .map(|f| (f.path, (f.mtime_ns, f.size_bytes)))
            .collect())
    }

    /// Rebuild the entire index from scratch.
    ///
    /// Deletes and recreates the database file, ensuring schema changes are
    /// applied cleanly. Use this instead of manually deleting the database.
    pub fn rebuild(&mut self) -> Result<IndexStats> {
        self.db.reset()?;
        self.index()
    }

    /// Rebuild the entire index from scratch with options.
    ///
    /// Deletes and recreates the database file, ensuring schema changes are
    /// applied cleanly. See [`index_with_options`](Self::index_with_options)
    /// for details on options.
    pub fn rebuild_with_options(&mut self, options: IndexOptions) -> Result<IndexStats> {
        self.db.reset()?;
        self.index_with_options(options)
    }
}

/// Returns whether a file's mtime or size differ from the indexed values.
///
/// Errors are conservatively reported as `true` (treat as changed) so callers
/// can re-index the file rather than skipping it. `NotFound` is also `true`
/// because a missing file always invalidates the indexed entry.
fn file_changed(file_path: &Path, indexed_mtime: i64, indexed_size: u64) -> bool {
    match std::fs::metadata(file_path) {
        Ok(metadata) => {
            let size = metadata.len();
            let mtime = mtime_ns(&metadata, file_path);
            mtime != indexed_mtime || size != indexed_size
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
        Err(e) => {
            warn!(
                path = %file_path.display(),
                error = %e,
                "Failed to read metadata during staleness check, treating as modified"
            );
            true
        }
    }
}

/// Convert a file's modification time to nanoseconds since UNIX epoch.
///
/// Returns 0 with a warning when the OS does not expose a usable mtime, so the
/// comparison stays deterministic but the fallback is observable in logs.
fn mtime_ns(metadata: &std::fs::Metadata, file_path: &Path) -> i64 {
    let modified = match metadata.modified() {
        Ok(t) => t,
        Err(e) => {
            warn!(
                path = %file_path.display(),
                error = %e,
                "Could not read mtime, defaulting to 0"
            );
            return 0;
        }
    };
    let duration = match modified.duration_since(UNIX_EPOCH) {
        Ok(d) => d,
        Err(e) => {
            warn!(
                path = %file_path.display(),
                error = %e,
                "mtime predates UNIX epoch, defaulting to 0"
            );
            return 0;
        }
    };
    // Nanoseconds since epoch fit in i64 until year 2262.
    #[allow(clippy::cast_possible_truncation)]
    let ns = duration.as_nanos() as i64;
    ns
}
