//! Update and rebuild operations for the Tethys index.
//!
//! Thin wrappers around the core indexing pipeline for incremental updates
//! and full rebuilds.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::UNIX_EPOCH;

use tracing::{debug, warn};

use crate::Tethys;
use crate::error::Result;
use crate::types::{IndexOptions, IndexStats, IndexUpdate, Language, StalenessReport};

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
    /// Equivalent to `get_stale_files().map(|r| r.is_stale())` but does not
    /// allocate the full report.
    pub fn needs_update(&self) -> Result<bool> {
        self.get_stale_files().map(|report| report.is_stale())
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
        let indexed_files = self.db.list_all_files()?;
        let mut indexed_map: HashMap<PathBuf, (i64, u64)> = indexed_files
            .into_iter()
            .map(|f| (f.path, (f.mtime_ns, f.size_bytes)))
            .collect();

        let mut modified = Vec::new();
        let mut added = Vec::new();
        let mut deleted = Vec::new();

        let mut skip_log = Vec::new();
        let disk_files = self.discover_files(&mut skip_log)?;

        for file_path in disk_files {
            let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if Language::from_extension(ext).is_none() {
                continue;
            }

            let relative = self.relative_path(&file_path).to_path_buf();

            if let Some((indexed_mtime, indexed_size)) = indexed_map.remove(&relative) {
                match std::fs::metadata(&file_path) {
                    Ok(metadata) => {
                        let size = metadata.len();
                        let mtime = metadata
                            .modified()
                            .ok()
                            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                            .map_or(0, |d| {
                                // Safety: nanoseconds since epoch fit in i64 until year 2262
                                #[allow(clippy::cast_possible_truncation)]
                                {
                                    d.as_nanos() as i64
                                }
                            });

                        if mtime != indexed_mtime || size != indexed_size {
                            modified.push(relative);
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        // File was deleted between discovery and metadata check
                        deleted.push(relative);
                    }
                    Err(e) => {
                        warn!(
                            path = %file_path.display(),
                            error = %e,
                            "Failed to read metadata during staleness check, treating as modified"
                        );
                        modified.push(relative);
                    }
                }
            } else {
                added.push(relative);
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
