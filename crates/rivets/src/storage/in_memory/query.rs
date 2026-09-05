//! Selection and ordering for the shared bounded List and Stale intents.

use super::InMemoryStorageInner;
use super::trait_impl::matches_common_filter;
use crate::domain::{Issue, IssueStatus, ListQuery, StaleQuery};

fn matches_list(issue: &Issue, query: &ListQuery) -> bool {
    query.status().is_none_or(|status| issue.status == status)
        && matches_common_filter(
            issue,
            query.priority(),
            query.issue_kind().as_ref(),
            query.label(),
        )
        && query
            .assignee()
            .is_none_or(|assignee| issue.assignee.as_deref() == Some(assignee))
}

fn matches_stale(issue: &Issue, query: &StaleQuery) -> bool {
    issue.updated_at < query.cutoff()
        && query
            .status()
            .map_or(issue.status != IssueStatus::Closed, |status| {
                issue.status == status
            })
}

/// Select and order the canonical List Issues result.
///
/// Candidates are borrowed while filtering and sorting. Only the selected
/// prefix is cloned, so a large requested limit does not allocate a
/// limit-sized result buffer.
pub(super) fn list_issues(inner: &InMemoryStorageInner, query: &ListQuery) -> Vec<Issue> {
    let mut candidates = inner
        .issues
        .values()
        .filter(|issue| matches_list(issue, query))
        .collect::<Vec<_>>();
    candidates.sort_unstable_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    candidates.truncate(query.limit().get());
    candidates.into_iter().cloned().collect()
}

/// Select and order the canonical Stale Issues result.
///
/// The stale age boundary is strict (`updated_at < cutoff`), and omitted
/// status means every non-Closed Workflow State. Explicit status filters,
/// including Closed, select only that state.
pub(super) fn stale_issues(inner: &InMemoryStorageInner, query: &StaleQuery) -> Vec<Issue> {
    let mut candidates = inner
        .issues
        .values()
        .filter(|issue| matches_stale(issue, query))
        .collect::<Vec<_>>();
    candidates.sort_unstable_by(|left, right| {
        left.updated_at
            .cmp(&right.updated_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    candidates.truncate(query.limit().get());
    candidates.into_iter().cloned().collect()
}
