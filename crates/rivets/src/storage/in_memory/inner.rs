//! Core in-memory storage data structures.
//!
//! This module contains the inner storage structure that holds all data
//! and is wrapped in `Arc<Mutex<>>` for thread safety.

use crate::domain::{DependencyType, Issue, IssueId, NewIssue};
use crate::error::{Error, Result};
use crate::id_generation::{IdGenerator, IdGeneratorConfig};
use petgraph::graph::{DiGraph, NodeIndex};
use std::collections::HashMap;

/// Inner storage structure (not thread-safe).
///
/// This contains the actual data structures for storing issues and
/// managing the dependency graph. It's wrapped in `Arc<Mutex<>>` for
/// thread safety.
///
/// # Graph Representation
///
/// The dependency graph uses petgraph's `DiGraph` with edges directed from
/// **dependent to dependency** (i.e., source -> target means source depends on target).
///
/// See the module-level documentation for detailed edge direction conventions
/// and blocking semantics.
pub(crate) struct InMemoryStorageInner {
    /// Issues indexed by ID for O(1) lookups
    pub(super) issues: HashMap<IssueId, Issue>,

    /// Dependency graph using petgraph.
    ///
    /// Nodes contain `IssueId` values, edges contain `DependencyType`.
    /// Edge direction: source (dependent) -> target (dependency).
    pub(super) graph: DiGraph<IssueId, DependencyType>,

    /// Mapping from IssueId to graph NodeIndex.
    ///
    /// Used to efficiently locate nodes in the graph. All issues in `self.issues`
    /// must have a corresponding entry in `self.node_map`.
    pub(super) node_map: HashMap<IssueId, NodeIndex>,

    /// ID generator for creating new issue IDs
    pub(super) id_generator: IdGenerator,

    /// Prefix for issue IDs (e.g., "rivets")
    prefix: String,
}

impl InMemoryStorageInner {
    /// Create a new empty storage instance
    pub(crate) fn new(prefix: String) -> Self {
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
    pub(super) fn update_id_generator_if_needed(&mut self) {
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
    pub(super) fn generate_id(&mut self, new_issue: &NewIssue) -> Result<IssueId> {
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
