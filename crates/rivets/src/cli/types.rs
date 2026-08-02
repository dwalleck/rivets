//! CLI value enums and batch operation results.
//!
//! The four Issue-vocabulary enums (IssueKind, IssueStatus, ResourceRole,
//! DependencyType) are consumed directly from `crate::domain` by the CLI
//! argument structs; their clap value names, Display, FromStr, and serde
//! attributes all live on the domain declarations. This module keeps only
//! the two sorting enums and the batch operation result types.
//!
//! `SortOrderArg` has no domain twin at all; `SortPolicyArg` mirrors the
//! domain `SortPolicy` variant-for-variant, but that domain type is not on
//! the wire (no serde derives) and is not part of the ADR-0004 vocabulary
//! scope, so the CLI-side value enum stays here until a domain twin earns
//! serde/Display/FromStr of its own.

use clap::ValueEnum;
use serde::Serialize;

use crate::domain::Issue;

// ============================================================================
// Batch Operation Results
// ============================================================================

/// Result of a batch operation on multiple issues.
///
/// Batch operations (update, close, reopen, label add/remove) process each
/// issue independently and save after each success. This allows partial
/// progress to be preserved even when some operations fail.
#[derive(Debug, Clone, Serialize)]
pub struct BatchResult {
    /// Issues that were successfully processed and saved
    pub succeeded: Vec<Issue>,
    /// Issues that failed with their error messages
    pub failed: Vec<BatchError>,
}

impl BatchResult {
    /// Create a new empty batch result
    pub fn new() -> Self {
        Self {
            succeeded: Vec::new(),
            failed: Vec::new(),
        }
    }

    /// Check if all operations succeeded (no failures)
    pub fn is_complete_success(&self) -> bool {
        self.failed.is_empty()
    }

    /// Check if all operations failed (no successes)
    pub fn is_complete_failure(&self) -> bool {
        self.succeeded.is_empty() && !self.failed.is_empty()
    }

    /// Check if there were any failures
    pub fn has_failures(&self) -> bool {
        !self.failed.is_empty()
    }

    /// Get the total number of operations attempted
    pub fn total(&self) -> usize {
        self.succeeded.len() + self.failed.len()
    }
}

impl Default for BatchResult {
    fn default() -> Self {
        Self::new()
    }
}

/// Error details for a failed batch operation on a single issue
#[derive(Debug, Clone, Serialize)]
pub struct BatchError {
    /// The issue ID that failed
    pub issue_id: String,
    /// Human-readable error message
    pub error: String,
}

// ============================================================================
// Value Enums
// ============================================================================

/// Sort order for list command
#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortOrderArg {
    /// Sort by priority (highest first)
    #[default]
    Priority,
    /// Sort by creation date (newest first)
    Newest,
    /// Sort by creation date (oldest first)
    Oldest,
    /// Sort by last update (most recent first)
    Updated,
}

impl std::fmt::Display for SortOrderArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Priority => write!(f, "priority"),
            Self::Newest => write!(f, "newest"),
            Self::Oldest => write!(f, "oldest"),
            Self::Updated => write!(f, "updated"),
        }
    }
}

/// Sort policy for ready command
#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortPolicyArg {
    /// Recent issues (48h) by priority, older by age
    #[default]
    Hybrid,
    /// Strict priority ordering (P0 -> P1 -> P2 -> P3 -> P4)
    Priority,
    /// Oldest issues first
    Oldest,
}

impl std::fmt::Display for SortPolicyArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Hybrid => write!(f, "hybrid"),
            Self::Priority => write!(f, "priority"),
            Self::Oldest => write!(f, "oldest"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_implementations() {
        assert_eq!(format!("{}", SortOrderArg::Priority), "priority");
        assert_eq!(format!("{}", SortPolicyArg::Hybrid), "hybrid");
    }
}
