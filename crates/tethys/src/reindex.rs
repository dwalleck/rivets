//! Update and rebuild operations for the Tethys index.
//!
//! Thin wrappers around the core indexing pipeline for incremental updates
//! and full rebuilds.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use tracing::{debug, warn};

use crate::Tethys;
use crate::db::normalize_path;
use crate::error::Result;
use crate::types::{IndexOptions, IndexStats, IndexUpdate, StalenessReport};

/// Classification of a single file's state relative to its indexed entry.
///
/// Returned by [`classify_indexed_file`] so both [`Tethys::needs_update`] and
/// [`Tethys::get_stale_files`] share one source of truth for "what changed?".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileChange {
    Unchanged,
    Modified,
    Deleted,
}

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
    ///
    /// The iteration shape is duplicated with `get_stale_files` deliberately:
    /// the early-exit version cannot reuse the report-building loop without
    /// allocating, which would defeat the purpose of having two methods. The
    /// shared change-classification logic lives in `classify_indexed_file`.
    pub fn needs_update(&self) -> Result<bool> {
        let mut indexed_map = self.load_indexed_map()?;

        // discover_files already emits warn! for each skipped directory; this
        // sink Vec is required by the API but its contents are unused.
        let mut skipped_dirs = Vec::new();
        let disk_files = self.discover_files(&mut skipped_dirs)?;

        for file_path in disk_files {
            let lookup = self.lookup_key(&file_path);
            match indexed_map.remove(&lookup) {
                None => return Ok(true),
                Some((indexed_mtime, indexed_size)) => {
                    if classify_indexed_file(&file_path, indexed_mtime, indexed_size)
                        != FileChange::Unchanged
                    {
                        return Ok(true);
                    }
                }
            }
        }

        // Anything still in the map is in the DB but no longer on disk — i.e. deleted.
        Ok(!indexed_map.is_empty())
    }

    /// Compare indexed files against the filesystem to find what needs re-indexing.
    ///
    /// Detects three categories of staleness:
    /// - **Modified**: files on disk whose mtime or size differ from the index
    /// - **Added**: source files on disk not yet in the index
    /// - **Deleted**: files in the index no longer present on disk
    ///
    /// Returned paths use forward-slash separators on all platforms, matching
    /// the [`IndexedFile::path`](crate::types::IndexedFile::path) form stored
    /// in the DB.
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

        // discover_files already emits warn! for each skipped directory; this
        // sink Vec is required by the API but its contents are unused.
        let mut skipped_dirs = Vec::new();
        let disk_files = self.discover_files(&mut skipped_dirs)?;

        for file_path in disk_files {
            let lookup = self.lookup_key(&file_path);

            if let Some((indexed_mtime, indexed_size)) = indexed_map.remove(&lookup) {
                match classify_indexed_file(&file_path, indexed_mtime, indexed_size) {
                    FileChange::Unchanged => {}
                    FileChange::Modified => modified.push(lookup),
                    FileChange::Deleted => deleted.push(lookup),
                }
            } else {
                added.push(lookup);
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

    /// Build the indexed-file lookup map keyed by normalized workspace-relative path.
    ///
    /// `IndexedFile::path` is stored normalized (forward slashes) by the DB
    /// layer; we re-normalize defensively so the contract holds even if a
    /// future code path inserts a path that bypassed `normalize_path`.
    fn load_indexed_map(&self) -> Result<HashMap<PathBuf, (i64, u64)>> {
        Ok(self
            .db
            .list_all_files()?
            .into_iter()
            .map(|f| {
                (
                    PathBuf::from(normalize_path(&f.path)),
                    (f.mtime_ns, f.size_bytes),
                )
            })
            .collect())
    }

    /// Compute the lookup key for a discovered disk path.
    ///
    /// Strips the workspace prefix and normalizes separators to match the
    /// forward-slash form stored by the DB layer (critical on Windows, where
    /// `entry.path()` yields backslash-separated paths but the DB stores
    /// forward slashes).
    fn lookup_key(&self, file_path: &Path) -> PathBuf {
        let relative = self.relative_path(file_path);
        PathBuf::from(normalize_path(relative.as_ref()))
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

/// Classify a file's state relative to its indexed mtime/size.
///
/// Errors are conservatively reported as `Modified` (treat as changed) so
/// callers re-index the file rather than skipping it. `NotFound` becomes
/// `Deleted` because a missing file always invalidates the indexed entry.
fn classify_indexed_file(file_path: &Path, indexed_mtime: i64, indexed_size: u64) -> FileChange {
    match std::fs::metadata(file_path) {
        Ok(metadata) => {
            let size = metadata.len();
            let mtime = mtime_ns(&metadata, file_path);
            if mtime != indexed_mtime || size != indexed_size {
                FileChange::Modified
            } else {
                FileChange::Unchanged
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => FileChange::Deleted,
        Err(e) => {
            warn!(
                path = %file_path.display(),
                error = %e,
                "Failed to read metadata during staleness check, treating as modified"
            );
            FileChange::Modified
        }
    }
}

/// Convert a file's modification time to nanoseconds since UNIX epoch.
///
/// Returns [`i64::MIN`] with a warning when the OS does not expose a usable
/// mtime. `i64::MIN` is unrepresentable as a real epoch nanosecond timestamp,
/// so it never silently matches a stored `0` (which the indexer also writes
/// on failure) — falling back here always biases toward "Modified", forcing a
/// re-index rather than a silent skip.
///
/// The error branches (OS `metadata.modified()` failure, pre-epoch mtime) are
/// intentionally untested; reproducing them portably requires a filesystem
/// mock.
fn mtime_ns(metadata: &std::fs::Metadata, file_path: &Path) -> i64 {
    let modified = match metadata.modified() {
        Ok(t) => t,
        Err(e) => {
            warn!(
                path = %file_path.display(),
                error = %e,
                "Could not read mtime, using i64::MIN sentinel to force re-index"
            );
            return i64::MIN;
        }
    };
    let duration = match modified.duration_since(UNIX_EPOCH) {
        Ok(d) => d,
        Err(e) => {
            warn!(
                path = %file_path.display(),
                error = %e,
                "mtime predates UNIX epoch, using i64::MIN sentinel to force re-index"
            );
            return i64::MIN;
        }
    };
    #[expect(
        clippy::cast_possible_truncation,
        reason = "ns since epoch fits in i64 until year 2262"
    )]
    {
        duration.as_nanos() as i64
    }
}

#[cfg(test)]
mod tests {
    use super::{FileChange, classify_indexed_file};
    use rstest::rstest;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn write_and_stat(dir: &Path, name: &str, content: &str) -> (std::path::PathBuf, i64, u64) {
        let path = dir.join(name);
        fs::write(&path, content).expect("failed to write file");
        let metadata = fs::metadata(&path).expect("metadata after write");
        let mtime = metadata
            .modified()
            .expect("modified time")
            .duration_since(std::time::UNIX_EPOCH)
            .expect("post-epoch mtime")
            .as_nanos();
        #[expect(
            clippy::cast_possible_truncation,
            reason = "ns since epoch fits in i64 until year 2262"
        )]
        let mtime = mtime as i64;
        (path, mtime, metadata.len())
    }

    /// `mtime_delta` and `size_delta` shift the *indexed* values relative to
    /// the real on-disk values: `0` means the indexed value matches reality,
    /// any non-zero value simulates the DB holding a stale entry.
    #[rstest]
    #[case::matches(0, 0, FileChange::Unchanged)]
    #[case::stale_indexed_size(0, 1, FileChange::Modified)]
    #[case::stale_indexed_mtime(1, 0, FileChange::Modified)]
    #[case::both_stale(1, 1, FileChange::Modified)]
    fn classify_existing_file(
        #[case] mtime_delta: i64,
        #[case] size_delta: u64,
        #[case] expected: FileChange,
    ) {
        let dir = TempDir::new().expect("tempdir");
        let (path, mtime, size) = write_and_stat(dir.path(), "a.rs", "fn a() {}");

        assert_eq!(
            classify_indexed_file(&path, mtime + mtime_delta, size + size_delta),
            expected
        );
    }

    #[test]
    fn classify_deleted_when_file_is_missing() {
        let dir = TempDir::new().expect("tempdir");
        let missing = dir.path().join("never-existed.rs");

        assert_eq!(classify_indexed_file(&missing, 0, 0), FileChange::Deleted);
    }
}
