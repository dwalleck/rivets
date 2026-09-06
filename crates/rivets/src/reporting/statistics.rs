//! Borrowed Workspace statistics aggregation.
//!
//! This module owns the fixed-size projection logic while storage owns the
//! snapshot lock and canonical blocked-set derivation.

use super::WorkspaceStatistics;
use crate::domain::{Issue, IssueId, IssueStatus};
use crate::error::{Error, Result};
use std::collections::{HashMap, HashSet};

/// The intrinsic Ready predicate shared by Ready queries and statistics.
///
/// Assignment is intentionally absent: a claimed Open Issue remains Ready for
/// Workspace-wide statistics, while query-specific assignment visibility is
/// applied by `ready_to_work` after this predicate.
pub(crate) fn is_intrinsically_ready(issue: &Issue, blocked: &HashSet<IssueId>) -> bool {
    issue.status == IssueStatus::Open && !blocked.contains(&issue.id)
}

/// Aggregate all report buckets from one borrowed storage snapshot.
pub(crate) fn aggregate(
    issues: &HashMap<IssueId, Issue>,
    blocked: &HashSet<IssueId>,
) -> Result<WorkspaceStatistics> {
    let mut by_status = super::StatusCounts::default();
    let mut by_priority = [0_usize; 5];
    let mut ready = 0_usize;

    for issue in issues.values() {
        match issue.status {
            IssueStatus::Open => by_status.open += 1,
            IssueStatus::InProgress => by_status.in_progress += 1,
            IssueStatus::Closed => by_status.closed += 1,
        }

        let count = by_priority
            .get_mut(usize::from(issue.priority))
            .ok_or(Error::InvalidPriority(issue.priority))?;
        *count += 1;

        if is_intrinsically_ready(issue, blocked) {
            ready += 1;
        }
    }

    Ok(WorkspaceStatistics {
        total: issues.len(),
        by_status,
        ready,
        blocked_by_dependencies: blocked.len(),
        by_priority,
    })
}
