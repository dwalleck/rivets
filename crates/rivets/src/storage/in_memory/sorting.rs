//! Sort policy implementations for the ready work algorithm.

use crate::domain::{Issue, SortPolicy};
use chrono::{Duration, Utc};

/// Time window (in hours) for the hybrid sort policy.
///
/// Issues created within this window are considered "recent" and sorted by priority.
/// Issues older than this threshold are sorted by age (oldest first) to prevent starvation.
///
/// The 48-hour default balances urgency (high-priority recent work surfaces quickly)
/// with fairness (older issues eventually get promoted regardless of priority).
///
/// Note: This window could be made configurable in future versions if use cases
/// arise requiring different urgency/fairness trade-offs.
pub(super) const HYBRID_SORT_RECENT_WINDOW_HOURS: i64 = 48;

/// Sort issues according to the specified sort policy.
///
/// # Sort Policies
///
/// - `Hybrid`: Recent issues (< 48h) sorted by priority, older issues by age
/// - `Priority`: Strict priority ordering (P0 -> P1 -> P2 -> P3 -> P4)
/// - `Oldest`: Creation date ascending (oldest first)
///
/// # Tiebreaker Philosophy
///
/// Within the same priority level, issues are sorted by creation date with
/// **oldest first** as the tiebreaker. This prevents starvation of older issues
/// that have been waiting longer at the same priority level.
///
/// As a final tiebreaker, `issue.id` is used to ensure **deterministic ordering**
/// when priority and creation timestamps are identical. This improves debugging
/// reproducibility and makes test assertions stable.
///
/// The cutoff window (see [`HYBRID_SORT_RECENT_WINDOW_HOURS`]) in Hybrid mode ensures:
/// 1. High-priority recent work gets immediate attention
/// 2. Older issues don't languish indefinitely (promoted after the window expires)
/// 3. Within each tier, FIFO ordering maintains fairness
pub(super) fn sort_by_policy(issues: &mut [Issue], policy: SortPolicy) {
    match policy {
        SortPolicy::Hybrid => {
            let now = Utc::now();
            let cutoff = now - Duration::hours(HYBRID_SORT_RECENT_WINDOW_HOURS);

            issues.sort_by(|a, b| {
                let a_is_recent = a.created_at > cutoff;
                let b_is_recent = b.created_at > cutoff;

                match (a_is_recent, b_is_recent) {
                    // Both recent: sort by priority (P0 first), then oldest first within priority,
                    // then by ID for determinism when timestamps match exactly.
                    (true, true) => a
                        .priority
                        .cmp(&b.priority)
                        .then(a.created_at.cmp(&b.created_at))
                        .then(a.id.cmp(&b.id)),
                    // Both old: sort by age (oldest first), then by ID for determinism
                    (false, false) => a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)),
                    // Recent issues come before older ones (urgency window)
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                }
            });
        }
        SortPolicy::Priority => {
            // Strict priority ordering with oldest-first tiebreaker, ID for determinism
            issues.sort_by(|a, b| {
                a.priority
                    .cmp(&b.priority)
                    .then(a.created_at.cmp(&b.created_at))
                    .then(a.id.cmp(&b.id))
            });
        }
        SortPolicy::Oldest => {
            // Pure age-based ordering (oldest first), ID for determinism
            issues.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
        }
    }
}
