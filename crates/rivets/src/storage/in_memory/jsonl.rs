//! JSONL persistence for in-memory storage.
//!
//! This module provides functions to load and save the in-memory storage
//! to JSONL (JSON Lines) files.

use super::graph::has_cycle_impl;
use super::inner::InMemoryStorageInner;
use crate::domain::{Issue, IssueId};
use crate::error::{Error, Result, StorageError};
use crate::storage::IssueStorage;
use rivets_jsonl::{read_jsonl_resilient, Warning as JsonlWarning};
use std::path::Path;
use std::sync::Arc;
use tokio::fs::File;
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::sync::Mutex;

/// Warnings that can occur during JSONL file loading.
///
/// These are non-fatal issues that don't prevent loading but indicate
/// data quality problems in the JSONL file. When warnings occur, the load
/// operation continues but problematic data is skipped or sanitized.
///
/// # Handling Warnings
///
/// Applications should log or report these warnings to users, as they indicate
/// data corruption or integrity issues that may need manual resolution.
///
/// **Example:**
/// ```no_run
/// # use rivets::storage::in_memory::load_from_jsonl;
/// # use std::path::Path;
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() -> anyhow::Result<()> {
/// let (storage, warnings) = load_from_jsonl(
///     Path::new(".rivets/issues.jsonl"),
///     "rivets".to_string()
/// ).await?;
///
/// for warning in warnings {
///     match warning {
///         rivets::storage::in_memory::LoadWarning::MalformedJson { line_number, error } => {
///             eprintln!("Skipped malformed JSON at line {}: {}", line_number, error);
///         }
///         rivets::storage::in_memory::LoadWarning::OrphanedDependency { from, to } => {
///             eprintln!("Skipped orphaned dependency: {} -> {}", from, to);
///         }
///         rivets::storage::in_memory::LoadWarning::CircularDependency { from, to } => {
///             eprintln!("Broke circular dependency: {} -> {}", from, to);
///         }
///         rivets::storage::in_memory::LoadWarning::InvalidIssueData { issue_id, line_number, error } => {
///             eprintln!("Skipped invalid issue {} at line {}: {}", issue_id, line_number, error);
///         }
///     }
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub enum LoadWarning {
    /// Malformed JSON line that couldn't be parsed
    ///
    /// **Effect**: Line is skipped entirely; no issue created from this line.
    /// **Common causes**: File corruption, manual editing errors, incomplete writes.
    MalformedJson { line_number: usize, error: String },

    /// Dependency references an issue that doesn't exist in the file
    ///
    /// **Effect**: The dependency edge is skipped; both issues are still loaded,
    /// but the dependency relationship is not created.
    /// **Common causes**: Partial exports, deleted dependencies, file corruption.
    OrphanedDependency { from: IssueId, to: IssueId },

    /// Adding a dependency would create a circular reference
    ///
    /// **Effect**: The dependency edge is skipped to break the cycle; both issues
    /// are loaded but one dependency edge is omitted.
    /// **Common causes**: Manual JSONL editing, bugs in earlier versions.
    CircularDependency { from: IssueId, to: IssueId },

    /// Issue data failed validation (invalid priority, title length, etc.)
    ///
    /// **Effect**: The entire issue is skipped and not loaded into storage.
    /// **Common causes**: Manual editing, version mismatches, data corruption.
    InvalidIssueData {
        issue_id: IssueId,
        line_number: usize,
        error: String,
    },
}

/// Load storage from a JSONL file.
///
/// This function reads a JSONL (JSON Lines) file where each line is a serialized `Issue`.
/// It reconstructs both the issues and their dependency graph.
///
/// # Error Handling
///
/// - **Malformed JSON**: Skips the line and adds a warning
/// - **Orphaned dependencies**: Skips the dependency edge and adds a warning
/// - **Circular dependencies**: Skips the dependency edge and adds a warning
///
/// # Memory Considerations
///
/// This function loads the entire JSONL file into memory during parsing. The three-pass
/// loading algorithm requires all issues to be held in memory simultaneously.
///
/// **Expected limits**:
/// - Small databases (< 1,000 issues): Negligible memory usage (~1-2 MB)
/// - Medium databases (1,000 - 10,000 issues): ~10-20 MB memory spike during load
/// - Large databases (> 10,000 issues): Consider the file size; expect memory usage
///   approximately 2-3x the JSONL file size during loading
///
/// For databases with tens of thousands of issues, monitor memory usage during load.
/// Future versions may implement streaming or chunked loading for very large databases.
///
/// # Performance Considerations
///
/// **ID Registration**: During the second pass, all loaded issue IDs are registered with
/// the ID generator via `register_id()`. This is an O(n) operation where n is the number
/// of issues. The ID generator maintains a hash set of used IDs to prevent future collisions,
/// so registration is O(1) per ID.
///
/// For typical databases (< 10,000 issues), ID registration completes in milliseconds.
/// For very large databases (> 100,000 issues), expect 10-50ms additional load time for
/// ID registration.
///
/// # Returns
///
/// Returns a tuple of `(storage, warnings)` where warnings contains all non-fatal
/// issues encountered during loading.
pub async fn load_from_jsonl(
    path: &Path,
    prefix: String,
) -> Result<(Box<dyn IssueStorage>, Vec<LoadWarning>)> {
    // First pass: Use rivets-jsonl for resilient parsing
    let (parsed_issues, jsonl_warnings) =
        read_jsonl_resilient::<Issue, _>(path)
            .await
            .map_err(|e| match e {
                rivets_jsonl::Error::Io(io_err) => Error::Io(io_err),
                rivets_jsonl::Error::Json(json_err) => Error::Json(json_err),
                rivets_jsonl::Error::InvalidFormat(msg) => StorageError::InvalidFormat(msg).into(),
            })?;

    let mut warnings = Vec::new();

    // Convert rivets_jsonl warnings to LoadWarnings
    for warning in jsonl_warnings {
        match warning {
            JsonlWarning::MalformedJson { line_number, error } => {
                warnings.push(LoadWarning::MalformedJson { line_number, error });
            }
            JsonlWarning::SkippedLine {
                line_number,
                reason,
            } => {
                // Map SkippedLine to MalformedJson since both indicate parsing issues
                warnings.push(LoadWarning::MalformedJson {
                    line_number,
                    error: reason,
                });
            }
        }
    }

    // Validate issues and filter out invalid ones
    // Note: line_number here is the record index (1-based) within successfully parsed records,
    // not the actual file line number if there were malformed/skipped lines.
    let mut issues = Vec::new();
    for (index, issue) in parsed_issues.into_iter().enumerate() {
        let record_number = index + 1; // 1-based indexing for user-friendly messages
        if let Err(validation_error) = issue.validate() {
            warnings.push(LoadWarning::InvalidIssueData {
                issue_id: issue.id.clone(),
                line_number: record_number,
                error: validation_error,
            });
            continue;
        }
        issues.push(issue);
    }

    // Create storage and import issues
    let storage = Arc::new(Mutex::new(InMemoryStorageInner::new(prefix)));
    let mut inner = storage.lock().await;

    // Second pass: Import issues and create graph nodes
    for issue in &issues {
        let node = inner.graph.add_node(issue.id.clone());
        inner.node_map.insert(issue.id.clone(), node);
        inner.issues.insert(issue.id.clone(), issue.clone());
        inner
            .id_generator
            .register_id(issue.id.as_str().to_string());
    }

    // Third pass: Reconstruct dependencies with cycle detection
    for issue in &issues {
        for dep in &issue.dependencies {
            // Check if dependency target exists
            if !inner.node_map.contains_key(&dep.depends_on_id) {
                warnings.push(LoadWarning::OrphanedDependency {
                    from: issue.id.clone(),
                    to: dep.depends_on_id.clone(),
                });
                continue;
            }

            // Check for cycles before adding edge
            if has_cycle_impl(&inner.graph, &inner.node_map, &issue.id, &dep.depends_on_id)? {
                warnings.push(LoadWarning::CircularDependency {
                    from: issue.id.clone(),
                    to: dep.depends_on_id.clone(),
                });
                continue;
            }

            // Safe to add edge
            let from_node = inner.node_map[&issue.id];
            let to_node = inner.node_map[&dep.depends_on_id];
            inner.graph.add_edge(from_node, to_node, dep.dep_type);
        }
    }

    // Release lock before returning
    drop(inner);

    Ok((Box::new(storage), warnings))
}

/// Save storage to a JSONL file with atomic writes.
///
/// This function writes all issues to a JSONL file, with each issue on its own line.
/// The write is atomic: it writes to a temporary file first, then renames it.
///
/// # Atomicity
///
/// The function uses a write-then-rename pattern which is atomic on POSIX systems.
/// If the process crashes or is interrupted, the original file remains unchanged.
pub async fn save_to_jsonl(storage: &dyn IssueStorage, path: &Path) -> Result<()> {
    // Create temp file path
    let temp_path = path.with_extension("tmp");

    // Open temp file
    let file = File::create(&temp_path).await.map_err(Error::Io)?;
    let mut writer = BufWriter::new(file);

    // Export all issues
    let mut issues = storage.export_all().await?;

    // Write each issue as a JSON line
    for issue in &mut issues {
        // Sort dependencies for deterministic serialization.
        // This ensures consistent JSONL output across saves, preventing spurious
        // diffs in version control when dependencies are added/removed in different orders.
        issue.dependencies.sort();

        let json = serde_json::to_string(&issue).map_err(StorageError::Serialization)?;

        writer.write_all(json.as_bytes()).await.map_err(Error::Io)?;

        writer.write_all(b"\n").await.map_err(Error::Io)?;
    }

    // Flush and close
    writer.flush().await.map_err(Error::Io)?;

    // Atomic rename
    tokio::fs::rename(&temp_path, path)
        .await
        .map_err(Error::Io)?;

    Ok(())
}
