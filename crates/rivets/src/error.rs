//! Error types for rivets CLI operations.

use crate::domain::IssueId;
use std::io;
use thiserror::Error;

/// The error type for rivets operations.
#[derive(Debug, Error)]
pub enum Error {
    /// IO error occurred.
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    /// Configuration error.
    #[error("Configuration error: {0}")]
    Config(String),

    /// Storage error.
    #[error("Storage error: {0}")]
    Storage(String),

    /// Issue not found.
    #[error("Issue not found: {0}")]
    IssueNotFound(IssueId),

    /// Cannot delete issue because other issues depend on it.
    #[error("Cannot delete {issue_id}: {dependent_count} other issue(s) depend on it. Dependents: {dependents:?}")]
    HasDependents {
        /// The issue that cannot be deleted
        issue_id: IssueId,
        /// The number of dependent issues
        dependent_count: usize,
        /// List of dependent issue IDs
        dependents: Vec<IssueId>,
    },

    /// Circular dependency detected.
    #[error(
        "Circular dependency detected: adding dependency from {from} to {to} would create a cycle"
    )]
    CircularDependency {
        /// The source issue
        from: IssueId,
        /// The target issue
        to: IssueId,
    },

    /// Invalid issue ID format.
    #[error("Invalid issue ID format: {0}")]
    InvalidIssueId(String),

    /// Invalid priority value.
    #[error("Invalid priority value: {0} (must be 0-4)")]
    InvalidPriority(u8),

    /// Dependency not found.
    #[error("Dependency not found: {from} -> {to}")]
    DependencyNotFound {
        /// The source issue
        from: IssueId,
        /// The target issue
        to: IssueId,
    },

    /// Issue already exists.
    #[error("Issue already exists: {0}")]
    IssueAlreadyExists(IssueId),

    /// JSON parsing error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

/// A specialized Result type for rivets operations.
pub type Result<T> = std::result::Result<T, Error>;
