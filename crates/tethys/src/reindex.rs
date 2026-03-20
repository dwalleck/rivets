//! Update and rebuild operations for the Tethys index.
//!
//! Thin wrappers around the core indexing pipeline for incremental updates
//! and full rebuilds.

use crate::Tethys;
use crate::error::Result;
use crate::types::{IndexOptions, IndexStats, IndexUpdate};

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
    /// **Note:** Currently unimplemented — always returns `true`. Proper change detection is tracked as a future enhancement.
    pub fn needs_update(&self) -> Result<bool> {
        Ok(true)
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
