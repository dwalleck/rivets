//! IssueStorage trait implementation for in-memory storage.

use super::InMemoryStorage;
use super::graph::{
    blocking_dependency_tree_impl, find_blocked_issues, find_blocking_edge, has_blocking_cycle_impl,
};
use super::sorting::sort_by_policy;
use crate::domain::{
    BlockingDependency, Dependency, DependencyType, Issue, IssueFilter, IssueId, IssueStatus,
    IssueUpdate, MAX_PRIORITY, NewIssue, NewResource, Note, ReadyFilter, ResourceId,
    ResourceUpdate, SortPolicy,
};
use crate::error::{Error, Result, StorageError};
use crate::storage::IssueStorage;
use async_trait::async_trait;
use chrono::Utc;
use petgraph::Direction;
use petgraph::visit::EdgeRef;
use std::collections::HashSet;

/// Check whether an Issue matches every generic list criterion.
fn matches_filter(issue: &Issue, filter: &IssueFilter) -> bool {
    filter
        .status
        .as_ref()
        .is_none_or(|status| &issue.status == status)
        && filter
            .priority
            .is_none_or(|priority| issue.priority == priority)
        && filter
            .issue_kind
            .as_ref()
            .is_none_or(|issue_kind| &issue.issue_kind == issue_kind)
        && filter
            .assignee
            .as_ref()
            .is_none_or(|assignee| issue.assignee.as_ref() == Some(assignee))
        && filter
            .label
            .as_ref()
            .is_none_or(|label| issue.labels.contains(label))
}

/// Check post-eligibility Ready query criteria.
fn matches_ready_filter(issue: &Issue, filter: &ReadyFilter) -> bool {
    filter.assignment.allows(issue.assignee.as_deref())
        && filter
            .priority
            .is_none_or(|priority| issue.priority == priority)
        && filter
            .issue_kind
            .as_ref()
            .is_none_or(|issue_kind| &issue.issue_kind == issue_kind)
        && filter
            .label
            .as_ref()
            .is_none_or(|label| issue.labels.contains(label))
}

#[async_trait]
impl IssueStorage for InMemoryStorage {
    async fn create(&mut self, new_issue: NewIssue) -> Result<Issue> {
        let mut inner = self.lock().await;

        // === Phase 1: All validations (no mutations) ===
        // Validate the new issue data (title, priority, etc.)
        new_issue.validate().map_err(StorageError::Validation)?;

        // Validate every prerequisite and reject duplicates before ID generation.
        let mut unique_prerequisites = HashSet::with_capacity(new_issue.prerequisites.len());
        for prerequisite_id in &new_issue.prerequisites {
            if !inner.issues.contains_key(prerequisite_id) {
                return Err(Error::IssueNotFound(prerequisite_id.clone()));
            }
            if !unique_prerequisites.insert(prerequisite_id) {
                return Err(StorageError::Validation(format!(
                    "Duplicate prerequisite: {prerequisite_id}"
                ))
                .into());
            }
        }

        // === Phase 2: ID generation ===
        let id = inner.generate_id(&new_issue)?;

        // === Phase 3: Cycle detection ===
        // We temporarily add the node to check for cycles, then clean up if needed
        let temp_node = inner.graph.add_node(id.clone());
        inner.node_map.insert(id.clone(), temp_node);

        for prerequisite_id in &new_issue.prerequisites {
            if has_blocking_cycle_impl(&inner.graph, &inner.node_map, &id, prerequisite_id)? {
                inner.graph.remove_node(temp_node);
                inner.node_map.remove(&id);
                return Err(Error::CircularDependency {
                    from: id,
                    to: prerequisite_id.clone(),
                });
            }
        }

        // === Phase 4: Create issue (all validations passed) ===
        let now = Utc::now();
        let notes = new_issue
            .initial_note
            .map(|content| vec![Note::from_parts(content, now)])
            .unwrap_or_default();

        let dependencies = new_issue
            .prerequisites
            .iter()
            .cloned()
            .map(|depends_on_id| Dependency {
                depends_on_id,
                dep_type: DependencyType::Blocks,
            })
            .collect::<Vec<_>>();

        let issue = Issue {
            id: id.clone(),
            title: new_issue.title,
            description: new_issue.description,
            status: IssueStatus::Open,
            priority: new_issue.priority,
            issue_kind: new_issue.issue_kind,
            assignee: new_issue.assignee,
            labels: new_issue.labels,
            design: new_issue.design,
            acceptance_criteria: new_issue.acceptance_criteria,
            notes,
            resources: vec![],
            next_resource_id: 1,
            dependencies: dependencies.clone(),
            created_at: now,
            updated_at: now,
            closed_at: None,
        };

        // Store issue (node already added during validation)
        inner.issues.insert(id.clone(), issue.clone());

        // Add Blocking edges after all validation and Issue insertion succeed.
        for prerequisite_id in new_issue.prerequisites {
            let dependent_node = inner.node_map[&id];
            let prerequisite_node = inner.node_map[&prerequisite_id];
            inner
                .graph
                .add_edge(dependent_node, prerequisite_node, DependencyType::Blocks);
        }

        Ok(issue)
    }

    async fn get(&self, id: &IssueId) -> Result<Option<Issue>> {
        let inner = self.lock().await;
        Ok(inner.issues.get(id).cloned())
    }

    async fn update(&mut self, id: &IssueId, updates: IssueUpdate) -> Result<Issue> {
        let mut inner = self.lock().await;
        let stored = inner
            .issues
            .get_mut(id)
            .ok_or_else(|| Error::IssueNotFound(id.clone()))?;
        let mut candidate = stored.clone();
        let now = Utc::now();

        if let Some(title) = updates.title {
            candidate.title = title;
        }
        if let Some(description) = updates.description {
            candidate.description = description;
        }
        if let Some(status) = updates.status {
            // The domain owns transition rules (ADR-0005); this is the single
            // application site, not a storage-local re-validation.
            candidate
                .status
                .validate_transition(status)
                .map_err(StorageError::InvalidStatusTransition)?;
            candidate.status = status;
            if status == IssueStatus::Closed && candidate.closed_at.is_none() {
                candidate.closed_at = Some(now);
            }
        }
        if let Some(priority) = updates.priority {
            if priority > MAX_PRIORITY {
                return Err(Error::InvalidPriority(priority));
            }
            candidate.priority = priority;
        }
        if let Some(issue_kind) = updates.issue_kind {
            candidate.issue_kind = issue_kind;
        }
        if let Some(assignee) = updates.assignee {
            candidate.assignee = assignee;
        }
        if let Some(design) = updates.design {
            candidate.design = Some(design);
        }
        if let Some(acceptance_criteria) = updates.acceptance_criteria {
            candidate.acceptance_criteria = Some(acceptance_criteria);
        }
        if let Some(note) = updates.note {
            candidate.append_note(note, now);
        }
        if let Some(labels) = updates.labels {
            candidate.labels = labels;
        }

        candidate.validate().map_err(StorageError::Validation)?;
        candidate.updated_at = now;

        *stored = candidate.clone();
        Ok(candidate)
    }

    async fn add_resource(&mut self, id: &IssueId, resource: NewResource) -> Result<Issue> {
        let mut inner = self.lock().await;
        let stored = inner
            .issues
            .get_mut(id)
            .ok_or_else(|| Error::IssueNotFound(id.clone()))?;
        let mut candidate = stored.clone();
        candidate
            .add_resource(resource)
            .map_err(StorageError::from)?;
        candidate.updated_at = Utc::now();

        *stored = candidate.clone();
        Ok(candidate)
    }

    async fn update_resource(
        &mut self,
        id: &IssueId,
        resource_id: &ResourceId,
        update: ResourceUpdate,
    ) -> Result<Issue> {
        let mut inner = self.lock().await;
        let stored = inner
            .issues
            .get_mut(id)
            .ok_or_else(|| Error::IssueNotFound(id.clone()))?;
        let mut candidate = stored.clone();
        candidate
            .update_resource(resource_id, update)
            .map_err(StorageError::from)?;
        candidate.updated_at = Utc::now();

        *stored = candidate.clone();
        Ok(candidate)
    }

    async fn remove_resource(&mut self, id: &IssueId, resource_id: &ResourceId) -> Result<Issue> {
        let mut inner = self.lock().await;
        let stored = inner
            .issues
            .get_mut(id)
            .ok_or_else(|| Error::IssueNotFound(id.clone()))?;
        let mut candidate = stored.clone();
        candidate
            .remove_resource(resource_id)
            .map_err(StorageError::from)?;
        candidate.updated_at = Utc::now();

        *stored = candidate.clone();
        Ok(candidate)
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
    async fn add_blocking_dependency(&mut self, dependency: BlockingDependency) -> Result<()> {
        let mut inner = self.lock().await;
        let dependent_id = dependency.dependent_id();
        let prerequisite_id = dependency.prerequisite_id();
        let dependent_node = *inner
            .node_map
            .get(dependent_id)
            .ok_or_else(|| Error::IssueNotFound(dependent_id.clone()))?;
        let prerequisite_node = *inner
            .node_map
            .get(prerequisite_id)
            .ok_or_else(|| Error::IssueNotFound(prerequisite_id.clone()))?;

        if find_blocking_edge(&inner.graph, dependent_node, prerequisite_node).is_some() {
            return Err(StorageError::DuplicateDependency {
                from: dependent_id.clone(),
                to: prerequisite_id.clone(),
            }
            .into());
        }
        if has_blocking_cycle_impl(&inner.graph, &inner.node_map, dependent_id, prerequisite_id)? {
            return Err(Error::CircularDependency {
                from: dependent_id.clone(),
                to: prerequisite_id.clone(),
            });
        }

        inner
            .graph
            .add_edge(dependent_node, prerequisite_node, DependencyType::Blocks);
        inner
            .issues
            .get_mut(dependent_id)
            .ok_or_else(|| Error::IssueNotFound(dependent_id.clone()))?
            .dependencies
            .push(Dependency {
                depends_on_id: prerequisite_id.clone(),
                dep_type: DependencyType::Blocks,
            });
        Ok(())
    }

    async fn remove_blocking_dependency(&mut self, dependency: &BlockingDependency) -> Result<()> {
        let mut inner = self.lock().await;
        let dependent_id = dependency.dependent_id();
        let prerequisite_id = dependency.prerequisite_id();
        let dependent_node = *inner
            .node_map
            .get(dependent_id)
            .ok_or_else(|| Error::IssueNotFound(dependent_id.clone()))?;
        let prerequisite_node = *inner
            .node_map
            .get(prerequisite_id)
            .ok_or_else(|| Error::IssueNotFound(prerequisite_id.clone()))?;
        let edge = find_blocking_edge(&inner.graph, dependent_node, prerequisite_node).ok_or_else(
            || Error::DependencyNotFound {
                from: dependent_id.clone(),
                to: prerequisite_id.clone(),
            },
        )?;

        inner.graph.remove_edge(edge);
        inner
            .issues
            .get_mut(dependent_id)
            .ok_or_else(|| Error::IssueNotFound(dependent_id.clone()))?
            .dependencies
            .retain(|record| {
                record.dep_type != DependencyType::Blocks
                    || record.depends_on_id != *prerequisite_id
            });
        Ok(())
    }

    async fn blocking_prerequisites(
        &self,
        dependent_id: &IssueId,
    ) -> Result<Vec<BlockingDependency>> {
        let inner = self.lock().await;
        let dependent_node = *inner
            .node_map
            .get(dependent_id)
            .ok_or_else(|| Error::IssueNotFound(dependent_id.clone()))?;
        let mut dependencies = inner
            .graph
            .edges(dependent_node)
            .filter(|edge| *edge.weight() == DependencyType::Blocks)
            .map(|edge| {
                BlockingDependency::from_valid_parts(
                    dependent_id.clone(),
                    inner.graph[edge.target()].clone(),
                )
            })
            .collect::<Vec<_>>();
        dependencies.sort();
        Ok(dependencies)
    }

    async fn blocking_dependents(
        &self,
        prerequisite_id: &IssueId,
    ) -> Result<Vec<BlockingDependency>> {
        let inner = self.lock().await;
        let prerequisite_node = *inner
            .node_map
            .get(prerequisite_id)
            .ok_or_else(|| Error::IssueNotFound(prerequisite_id.clone()))?;
        let mut dependencies = inner
            .graph
            .edges_directed(prerequisite_node, Direction::Incoming)
            .filter(|edge| *edge.weight() == DependencyType::Blocks)
            .map(|edge| {
                BlockingDependency::from_valid_parts(
                    inner.graph[edge.source()].clone(),
                    prerequisite_id.clone(),
                )
            })
            .collect::<Vec<_>>();
        dependencies.sort();
        Ok(dependencies)
    }

    async fn blocking_dependency_tree(
        &self,
        dependent_id: &IssueId,
        max_depth: Option<usize>,
    ) -> Result<Vec<(BlockingDependency, usize)>> {
        let inner = self.lock().await;
        blocking_dependency_tree_impl(&inner.graph, &inner.node_map, dependent_id, max_depth)
    }

    async fn list(&self, filter: &IssueFilter) -> Result<Vec<Issue>> {
        let inner = self.lock().await;

        let mut issues: Vec<Issue> = inner
            .issues
            .values()
            .filter(|issue| matches_filter(issue, filter))
            .cloned()
            .collect();

        // Sort by created_at (most recent first)
        issues.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        // Apply limit if specified
        if let Some(limit) = filter.limit {
            issues.truncate(limit);
        }

        Ok(issues)
    }

    async fn ready_to_work(
        &self,
        filter: &ReadyFilter,
        sort_policy: Option<SortPolicy>,
    ) -> Result<Vec<Issue>> {
        let inner = self.lock().await;
        let blocked = find_blocked_issues(&inner.graph, &inner.node_map, &inner.issues);

        let mut ready = inner
            .issues
            .values()
            .filter(|issue| {
                issue.status == IssueStatus::Open
                    && !blocked.contains(&issue.id)
                    && matches_ready_filter(issue, filter)
            })
            .cloned()
            .collect::<Vec<_>>();

        sort_by_policy(&mut ready, sort_policy.unwrap_or_default());
        if let Some(limit) = filter.limit {
            ready.truncate(limit);
        }

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
                    if let Some(blocker) = inner.issues.get(blocker_id)
                        && blocker.status != IssueStatus::Closed
                    {
                        blockers.push(blocker.clone());
                    }
                }
            }

            if !blockers.is_empty() {
                blocked_list.push((issue.clone(), blockers));
            }
        }

        Ok(blocked_list)
    }

    async fn add_label(&mut self, id: &IssueId, label: &str) -> Result<Issue> {
        let mut inner = self.lock().await;

        let issue = inner
            .issues
            .get_mut(id)
            .ok_or_else(|| Error::IssueNotFound(id.clone()))?;

        // Only add if not already present (idempotent)
        if !issue.labels.contains(&label.to_string()) {
            issue.labels.push(label.to_string());
            issue.updated_at = chrono::Utc::now();
        }

        Ok(issue.clone())
    }

    async fn remove_label(&mut self, id: &IssueId, label: &str) -> Result<Issue> {
        let mut inner = self.lock().await;

        let issue = inner
            .issues
            .get_mut(id)
            .ok_or_else(|| Error::IssueNotFound(id.clone()))?;

        // Only remove if present (idempotent)
        let original_len = issue.labels.len();
        issue.labels.retain(|l| l != label);
        if issue.labels.len() != original_len {
            issue.updated_at = chrono::Utc::now();
        }

        Ok(issue.clone())
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

    async fn reload(&mut self) -> Result<()> {
        // In-memory storage has no backing store to reload from
        // This is a no-op for this implementation
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{IssueFilter, IssueKind, IssueStatus, ReadyAssignmentFilter};
    use crate::storage::in_memory::inner::InMemoryStorageInner;
    use rstest::rstest;
    use std::collections::BTreeSet;
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use tokio::sync::Mutex;

    fn create_test_issue() -> Issue {
        Issue {
            id: IssueId::new("test-123"),
            title: "Test Issue".to_string(),
            description: String::new(),
            status: IssueStatus::Open,
            priority: 2,
            issue_kind: IssueKind::Task,
            assignee: Some("alice".to_string()),
            labels: vec!["bug".to_string(), "urgent".to_string()],
            design: None,
            acceptance_criteria: None,
            notes: vec![],
            resources: vec![],
            next_resource_id: 1,
            dependencies: Vec::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            closed_at: None,
        }
    }

    #[test]
    fn test_matches_filter_empty_filter_matches_all() {
        let issue = create_test_issue();
        let filter = IssueFilter::default();
        assert!(matches_filter(&issue, &filter));
    }

    #[rstest]
    #[case::status_matches(Some(IssueStatus::Open), true)]
    #[case::status_does_not_match(Some(IssueStatus::Closed), false)]
    fn test_matches_filter_status(#[case] status: Option<IssueStatus>, #[case] expected: bool) {
        let issue = create_test_issue();
        let filter = IssueFilter {
            status,
            ..Default::default()
        };
        assert_eq!(matches_filter(&issue, &filter), expected);
    }

    #[rstest]
    #[case::priority_matches(Some(2), true)]
    #[case::priority_does_not_match(Some(1), false)]
    fn test_matches_filter_priority(#[case] priority: Option<u8>, #[case] expected: bool) {
        let issue = create_test_issue();
        let filter = IssueFilter {
            priority,
            ..Default::default()
        };
        assert_eq!(matches_filter(&issue, &filter), expected);
    }

    #[rstest]
    #[case::kind_matches(Some(IssueKind::Task), true)]
    #[case::kind_does_not_match(Some(IssueKind::Bug), false)]
    fn test_matches_filter_issue_kind(
        #[case] issue_kind: Option<IssueKind>,
        #[case] expected: bool,
    ) {
        let issue = create_test_issue();
        let filter = IssueFilter {
            issue_kind,
            ..Default::default()
        };
        assert_eq!(matches_filter(&issue, &filter), expected);
    }

    #[rstest]
    #[case::assignee_matches(Some("alice".to_string()), true)]
    #[case::assignee_does_not_match(Some("bob".to_string()), false)]
    fn test_matches_filter_assignee(#[case] assignee: Option<String>, #[case] expected: bool) {
        let issue = create_test_issue();
        let filter = IssueFilter {
            assignee,
            ..Default::default()
        };
        assert_eq!(matches_filter(&issue, &filter), expected);
    }

    #[rstest]
    #[case::label_matches(Some("bug".to_string()), true)]
    #[case::label_does_not_match(Some("feature".to_string()), false)]
    fn test_matches_filter_label(#[case] label: Option<String>, #[case] expected: bool) {
        let issue = create_test_issue();
        let filter = IssueFilter {
            label,
            ..Default::default()
        };
        assert_eq!(matches_filter(&issue, &filter), expected);
    }

    #[test]
    fn test_matches_filter_multiple_criteria() {
        let issue = create_test_issue();

        // All criteria match
        let filter = IssueFilter {
            status: Some(IssueStatus::Open),
            priority: Some(2),
            issue_kind: Some(IssueKind::Task),
            assignee: Some("alice".to_string()),
            label: Some("bug".to_string()),
            limit: None,
        };
        assert!(matches_filter(&issue, &filter));

        // One criterion doesn't match
        let filter = IssueFilter {
            status: Some(IssueStatus::Open),
            priority: Some(1), // Doesn't match
            ..Default::default()
        };
        assert!(!matches_filter(&issue, &filter));
    }

    #[tokio::test]
    async fn ready_stress_fixture_matches_oracle_within_budget() {
        const ISSUE_COUNT: usize = 10_000;
        const EDGE_COUNT: usize = 50_000;

        let mut inner = InMemoryStorageInner::new("test".to_string());
        let timestamp = chrono::Utc::now();
        for index in 0..ISSUE_COUNT {
            let id = IssueId::new(format!("stress-{index:05}"));
            let node = inner.graph.add_node(id.clone());
            inner.node_map.insert(id.clone(), node);
            let status = if index < 3 {
                IssueStatus::Open
            } else {
                match index % 10 {
                    0 => IssueStatus::InProgress,
                    1 => IssueStatus::Closed,
                    _ => IssueStatus::Open,
                }
            };
            let assignee = match index % 3 {
                0 => None,
                1 => Some("alice".to_string()),
                _ => Some("bob".to_string()),
            };
            inner.issues.insert(
                id.clone(),
                Issue {
                    id,
                    title: format!("Stress Issue {index}"),
                    description: String::new(),
                    status,
                    priority: (index % 5) as u8,
                    issue_kind: if index % 2 == 0 {
                        IssueKind::Task
                    } else {
                        IssueKind::Bug
                    },
                    assignee,
                    labels: if index % 2 == 0 {
                        vec!["even".to_string()]
                    } else {
                        vec!["odd".to_string()]
                    },
                    design: None,
                    acceptance_criteria: None,
                    notes: Vec::new(),
                    resources: Vec::new(),
                    next_resource_id: 1,
                    dependencies: Vec::new(),
                    created_at: timestamp,
                    updated_at: timestamp,
                    closed_at: None,
                },
            );
        }

        let mut blocking_edges = Vec::with_capacity(EDGE_COUNT / 4);
        for edge_index in 0..EDGE_COUNT {
            let source_index = 3 + edge_index % (ISSUE_COUNT - 3);
            let mut target_index = (edge_index * 37 + 11) % ISSUE_COUNT;
            if target_index == source_index {
                target_index = (target_index + 1) % ISSUE_COUNT;
            }
            let source_id = IssueId::new(format!("stress-{source_index:05}"));
            let target_id = IssueId::new(format!("stress-{target_index:05}"));
            let kind = match edge_index % 4 {
                0 => DependencyType::Blocks,
                1 => DependencyType::Related,
                2 => DependencyType::ParentChild,
                _ => DependencyType::DiscoveredFrom,
            };
            inner
                .graph
                .add_edge(inner.node_map[&source_id], inner.node_map[&target_id], kind);
            if kind == DependencyType::Blocks {
                blocking_edges.push((source_id, target_id));
            }
        }
        assert_eq!(inner.graph.edge_count(), EDGE_COUNT);

        let unresolved = blocking_edges
            .iter()
            .filter(|(_, prerequisite_id)| {
                inner.issues[prerequisite_id].status != IssueStatus::Closed
            })
            .map(|(dependent_id, _)| dependent_id.clone())
            .collect::<HashSet<_>>();
        let oracle_issues = inner
            .issues
            .values()
            .map(|issue| (issue.id.clone(), issue.status, issue.assignee.clone()))
            .collect::<Vec<_>>();
        let storage: InMemoryStorage = Arc::new(Mutex::new(inner));

        enum OracleAssignment {
            Unassigned,
            Assignee(&'static str),
            All,
        }

        for (assignment, oracle_assignment) in [
            (
                ReadyAssignmentFilter::Unassigned,
                OracleAssignment::Unassigned,
            ),
            (
                ReadyAssignmentFilter::Assignee("alice".to_string()),
                OracleAssignment::Assignee("alice"),
            ),
            (ReadyAssignmentFilter::All, OracleAssignment::All),
        ] {
            let expected = oracle_issues
                .iter()
                .filter(|(id, status, assignee)| {
                    *status == IssueStatus::Open
                        && !unresolved.contains(id)
                        && match oracle_assignment {
                            OracleAssignment::Unassigned => assignee.is_none(),
                            OracleAssignment::Assignee(expected) => {
                                assignee.as_deref() == Some(expected)
                            }
                            OracleAssignment::All => true,
                        }
                })
                .map(|(id, _, _)| id.clone())
                .collect::<BTreeSet<_>>();
            assert!(
                !expected.is_empty(),
                "each Assignment mode needs a positive control"
            );

            let started = Instant::now();
            let actual = storage
                .ready_to_work(
                    &ReadyFilter {
                        assignment,
                        ..Default::default()
                    },
                    None,
                )
                .await
                .unwrap()
                .into_iter()
                .map(|issue| issue.id)
                .collect::<BTreeSet<_>>();
            let elapsed = started.elapsed();
            assert_eq!(actual, expected);
            assert!(
                elapsed <= Duration::from_secs(2),
                "Ready query took {elapsed:?}"
            );
        }
    }
}
