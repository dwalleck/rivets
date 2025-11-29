//! Dependency graph operations using petgraph.
//!
//! This module provides graph algorithms for the in-memory storage:
//! - Cycle detection
//! - Dependency tree traversal (BFS)
//! - Blocked issue detection with transitive parent-child propagation

use crate::domain::{Dependency, DependencyType, Issue, IssueId, IssueStatus};
use crate::error::{Error, Result};
use petgraph::algo;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use std::collections::{HashMap, HashSet, VecDeque};

/// Maximum depth for BFS traversal in blocking detection.
///
/// This limit prevents infinite loops and handles extremely deep hierarchies gracefully.
const MAX_BLOCKING_DEPTH: usize = 50;

/// Internal implementation of dependency tree traversal.
///
/// Uses BFS to traverse the dependency graph, returning all transitive
/// dependencies with their depth level.
pub(super) fn get_dependency_tree_impl(
    graph: &DiGraph<IssueId, DependencyType>,
    node_map: &HashMap<IssueId, NodeIndex>,
    id: &IssueId,
    max_depth: Option<usize>,
) -> Result<Vec<(Dependency, usize)>> {
    let start_node = node_map
        .get(id)
        .ok_or_else(|| Error::IssueNotFound(id.clone()))?;

    let mut result = Vec::new();
    let mut visited = HashSet::new();
    let mut queue: VecDeque<(NodeIndex, usize)> = VecDeque::new();

    // Start BFS from direct dependencies (depth 1)
    for edge in graph.edges(*start_node) {
        let target_node = edge.target();
        if visited.insert(target_node) {
            queue.push_back((target_node, 1));
            result.push((
                Dependency {
                    depends_on_id: graph[target_node].clone(),
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
        for edge in graph.edges(current_node) {
            let target_node = edge.target();
            if visited.insert(target_node) {
                let next_depth = depth + 1;
                queue.push_back((target_node, next_depth));
                result.push((
                    Dependency {
                        depends_on_id: graph[target_node].clone(),
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
                if let Some(blocker) = issues.get(blocker_id) {
                    if blocker.status != IssueStatus::Closed {
                        blocked.insert(id.clone());
                        break;
                    }
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
