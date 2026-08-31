//! Dependency graph operations using petgraph.
//!
//! This module provides graph algorithms for the in-memory storage:
//! - Dependency tree traversal (BFS)
//! - Direct blocked issue detection

use crate::domain::{BlockingDependency, DependencyType, Issue, IssueId, IssueStatus};
use crate::error::{Error, Result};
use petgraph::algo;
use petgraph::graph::{DiGraph, EdgeIndex, NodeIndex};
use petgraph::visit::EdgeRef;
use std::collections::{HashMap, HashSet, VecDeque};

/// Find the Blocking edge for one endpoint pair, ignoring parallel other kinds.
pub(super) fn find_blocking_edge(
    graph: &DiGraph<IssueId, DependencyType>,
    dependent_node: NodeIndex,
    prerequisite_node: NodeIndex,
) -> Option<EdgeIndex> {
    graph
        .edges_connecting(dependent_node, prerequisite_node)
        .find(|edge| *edge.weight() == DependencyType::Blocks)
        .map(|edge| edge.id())
}

/// Return whether one Issue has a direct unresolved Blocking Dependency.
///
/// This scans only the target Issue's outgoing edges, avoiding the
/// all-Workspace blocked set used by Ready queries.
pub(super) fn has_unresolved_blocking_dependency(
    graph: &DiGraph<IssueId, DependencyType>,
    node_map: &HashMap<IssueId, NodeIndex>,
    issues: &HashMap<IssueId, Issue>,
    issue_id: &IssueId,
) -> Result<bool> {
    let node = node_map
        .get(issue_id)
        .ok_or_else(|| Error::IssueNotFound(issue_id.clone()))?;
    for edge in graph.edges(*node) {
        if *edge.weight() != DependencyType::Blocks {
            continue;
        }
        let prerequisite_id = &graph[edge.target()];
        let prerequisite = issues
            .get(prerequisite_id)
            .ok_or_else(|| Error::IssueNotFound(prerequisite_id.clone()))?;
        if prerequisite.status != IssueStatus::Closed {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Traverse only Blocking Dependencies in dependent-to-prerequisite order.
pub(super) fn blocking_dependency_tree_impl(
    graph: &DiGraph<IssueId, DependencyType>,
    node_map: &HashMap<IssueId, NodeIndex>,
    dependent_id: &IssueId,
    max_depth: Option<usize>,
) -> Result<Vec<(BlockingDependency, usize)>> {
    let start_node = node_map
        .get(dependent_id)
        .ok_or_else(|| Error::IssueNotFound(dependent_id.clone()))?;
    let mut result = Vec::new();
    let mut visited = HashSet::from([*start_node]);
    let mut queue = VecDeque::from([(*start_node, 0)]);

    while let Some((dependent_node, depth)) = queue.pop_front() {
        if max_depth.is_some_and(|maximum| depth >= maximum) {
            continue;
        }

        let mut prerequisite_nodes = graph
            .edges(dependent_node)
            .filter(|edge| *edge.weight() == DependencyType::Blocks)
            .map(|edge| edge.target())
            .collect::<Vec<_>>();
        prerequisite_nodes.sort_by(|left, right| graph[*left].cmp(&graph[*right]));

        for prerequisite_node in prerequisite_nodes {
            if visited.insert(prerequisite_node) {
                let next_depth = depth + 1;
                result.push((
                    BlockingDependency::from_valid_parts(
                        graph[dependent_node].clone(),
                        graph[prerequisite_node].clone(),
                    ),
                    next_depth,
                ));
                queue.push_back((prerequisite_node, next_depth));
            }
        }
    }

    Ok(result)
}

/// Check whether one Blocking edge would create a Blocking-only cycle.
pub(super) fn has_blocking_cycle_impl(
    graph: &DiGraph<IssueId, DependencyType>,
    node_map: &HashMap<IssueId, NodeIndex>,
    dependent_id: &IssueId,
    prerequisite_id: &IssueId,
) -> Result<bool> {
    let dependent_node = node_map
        .get(dependent_id)
        .ok_or_else(|| Error::IssueNotFound(dependent_id.clone()))?;
    let prerequisite_node = node_map
        .get(prerequisite_id)
        .ok_or_else(|| Error::IssueNotFound(prerequisite_id.clone()))?;
    let mut visited = HashSet::new();
    let mut stack = vec![*prerequisite_node];

    while let Some(node) = stack.pop() {
        if node == *dependent_node {
            return Ok(true);
        }
        if !visited.insert(node) {
            continue;
        }
        stack.extend(
            graph
                .edges(node)
                .filter(|edge| *edge.weight() == DependencyType::Blocks)
                .map(|edge| edge.target()),
        );
    }

    Ok(false)
}

/// Internal implementation of cycle detection.
///
/// Uses petgraph's `has_path_connecting` to check if adding
/// an edge from `from` to `to` would create a cycle.
pub(super) fn has_cycle_impl(
    graph: &DiGraph<IssueId, DependencyType>,
    node_map: &HashMap<IssueId, NodeIndex>,
    from: &IssueId,
    to: &IssueId,
) -> Result<bool> {
    let from_node = node_map
        .get(from)
        .ok_or_else(|| Error::IssueNotFound(from.clone()))?;
    let to_node = node_map
        .get(to)
        .ok_or_else(|| Error::IssueNotFound(to.clone()))?;

    // Check if there's already a path from `to` to `from`
    // If so, adding `from -> to` would create a cycle
    Ok(algo::has_path_connecting(graph, *to_node, *from_node, None))
}

/// Find Issues blocked by direct unresolved Blocking Dependencies.
///
/// Closed Issues are not candidates. For every other Issue, only outgoing
/// `Blocks` edges participate: the dependent is blocked while the target
/// prerequisite is not Closed. Parentage, Related Associations, and Discovery
/// Origins never affect blockedness.
///
/// Edges point from dependent to prerequisite, so `edge.target()` is the
/// prerequisite whose Workflow State determines whether the edge is resolved.
pub(super) fn find_blocked_issues(
    graph: &DiGraph<IssueId, DependencyType>,
    node_map: &HashMap<IssueId, NodeIndex>,
    issues: &HashMap<IssueId, Issue>,
) -> HashSet<IssueId> {
    let mut blocked = HashSet::new();

    // Edge direction: dependent -> prerequisite. Scan outgoing Blocks edges
    // and resolve each one solely from the prerequisite's Workflow State.
    for (id, issue) in issues {
        // Closed Issues cannot be Ready and are not reported as Blocked.
        if issue.status == IssueStatus::Closed {
            continue;
        }

        // Defensive: skip if node_map is somehow inconsistent
        let Some(&node) = node_map.get(id) else {
            continue;
        };

        for edge in graph.edges(node) {
            if edge.weight() == &DependencyType::Blocks {
                let blocker_id = &graph[edge.target()];
                if let Some(blocker) = issues.get(blocker_id)
                    && blocker.status != IssueStatus::Closed
                {
                    blocked.insert(id.clone());
                    break;
                }
            }
        }
    }

    blocked
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::IssueKind;
    use chrono::Utc;
    use std::time::{Duration, Instant};

    #[test]
    fn blocking_graph_and_ready_derivation_stay_within_scale_budget() {
        const ISSUE_COUNT: usize = 10_000;
        const EDGE_COUNT: usize = 50_000;
        let mut graph = DiGraph::new();
        let mut node_map = HashMap::with_capacity(ISSUE_COUNT);
        let mut issues = HashMap::with_capacity(ISSUE_COUNT);
        let timestamp = Utc::now();
        let nodes = (0..ISSUE_COUNT)
            .map(|index| {
                let issue_id = IssueId::new(format!("test-{index:05}"));
                let node = graph.add_node(issue_id.clone());
                node_map.insert(issue_id.clone(), node);
                issues.insert(
                    issue_id.clone(),
                    Issue {
                        id: issue_id,
                        title: format!("Issue {index}"),
                        description: String::new(),
                        status: IssueStatus::Open,
                        priority: 2,
                        issue_kind: IssueKind::Task,
                        assignee: None,
                        labels: Vec::new(),
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
                node
            })
            .collect::<Vec<_>>();
        for pair in nodes.windows(2) {
            graph.add_edge(pair[0], pair[1], DependencyType::Blocks);
        }
        for index in 0..(EDGE_COUNT - (ISSUE_COUNT - 1)) {
            let kind = match index % 3 {
                0 => DependencyType::Related,
                1 => DependencyType::ParentChild,
                _ => DependencyType::DiscoveredFrom,
            };
            graph.add_edge(nodes[0], nodes[1], kind);
        }
        assert_eq!(graph.edge_count(), EDGE_COUNT);

        let started = Instant::now();
        assert!(find_blocking_edge(&graph, nodes[0], nodes[1]).is_some());
        let edge_lookup_elapsed = started.elapsed();
        assert!(
            edge_lookup_elapsed <= Duration::from_millis(10),
            "Blocking edge lookup took {edge_lookup_elapsed:?}"
        );

        let started = Instant::now();
        assert!(
            has_unresolved_blocking_dependency(
                &graph,
                &node_map,
                &issues,
                &IssueId::new("test-00000"),
            )
            .unwrap()
        );
        let claim_lookup_elapsed = started.elapsed();
        assert!(
            claim_lookup_elapsed <= Duration::from_millis(10),
            "Claim blockedness lookup took {claim_lookup_elapsed:?}"
        );

        let started = Instant::now();
        let tree =
            blocking_dependency_tree_impl(&graph, &node_map, &IssueId::new("test-00000"), None)
                .unwrap();
        let tree_elapsed = started.elapsed();
        assert_eq!(tree.len(), ISSUE_COUNT - 1);
        assert!(
            tree_elapsed <= Duration::from_millis(50),
            "Blocking tree took {tree_elapsed:?}"
        );

        let started = Instant::now();
        assert!(
            has_blocking_cycle_impl(
                &graph,
                &node_map,
                &IssueId::new("test-09999"),
                &IssueId::new("test-00000"),
            )
            .unwrap()
        );
        let cycle_elapsed = started.elapsed();
        assert!(
            cycle_elapsed <= Duration::from_millis(50),
            "Blocking cycle query took {cycle_elapsed:?}"
        );

        let started = Instant::now();
        let blocked = find_blocked_issues(&graph, &node_map, &issues);
        let ready_count = issues
            .values()
            .filter(|issue| {
                issue.status == IssueStatus::Open
                    && issue.assignee.is_none()
                    && !blocked.contains(&issue.id)
            })
            .count();
        let ready_elapsed = started.elapsed();
        assert_eq!(blocked.len(), ISSUE_COUNT - 1);
        assert_eq!(ready_count, 1);
        assert!(
            ready_elapsed <= Duration::from_secs(2),
            "Ready derivation took {ready_elapsed:?}"
        );
    }
}
