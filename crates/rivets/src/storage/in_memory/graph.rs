//! Dependency graph operations using petgraph.
//!
//! This module provides graph algorithms for the in-memory storage:
//! - Cycle detection
//! - Dependency tree traversal (BFS)
//! - Blocked issue detection with transitive parent-child propagation

use crate::domain::{
    BlockingDependency, DependencyType, DiscoveryOrigin, Issue, IssueId, IssueStatus,
    RelatedAssociation,
};
use crate::error::{Error, Result};
use petgraph::Direction;
use petgraph::algo;
use petgraph::graph::{DiGraph, EdgeIndex, NodeIndex};
use petgraph::visit::EdgeRef;
use std::collections::{HashMap, HashSet, VecDeque};

/// Maximum depth for BFS traversal in blocking detection.
///
/// This limit prevents infinite loops and handles extremely deep hierarchies gracefully.
const MAX_BLOCKING_DEPTH: usize = 50;

/// Find an edge of one relationship kind for an exact endpoint pair.
pub(super) fn find_edge(
    graph: &DiGraph<IssueId, DependencyType>,
    source_node: NodeIndex,
    target_node: NodeIndex,
    dependency_type: DependencyType,
) -> Option<EdgeIndex> {
    graph
        .edges_connecting(source_node, target_node)
        .find(|edge| *edge.weight() == dependency_type)
        .map(|edge| edge.id())
}

/// Find the Blocking edge for one endpoint pair, ignoring parallel other kinds.
pub(super) fn find_blocking_edge(
    graph: &DiGraph<IssueId, DependencyType>,
    dependent_node: NodeIndex,
    prerequisite_node: NodeIndex,
) -> Option<EdgeIndex> {
    find_edge(
        graph,
        dependent_node,
        prerequisite_node,
        DependencyType::Blocks,
    )
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

/// Check whether an edge of one relationship kind would create a cycle.
///
/// Edges are stored dependent-to-dependency, so adding `from -> to` is cyclic
/// exactly when a path of the same kind already exists from `to` to `from`.
pub(super) fn has_cycle_for_type_impl(
    graph: &DiGraph<IssueId, DependencyType>,
    node_map: &HashMap<IssueId, NodeIndex>,
    from: &IssueId,
    to: &IssueId,
    dependency_type: DependencyType,
) -> Result<bool> {
    let from_node = node_map
        .get(from)
        .ok_or_else(|| Error::IssueNotFound(from.clone()))?;
    let to_node = node_map
        .get(to)
        .ok_or_else(|| Error::IssueNotFound(to.clone()))?;
    let mut visited = HashSet::new();
    let mut stack = vec![*to_node];

    while let Some(node) = stack.pop() {
        if node == *from_node {
            return Ok(true);
        }
        if !visited.insert(node) {
            continue;
        }
        stack.extend(
            graph
                .edges(node)
                .filter(|edge| *edge.weight() == dependency_type)
                .map(|edge| edge.target()),
        );
    }

    Ok(false)
}

/// Check whether one Blocking edge would create a Blocking-only cycle.
pub(super) fn has_blocking_cycle_impl(
    graph: &DiGraph<IssueId, DependencyType>,
    node_map: &HashMap<IssueId, NodeIndex>,
    dependent_id: &IssueId,
    prerequisite_id: &IssueId,
) -> Result<bool> {
    has_cycle_for_type_impl(
        graph,
        node_map,
        dependent_id,
        prerequisite_id,
        DependencyType::Blocks,
    )
}

/// Return all Related Associations touching an existing Issue.
pub(super) fn related_associations_impl(
    graph: &DiGraph<IssueId, DependencyType>,
    node_map: &HashMap<IssueId, NodeIndex>,
    issue_id: &IssueId,
) -> Result<Vec<RelatedAssociation>> {
    let node = node_map
        .get(issue_id)
        .ok_or_else(|| Error::IssueNotFound(issue_id.clone()))?;
    let mut associations = Vec::new();
    for edge in graph
        .edges(*node)
        .filter(|edge| *edge.weight() == DependencyType::Related)
    {
        match RelatedAssociation::new(issue_id.clone(), graph[edge.target()].clone()) {
            Ok(association) => associations.push(association),
            Err(error) => tracing::warn!(
                issue_id = ?issue_id,
                error = ?error,
                "Skipping invalid Related graph edge"
            ),
        }
    }
    for edge in graph
        .edges_directed(*node, Direction::Incoming)
        .filter(|edge| *edge.weight() == DependencyType::Related)
    {
        match RelatedAssociation::new(issue_id.clone(), graph[edge.source()].clone()) {
            Ok(association) => associations.push(association),
            Err(error) => tracing::warn!(
                issue_id = ?issue_id,
                error = ?error,
                "Skipping invalid Related graph edge"
            ),
        }
    }
    associations.sort();
    associations.dedup();
    Ok(associations)
}

/// Return Discovery Origins for an existing discovered Issue.
pub(super) fn discovery_origins_impl(
    graph: &DiGraph<IssueId, DependencyType>,
    node_map: &HashMap<IssueId, NodeIndex>,
    discovered_issue_id: &IssueId,
) -> Result<Vec<DiscoveryOrigin>> {
    let node = node_map
        .get(discovered_issue_id)
        .ok_or_else(|| Error::IssueNotFound(discovered_issue_id.clone()))?;
    let mut origins = Vec::new();
    for edge in graph
        .edges(*node)
        .filter(|edge| *edge.weight() == DependencyType::DiscoveredFrom)
    {
        match DiscoveryOrigin::new(discovered_issue_id.clone(), graph[edge.target()].clone()) {
            Ok(origin) => origins.push(origin),
            Err(error) => tracing::warn!(
                discovered_issue_id = ?discovered_issue_id,
                error = ?error,
                "Skipping invalid Discovery graph edge"
            ),
        }
    }
    origins.sort();
    origins.dedup();
    Ok(origins)
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

/// Find all blocked issues using BFS traversal.
///
/// This method identifies issues that are blocked either:
/// 1. Directly: via `Blocks` dependencies to open/in_progress issues
/// 2. Transitively: via `ParentChild` relationships (if parent is blocked, children are too)
///
/// The BFS traversal has a depth limit of 50 to prevent infinite loops in
/// malformed dependency graphs.
///
/// # Algorithm
///
/// 1. Pre-filter to only consider non-closed issues (optimization)
/// 2. Find all issues with direct `Blocks` dependencies to unclosed issues
/// 3. Use BFS to propagate blocking through parent-child relationships
/// 4. Return the set of all blocked issue IDs
///
/// # Edge Direction Reminder
///
/// - Edges point from **dependent -> dependency** (source depends on target)
/// - For `Blocks`: blocked_issue -> blocker, so `edge.target()` is the blocker
/// - For `ParentChild`: child -> parent, so `Direction::Incoming` finds children
///
/// # Non-Blocking Dependency Types
///
/// - `Related`: Informational only, does not block
/// - `DiscoveredFrom`: Provenance only, does not block
pub(super) fn find_blocked_issues(
    graph: &DiGraph<IssueId, DependencyType>,
    node_map: &HashMap<IssueId, NodeIndex>,
    issues: &HashMap<IssueId, Issue>,
) -> HashSet<IssueId> {
    let mut blocked = HashSet::new();

    // Phase 1: Find directly blocked issues (only check non-closed issues for performance)
    // An issue is directly blocked if it has a 'Blocks' dependency on an unclosed issue.
    //
    // Edge direction: blocked_issue -> blocker (dependent -> dependency)
    // So we iterate outgoing edges and check if the target (blocker) is unclosed.
    for (id, issue) in issues {
        // Skip closed issues - they cannot be "ready to work" anyway
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

    // Phase 2: Propagate blocking through parent-child relationships
    // If a parent issue is blocked, all its children are also blocked.
    //
    // Edge direction for ParentChild: child -> parent (child depends on parent)
    // To find children of a blocked parent, we look for INCOMING edges to that parent,
    // where the edge type is ParentChild. The edge.source() gives us the child.
    let mut to_process: VecDeque<(IssueId, usize)> =
        blocked.iter().map(|id| (id.clone(), 0)).collect();

    while let Some((id, depth)) = to_process.pop_front() {
        if depth >= MAX_BLOCKING_DEPTH {
            continue;
        }

        // Defensive: skip if node_map is somehow inconsistent
        let Some(&node) = node_map.get(&id) else {
            continue;
        };

        // Find children: issues that have ParentChild edges pointing TO this issue
        // Since edge direction is child -> parent, incoming edges to 'node' come from children
        for edge in graph.edges_directed(node, Direction::Incoming) {
            if edge.weight() == &DependencyType::ParentChild {
                let child_id = &graph[edge.source()];
                if blocked.insert(child_id.clone()) {
                    to_process.push_back((child_id.clone(), depth + 1));
                }
            }
        }
    }

    blocked
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn blocking_graph_stays_within_scale_budget() {
        const ISSUE_COUNT: usize = 10_000;
        const EDGE_COUNT: usize = 50_000;
        let mut graph = DiGraph::new();
        let mut node_map = HashMap::with_capacity(ISSUE_COUNT);
        let nodes = (0..ISSUE_COUNT)
            .map(|index| {
                let issue_id = IssueId::new(format!("test-{index:05}"));
                let node = graph.add_node(issue_id.clone());
                node_map.insert(issue_id, node);
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
    }
}
