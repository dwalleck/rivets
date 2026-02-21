//! IssueStorage trait implementation for in-memory storage.

use super::graph::{find_blocked_issues, get_dependency_tree_impl, has_cycle_impl};
use super::sorting::sort_by_policy;
use super::InMemoryStorage;
use crate::domain::{
    Dependency, DependencyType, Issue, IssueFilter, IssueId, IssueStatus, IssueUpdate, NewIssue,
    SortPolicy, MAX_PRIORITY,
};
use crate::error::{Error, Result, StorageError};
use crate::storage::IssueStorage;
use async_trait::async_trait;
use chrono::Utc;
use petgraph::visit::EdgeRef;
use petgraph::Direction;

/// Check if an issue matches all criteria in the filter.
///
/// This is shared logic used by both `list()` and `ready_to_work()` to apply
/// optional filters for status, priority, type, assignee, and label.
fn matches_filter(issue: &Issue, filter: &IssueFilter) -> bool {
    filter
        .status
        .as_ref()
        .is_none_or(|status| &issue.status == status)
        && filter
            .priority
            .is_none_or(|priority| issue.priority == priority)
        && filter
            .issue_type
            .as_ref()
            .is_none_or(|issue_type| &issue.issue_type == issue_type)
        && filter
            .assignee
            .as_ref()
            .is_none_or(|assignee| issue.assignee.as_ref() == Some(assignee))
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
            if has_cycle_impl(&inner.graph, &inner.node_map, &id, depends_on_id)? {
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
            if priority > MAX_PRIORITY {
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
        if let Some(labels) = updates.labels {
            issue.labels = labels;
        }

        // Validate the updated issue to ensure data integrity
        // This catches invalid titles, descriptions, or priorities that may have been set
        issue.validate().map_err(StorageError::Validation)?;

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
            return Err(StorageError::DuplicateDependency {
                from: from.clone(),
                to: to.clone(),
            }
            .into());
        }

        // Check for cycles (must be done after duplicate check to avoid false positives)
        if has_cycle_impl(&inner.graph, &inner.node_map, from, to)? {
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
        has_cycle_impl(&inner.graph, &inner.node_map, from, to)
    }

    async fn get_dependency_tree(
        &self,
        id: &IssueId,
        max_depth: Option<usize>,
    ) -> Result<Vec<(Dependency, usize)>> {
        let inner = self.lock().await;
        get_dependency_tree_impl(&inner.graph, &inner.node_map, id, max_depth)
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
        filter: Option<&IssueFilter>,
        sort_policy: Option<SortPolicy>,
    ) -> Result<Vec<Issue>> {
        let inner = self.lock().await;

        // Find all blocked issues using BFS traversal
        let blocked = find_blocked_issues(&inner.graph, &inner.node_map, &inner.issues);

        // Filter out blocked and closed issues
        let mut ready: Vec<Issue> = inner
            .issues
            .values()
            .filter(|issue| issue.status != IssueStatus::Closed && !blocked.contains(&issue.id))
            .cloned()
            .collect();

        // Apply additional filter if provided
        if let Some(filter) = filter {
            ready.retain(|issue| matches_filter(issue, filter));
        }

        // Apply sort policy
        let policy = sort_policy.unwrap_or_default();
        sort_by_policy(&mut ready, policy);

        // Apply limit if specified
        if let Some(filter) = filter {
            if let Some(limit) = filter.limit {
                ready.truncate(limit);
            }
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
    use crate::domain::{IssueFilter, IssueStatus, IssueType};
    use rstest::rstest;

    fn create_test_issue() -> Issue {
        Issue {
            id: IssueId::new("test-123"),
            title: "Test Issue".to_string(),
            description: String::new(),
            status: IssueStatus::Open,
            priority: 2,
            issue_type: IssueType::Task,
            assignee: Some("alice".to_string()),
            labels: vec!["bug".to_string(), "urgent".to_string()],
            design: None,
            acceptance_criteria: None,
            notes: None,
            external_ref: None,
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
    #[case::type_matches(Some(IssueType::Task), true)]
    #[case::type_does_not_match(Some(IssueType::Bug), false)]
    fn test_matches_filter_issue_type(
        #[case] issue_type: Option<IssueType>,
        #[case] expected: bool,
    ) {
        let issue = create_test_issue();
        let filter = IssueFilter {
            issue_type,
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
            issue_type: Some(IssueType::Task),
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
}
