//! In-memory storage backend using HashMap and petgraph.
//!
//! This module provides a fast, ephemeral storage implementation suitable for:
//! - Testing and development
//! - Short-lived CLI sessions
//! - MVP development phase
//!
//! # Architecture
//!
//! The implementation uses:
//! - `HashMap<IssueId, Issue>` for O(1) issue lookups
//! - `petgraph::DiGraph` for dependency graph management
//! - `HashMap<IssueId, NodeIndex>` for mapping issues to graph nodes
//!
//! # Thread Safety
//!
//! The storage is wrapped in `Arc<Mutex<>>` to provide thread-safe access
//! in async contexts. All operations acquire the mutex lock.

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
use std::sync::Arc;
use tokio::sync::Mutex;

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

    /// Update the ID generator's database size based on current issue count
    fn update_id_generator_size(&mut self) {
        self.id_generator = IdGenerator::new(IdGeneratorConfig {
            prefix: self.prefix.clone(),
            database_size: self.issues.len(),
        });

        // Register all existing IDs with the new generator
        for id in self.issues.keys() {
            self.id_generator.register_id(id.as_str().to_string());
        }
    }

    /// Generate a new unique ID for an issue
    fn generate_id(&mut self, new_issue: &NewIssue) -> Result<IssueId> {
        self.update_id_generator_size();

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

#[async_trait]
impl IssueStorage for InMemoryStorage {
    async fn create(&mut self, new_issue: NewIssue) -> Result<Issue> {
        let mut inner = self.lock().await;

        // Validate the new issue
        new_issue
            .validate()
            .map_err(|e| Error::Storage(format!("Validation failed: {}", e)))?;

        // Generate unique ID
        let id = inner.generate_id(&new_issue)?;

        // Create the issue
        let now = Utc::now();
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
            created_at: now,
            updated_at: now,
            closed_at: None,
        };

        // Add to graph
        let node = inner.graph.add_node(id.clone());
        inner.node_map.insert(id.clone(), node);

        // Store issue
        inner.issues.insert(id.clone(), issue.clone());

        // Add dependencies if provided
        for (depends_on_id, dep_type) in new_issue.dependencies {
            // Validate dependency exists
            if !inner.issues.contains_key(&depends_on_id) {
                return Err(Error::IssueNotFound(depends_on_id));
            }

            // Check for cycles
            if inner.has_cycle_impl(&id, &depends_on_id)? {
                return Err(Error::CircularDependency {
                    from: id,
                    to: depends_on_id,
                });
            }

            // Add edge to graph
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

        // Add edge to graph
        let from_node = inner.node_map[from];
        let to_node = inner.node_map[to];
        inner.graph.add_edge(from_node, to_node, dep_type);

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
        let exported = storage.export_all().await.unwrap();
        assert_eq!(exported.len(), 2);

        // Create new storage and import
        let mut new_storage = new_in_memory_storage("test".to_string());
        new_storage.import_issues(exported).await.unwrap();

        // Verify imported issues
        let retrieved1 = new_storage.get(&issue1.id).await.unwrap();
        let retrieved2 = new_storage.get(&issue2.id).await.unwrap();
        assert!(retrieved1.is_some());
        assert!(retrieved2.is_some());
    }

    #[tokio::test]
    async fn test_save() {
        let storage = new_in_memory_storage("test".to_string());
        // Save is a no-op for in-memory storage, but should not error
        storage.save().await.unwrap();
    }
}
