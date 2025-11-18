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
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use std::sync::Arc;
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::sync::Mutex;

/// Warnings that can occur during JSONL file loading.
///
/// These are non-fatal issues that don't prevent loading but indicate
/// data quality problems in the JSONL file.
#[derive(Debug, Clone)]
pub enum LoadWarning {
    /// Malformed JSON line that couldn't be parsed
    MalformedJson { line_number: usize, error: String },

    /// Dependency references an issue that doesn't exist in the file
    OrphanedDependency { from: IssueId, to: IssueId },

    /// Adding a dependency would create a circular reference
    CircularDependency { from: IssueId, to: IssueId },
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
    let file = File::open(path).await.map_err(Error::Io)?;
    let reader = BufReader::new(file);
    let mut lines = reader.lines();

    let mut issues = Vec::new();
    let mut warnings = Vec::new();
    let mut line_number = 0;

    // First pass: Parse all issues from JSONL
    while let Some(line) = lines.next_line().await.map_err(Error::Io)? {
        line_number += 1;

        match serde_json::from_str::<Issue>(&line) {
            Ok(issue) => {
                issues.push(issue);
            }
            Err(e) => {
                warnings.push(LoadWarning::MalformedJson {
                    line_number,
                    error: e.to_string(),
                });
            }
        }
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
    let issues = storage.export_all().await?;

    // Write each issue as a JSON line
    for issue in issues {
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

        // Validate the new issue
        new_issue
            .validate()
            .map_err(|e| Error::Storage(format!("Validation failed: {}", e)))?;

        // Validate all dependencies BEFORE modifying storage
        // This prevents graph corruption if validation fails after issue is added
        for (depends_on_id, _dep_type) in &new_issue.dependencies {
            // Validate dependency exists
            if !inner.issues.contains_key(depends_on_id) {
                return Err(Error::IssueNotFound(depends_on_id.clone()));
            }
        }

        // Generate unique ID
        let id = inner.generate_id(&new_issue)?;

        // Check for cycles BEFORE adding to graph
        // We temporarily add the node to check cycles, but haven't stored the issue yet
        let temp_node = inner.graph.add_node(id.clone());
        inner.node_map.insert(id.clone(), temp_node);

        for (depends_on_id, _dep_type) in &new_issue.dependencies {
            if inner.has_cycle_impl(&id, depends_on_id)? {
                // Rollback: remove the temporary node
                inner.graph.remove_node(temp_node);
                inner.node_map.remove(&id);
                return Err(Error::CircularDependency {
                    from: id,
                    to: depends_on_id.clone(),
                });
            }
        }

        // All validations passed - now it's safe to add the issue
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

        // Check for cycles
        if inner.has_cycle_impl(from, to)? {
            return Err(Error::CircularDependency {
                from: from.clone(),
                to: to.clone(),
            });
        }

        // Check for duplicate dependency
        let issue = inner
            .issues
            .get(from)
            .ok_or_else(|| Error::IssueNotFound(from.clone()))?;

        if issue
            .dependencies
            .iter()
            .any(|dep| dep.depends_on_id == *to)
        {
            return Err(Error::Storage(format!(
                "Dependency already exists: {} -> {}",
                from, to
            )));
        }

        // Add edge to graph
        let from_node = inner.node_map[from];
        let to_node = inner.node_map[to];
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
    async fn test_save_load_jsonl_round_trip() {
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
}
