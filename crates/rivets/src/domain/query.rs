//! Validated query values shared by List and Stale storage intents.

use super::{IssueFilter, IssueKind, IssueStatus, Label, MAX_PRIORITY};
use chrono::{DateTime, Duration, Utc};
use std::num::NonZeroUsize;
use thiserror::Error;

/// Why a bounded List or Stale query could not be constructed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum QueryError {
    /// A bounded query must always state its result limit.
    #[error("query limit is required")]
    MissingLimit,
    /// A bounded query limit must be greater than zero.
    #[error("query limit must be greater than zero (got {0})")]
    InvalidLimit(usize),
    /// Priority values outside the canonical P0-P4 range are rejected.
    #[error("invalid priority value: {0} (must be 0-4)")]
    InvalidPriority(u8),
    /// The requested stale age cannot be represented by the supplied instant.
    #[error("stale query cutoff cannot be represented for {days} days")]
    CutoffOverflow {
        /// Number of days subtracted from the supplied current instant.
        days: u32,
    },
}

/// Validated, opaque input for the canonical List Issues intent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListQuery {
    limit: NonZeroUsize,
    status: Option<IssueStatus>,
    priority: Option<u8>,
    issue_kind: Option<IssueKind>,
    assignee: Option<String>,
    label: Option<Label>,
}

impl TryFrom<IssueFilter> for ListQuery {
    type Error = QueryError;

    fn try_from(filter: IssueFilter) -> Result<Self, Self::Error> {
        let raw_limit = filter.limit.ok_or(QueryError::MissingLimit)?;
        let limit = NonZeroUsize::new(raw_limit).ok_or(QueryError::InvalidLimit(raw_limit))?;
        if let Some(priority) = filter.priority
            && priority > MAX_PRIORITY
        {
            return Err(QueryError::InvalidPriority(priority));
        }

        Ok(Self {
            limit,
            status: filter.status,
            priority: filter.priority,
            issue_kind: filter.issue_kind,
            assignee: filter.assignee,
            label: filter.label,
        })
    }
}

impl ListQuery {
    /// Return the required positive result limit.
    #[must_use]
    pub const fn limit(&self) -> NonZeroUsize {
        self.limit
    }

    /// Return the optional Workflow State filter.
    #[must_use]
    pub const fn status(&self) -> Option<IssueStatus> {
        self.status
    }

    /// Return the optional canonical priority filter.
    #[must_use]
    pub const fn priority(&self) -> Option<u8> {
        self.priority
    }

    /// Return the optional Issue Kind filter.
    #[must_use]
    pub const fn issue_kind(&self) -> Option<IssueKind> {
        self.issue_kind
    }

    /// Return the optional exact Assignee filter.
    #[must_use]
    pub fn assignee(&self) -> Option<&str> {
        self.assignee.as_deref()
    }

    /// Return the optional canonical Label filter.
    #[must_use]
    pub fn label(&self) -> Option<&Label> {
        self.label.as_ref()
    }
}

/// Validated, opaque input for the canonical Stale Issues intent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleQuery {
    limit: NonZeroUsize,
    status: Option<IssueStatus>,
    days: u32,
    cutoff: DateTime<Utc>,
}

impl StaleQuery {
    /// Construct a stale query using one caller-supplied current instant.
    ///
    /// The stale boundary is strict: storage selects Issues whose `updated_at`
    /// is earlier than the stored cutoff. Subtraction is checked so a caller
    /// cannot silently receive an invalid or wrapped time boundary.
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::CutoffOverflow`] when subtracting `days` from
    /// `now` falls outside Chrono's representable timestamp range.
    pub fn new(
        limit: NonZeroUsize,
        status: Option<IssueStatus>,
        days: u32,
        now: DateTime<Utc>,
    ) -> Result<Self, QueryError> {
        let duration = Duration::days(i64::from(days));
        let cutoff = now
            .checked_sub_signed(duration)
            .ok_or(QueryError::CutoffOverflow { days })?;
        Ok(Self {
            limit,
            status,
            days,
            cutoff,
        })
    }

    /// Return the required positive result limit.
    #[must_use]
    pub const fn limit(&self) -> NonZeroUsize {
        self.limit
    }

    /// Return the optional Workflow State filter.
    #[must_use]
    pub const fn status(&self) -> Option<IssueStatus> {
        self.status
    }

    /// Return the requested stale age in days.
    #[must_use]
    pub const fn days(&self) -> u32 {
        self.days
    }

    /// Return the checked strict cutoff instant.
    #[must_use]
    pub const fn cutoff(&self) -> DateTime<Utc> {
        self.cutoff
    }
}
