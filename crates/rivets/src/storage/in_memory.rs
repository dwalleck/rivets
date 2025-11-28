//! In-memory storage backend using HashMap and petgraph.
//!
//! This module provides a fast, **ephemeral** storage implementation where all data
//! is held in RAM and **lost when the process exits**. It is suitable for:
//!
//! - Testing and development
//! - Short-lived CLI sessions
//! - MVP development phase
//! - Performance benchmarking
//!
//! # Persistence
//!
//! This backend supports **optional JSONL persistence** via the `load_from_jsonl()` and
//! `save_to_jsonl()` functions. Data can be loaded from and saved to disk while maintaining
//! fast in-memory operations.
//!
//! - **In-memory only**: Use `new_in_memory_storage()` for ephemeral storage
//! - **With persistence**: Use `load_from_jsonl()` to load from disk, then periodically
//!   call `save_to_jsonl()` to persist changes
//!
//! The trait's `save()` method is a no-op for in-memory storage. Use `save_to_jsonl()`
//! directly for file-based persistence.
//!
//! For future backends:
//! - `StorageBackend::Jsonl` - Dedicated JSONL backend (future)
//! - `StorageBackend::PostgreSQL` - Production-ready database backend (future)
//!
//! # Architecture
//!
//! The implementation uses:
//! - `HashMap<IssueId, Issue>` for O(1) issue lookups
//! - `petgraph::DiGraph` for dependency graph with cycle detection
//! - `HashMap<IssueId, NodeIndex>` for mapping issues to graph nodes
//! - Hash-based ID generation with adaptive length (4-6 chars)
//!
//! # Thread Safety
//!
//! The storage is wrapped in `Arc<Mutex<InMemoryStorageInner>>` to provide thread-safe
//! access in async contexts. All operations acquire the mutex lock, ensuring safe
//! concurrent access from multiple tasks.
//!
//! # Performance Characteristics
//!
//! - Create: O(1) amortized, O(n) when crossing ID length thresholds (500, 1500 issues)
//! - Read: O(1) for single issue lookups
//! - Update: O(1) for issue updates
//! - Delete: O(d) where d is number of dependencies
//! - Dependencies: O(d) where d is number of edges in the graph
//! - Ready to work: O(n + e) where n is issues, e is edges (BFS traversal)

use crate::domain::{
    Dependency, DependencyType, Issue, IssueFilter, IssueId, IssueStatus, IssueUpdate, NewIssue,
};
use crate::error::{Error, Result};
use crate::id_generation::{IdGenerator, IdGeneratorConfig};
use crate::storage::IssueStorage;
use async_trait::async_trait;
use chrono::Utc;
use petgraph::algo;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use rivets_jsonl::{read_jsonl_resilient, Warning as JsonlWarning};
use std::collections::{HashMap, HashSet, VecDeque};
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
///             eprintln!("⚠️  Skipped malformed JSON at line {}: {}", line_number, error);
///         }
///         rivets::storage::in_memory::LoadWarning::OrphanedDependency { from, to } => {
///             eprintln!("⚠️  Skipped orphaned dependency: {} -> {}", from, to);
///         }
///         rivets::storage::in_memory::LoadWarning::CircularDependency { from, to } => {
///             eprintln!("⚠️  Broke circular dependency: {} -> {}", from, to);
///         }
///         rivets::storage::in_memory::LoadWarning::InvalidIssueData { issue_id, line_number, error } => {
///             eprintln!("⚠️  Skipped invalid issue {} at line {}: {}", issue_id, line_number, error);
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

/// Inner storage structure (not thread-safe).
///
/// This contains the actual data structures for storing issues and
/// managing the dependency graph. It's wrapped in Arc<Mutex<>> for
/// thread safety.
struct InMemoryStorageInner {
    /// Issues indexed by ID for O(1) lookups
    issues: HashMap<IssueId, Issue>,

    /// Dependency graph using petgraph
    graph: DiGraph<IssueId, DependencyType>,

    /// Mapping from IssueId to graph NodeIndex
    node_map: HashMap<IssueId, NodeIndex>,

    /// ID generator for creating new issue IDs
    id_generator: IdGenerator,

    /// Prefix for issue IDs (e.g., "rivets")
    prefix: String,
}

impl InMemoryStorageInner {
    /// Create a new empty storage instance
    fn new(prefix: String) -> Self {
        let config = IdGeneratorConfig {
            prefix: prefix.clone(),
            database_size: 0,
        };

        Self {
            issues: HashMap::new(),
            graph: DiGraph::new(),
            node_map: HashMap::new(),
            id_generator: IdGenerator::new(config),
            prefix,
        }
    }

    /// Update the ID generator's database size if we've crossed a threshold.
    ///
    /// ID length changes at 500 and 1500 issues, so we only need to update
    /// when crossing these boundaries. This avoids O(n) re-registration on every create.
    fn update_id_generator_if_needed(&mut self) {
        let current_size = self.issues.len();
        let old_size = self.id_generator.database_size();

        // Determine if we've crossed a length threshold
        let needs_update = match (old_size, current_size) {
            // Crossing 500 boundary (4 -> 5 chars)
            (0..=500, 501..) => true,
            // Crossing 1500 boundary (5 -> 6 chars)
            (0..=1500, 1501..) => true,
            // Crossing backwards (rare, but possible after deletes)
            (501.., 0..=500) => true,
            (1501.., 0..=1500) => true,
            _ => false,
        };

        if needs_update {
            // Only recreate generator when crossing length thresholds
            self.id_generator = IdGenerator::new(IdGeneratorConfig {
                prefix: self.prefix.clone(),
                database_size: current_size,
            });

            // Re-register all existing IDs (O(n), but only at thresholds)
            for id in self.issues.keys() {
                self.id_generator.register_id(id.as_str().to_string());
            }
        }
    }

    /// Generate a new unique ID for an issue
    fn generate_id(&mut self, new_issue: &NewIssue) -> Result<IssueId> {
        // Update generator config if we've crossed a length threshold
        self.update_id_generator_if_needed();

        let id_str = self
            .id_generator
            .generate(
                &new_issue.title,
                &new_issue.description,
                new_issue.assignee.as_deref(),
                None, // No parent ID for top-level issues
            )
            .map_err(|e| Error::Storage(format!("ID generation failed: {}", e)))?;

        Ok(IssueId::new(id_str))
    }
}

/// Thread-safe in-memory storage.
///
/// This type is used internally for implementing the IssueStorage trait.
type InMemoryStorage = Arc<Mutex<InMemoryStorageInner>>;

/// Create a new in-memory storage instance.
///
/// # Arguments
///
/// * `prefix` - The prefix for issue IDs (e.g., "rivets")
///
/// # Example
///
/// ```
/// use rivets::storage::in_memory::new_in_memory_storage;
///
/// #[tokio::main(flavor = "current_thread")]
/// async fn main() {
///     let storage = new_in_memory_storage("rivets".to_string());
///     // Use storage...
/// }
/// ```
pub fn new_in_memory_storage(prefix: String) -> Box<dyn IssueStorage> {
    Box::new(Arc::new(Mutex::new(InMemoryStorageInner::new(prefix))))
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
///
/// # Example
///
/// ```no_run
/// use rivets::storage::in_memory::load_from_jsonl;
/// use std::path::Path;
///
/// #[tokio::main(flavor = "current_thread")]
/// async fn main() -> anyhow::Result<()> {
///     let (storage, warnings) = load_from_jsonl(
///         Path::new(".rivets/issues.jsonl"),
///         "rivets".to_string()
///     ).await?;
///
///     if !warnings.is_empty() {
///         eprintln!("Loaded with {} warnings", warnings.len());
///     }
///
///     Ok(())
/// }
/// ```
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
                rivets_jsonl::Error::InvalidFormat(msg) => Error::Storage(msg),
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
            if inner.has_cycle_impl(&issue.id, &dep.depends_on_id)? {
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
///
/// # Example
///
/// ```no_run
/// use rivets::storage::in_memory::new_in_memory_storage;
/// use rivets::storage::in_memory::save_to_jsonl;
/// use std::path::Path;
///
/// #[tokio::main(flavor = "current_thread")]
/// async fn main() -> anyhow::Result<()> {
///     let storage = new_in_memory_storage("rivets".to_string());
///     // ... create some issues ...
///
///     save_to_jsonl(storage.as_ref(), Path::new(".rivets/issues.jsonl")).await?;
///     Ok(())
/// }
/// ```
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

        let json = serde_json::to_string(&issue)
            .map_err(|e| Error::Storage(format!("JSON serialization failed: {}", e)))?;

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

#[async_trait]
impl IssueStorage for InMemoryStorage {
    async fn create(&mut self, new_issue: NewIssue) -> Result<Issue> {
        let mut inner = self.lock().await;

        // === Phase 1: All validations (no mutations) ===
        // Validate the new issue data (title, priority, etc.)
        new_issue
            .validate()
            .map_err(|e| Error::Storage(format!("Validation failed: {}", e)))?;

        // Validate all dependency targets exist
        for (depends_on_id, _dep_type) in &new_issue.dependencies {
            if !inner.issues.contains_key(depends_on_id) {
                return Err(Error::IssueNotFound(depends_on_id.clone()));
            }
        }

        // === Phase 2: ID generation ===
        let id = inner.generate_id(&new_issue)?;

        // === Phase 3: Cycle detection ===
        // We temporarily add the node to check for cycles, then clean up if needed
        let temp_node = inner.graph.add_node(id.clone());
        inner.node_map.insert(id.clone(), temp_node);

        for (depends_on_id, _dep_type) in &new_issue.dependencies {
            if inner.has_cycle_impl(&id, depends_on_id)? {
                // Rollback: remove the temporary node before returning error
                inner.graph.remove_node(temp_node);
                inner.node_map.remove(&id);
                return Err(Error::CircularDependency {
                    from: id,
                    to: depends_on_id.clone(),
                });
            }
        }

        // === Phase 4: Create issue (all validations passed) ===
        let now = Utc::now();

        // Convert dependencies from tuples to Dependency structs
        let dependencies: Vec<Dependency> = new_issue
            .dependencies
            .iter()
            .map(|(depends_on_id, dep_type)| Dependency {
                depends_on_id: depends_on_id.clone(),
                dep_type: *dep_type,
            })
            .collect();

        let issue = Issue {
            id: id.clone(),
            title: new_issue.title,
            description: new_issue.description,
            status: IssueStatus::Open,
            priority: new_issue.priority,
            issue_type: new_issue.issue_type,
            assignee: new_issue.assignee,
            labels: new_issue.labels,
            design: new_issue.design,
            acceptance_criteria: new_issue.acceptance_criteria,
            notes: new_issue.notes,
            external_ref: new_issue.external_ref,
            dependencies: dependencies.clone(),
            created_at: now,
            updated_at: now,
            closed_at: None,
        };

        // Store issue (node already added during validation)
        inner.issues.insert(id.clone(), issue.clone());

        // Add dependency edges (all validations passed, so this is safe)
        for (depends_on_id, dep_type) in new_issue.dependencies {
            let from_node = inner.node_map[&id];
            let to_node = inner.node_map[&depends_on_id];
            inner.graph.add_edge(from_node, to_node, dep_type);
        }

        Ok(issue)
    }

    async fn get(&self, id: &IssueId) -> Result<Option<Issue>> {
        let inner = self.lock().await;
        Ok(inner.issues.get(id).cloned())
    }

    async fn update(&mut self, id: &IssueId, updates: IssueUpdate) -> Result<Issue> {
        let mut inner = self.lock().await;

        let issue = inner
            .issues
            .get_mut(id)
            .ok_or_else(|| Error::IssueNotFound(id.clone()))?;

        // Apply updates
        if let Some(title) = updates.title {
            issue.title = title;
        }
        if let Some(description) = updates.description {
            issue.description = description;
        }
        if let Some(status) = updates.status {
            issue.status = status;
            // Set closed_at if status is closed
            if status == IssueStatus::Closed && issue.closed_at.is_none() {
                issue.closed_at = Some(Utc::now());
            }
        }
        if let Some(priority) = updates.priority {
            if priority > 4 {
                return Err(Error::InvalidPriority(priority));
            }
            issue.priority = priority;
        }
        if let Some(assignee_opt) = updates.assignee {
            issue.assignee = assignee_opt;
        }
        if let Some(design) = updates.design {
            issue.design = Some(design);
        }
        if let Some(acceptance_criteria) = updates.acceptance_criteria {
            issue.acceptance_criteria = Some(acceptance_criteria);
        }
        if let Some(notes) = updates.notes {
            issue.notes = Some(notes);
        }
        if let Some(external_ref) = updates.external_ref {
            issue.external_ref = Some(external_ref);
        }

        // Validate the updated issue to ensure data integrity
        // This catches invalid titles, descriptions, or priorities that may have been set
        issue
            .validate()
            .map_err(|e| Error::Storage(format!("Validation failed: {}", e)))?;

        issue.updated_at = Utc::now();

        Ok(issue.clone())
    }

    async fn delete(&mut self, id: &IssueId) -> Result<()> {
        let mut inner = self.lock().await;

        // Check if issue exists
        if !inner.issues.contains_key(id) {
            return Err(Error::IssueNotFound(id.clone()));
        }

        // Check for dependents
        let node = inner.node_map[id];
        let dependents: Vec<_> = inner
            .graph
            .edges_directed(node, Direction::Incoming)
            .map(|edge| inner.graph[edge.source()].clone())
            .collect();

        if !dependents.is_empty() {
            return Err(Error::HasDependents {
                issue_id: id.clone(),
                dependent_count: dependents.len(),
                dependents,
            });
        }

        // Remove from graph
        inner.graph.remove_node(node);
        inner.node_map.remove(id);

        // Remove from issues
        inner.issues.remove(id);

        Ok(())
    }

    async fn add_dependency(
        &mut self,
        from: &IssueId,
        to: &IssueId,
        dep_type: DependencyType,
    ) -> Result<()> {
        let mut inner = self.lock().await;

        // Validate both issues exist
        if !inner.issues.contains_key(from) {
            return Err(Error::IssueNotFound(from.clone()));
        }
        if !inner.issues.contains_key(to) {
            return Err(Error::IssueNotFound(to.clone()));
        }

        // Get node indices (we know they exist from the checks above)
        let from_node = inner.node_map[from];
        let to_node = inner.node_map[to];

        // Check for duplicate dependency using graph lookup (O(1) with find_edge)
        // This is more efficient than iterating through the issue.dependencies vector
        if inner.graph.find_edge(from_node, to_node).is_some() {
            return Err(Error::Storage(format!(
                "Dependency already exists: {} -> {}",
                from, to
            )));
        }

        // Check for cycles (must be done after duplicate check to avoid false positives)
        if inner.has_cycle_impl(from, to)? {
            return Err(Error::CircularDependency {
                from: from.clone(),
                to: to.clone(),
            });
        }

        // Add edge to graph
        inner.graph.add_edge(from_node, to_node, dep_type);

        // Also add to issue's dependencies vector for JSONL serialization
        let issue = inner
            .issues
            .get_mut(from)
            .ok_or_else(|| Error::IssueNotFound(from.clone()))?;
        issue.dependencies.push(Dependency {
            depends_on_id: to.clone(),
            dep_type,
        });

        Ok(())
    }

    async fn remove_dependency(&mut self, from: &IssueId, to: &IssueId) -> Result<()> {
        let mut inner = self.lock().await;

        let from_node = inner
            .node_map
            .get(from)
            .ok_or_else(|| Error::IssueNotFound(from.clone()))?;
        let to_node = inner
            .node_map
            .get(to)
            .ok_or_else(|| Error::IssueNotFound(to.clone()))?;

        // Find and remove the edge
        let edge = inner.graph.find_edge(*from_node, *to_node).ok_or_else(|| {
            Error::DependencyNotFound {
                from: from.clone(),
                to: to.clone(),
            }
        })?;

        inner.graph.remove_edge(edge);

        // Also remove from issue's dependencies vector for JSONL serialization
        let issue = inner
            .issues
            .get_mut(from)
            .ok_or_else(|| Error::IssueNotFound(from.clone()))?;
        issue.dependencies.retain(|dep| dep.depends_on_id != *to);

        Ok(())
    }

    async fn get_dependencies(&self, id: &IssueId) -> Result<Vec<Dependency>> {
        let inner = self.lock().await;

        let node = inner
            .node_map
            .get(id)
            .ok_or_else(|| Error::IssueNotFound(id.clone()))?;

        let deps = inner
            .graph
            .edges(*node)
            .map(|edge| Dependency {
                depends_on_id: inner.graph[edge.target()].clone(),
                dep_type: *edge.weight(),
            })
            .collect();

        Ok(deps)
    }

    async fn get_dependents(&self, id: &IssueId) -> Result<Vec<Dependency>> {
        let inner = self.lock().await;

        let node = inner
            .node_map
            .get(id)
            .ok_or_else(|| Error::IssueNotFound(id.clone()))?;

        let deps = inner
            .graph
            .edges_directed(*node, Direction::Incoming)
            .map(|edge| Dependency {
                depends_on_id: inner.graph[edge.source()].clone(),
                dep_type: *edge.weight(),
            })
            .collect();

        Ok(deps)
    }

    async fn has_cycle(&self, from: &IssueId, to: &IssueId) -> Result<bool> {
        let inner = self.lock().await;
        inner.has_cycle_impl(from, to)
    }

    async fn get_dependency_tree(
        &self,
        id: &IssueId,
        max_depth: Option<usize>,
    ) -> Result<Vec<(Dependency, usize)>> {
        let inner = self.lock().await;
        inner.get_dependency_tree_impl(id, max_depth)
    }

    async fn list(&self, filter: &IssueFilter) -> Result<Vec<Issue>> {
        let inner = self.lock().await;

        let mut issues: Vec<Issue> = inner
            .issues
            .values()
            .filter(|issue| {
                // Apply status filter
                if let Some(status) = &filter.status {
                    if &issue.status != status {
                        return false;
                    }
                }

                // Apply priority filter
                if let Some(priority) = filter.priority {
                    if issue.priority != priority {
                        return false;
                    }
                }

                // Apply type filter
                if let Some(issue_type) = &filter.issue_type {
                    if &issue.issue_type != issue_type {
                        return false;
                    }
                }

                // Apply assignee filter
                if let Some(assignee) = &filter.assignee {
                    if issue.assignee.as_ref() != Some(assignee) {
                        return false;
                    }
                }

                // Apply label filter
                if let Some(label) = &filter.label {
                    if !issue.labels.contains(label) {
                        return false;
                    }
                }

                true
            })
            .cloned()
            .collect();

        // Sort by created_at (most recent first)
        issues.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        Ok(issues)
    }

    async fn ready_to_work(&self, filter: Option<&IssueFilter>) -> Result<Vec<Issue>> {
        let inner = self.lock().await;

        // Find all blocked issues
        let mut blocked = HashSet::new();

        // Direct blocks: issues with blocking dependencies
        for id in inner.issues.keys() {
            let node = inner.node_map[id];

            for edge in inner.graph.edges(node) {
                if edge.weight() == &DependencyType::Blocks {
                    let blocker_id = &inner.graph[edge.target()];
                    if let Some(blocker) = inner.issues.get(blocker_id) {
                        if blocker.status != IssueStatus::Closed {
                            blocked.insert(id.clone());
                            break;
                        }
                    }
                }
            }
        }

        // Transitive blocking via parent-child (BFS with depth limit)
        let mut to_process: VecDeque<(IssueId, usize)> =
            blocked.iter().map(|id| (id.clone(), 0)).collect();

        while let Some((id, depth)) = to_process.pop_front() {
            if depth >= 50 {
                continue;
            }

            // Find children (issues that depend on this one via parent-child)
            let node = inner.node_map[&id];
            for edge in inner.graph.edges_directed(node, Direction::Incoming) {
                if edge.weight() == &DependencyType::ParentChild {
                    let child_id = &inner.graph[edge.source()];
                    if blocked.insert(child_id.clone()) {
                        to_process.push_back((child_id.clone(), depth + 1));
                    }
                }
            }
        }

        // Filter out blocked and closed issues
        let mut ready: Vec<Issue> = inner
            .issues
            .values()
            .filter(|issue| issue.status != IssueStatus::Closed && !blocked.contains(&issue.id))
            .cloned()
            .collect();

        // Apply additional filter if provided
        if let Some(filter) = filter {
            ready.retain(|issue| {
                // Apply status filter
                if let Some(status) = &filter.status {
                    if &issue.status != status {
                        return false;
                    }
                }

                // Apply priority filter
                if let Some(priority) = filter.priority {
                    if issue.priority != priority {
                        return false;
                    }
                }

                // Apply type filter
                if let Some(issue_type) = &filter.issue_type {
                    if &issue.issue_type != issue_type {
                        return false;
                    }
                }

                // Apply assignee filter
                if let Some(assignee) = &filter.assignee {
                    if issue.assignee.as_ref() != Some(assignee) {
                        return false;
                    }
                }

                // Apply label filter
                if let Some(label) = &filter.label {
                    if !issue.labels.contains(label) {
                        return false;
                    }
                }

                true
            });
        }

        // Sort by priority (lower number = higher priority) then by age
        ready.sort_by(|a, b| {
            a.priority
                .cmp(&b.priority)
                .then(a.created_at.cmp(&b.created_at))
        });

        Ok(ready)
    }

    async fn blocked_issues(&self) -> Result<Vec<(Issue, Vec<Issue>)>> {
        let inner = self.lock().await;

        let mut blocked_list = Vec::new();

        for (id, issue) in &inner.issues {
            if issue.status == IssueStatus::Closed {
                continue;
            }

            let node = inner.node_map[id];
            let mut blockers = Vec::new();

            for edge in inner.graph.edges(node) {
                if edge.weight() == &DependencyType::Blocks {
                    let blocker_id = &inner.graph[edge.target()];
                    if let Some(blocker) = inner.issues.get(blocker_id) {
                        if blocker.status != IssueStatus::Closed {
                            blockers.push(blocker.clone());
                        }
                    }
                }
            }

            if !blockers.is_empty() {
                blocked_list.push((issue.clone(), blockers));
            }
        }

        Ok(blocked_list)
    }

    async fn import_issues(&mut self, issues: Vec<Issue>) -> Result<()> {
        let mut inner = self.lock().await;

        // First pass: Add all issues and create nodes
        for issue in &issues {
            // Add to graph
            let node = inner.graph.add_node(issue.id.clone());
            inner.node_map.insert(issue.id.clone(), node);

            // Store issue
            inner.issues.insert(issue.id.clone(), issue.clone());

            // Register ID with generator
            inner
                .id_generator
                .register_id(issue.id.as_str().to_string());
        }

        // Second pass: Reconstruct dependency edges
        // Now that all issues are loaded, we can safely add edges
        for issue in &issues {
            for dep in &issue.dependencies {
                // Verify the dependency target exists
                if !inner.node_map.contains_key(&dep.depends_on_id) {
                    // Skip orphaned dependencies (target doesn't exist)
                    // This provides resilience for corrupted JSONL files
                    continue;
                }

                let from_node = inner.node_map[&issue.id];
                let to_node = inner.node_map[&dep.depends_on_id];

                // Add edge to graph
                inner.graph.add_edge(from_node, to_node, dep.dep_type);
            }
        }

        Ok(())
    }

    async fn export_all(&self) -> Result<Vec<Issue>> {
        let inner = self.lock().await;
        Ok(inner.issues.values().cloned().collect())
    }

    async fn save(&self) -> Result<()> {
        // In-memory storage doesn't persist to disk
        // This is a no-op for this implementation
        Ok(())
    }
}

impl InMemoryStorageInner {
    /// Internal implementation of dependency tree traversal.
    ///
    /// Uses BFS to traverse the dependency graph, returning all transitive
    /// dependencies with their depth level.
    fn get_dependency_tree_impl(
        &self,
        id: &IssueId,
        max_depth: Option<usize>,
    ) -> Result<Vec<(Dependency, usize)>> {
        let start_node = self
            .node_map
            .get(id)
            .ok_or_else(|| Error::IssueNotFound(id.clone()))?;

        let mut result = Vec::new();
        let mut visited = HashSet::new();
        let mut queue: VecDeque<(NodeIndex, usize)> = VecDeque::new();

        // Start BFS from direct dependencies (depth 1)
        for edge in self.graph.edges(*start_node) {
            let target_node = edge.target();
            if visited.insert(target_node) {
                queue.push_back((target_node, 1));
                result.push((
                    Dependency {
                        depends_on_id: self.graph[target_node].clone(),
                        dep_type: *edge.weight(),
                    },
                    1,
                ));
            }
        }

        // BFS traversal for transitive dependencies
        while let Some((current_node, depth)) = queue.pop_front() {
            // Check max depth limit
            if let Some(max) = max_depth {
                if depth >= max {
                    continue;
                }
            }

            // Explore dependencies of current node
            for edge in self.graph.edges(current_node) {
                let target_node = edge.target();
                if visited.insert(target_node) {
                    let next_depth = depth + 1;
                    queue.push_back((target_node, next_depth));
                    result.push((
                        Dependency {
                            depends_on_id: self.graph[target_node].clone(),
                            dep_type: *edge.weight(),
                        },
                        next_depth,
                    ));
                }
            }
        }

        Ok(result)
    }

    /// Internal implementation of cycle detection.
    ///
    /// Uses petgraph's `has_path_connecting` to check if adding
    /// an edge from `from` to `to` would create a cycle.
    fn has_cycle_impl(&self, from: &IssueId, to: &IssueId) -> Result<bool> {
        let from_node = self
            .node_map
            .get(from)
            .ok_or_else(|| Error::IssueNotFound(from.clone()))?;
        let to_node = self
            .node_map
            .get(to)
            .ok_or_else(|| Error::IssueNotFound(to.clone()))?;

        // Check if there's already a path from `to` to `from`
        // If so, adding `from -> to` would create a cycle
        Ok(algo::has_path_connecting(
            &self.graph,
            *to_node,
            *from_node,
            None,
        ))
    }

    /// Export all dependencies from the graph.
    ///
    /// Returns a list of (from, to, type) tuples representing all edges in the dependency graph.
    ///
    /// **Note**: This is a helper method for future use (JSONL backend, extended trait API).
    /// Not currently exposed through the IssueStorage trait.
    #[allow(dead_code)]
    fn export_dependencies(&self) -> Vec<(IssueId, IssueId, DependencyType)> {
        self.graph
            .edge_references()
            .map(|edge| {
                let from = &self.graph[edge.source()];
                let to = &self.graph[edge.target()];
                let dep_type = *edge.weight();
                (from.clone(), to.clone(), dep_type)
            })
            .collect()
    }

    /// Import dependencies into the graph.
    ///
    /// Assumes all referenced issues have already been imported.
    /// Skips dependencies where either endpoint doesn't exist.
    ///
    /// **Note**: This is a helper method for future use (JSONL backend, extended trait API).
    /// Not currently exposed through the IssueStorage trait.
    #[allow(dead_code)]
    fn import_dependencies(&mut self, dependencies: Vec<(IssueId, IssueId, DependencyType)>) {
        for (from_id, to_id, dep_type) in dependencies {
            // Skip if either issue doesn't exist
            if !self.node_map.contains_key(&from_id) || !self.node_map.contains_key(&to_id) {
                continue;
            }

            let from_node = self.node_map[&from_id];
            let to_node = self.node_map[&to_id];

            // Add edge (skip cycle check since we're importing existing data)
            self.graph.add_edge(from_node, to_node, dep_type);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{IssueStatus, IssueType};

    fn create_test_issue(title: &str) -> NewIssue {
        NewIssue {
            title: title.to_string(),
            description: "Test description".to_string(),
            priority: 2,
            issue_type: IssueType::Task,
            assignee: None,
            labels: vec![],
            design: None,
            acceptance_criteria: None,
            notes: None,
            external_ref: None,
            dependencies: vec![],
        }
    }

    #[tokio::test]
    async fn test_create_issue() {
        let mut storage = new_in_memory_storage("test".to_string());

        let new_issue = create_test_issue("Test Issue");
        let issue = storage.create(new_issue).await.unwrap();

        assert!(issue.id.as_str().starts_with("test-"));
        assert_eq!(issue.title, "Test Issue");
        assert_eq!(issue.status, IssueStatus::Open);
        assert_eq!(issue.priority, 2);
    }

    #[tokio::test]
    async fn test_get_issue() {
        let mut storage = new_in_memory_storage("test".to_string());

        let new_issue = create_test_issue("Test Issue");
        let created = storage.create(new_issue).await.unwrap();

        // Get existing issue
        let retrieved = storage.get(&created.id).await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().title, "Test Issue");

        // Get non-existing issue
        let non_existing = storage
            .get(&IssueId::new("test-nonexistent"))
            .await
            .unwrap();
        assert!(non_existing.is_none());
    }

    #[tokio::test]
    async fn test_update_issue() {
        let mut storage = new_in_memory_storage("test".to_string());

        let new_issue = create_test_issue("Original Title");
        let created = storage.create(new_issue).await.unwrap();

        let updates = IssueUpdate {
            title: Some("Updated Title".to_string()),
            status: Some(IssueStatus::InProgress),
            priority: Some(1),
            ..Default::default()
        };

        let updated = storage.update(&created.id, updates).await.unwrap();
        assert_eq!(updated.title, "Updated Title");
        assert_eq!(updated.status, IssueStatus::InProgress);
        assert_eq!(updated.priority, 1);
    }

    #[tokio::test]
    async fn test_delete_issue() {
        let mut storage = new_in_memory_storage("test".to_string());

        let new_issue = create_test_issue("To Delete");
        let created = storage.create(new_issue).await.unwrap();

        // Delete should succeed
        storage.delete(&created.id).await.unwrap();

        // Issue should no longer exist
        let retrieved = storage.get(&created.id).await.unwrap();
        assert!(retrieved.is_none());
    }

    #[tokio::test]
    async fn test_delete_with_dependents() {
        let mut storage = new_in_memory_storage("test".to_string());

        let issue1 = storage.create(create_test_issue("Issue 1")).await.unwrap();
        let issue2 = storage.create(create_test_issue("Issue 2")).await.unwrap();

        // Issue 2 depends on Issue 1
        storage
            .add_dependency(&issue2.id, &issue1.id, DependencyType::Blocks)
            .await
            .unwrap();

        // Deleting issue1 should fail because issue2 depends on it
        let result = storage.delete(&issue1.id).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::HasDependents { .. }));
    }

    #[tokio::test]
    async fn test_add_dependency() {
        let mut storage = new_in_memory_storage("test".to_string());

        let issue1 = storage.create(create_test_issue("Issue 1")).await.unwrap();
        let issue2 = storage.create(create_test_issue("Issue 2")).await.unwrap();

        // Add dependency: issue2 depends on issue1
        storage
            .add_dependency(&issue2.id, &issue1.id, DependencyType::Blocks)
            .await
            .unwrap();

        // Get dependencies for issue2
        let deps = storage.get_dependencies(&issue2.id).await.unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].depends_on_id, issue1.id);
        assert_eq!(deps[0].dep_type, DependencyType::Blocks);

        // Get dependents for issue1
        let dependents = storage.get_dependents(&issue1.id).await.unwrap();
        assert_eq!(dependents.len(), 1);
        assert_eq!(dependents[0].depends_on_id, issue2.id);
    }

    #[tokio::test]
    async fn test_remove_dependency() {
        let mut storage = new_in_memory_storage("test".to_string());

        let issue1 = storage.create(create_test_issue("Issue 1")).await.unwrap();
        let issue2 = storage.create(create_test_issue("Issue 2")).await.unwrap();

        storage
            .add_dependency(&issue2.id, &issue1.id, DependencyType::Blocks)
            .await
            .unwrap();

        // Remove the dependency
        storage
            .remove_dependency(&issue2.id, &issue1.id)
            .await
            .unwrap();

        // Dependency should be gone
        let deps = storage.get_dependencies(&issue2.id).await.unwrap();
        assert_eq!(deps.len(), 0);
    }

    #[tokio::test]
    async fn test_cycle_detection() {
        let mut storage = new_in_memory_storage("test".to_string());

        let issue1 = storage.create(create_test_issue("Issue 1")).await.unwrap();
        let issue2 = storage.create(create_test_issue("Issue 2")).await.unwrap();
        let issue3 = storage.create(create_test_issue("Issue 3")).await.unwrap();

        // Create chain: 1 -> 2 -> 3
        storage
            .add_dependency(&issue1.id, &issue2.id, DependencyType::Blocks)
            .await
            .unwrap();
        storage
            .add_dependency(&issue2.id, &issue3.id, DependencyType::Blocks)
            .await
            .unwrap();

        // Adding 3 -> 1 would create a cycle
        let result = storage
            .add_dependency(&issue3.id, &issue1.id, DependencyType::Blocks)
            .await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            Error::CircularDependency { .. }
        ));
    }

    #[tokio::test]
    async fn test_list_with_filter() {
        let mut storage = new_in_memory_storage("test".to_string());

        let mut issue1 = create_test_issue("Issue 1");
        issue1.priority = 1;
        storage.create(issue1).await.unwrap();

        let mut issue2 = create_test_issue("Issue 2");
        issue2.priority = 2;
        storage.create(issue2).await.unwrap();

        // Filter by priority
        let filter = IssueFilter {
            priority: Some(1),
            ..Default::default()
        };
        let results = storage.list(&filter).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Issue 1");
    }

    #[tokio::test]
    async fn test_ready_to_work() {
        let mut storage = new_in_memory_storage("test".to_string());

        let issue1 = storage.create(create_test_issue("Blocker")).await.unwrap();
        let issue2 = storage.create(create_test_issue("Blocked")).await.unwrap();
        let _issue3 = storage.create(create_test_issue("Ready")).await.unwrap();

        // issue2 is blocked by issue1
        storage
            .add_dependency(&issue2.id, &issue1.id, DependencyType::Blocks)
            .await
            .unwrap();

        // Get ready issues
        let ready = storage.ready_to_work(None).await.unwrap();

        // issue3 and issue1 should be ready, issue2 should be blocked
        assert_eq!(ready.len(), 2);
        let ready_titles: Vec<_> = ready.iter().map(|i| i.title.as_str()).collect();
        assert!(ready_titles.contains(&"Blocker"));
        assert!(ready_titles.contains(&"Ready"));
        assert!(!ready_titles.contains(&"Blocked"));
    }

    #[tokio::test]
    async fn test_blocked_issues() {
        let mut storage = new_in_memory_storage("test".to_string());

        let issue1 = storage.create(create_test_issue("Blocker")).await.unwrap();
        let issue2 = storage.create(create_test_issue("Blocked")).await.unwrap();

        storage
            .add_dependency(&issue2.id, &issue1.id, DependencyType::Blocks)
            .await
            .unwrap();

        let blocked = storage.blocked_issues().await.unwrap();
        assert_eq!(blocked.len(), 1);
        assert_eq!(blocked[0].0.title, "Blocked");
        assert_eq!(blocked[0].1[0].title, "Blocker");
    }

    #[tokio::test]
    async fn test_import_export() {
        let mut storage = new_in_memory_storage("test".to_string());

        let issue1 = storage.create(create_test_issue("Issue 1")).await.unwrap();
        let issue2 = storage.create(create_test_issue("Issue 2")).await.unwrap();

        // Export all issues
        let exported_issues = storage.export_all().await.unwrap();
        assert_eq!(exported_issues.len(), 2);

        // Create new storage and import
        let mut new_storage = new_in_memory_storage("test".to_string());
        new_storage.import_issues(exported_issues).await.unwrap();

        // Verify imported issues
        let retrieved1 = new_storage.get(&issue1.id).await.unwrap();
        let retrieved2 = new_storage.get(&issue2.id).await.unwrap();
        assert!(retrieved1.is_some());
        assert!(retrieved2.is_some());

        assert_eq!(retrieved1.unwrap().title, "Issue 1");
        assert_eq!(retrieved2.unwrap().title, "Issue 2");

        // NOTE: Dependencies are NOT preserved by import_issues().
        // This is a known limitation of the current IssueStorage trait API.
        // See documentation on import_issues() for details and workarounds.
    }

    #[tokio::test]
    async fn test_save() {
        let storage = new_in_memory_storage("test".to_string());
        // Save is a no-op for in-memory storage, but should not error
        storage.save().await.unwrap();
    }

    #[tokio::test]
    async fn test_concurrent_creates() {
        use std::sync::Arc;
        use tokio::sync::Mutex as TokioMutex;

        // Verify Arc<Mutex<>> correctness under concurrent mutations
        let storage = Arc::new(TokioMutex::new(new_in_memory_storage("test".to_string())));

        // Spawn multiple concurrent tasks creating issues
        let mut handles = vec![];
        for i in 0..10 {
            let storage_clone = Arc::clone(&storage);
            let handle = tokio::spawn(async move {
                let mut storage_guard = storage_clone.lock().await;
                storage_guard
                    .create(create_test_issue(&format!("Concurrent Issue {}", i)))
                    .await
                    .unwrap()
            });
            handles.push(handle);
        }

        // Wait for all tasks to complete
        let mut created_ids = vec![];
        for handle in handles {
            let issue = handle.await.unwrap();
            created_ids.push(issue.id);
        }

        // Verify all issues were created with unique IDs
        assert_eq!(created_ids.len(), 10);
        let unique_ids: std::collections::HashSet<_> = created_ids.iter().collect();
        assert_eq!(unique_ids.len(), 10, "All IDs should be unique");

        // Verify we can retrieve all issues
        let storage_guard = storage.lock().await;
        for id in created_ids {
            let issue = storage_guard.get(&id).await.unwrap();
            assert!(issue.is_some());
        }
    }

    #[tokio::test]
    async fn test_performance_benchmark() {
        // Verify claim: "1000 issues in <10ms"
        use std::time::Instant;

        let mut storage = new_in_memory_storage("test".to_string());

        let start = Instant::now();

        // Create 1000 issues
        for i in 0..1000 {
            storage
                .create(create_test_issue(&format!("Performance Test Issue {}", i)))
                .await
                .unwrap();
        }

        let duration = start.elapsed();

        // Verify we created 1000 issues
        let all_issues = storage.export_all().await.unwrap();
        assert_eq!(all_issues.len(), 1000);

        // Log performance for visibility
        println!("Created 1000 issues in {:?}", duration);

        // Allow some buffer above 10ms for CI environments
        // In local testing this should be well under 10ms
        assert!(
            duration.as_millis() < 100,
            "Creating 1000 issues took {:?}, expected < 100ms",
            duration
        );
    }

    #[tokio::test]
    async fn test_jsonl_persistence_round_trip() {
        use tempfile::tempdir;

        // Create storage with some issues
        let mut storage = new_in_memory_storage("test".to_string());

        let issue1 = storage.create(create_test_issue("Issue 1")).await.unwrap();
        let issue2 = storage.create(create_test_issue("Issue 2")).await.unwrap();
        let issue3 = storage.create(create_test_issue("Issue 3")).await.unwrap();

        // Add dependencies
        storage
            .add_dependency(&issue2.id, &issue1.id, DependencyType::Blocks)
            .await
            .unwrap();
        storage
            .add_dependency(&issue3.id, &issue2.id, DependencyType::Related)
            .await
            .unwrap();

        // Save to JSONL
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("test.jsonl");

        super::save_to_jsonl(storage.as_ref(), &file_path)
            .await
            .unwrap();

        // Load from JSONL
        let (loaded_storage, warnings) = super::load_from_jsonl(&file_path, "test".to_string())
            .await
            .unwrap();

        // Verify no warnings
        assert!(
            warnings.is_empty(),
            "Expected no warnings, got: {:?}",
            warnings
        );

        // Verify all issues loaded
        let loaded_issues = loaded_storage.export_all().await.unwrap();
        assert_eq!(loaded_issues.len(), 3);

        // Verify issues match
        let loaded_issue1 = loaded_storage.get(&issue1.id).await.unwrap().unwrap();
        assert_eq!(loaded_issue1.title, issue1.title);
        assert_eq!(loaded_issue1.dependencies.len(), 0);

        let loaded_issue2 = loaded_storage.get(&issue2.id).await.unwrap().unwrap();
        assert_eq!(loaded_issue2.title, issue2.title);
        assert_eq!(loaded_issue2.dependencies.len(), 1);
        assert_eq!(loaded_issue2.dependencies[0].depends_on_id, issue1.id);
        assert_eq!(
            loaded_issue2.dependencies[0].dep_type,
            DependencyType::Blocks
        );

        let loaded_issue3 = loaded_storage.get(&issue3.id).await.unwrap().unwrap();
        assert_eq!(loaded_issue3.title, issue3.title);
        assert_eq!(loaded_issue3.dependencies.len(), 1);
        assert_eq!(loaded_issue3.dependencies[0].depends_on_id, issue2.id);
        assert_eq!(
            loaded_issue3.dependencies[0].dep_type,
            DependencyType::Related
        );

        // Verify dependency graph is correctly reconstructed
        let deps = loaded_storage.get_dependencies(&issue2.id).await.unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].depends_on_id, issue1.id);

        temp_dir.close().unwrap();
    }

    #[tokio::test]
    async fn test_load_jsonl_with_malformed_json() {
        use tempfile::tempdir;

        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("malformed.jsonl");

        // Create a JSONL file with one valid and one malformed line
        let mut storage = new_in_memory_storage("test".to_string());
        let issue1 = storage
            .create(create_test_issue("Valid Issue"))
            .await
            .unwrap();

        super::save_to_jsonl(storage.as_ref(), &file_path)
            .await
            .unwrap();

        // Append malformed JSON (without extra newlines)
        let existing_content = tokio::fs::read_to_string(&file_path).await.unwrap();
        let new_content = format!(
            "{}{{\"invalid\": \"json\", \"missing\": \"fields\"}}\n",
            existing_content
        );
        tokio::fs::write(&file_path, new_content).await.unwrap();

        // Load - should skip malformed line and add warning
        let (loaded_storage, warnings) = super::load_from_jsonl(&file_path, "test".to_string())
            .await
            .unwrap();

        // Should have 1 warning for malformed JSON
        assert_eq!(warnings.len(), 1, "Warnings: {:?}", warnings);
        match &warnings[0] {
            super::LoadWarning::MalformedJson {
                line_number,
                error: _,
            } => {
                assert_eq!(*line_number, 2);
            }
            _ => panic!("Expected MalformedJson warning"),
        }

        // Valid issue should still be loaded
        let loaded_issues = loaded_storage.export_all().await.unwrap();
        assert_eq!(loaded_issues.len(), 1);
        assert_eq!(loaded_issues[0].id, issue1.id);

        temp_dir.close().unwrap();
    }

    #[tokio::test]
    async fn test_load_jsonl_with_orphaned_dependency() {
        use tempfile::tempdir;

        // Create an issue with a dependency that doesn't exist
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("orphaned.jsonl");

        // Manually create JSONL with orphaned dependency
        let issue = Issue {
            id: IssueId::new("test-1"),
            title: "Issue with orphaned dep".to_string(),
            description: "Test".to_string(),
            status: IssueStatus::Open,
            priority: 1,
            issue_type: IssueType::Task,
            assignee: None,
            labels: vec![],
            design: None,
            acceptance_criteria: None,
            notes: None,
            external_ref: None,
            dependencies: vec![Dependency {
                depends_on_id: IssueId::new("nonexistent-issue"),
                dep_type: DependencyType::Blocks,
            }],
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            closed_at: None,
        };

        let json = serde_json::to_string(&issue).unwrap();
        tokio::fs::write(&file_path, format!("{}\n", json))
            .await
            .unwrap();

        // Load - should skip orphaned dependency and add warning
        let (loaded_storage, warnings) = super::load_from_jsonl(&file_path, "test".to_string())
            .await
            .unwrap();

        // Should have 1 warning for orphaned dependency
        assert_eq!(warnings.len(), 1);
        match &warnings[0] {
            super::LoadWarning::OrphanedDependency { from, to } => {
                assert_eq!(from.as_str(), "test-1");
                assert_eq!(to.as_str(), "nonexistent-issue");
            }
            _ => panic!("Expected OrphanedDependency warning"),
        }

        // Issue should be loaded but without the dependency
        let loaded_issue = loaded_storage
            .get(&IssueId::new("test-1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(loaded_issue.title, "Issue with orphaned dep");

        // Dependency graph should be empty
        let deps = loaded_storage
            .get_dependencies(&IssueId::new("test-1"))
            .await
            .unwrap();
        assert_eq!(deps.len(), 0);

        temp_dir.close().unwrap();
    }

    #[tokio::test]
    async fn test_save_jsonl_atomic_write() {
        use tempfile::tempdir;

        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("atomic.jsonl");

        // Create initial file
        let mut storage = new_in_memory_storage("test".to_string());
        storage.create(create_test_issue("Issue 1")).await.unwrap();

        super::save_to_jsonl(storage.as_ref(), &file_path)
            .await
            .unwrap();

        // Verify temp file doesn't exist after successful write
        let temp_path = file_path.with_extension("tmp");
        assert!(
            !temp_path.exists(),
            "Temp file should be removed after atomic rename"
        );

        // Verify final file exists
        assert!(file_path.exists(), "Final file should exist");

        temp_dir.close().unwrap();
    }

    #[tokio::test]
    async fn test_load_jsonl_with_circular_dependency() {
        use tempfile::tempdir;

        // Create two issues with circular dependencies manually
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("circular.jsonl");

        let issue1 = Issue {
            id: IssueId::new("test-1"),
            title: "Issue 1".to_string(),
            description: "Test".to_string(),
            status: IssueStatus::Open,
            priority: 1,
            issue_type: IssueType::Task,
            assignee: None,
            labels: vec![],
            design: None,
            acceptance_criteria: None,
            notes: None,
            external_ref: None,
            dependencies: vec![Dependency {
                depends_on_id: IssueId::new("test-2"),
                dep_type: DependencyType::Blocks,
            }],
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            closed_at: None,
        };

        let issue2 = Issue {
            id: IssueId::new("test-2"),
            title: "Issue 2".to_string(),
            description: "Test".to_string(),
            status: IssueStatus::Open,
            priority: 1,
            issue_type: IssueType::Task,
            assignee: None,
            labels: vec![],
            design: None,
            acceptance_criteria: None,
            notes: None,
            external_ref: None,
            dependencies: vec![Dependency {
                depends_on_id: IssueId::new("test-1"),
                dep_type: DependencyType::Blocks,
            }],
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            closed_at: None,
        };

        // Write both issues to JSONL
        let json1 = serde_json::to_string(&issue1).unwrap();
        let json2 = serde_json::to_string(&issue2).unwrap();
        tokio::fs::write(&file_path, format!("{}\n{}\n", json1, json2))
            .await
            .unwrap();

        // Load - should skip circular dependency and add warning
        let (loaded_storage, warnings) = super::load_from_jsonl(&file_path, "test".to_string())
            .await
            .unwrap();

        // Should have 1 warning for circular dependency
        // (one of the edges will be skipped to break the cycle)
        assert_eq!(warnings.len(), 1);
        match &warnings[0] {
            super::LoadWarning::CircularDependency { from, to } => {
                // One of the dependencies should be flagged
                assert!(
                    (from.as_str() == "test-1" && to.as_str() == "test-2")
                        || (from.as_str() == "test-2" && to.as_str() == "test-1")
                );
            }
            _ => panic!(
                "Expected CircularDependency warning, got: {:?}",
                warnings[0]
            ),
        }

        // Both issues should be loaded
        let loaded_issues = loaded_storage.export_all().await.unwrap();
        assert_eq!(loaded_issues.len(), 2);

        // Dependency graph should have only one edge (cycle broken)
        let deps1 = loaded_storage
            .get_dependencies(&IssueId::new("test-1"))
            .await
            .unwrap();
        let deps2 = loaded_storage
            .get_dependencies(&IssueId::new("test-2"))
            .await
            .unwrap();

        // Total dependencies should be 1 (cycle broken)
        assert_eq!(
            deps1.len() + deps2.len(),
            1,
            "Cycle should be broken, only one dependency edge should exist"
        );

        temp_dir.close().unwrap();
    }

    #[tokio::test]
    async fn test_full_persistence_workflow_integration() {
        use crate::domain::{DependencyType, IssueStatus, IssueType, IssueUpdate, NewIssue};
        use tempfile::tempdir;

        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("workflow.jsonl");

        // Step 1: Create storage and add issues
        let mut storage = super::new_in_memory_storage("test".to_string());

        let issue1 = storage
            .create(NewIssue {
                title: "Foundation work".to_string(),
                description: "Base implementation".to_string(),
                priority: 3,
                issue_type: IssueType::Task,
                assignee: Some("alice".to_string()),
                labels: vec!["backend".to_string()],
                design: None,
                acceptance_criteria: None,
                notes: None,
                external_ref: None,
                dependencies: vec![],
            })
            .await
            .unwrap();

        let issue2 = storage
            .create(NewIssue {
                title: "Build on foundation".to_string(),
                description: "Depends on issue1".to_string(),
                priority: 2,
                issue_type: IssueType::Feature,
                assignee: Some("bob".to_string()),
                labels: vec!["frontend".to_string()],
                design: None,
                acceptance_criteria: None,
                notes: None,
                external_ref: Some("GH-123".to_string()),
                dependencies: vec![],
            })
            .await
            .unwrap();

        let issue3 = storage
            .create(NewIssue {
                title: "Polish UI".to_string(),
                description: "Final touches".to_string(),
                priority: 1,
                issue_type: IssueType::Task,
                assignee: None,
                labels: vec![],
                design: None,
                acceptance_criteria: None,
                notes: None,
                external_ref: None,
                dependencies: vec![],
            })
            .await
            .unwrap();

        // Step 2: Add dependencies
        storage
            .add_dependency(&issue2.id, &issue1.id, DependencyType::Blocks)
            .await
            .unwrap();
        storage
            .add_dependency(&issue3.id, &issue2.id, DependencyType::Related)
            .await
            .unwrap();

        // Step 3: Update an issue
        storage
            .update(
                &issue1.id,
                IssueUpdate {
                    title: None,
                    description: Some("Updated description".to_string()),
                    status: Some(IssueStatus::InProgress),
                    priority: None,
                    assignee: None,
                    design: None,
                    acceptance_criteria: None,
                    notes: None,
                    external_ref: None,
                },
            )
            .await
            .unwrap();

        // Step 4: Save to JSONL
        super::save_to_jsonl(storage.as_ref(), &file_path)
            .await
            .unwrap();

        // Step 5: Load from JSONL into new storage
        let (reloaded_storage, warnings) = super::load_from_jsonl(&file_path, "test".to_string())
            .await
            .unwrap();

        // Verify no warnings during load
        assert!(
            warnings.is_empty(),
            "Expected clean load, got warnings: {:?}",
            warnings
        );

        // Step 6: Verify all issues loaded correctly
        let loaded_issues = reloaded_storage.export_all().await.unwrap();
        assert_eq!(loaded_issues.len(), 3, "Should have 3 issues");

        // Step 7: Verify issue1 was updated correctly
        let loaded_issue1 = reloaded_storage.get(&issue1.id).await.unwrap().unwrap();
        assert_eq!(loaded_issue1.status, IssueStatus::InProgress);
        assert_eq!(loaded_issue1.description, "Updated description");
        assert_eq!(loaded_issue1.title, "Foundation work");
        assert_eq!(loaded_issue1.assignee, Some("alice".to_string()));
        assert_eq!(loaded_issue1.labels, vec!["backend".to_string()]);

        // Step 8: Verify issue2 loaded with correct properties
        let loaded_issue2 = reloaded_storage.get(&issue2.id).await.unwrap().unwrap();
        assert_eq!(loaded_issue2.title, "Build on foundation");
        assert_eq!(loaded_issue2.external_ref, Some("GH-123".to_string()));
        assert_eq!(loaded_issue2.priority, 2);

        // Step 9: Verify dependencies were preserved
        let deps2 = reloaded_storage.get_dependencies(&issue2.id).await.unwrap();
        assert_eq!(deps2.len(), 1);
        assert_eq!(deps2[0].depends_on_id, issue1.id);
        assert_eq!(deps2[0].dep_type, DependencyType::Blocks);

        let deps3 = reloaded_storage.get_dependencies(&issue3.id).await.unwrap();
        assert_eq!(deps3.len(), 1);
        assert_eq!(deps3[0].depends_on_id, issue2.id);
        assert_eq!(deps3[0].dep_type, DependencyType::Related);

        // Step 10: Verify dependents (reverse dependencies)
        let dependents1 = reloaded_storage.get_dependents(&issue1.id).await.unwrap();
        assert_eq!(dependents1.len(), 1);
        // get_dependents returns dependencies where depends_on_id is the ID of the depending issue
        assert_eq!(dependents1[0].depends_on_id, issue2.id);

        // Step 11: Verify blocked issues query
        let blocked = reloaded_storage.blocked_issues().await.unwrap();
        // issue2 is blocked by issue1 (in progress)
        // issue3 depends on issue2 (open) but it's a Related dependency, not blocking
        assert!(
            blocked
                .iter()
                .any(|(issue, _blockers)| issue.id == issue2.id),
            "Issue2 should be in blocked list"
        );

        // Step 12: Verify ready-to-work issues
        let ready = reloaded_storage.ready_to_work(None).await.unwrap();
        // issue1 has no dependencies and is in progress
        // issue3 has only Related dependencies, so it's ready
        assert!(
            ready.iter().any(|i| i.id == issue3.id),
            "Issue3 should be ready (Related deps don't block)"
        );

        // Step 13: Perform operations on reloaded storage
        // Add a new dependency
        let mut reloaded_storage_mut = reloaded_storage;
        reloaded_storage_mut
            .add_dependency(&issue3.id, &issue1.id, DependencyType::Blocks)
            .await
            .unwrap();

        // Verify the new dependency
        let deps3_updated = reloaded_storage_mut
            .get_dependencies(&issue3.id)
            .await
            .unwrap();
        assert_eq!(deps3_updated.len(), 2);
        assert!(deps3_updated
            .iter()
            .any(|d| d.depends_on_id == issue1.id && d.dep_type == DependencyType::Blocks));

        // Step 14: Update an issue after reload
        reloaded_storage_mut
            .update(
                &issue2.id,
                IssueUpdate {
                    title: None,
                    description: None,
                    status: Some(IssueStatus::Closed),
                    priority: None,
                    assignee: None,
                    design: None,
                    acceptance_criteria: None,
                    notes: None,
                    external_ref: None,
                },
            )
            .await
            .unwrap();

        let final_issue2 = reloaded_storage_mut.get(&issue2.id).await.unwrap().unwrap();
        assert_eq!(final_issue2.status, IssueStatus::Closed);

        // Step 15: Save again and verify roundtrip consistency
        let file_path2 = temp_dir.path().join("workflow2.jsonl");
        super::save_to_jsonl(reloaded_storage_mut.as_ref(), &file_path2)
            .await
            .unwrap();

        let (final_storage, final_warnings) =
            super::load_from_jsonl(&file_path2, "test".to_string())
                .await
                .unwrap();

        assert!(final_warnings.is_empty());
        let final_issues = final_storage.export_all().await.unwrap();
        assert_eq!(final_issues.len(), 3);

        temp_dir.close().unwrap();
    }

    #[tokio::test]
    async fn test_id_generator_state_preservation_on_reload() {
        use crate::domain::{IssueType, NewIssue};
        use tempfile::tempdir;

        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("id_gen_test.jsonl");

        // Step 1: Create storage and add some issues
        let mut storage = super::new_in_memory_storage("test".to_string());

        let issue1 = storage
            .create(NewIssue {
                title: "First issue".to_string(),
                description: "Description".to_string(),
                priority: 2,
                issue_type: IssueType::Task,
                assignee: None,
                labels: vec![],
                design: None,
                acceptance_criteria: None,
                notes: None,
                external_ref: None,
                dependencies: vec![],
            })
            .await
            .unwrap();

        let issue2 = storage
            .create(NewIssue {
                title: "Second issue".to_string(),
                description: "Description".to_string(),
                priority: 2,
                issue_type: IssueType::Task,
                assignee: None,
                labels: vec![],
                design: None,
                acceptance_criteria: None,
                notes: None,
                external_ref: None,
                dependencies: vec![],
            })
            .await
            .unwrap();

        // Collect existing IDs
        let existing_ids: Vec<String> = vec![
            issue1.id.as_str().to_string(),
            issue2.id.as_str().to_string(),
        ];

        // Step 2: Save to JSONL
        super::save_to_jsonl(storage.as_ref(), &file_path)
            .await
            .unwrap();

        // Step 3: Load from JSONL
        let (mut reloaded_storage, warnings) =
            super::load_from_jsonl(&file_path, "test".to_string())
                .await
                .unwrap();

        assert!(warnings.is_empty());

        // Step 4: Create new issues after reload and verify no ID collisions
        let issue3 = reloaded_storage
            .create(NewIssue {
                title: "Third issue after reload".to_string(),
                description: "Description".to_string(),
                priority: 2,
                issue_type: IssueType::Task,
                assignee: None,
                labels: vec![],
                design: None,
                acceptance_criteria: None,
                notes: None,
                external_ref: None,
                dependencies: vec![],
            })
            .await
            .unwrap();

        let issue4 = reloaded_storage
            .create(NewIssue {
                title: "Fourth issue after reload".to_string(),
                description: "Description".to_string(),
                priority: 2,
                issue_type: IssueType::Task,
                assignee: None,
                labels: vec![],
                design: None,
                acceptance_criteria: None,
                notes: None,
                external_ref: None,
                dependencies: vec![],
            })
            .await
            .unwrap();

        // Step 5: Verify no collisions - new IDs should be different from existing ones
        assert!(
            !existing_ids.contains(&issue3.id.as_str().to_string()),
            "New issue3 ID '{}' collides with existing IDs: {:?}",
            issue3.id.as_str(),
            existing_ids
        );
        assert!(
            !existing_ids.contains(&issue4.id.as_str().to_string()),
            "New issue4 ID '{}' collides with existing IDs: {:?}",
            issue4.id.as_str(),
            existing_ids
        );

        // Step 6: Verify all 4 issues exist
        let all_issues = reloaded_storage.export_all().await.unwrap();
        assert_eq!(all_issues.len(), 4);

        temp_dir.close().unwrap();
    }

    #[tokio::test]
    async fn test_load_jsonl_with_invalid_issue_data() {
        use crate::domain::{IssueId, IssueStatus, IssueType};
        use chrono::Utc;
        use tempfile::tempdir;

        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("invalid.jsonl");

        // Create an issue with invalid priority (> 4)
        let invalid_issue = Issue {
            id: IssueId::new("test-invalid"),
            title: "Invalid priority issue".to_string(),
            description: "This has invalid priority".to_string(),
            status: IssueStatus::Open,
            priority: 10, // Invalid - exceeds maximum of 4
            issue_type: IssueType::Task,
            assignee: None,
            labels: vec![],
            design: None,
            acceptance_criteria: None,
            notes: None,
            external_ref: None,
            dependencies: vec![],
            created_at: Utc::now(),
            updated_at: Utc::now(),
            closed_at: None,
        };

        // Create a valid issue
        let valid_issue = Issue {
            id: IssueId::new("test-valid"),
            title: "Valid issue".to_string(),
            description: "This is valid".to_string(),
            status: IssueStatus::Open,
            priority: 2,
            issue_type: IssueType::Task,
            assignee: None,
            labels: vec![],
            design: None,
            acceptance_criteria: None,
            notes: None,
            external_ref: None,
            dependencies: vec![],
            created_at: Utc::now(),
            updated_at: Utc::now(),
            closed_at: None,
        };

        // Write both issues to JSONL
        let json_invalid = serde_json::to_string(&invalid_issue).unwrap();
        let json_valid = serde_json::to_string(&valid_issue).unwrap();
        tokio::fs::write(&file_path, format!("{}\n{}\n", json_invalid, json_valid))
            .await
            .unwrap();

        // Load - should get a warning for the invalid issue
        let (loaded_storage, warnings) = super::load_from_jsonl(&file_path, "test".to_string())
            .await
            .unwrap();

        // Should have 1 warning for invalid issue data
        assert_eq!(warnings.len(), 1);
        match &warnings[0] {
            super::LoadWarning::InvalidIssueData {
                issue_id,
                line_number,
                error,
            } => {
                assert_eq!(issue_id.as_str(), "test-invalid");
                assert_eq!(*line_number, 1);
                assert!(error.contains("Priority"));
                assert!(error.contains("exceeds maximum"));
            }
            _ => panic!("Expected InvalidIssueData warning, got: {:?}", warnings[0]),
        }

        // Only the valid issue should be loaded
        let loaded_issues = loaded_storage.export_all().await.unwrap();
        assert_eq!(loaded_issues.len(), 1);
        assert_eq!(loaded_issues[0].id.as_str(), "test-valid");

        temp_dir.close().unwrap();
    }

    // ========== Dependency Type Tests ==========

    #[tokio::test]
    async fn test_all_dependency_types() {
        let mut storage = new_in_memory_storage("test".to_string());

        let issue1 = storage.create(create_test_issue("Blocker")).await.unwrap();
        let issue2 = storage.create(create_test_issue("Related")).await.unwrap();
        let issue3 = storage.create(create_test_issue("Parent")).await.unwrap();
        let issue4 = storage
            .create(create_test_issue("Discovered"))
            .await
            .unwrap();
        let main_issue = storage
            .create(create_test_issue("Main Issue"))
            .await
            .unwrap();

        // Add all 4 dependency types
        storage
            .add_dependency(&main_issue.id, &issue1.id, DependencyType::Blocks)
            .await
            .unwrap();
        storage
            .add_dependency(&main_issue.id, &issue2.id, DependencyType::Related)
            .await
            .unwrap();
        storage
            .add_dependency(&main_issue.id, &issue3.id, DependencyType::ParentChild)
            .await
            .unwrap();
        storage
            .add_dependency(&main_issue.id, &issue4.id, DependencyType::DiscoveredFrom)
            .await
            .unwrap();

        // Verify all dependencies
        let deps = storage.get_dependencies(&main_issue.id).await.unwrap();
        assert_eq!(deps.len(), 4);

        // Verify each type
        assert!(deps
            .iter()
            .any(|d| d.depends_on_id == issue1.id && d.dep_type == DependencyType::Blocks));
        assert!(deps
            .iter()
            .any(|d| d.depends_on_id == issue2.id && d.dep_type == DependencyType::Related));
        assert!(deps
            .iter()
            .any(|d| d.depends_on_id == issue3.id && d.dep_type == DependencyType::ParentChild));
        assert!(deps
            .iter()
            .any(|d| d.depends_on_id == issue4.id && d.dep_type == DependencyType::DiscoveredFrom));
    }

    #[tokio::test]
    async fn test_blocks_dependency_type() {
        let mut storage = new_in_memory_storage("test".to_string());

        let blocker = storage
            .create(create_test_issue("Blocker Task"))
            .await
            .unwrap();
        let blocked = storage
            .create(create_test_issue("Blocked Task"))
            .await
            .unwrap();

        storage
            .add_dependency(&blocked.id, &blocker.id, DependencyType::Blocks)
            .await
            .unwrap();

        let deps = storage.get_dependencies(&blocked.id).await.unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].dep_type, DependencyType::Blocks);

        // Blocked task should appear in blocked_issues
        let blocked_list = storage.blocked_issues().await.unwrap();
        assert_eq!(blocked_list.len(), 1);
        assert_eq!(blocked_list[0].0.id, blocked.id);
    }

    #[tokio::test]
    async fn test_related_dependency_type() {
        let mut storage = new_in_memory_storage("test".to_string());

        let issue1 = storage
            .create(create_test_issue("Related Issue 1"))
            .await
            .unwrap();
        let issue2 = storage
            .create(create_test_issue("Related Issue 2"))
            .await
            .unwrap();

        storage
            .add_dependency(&issue2.id, &issue1.id, DependencyType::Related)
            .await
            .unwrap();

        let deps = storage.get_dependencies(&issue2.id).await.unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].dep_type, DependencyType::Related);

        // Related dependency should NOT block work
        let ready = storage.ready_to_work(None).await.unwrap();
        assert_eq!(ready.len(), 2);
    }

    #[tokio::test]
    async fn test_parent_child_dependency_type() {
        let mut storage = new_in_memory_storage("test".to_string());

        let mut epic = create_test_issue("Epic");
        epic.issue_type = IssueType::Epic;
        let epic = storage.create(epic).await.unwrap();

        let subtask = storage.create(create_test_issue("Subtask")).await.unwrap();

        storage
            .add_dependency(&subtask.id, &epic.id, DependencyType::ParentChild)
            .await
            .unwrap();

        let deps = storage.get_dependencies(&subtask.id).await.unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].dep_type, DependencyType::ParentChild);

        // Get dependents of epic
        let dependents = storage.get_dependents(&epic.id).await.unwrap();
        assert_eq!(dependents.len(), 1);
        assert_eq!(dependents[0].depends_on_id, subtask.id);
    }

    #[tokio::test]
    async fn test_discovered_from_dependency_type() {
        let mut storage = new_in_memory_storage("test".to_string());

        let original_task = storage
            .create(create_test_issue("Original Task"))
            .await
            .unwrap();
        let discovered_bug = storage
            .create(create_test_issue("Discovered Bug"))
            .await
            .unwrap();

        storage
            .add_dependency(
                &discovered_bug.id,
                &original_task.id,
                DependencyType::DiscoveredFrom,
            )
            .await
            .unwrap();

        let deps = storage.get_dependencies(&discovered_bug.id).await.unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].dep_type, DependencyType::DiscoveredFrom);

        // DiscoveredFrom should NOT block work
        let ready = storage.ready_to_work(None).await.unwrap();
        assert_eq!(ready.len(), 2);
    }

    // ========== Dependency Tree Tests ==========

    #[tokio::test]
    async fn test_dependency_tree_simple_chain() {
        let mut storage = new_in_memory_storage("test".to_string());

        let issue_a = storage.create(create_test_issue("A")).await.unwrap();
        let issue_b = storage.create(create_test_issue("B")).await.unwrap();
        let issue_c = storage.create(create_test_issue("C")).await.unwrap();

        // A -> B -> C
        storage
            .add_dependency(&issue_a.id, &issue_b.id, DependencyType::Blocks)
            .await
            .unwrap();
        storage
            .add_dependency(&issue_b.id, &issue_c.id, DependencyType::Blocks)
            .await
            .unwrap();

        // Get tree from A
        let tree = storage
            .get_dependency_tree(&issue_a.id, None)
            .await
            .unwrap();
        assert_eq!(tree.len(), 2);

        // B should be at depth 1
        assert!(tree
            .iter()
            .any(|(d, depth)| d.depends_on_id == issue_b.id && *depth == 1));

        // C should be at depth 2
        assert!(tree
            .iter()
            .any(|(d, depth)| d.depends_on_id == issue_c.id && *depth == 2));
    }

    #[tokio::test]
    async fn test_dependency_tree_diamond() {
        let mut storage = new_in_memory_storage("test".to_string());

        //       A
        //      / \
        //     B   C
        //      \ /
        //       D

        let issue_a = storage.create(create_test_issue("A")).await.unwrap();
        let issue_b = storage.create(create_test_issue("B")).await.unwrap();
        let issue_c = storage.create(create_test_issue("C")).await.unwrap();
        let issue_d = storage.create(create_test_issue("D")).await.unwrap();

        storage
            .add_dependency(&issue_a.id, &issue_b.id, DependencyType::Blocks)
            .await
            .unwrap();
        storage
            .add_dependency(&issue_a.id, &issue_c.id, DependencyType::Blocks)
            .await
            .unwrap();
        storage
            .add_dependency(&issue_b.id, &issue_d.id, DependencyType::Blocks)
            .await
            .unwrap();
        storage
            .add_dependency(&issue_c.id, &issue_d.id, DependencyType::Blocks)
            .await
            .unwrap();

        let tree = storage
            .get_dependency_tree(&issue_a.id, None)
            .await
            .unwrap();

        // Should have B, C at depth 1, D at depth 2
        // D should only appear once due to visited tracking
        assert_eq!(tree.len(), 3);

        let depth_1_count = tree.iter().filter(|(_, depth)| *depth == 1).count();
        let depth_2_count = tree.iter().filter(|(_, depth)| *depth == 2).count();

        assert_eq!(depth_1_count, 2);
        assert_eq!(depth_2_count, 1);
    }

    #[tokio::test]
    async fn test_dependency_tree_with_max_depth() {
        let mut storage = new_in_memory_storage("test".to_string());

        let issue_a = storage.create(create_test_issue("A")).await.unwrap();
        let issue_b = storage.create(create_test_issue("B")).await.unwrap();
        let issue_c = storage.create(create_test_issue("C")).await.unwrap();
        let issue_d = storage.create(create_test_issue("D")).await.unwrap();

        // A -> B -> C -> D
        storage
            .add_dependency(&issue_a.id, &issue_b.id, DependencyType::Blocks)
            .await
            .unwrap();
        storage
            .add_dependency(&issue_b.id, &issue_c.id, DependencyType::Blocks)
            .await
            .unwrap();
        storage
            .add_dependency(&issue_c.id, &issue_d.id, DependencyType::Blocks)
            .await
            .unwrap();

        // Get tree with max_depth = 2
        let tree = storage
            .get_dependency_tree(&issue_a.id, Some(2))
            .await
            .unwrap();

        // Should only include B and C
        assert_eq!(tree.len(), 2);
        assert!(tree.iter().any(|(d, _)| d.depends_on_id == issue_b.id));
        assert!(tree.iter().any(|(d, _)| d.depends_on_id == issue_c.id));
        assert!(!tree.iter().any(|(d, _)| d.depends_on_id == issue_d.id));
    }

    #[tokio::test]
    async fn test_dependency_tree_empty() {
        let mut storage = new_in_memory_storage("test".to_string());

        let issue = storage
            .create(create_test_issue("No Dependencies"))
            .await
            .unwrap();

        let tree = storage.get_dependency_tree(&issue.id, None).await.unwrap();

        assert!(tree.is_empty());
    }

    #[tokio::test]
    async fn test_dependency_tree_not_found() {
        let storage = new_in_memory_storage("test".to_string());

        let result = storage
            .get_dependency_tree(&IssueId::new("nonexistent"), None)
            .await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::IssueNotFound(_)));
    }

    #[tokio::test]
    async fn test_dependency_tree_mixed_types() {
        let mut storage = new_in_memory_storage("test".to_string());

        let main = storage.create(create_test_issue("Main")).await.unwrap();
        let blocker = storage.create(create_test_issue("Blocker")).await.unwrap();
        let related = storage.create(create_test_issue("Related")).await.unwrap();
        let deep = storage.create(create_test_issue("Deep")).await.unwrap();

        storage
            .add_dependency(&main.id, &blocker.id, DependencyType::Blocks)
            .await
            .unwrap();
        storage
            .add_dependency(&main.id, &related.id, DependencyType::Related)
            .await
            .unwrap();
        storage
            .add_dependency(&blocker.id, &deep.id, DependencyType::ParentChild)
            .await
            .unwrap();

        let tree = storage.get_dependency_tree(&main.id, None).await.unwrap();

        assert_eq!(tree.len(), 3);

        // Check different dependency types are preserved
        let blocks_count = tree
            .iter()
            .filter(|(d, _)| d.dep_type == DependencyType::Blocks)
            .count();
        let related_count = tree
            .iter()
            .filter(|(d, _)| d.dep_type == DependencyType::Related)
            .count();
        let parent_child_count = tree
            .iter()
            .filter(|(d, _)| d.dep_type == DependencyType::ParentChild)
            .count();

        assert_eq!(blocks_count, 1);
        assert_eq!(related_count, 1);
        assert_eq!(parent_child_count, 1);
    }

    // ========== Cycle Detection Tests ==========

    #[tokio::test]
    async fn test_self_dependency_cycle() {
        let mut storage = new_in_memory_storage("test".to_string());

        let issue = storage
            .create(create_test_issue("Self Referencing"))
            .await
            .unwrap();

        // Try to add self-dependency
        let result = storage
            .add_dependency(&issue.id, &issue.id, DependencyType::Blocks)
            .await;

        // Self-dependency should fail as a cycle
        // Note: has_path_connecting returns true for same node
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_two_node_cycle() {
        let mut storage = new_in_memory_storage("test".to_string());

        let issue1 = storage.create(create_test_issue("Issue 1")).await.unwrap();
        let issue2 = storage.create(create_test_issue("Issue 2")).await.unwrap();

        // 1 -> 2
        storage
            .add_dependency(&issue1.id, &issue2.id, DependencyType::Blocks)
            .await
            .unwrap();

        // Try 2 -> 1 (would create cycle)
        let result = storage
            .add_dependency(&issue2.id, &issue1.id, DependencyType::Blocks)
            .await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            Error::CircularDependency { .. }
        ));
    }

    #[tokio::test]
    async fn test_long_chain_cycle_detection() {
        let mut storage = new_in_memory_storage("test".to_string());

        // Create chain of 10 issues
        let mut issues = Vec::new();
        for i in 0..10 {
            let issue = storage
                .create(create_test_issue(&format!("Issue {}", i)))
                .await
                .unwrap();
            issues.push(issue);
        }

        // Create linear chain: 0 -> 1 -> 2 -> ... -> 9
        for i in 0..9 {
            storage
                .add_dependency(&issues[i].id, &issues[i + 1].id, DependencyType::Blocks)
                .await
                .unwrap();
        }

        // Try to close the cycle: 9 -> 0
        let result = storage
            .add_dependency(&issues[9].id, &issues[0].id, DependencyType::Blocks)
            .await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            Error::CircularDependency { .. }
        ));
    }

    #[tokio::test]
    async fn test_has_cycle_method() {
        let mut storage = new_in_memory_storage("test".to_string());

        let issue1 = storage.create(create_test_issue("Issue 1")).await.unwrap();
        let issue2 = storage.create(create_test_issue("Issue 2")).await.unwrap();
        let issue3 = storage.create(create_test_issue("Issue 3")).await.unwrap();
        let issue4 = storage.create(create_test_issue("Issue 4")).await.unwrap();

        // 1 -> 2 -> 3
        storage
            .add_dependency(&issue1.id, &issue2.id, DependencyType::Blocks)
            .await
            .unwrap();
        storage
            .add_dependency(&issue2.id, &issue3.id, DependencyType::Blocks)
            .await
            .unwrap();

        // Check has_cycle
        // 3 -> 1 would create cycle (path 1->2->3 exists, so 3->1 closes the loop)
        assert!(storage.has_cycle(&issue3.id, &issue1.id).await.unwrap());

        // 1 -> 3 would NOT create cycle (no path from 3 to 1)
        assert!(!storage.has_cycle(&issue1.id, &issue3.id).await.unwrap());

        // 3 -> 2 WOULD create cycle (path 2->3 exists, so 3->2 closes the loop)
        assert!(storage.has_cycle(&issue3.id, &issue2.id).await.unwrap());

        // 4 has no dependencies, so no cycles possible with it
        assert!(!storage.has_cycle(&issue4.id, &issue1.id).await.unwrap());
        assert!(!storage.has_cycle(&issue1.id, &issue4.id).await.unwrap());
    }

    // ========== Edge Cases ==========

    #[tokio::test]
    async fn test_duplicate_dependency() {
        let mut storage = new_in_memory_storage("test".to_string());

        let issue1 = storage.create(create_test_issue("Issue 1")).await.unwrap();
        let issue2 = storage.create(create_test_issue("Issue 2")).await.unwrap();

        // Add dependency
        storage
            .add_dependency(&issue2.id, &issue1.id, DependencyType::Blocks)
            .await
            .unwrap();

        // Try to add same dependency again
        let result = storage
            .add_dependency(&issue2.id, &issue1.id, DependencyType::Blocks)
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_remove_nonexistent_dependency() {
        let mut storage = new_in_memory_storage("test".to_string());

        let issue1 = storage.create(create_test_issue("Issue 1")).await.unwrap();
        let issue2 = storage.create(create_test_issue("Issue 2")).await.unwrap();

        // Try to remove nonexistent dependency
        let result = storage.remove_dependency(&issue2.id, &issue1.id).await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            Error::DependencyNotFound { .. }
        ));
    }

    #[tokio::test]
    async fn test_dependency_on_nonexistent_issue() {
        let mut storage = new_in_memory_storage("test".to_string());

        let issue = storage.create(create_test_issue("Issue")).await.unwrap();

        let result = storage
            .add_dependency(
                &issue.id,
                &IssueId::new("nonexistent"),
                DependencyType::Blocks,
            )
            .await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::IssueNotFound(_)));
    }

    // ========== Performance Benchmark ==========

    #[tokio::test]
    async fn test_cycle_check_performance_1000_issues() {
        use std::time::Instant;

        let mut storage = new_in_memory_storage("bench".to_string());

        // Create 1000 issues
        let mut issues = Vec::with_capacity(1000);
        for i in 0..1000 {
            let issue = storage
                .create(create_test_issue(&format!("Performance Issue {}", i)))
                .await
                .unwrap();
            issues.push(issue);
        }

        // Create a linear chain of 500 dependencies
        for i in 0..499 {
            storage
                .add_dependency(&issues[i].id, &issues[i + 1].id, DependencyType::Blocks)
                .await
                .unwrap();
        }

        // Benchmark: check if adding a cycle would be detected
        let start = Instant::now();

        // Perform 100 cycle checks (checking if 499 -> 0 would create a cycle)
        for _ in 0..100 {
            let would_cycle = storage
                .has_cycle(&issues[499].id, &issues[0].id)
                .await
                .unwrap();
            assert!(would_cycle);
        }

        let duration = start.elapsed();

        // 100 cycle checks should complete in under 10ms
        // (i.e., each check should be under 0.1ms)
        println!(
            "100 cycle checks on 1000 issues with 500 deps completed in {:?}",
            duration
        );
        assert!(
            duration.as_millis() < 10,
            "Cycle check performance too slow: {:?} (expected < 10ms)",
            duration
        );
    }

    #[tokio::test]
    async fn test_dependency_tree_performance() {
        use std::time::Instant;

        let mut storage = new_in_memory_storage("bench".to_string());

        // Create 1000 issues
        let mut issues = Vec::with_capacity(1000);
        for i in 0..1000 {
            let issue = storage
                .create(create_test_issue(&format!("Tree Issue {}", i)))
                .await
                .unwrap();
            issues.push(issue);
        }

        // Create a balanced binary tree structure
        // Each issue at level n depends on 2 issues at level n+1
        for i in 0..500 {
            let left_child = i * 2 + 1;
            let right_child = i * 2 + 2;
            if left_child < 1000 {
                storage
                    .add_dependency(
                        &issues[i].id,
                        &issues[left_child].id,
                        DependencyType::Blocks,
                    )
                    .await
                    .unwrap();
            }
            if right_child < 1000 {
                storage
                    .add_dependency(
                        &issues[i].id,
                        &issues[right_child].id,
                        DependencyType::Blocks,
                    )
                    .await
                    .unwrap();
            }
        }

        let start = Instant::now();

        // Get full dependency tree from root
        let tree = storage
            .get_dependency_tree(&issues[0].id, None)
            .await
            .unwrap();

        let duration = start.elapsed();

        // Tree traversal should complete in under 10ms
        println!(
            "Dependency tree of {} nodes completed in {:?}",
            tree.len(),
            duration
        );
        assert!(
            duration.as_millis() < 10,
            "Dependency tree traversal too slow: {:?} (expected < 10ms)",
            duration
        );

        // Verify tree contains all 999 descendants
        assert_eq!(tree.len(), 999);
    }

    // =======================================================================
    // Graph-Vector Synchronization Tests
    //
    // These tests verify that the dependency graph (petgraph edges) and the
    // issue.dependencies vector stay synchronized after all operations.
    // This is critical for data integrity during JSONL serialization.
    // =======================================================================

    /// Helper function to verify graph-vector synchronization for a specific issue.
    /// Returns an error message if synchronization is broken, None if synchronized.
    async fn verify_sync_for_issue(
        storage: &dyn IssueStorage,
        issue_id: &IssueId,
    ) -> Option<String> {
        // Get dependencies from graph via get_dependencies()
        let graph_deps = match storage.get_dependencies(issue_id).await {
            Ok(deps) => deps,
            Err(e) => return Some(format!("Failed to get graph deps for {}: {}", issue_id, e)),
        };

        // Get issue to access vector dependencies
        let issue = match storage.get(issue_id).await {
            Ok(Some(issue)) => issue,
            Ok(None) => return Some(format!("Issue {} not found", issue_id)),
            Err(e) => return Some(format!("Failed to get issue {}: {}", issue_id, e)),
        };

        let vector_deps = &issue.dependencies;

        // Check count matches
        if graph_deps.len() != vector_deps.len() {
            return Some(format!(
                "Issue {}: graph has {} deps, vector has {} deps",
                issue_id,
                graph_deps.len(),
                vector_deps.len()
            ));
        }

        // Check each graph dependency exists in vector
        for graph_dep in &graph_deps {
            let found = vector_deps.iter().any(|v| {
                v.depends_on_id == graph_dep.depends_on_id && v.dep_type == graph_dep.dep_type
            });
            if !found {
                return Some(format!(
                    "Issue {}: graph dep {:?} not found in vector",
                    issue_id, graph_dep
                ));
            }
        }

        // Check each vector dependency exists in graph
        for vector_dep in vector_deps {
            let found = graph_deps.iter().any(|g| {
                g.depends_on_id == vector_dep.depends_on_id && g.dep_type == vector_dep.dep_type
            });
            if !found {
                return Some(format!(
                    "Issue {}: vector dep {:?} not found in graph",
                    issue_id, vector_dep
                ));
            }
        }

        None
    }

    /// Helper function to verify synchronization for all issues in storage.
    async fn verify_all_issues_synchronized(
        storage: &dyn IssueStorage,
    ) -> std::result::Result<(), String> {
        let all_issues = storage.export_all().await.map_err(|e| e.to_string())?;

        for issue in &all_issues {
            if let Some(err) = verify_sync_for_issue(storage, &issue.id).await {
                return Err(err);
            }
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_sync_after_add_dependency() {
        let mut storage = new_in_memory_storage("test".to_string());

        let issue1 = storage.create(create_test_issue("Issue 1")).await.unwrap();
        let issue2 = storage.create(create_test_issue("Issue 2")).await.unwrap();
        let issue3 = storage.create(create_test_issue("Issue 3")).await.unwrap();

        // Add multiple dependencies
        storage
            .add_dependency(&issue2.id, &issue1.id, DependencyType::Blocks)
            .await
            .unwrap();
        storage
            .add_dependency(&issue3.id, &issue1.id, DependencyType::Related)
            .await
            .unwrap();
        storage
            .add_dependency(&issue3.id, &issue2.id, DependencyType::ParentChild)
            .await
            .unwrap();

        // Verify synchronization for all issues
        verify_all_issues_synchronized(storage.as_ref())
            .await
            .expect("Graph and vector should be synchronized after add_dependency");

        // Explicitly verify issue3 has both dependencies
        let issue3_updated = storage.get(&issue3.id).await.unwrap().unwrap();
        assert_eq!(
            issue3_updated.dependencies.len(),
            2,
            "Issue 3 should have 2 dependencies in vector"
        );

        let graph_deps = storage.get_dependencies(&issue3.id).await.unwrap();
        assert_eq!(
            graph_deps.len(),
            2,
            "Issue 3 should have 2 dependencies in graph"
        );
    }

    #[tokio::test]
    async fn test_sync_after_remove_dependency() {
        let mut storage = new_in_memory_storage("test".to_string());

        let issue1 = storage.create(create_test_issue("Issue 1")).await.unwrap();
        let issue2 = storage.create(create_test_issue("Issue 2")).await.unwrap();

        // Add and then remove dependency
        storage
            .add_dependency(&issue2.id, &issue1.id, DependencyType::Blocks)
            .await
            .unwrap();

        // Verify sync before removal
        verify_all_issues_synchronized(storage.as_ref())
            .await
            .expect("Should be synchronized before removal");

        // Remove the dependency
        storage
            .remove_dependency(&issue2.id, &issue1.id)
            .await
            .unwrap();

        // Verify sync after removal
        verify_all_issues_synchronized(storage.as_ref())
            .await
            .expect("Graph and vector should be synchronized after remove_dependency");

        // Explicitly verify issue2 has no dependencies
        let issue2_updated = storage.get(&issue2.id).await.unwrap().unwrap();
        assert_eq!(
            issue2_updated.dependencies.len(),
            0,
            "Issue 2 should have 0 dependencies in vector after removal"
        );

        let graph_deps = storage.get_dependencies(&issue2.id).await.unwrap();
        assert_eq!(
            graph_deps.len(),
            0,
            "Issue 2 should have 0 dependencies in graph after removal"
        );
    }

    #[tokio::test]
    async fn test_sync_after_create_with_dependencies() {
        let mut storage = new_in_memory_storage("test".to_string());

        // Create base issues
        let issue1 = storage.create(create_test_issue("Issue 1")).await.unwrap();
        let issue2 = storage.create(create_test_issue("Issue 2")).await.unwrap();

        // Create issue with dependencies via NewIssue
        let new_issue = NewIssue {
            title: "Issue with deps".to_string(),
            description: "Has dependencies at creation".to_string(),
            priority: 2,
            issue_type: IssueType::Task,
            assignee: None,
            labels: vec![],
            design: None,
            acceptance_criteria: None,
            notes: None,
            external_ref: None,
            dependencies: vec![
                (issue1.id.clone(), DependencyType::Blocks),
                (issue2.id.clone(), DependencyType::Related),
            ],
        };

        let issue3 = storage.create(new_issue).await.unwrap();

        // Verify synchronization
        verify_all_issues_synchronized(storage.as_ref())
            .await
            .expect("Graph and vector should be synchronized after create with dependencies");

        // Verify issue3 has correct dependencies
        assert_eq!(
            issue3.dependencies.len(),
            2,
            "Issue 3 should have 2 dependencies"
        );

        let graph_deps = storage.get_dependencies(&issue3.id).await.unwrap();
        assert_eq!(
            graph_deps.len(),
            2,
            "Issue 3 should have 2 graph dependencies"
        );
    }

    #[tokio::test]
    async fn test_sync_after_update_does_not_affect_deps() {
        let mut storage = new_in_memory_storage("test".to_string());

        let issue1 = storage.create(create_test_issue("Issue 1")).await.unwrap();
        let issue2 = storage.create(create_test_issue("Issue 2")).await.unwrap();

        // Add dependency
        storage
            .add_dependency(&issue2.id, &issue1.id, DependencyType::Blocks)
            .await
            .unwrap();

        // Update issue2 (should not affect dependencies)
        storage
            .update(
                &issue2.id,
                IssueUpdate {
                    title: Some("Updated Issue 2".to_string()),
                    status: Some(IssueStatus::InProgress),
                    priority: Some(1),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        // Verify synchronization still holds
        verify_all_issues_synchronized(storage.as_ref())
            .await
            .expect("Graph and vector should remain synchronized after update");

        // Verify dependency is still present
        let issue2_updated = storage.get(&issue2.id).await.unwrap().unwrap();
        assert_eq!(
            issue2_updated.dependencies.len(),
            1,
            "Issue 2 should still have 1 dependency after update"
        );
    }

    #[tokio::test]
    async fn test_sync_complex_dependency_chain() {
        let mut storage = new_in_memory_storage("test".to_string());

        // Create chain of issues: 1 <- 2 <- 3 <- 4 <- 5
        let mut issues = Vec::new();
        for i in 1..=5 {
            let issue = storage
                .create(create_test_issue(&format!("Issue {}", i)))
                .await
                .unwrap();
            issues.push(issue);
        }

        // Add chain dependencies
        for i in 1..5 {
            storage
                .add_dependency(&issues[i].id, &issues[i - 1].id, DependencyType::Blocks)
                .await
                .unwrap();
        }

        // Verify synchronization
        verify_all_issues_synchronized(storage.as_ref())
            .await
            .expect("Graph and vector should be synchronized for chain");

        // Add cross-links
        storage
            .add_dependency(&issues[4].id, &issues[0].id, DependencyType::Related)
            .await
            .unwrap();
        storage
            .add_dependency(&issues[3].id, &issues[0].id, DependencyType::Related)
            .await
            .unwrap();

        // Verify synchronization after cross-links
        verify_all_issues_synchronized(storage.as_ref())
            .await
            .expect("Graph and vector should be synchronized after cross-links");

        // Remove some dependencies
        storage
            .remove_dependency(&issues[2].id, &issues[1].id)
            .await
            .unwrap();

        // Verify synchronization after removal
        verify_all_issues_synchronized(storage.as_ref())
            .await
            .expect("Graph and vector should be synchronized after removal in chain");
    }

    #[tokio::test]
    async fn test_sync_after_jsonl_round_trip() {
        use tempfile::tempdir;

        let mut storage = new_in_memory_storage("test".to_string());

        // Create issues with dependencies
        let issue1 = storage.create(create_test_issue("Issue 1")).await.unwrap();
        let issue2 = storage.create(create_test_issue("Issue 2")).await.unwrap();
        let issue3 = storage.create(create_test_issue("Issue 3")).await.unwrap();

        storage
            .add_dependency(&issue2.id, &issue1.id, DependencyType::Blocks)
            .await
            .unwrap();
        storage
            .add_dependency(&issue3.id, &issue2.id, DependencyType::Related)
            .await
            .unwrap();
        storage
            .add_dependency(&issue3.id, &issue1.id, DependencyType::ParentChild)
            .await
            .unwrap();

        // Save to JSONL
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("sync_test.jsonl");
        super::save_to_jsonl(storage.as_ref(), &file_path)
            .await
            .unwrap();

        // Load from JSONL
        let (loaded_storage, warnings) = super::load_from_jsonl(&file_path, "test".to_string())
            .await
            .unwrap();

        assert!(
            warnings.is_empty(),
            "Should have no warnings: {:?}",
            warnings
        );

        // Verify synchronization in loaded storage
        verify_all_issues_synchronized(loaded_storage.as_ref())
            .await
            .expect("Graph and vector should be synchronized after JSONL round-trip");

        // Perform operations on loaded storage and verify sync
        let mut loaded_storage = loaded_storage;
        let issue4 = loaded_storage
            .create(create_test_issue("Issue 4"))
            .await
            .unwrap();
        loaded_storage
            .add_dependency(&issue4.id, &issue3.id, DependencyType::Blocks)
            .await
            .unwrap();

        verify_all_issues_synchronized(loaded_storage.as_ref())
            .await
            .expect("Graph and vector should be synchronized after operations on loaded storage");

        temp_dir.close().unwrap();
    }

    #[tokio::test]
    async fn test_sync_multiple_dependency_types() {
        let mut storage = new_in_memory_storage("test".to_string());

        let issue1 = storage.create(create_test_issue("Issue 1")).await.unwrap();
        let issue2 = storage.create(create_test_issue("Issue 2")).await.unwrap();

        // Add dependencies of all types from issue2 to issue1
        // Note: multiple edges with different types between same nodes
        storage
            .add_dependency(&issue2.id, &issue1.id, DependencyType::Blocks)
            .await
            .unwrap();

        // Verify sync
        verify_all_issues_synchronized(storage.as_ref())
            .await
            .expect("Graph and vector should be synchronized with Blocks dependency");

        // Attempt to add duplicate should fail
        let result = storage
            .add_dependency(&issue2.id, &issue1.id, DependencyType::Blocks)
            .await;
        assert!(result.is_err(), "Should not allow duplicate dependency");

        // Original sync should still hold
        verify_all_issues_synchronized(storage.as_ref())
            .await
            .expect("Graph and vector should remain synchronized after duplicate attempt");
    }

    #[tokio::test]
    async fn test_sync_after_failed_cycle_detection() {
        let mut storage = new_in_memory_storage("test".to_string());

        let issue1 = storage.create(create_test_issue("Issue 1")).await.unwrap();
        let issue2 = storage.create(create_test_issue("Issue 2")).await.unwrap();
        let issue3 = storage.create(create_test_issue("Issue 3")).await.unwrap();

        // Create chain: 1 <- 2 <- 3
        storage
            .add_dependency(&issue2.id, &issue1.id, DependencyType::Blocks)
            .await
            .unwrap();
        storage
            .add_dependency(&issue3.id, &issue2.id, DependencyType::Blocks)
            .await
            .unwrap();

        // Attempt to create cycle (should fail)
        let result = storage
            .add_dependency(&issue1.id, &issue3.id, DependencyType::Blocks)
            .await;
        assert!(result.is_err(), "Cycle creation should fail");

        // Sync should still hold after failed cycle attempt
        verify_all_issues_synchronized(storage.as_ref())
            .await
            .expect("Graph and vector should be synchronized after failed cycle attempt");

        // Verify original dependencies are intact
        let deps2 = storage.get_dependencies(&issue2.id).await.unwrap();
        assert_eq!(deps2.len(), 1);
        let deps3 = storage.get_dependencies(&issue3.id).await.unwrap();
        assert_eq!(deps3.len(), 1);
    }

    #[tokio::test]
    async fn test_sync_with_import_issues() {
        let mut storage = new_in_memory_storage("test".to_string());

        // Create issues with dependencies directly
        let now = chrono::Utc::now();
        let issues = vec![
            Issue {
                id: IssueId::new("test-1"),
                title: "Issue 1".to_string(),
                description: "Test".to_string(),
                status: IssueStatus::Open,
                priority: 2,
                issue_type: IssueType::Task,
                assignee: None,
                labels: vec![],
                design: None,
                acceptance_criteria: None,
                notes: None,
                external_ref: None,
                dependencies: vec![],
                created_at: now,
                updated_at: now,
                closed_at: None,
            },
            Issue {
                id: IssueId::new("test-2"),
                title: "Issue 2".to_string(),
                description: "Test".to_string(),
                status: IssueStatus::Open,
                priority: 2,
                issue_type: IssueType::Task,
                assignee: None,
                labels: vec![],
                design: None,
                acceptance_criteria: None,
                notes: None,
                external_ref: None,
                dependencies: vec![Dependency {
                    depends_on_id: IssueId::new("test-1"),
                    dep_type: DependencyType::Blocks,
                }],
                created_at: now,
                updated_at: now,
                closed_at: None,
            },
        ];

        // Import issues
        storage.import_issues(issues).await.unwrap();

        // Verify synchronization
        verify_all_issues_synchronized(storage.as_ref())
            .await
            .expect("Graph and vector should be synchronized after import_issues");
    }

    #[tokio::test]
    async fn test_sync_stress_many_operations() {
        let mut storage = new_in_memory_storage("test".to_string());

        // Create 20 issues
        let mut issues = Vec::new();
        for i in 0..20 {
            let issue = storage
                .create(create_test_issue(&format!("Issue {}", i)))
                .await
                .unwrap();
            issues.push(issue);
        }

        // Add many dependencies (avoiding cycles)
        for i in 1..20 {
            for j in 0..i {
                // Only add some dependencies to avoid too many
                if (i + j) % 3 == 0 {
                    storage
                        .add_dependency(&issues[i].id, &issues[j].id, DependencyType::Related)
                        .await
                        .unwrap();
                }
            }
        }

        // Verify sync after many additions
        verify_all_issues_synchronized(storage.as_ref())
            .await
            .expect("Graph and vector should be synchronized after many additions");

        // Remove some dependencies
        for i in 1..20 {
            for j in 0..i {
                if (i + j) % 6 == 0 {
                    let _ = storage
                        .remove_dependency(&issues[i].id, &issues[j].id)
                        .await;
                }
            }
        }

        // Verify sync after removals
        verify_all_issues_synchronized(storage.as_ref())
            .await
            .expect("Graph and vector should be synchronized after many removals");
    }
}
