//! Error types for rivets CLI operations.

use crate::domain::IssueId;
use std::io;
use thiserror::Error;

/// Configuration-related errors.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// No rivets repository found in directory tree.
    #[error("Not a rivets repository (or any of the parent directories). Run 'rivets init' to create a new repository.")]
    NotInitialized,

    /// Rivets is already initialized in the target directory.
    #[error("Rivets is already initialized in this directory. Found existing '{0}'")]
    AlreadyInitialized(String),

    /// Invalid issue ID prefix format.
    #[error("Invalid prefix: {0}")]
    InvalidPrefix(String),

    /// Failed to parse the YAML config file.
    #[error("Failed to parse config file '{path}': {source}")]
    Parse {
        /// Path to the config file that failed to parse.
        path: String,
        /// The underlying YAML parse error.
        source: serde_yaml::Error,
    },

    /// YAML serialization error.
    #[error("YAML serialization error")]
    Yaml(#[source] serde_yaml::Error),

    /// data_file path must be relative, not absolute.
    #[error("data_file must be a relative path")]
    AbsoluteDataPath,

    /// Unknown storage backend specified in config.
    #[error("Unknown storage backend '{0}'. Supported backends: jsonl, postgresql")]
    UnknownBackend(String),

    /// Storage backend exists but is not yet implemented.
    #[error("Storage backend not yet implemented: {0}")]
    UnsupportedBackend(String),
}

/// Storage-layer errors.
#[derive(Debug, Error)]
pub enum StorageError {
    /// Issue data failed validation.
    #[error("Validation failed: {0}")]
    Validation(String),

    /// Failed to generate a unique issue ID.
    #[error("ID generation failed: {0}")]
    IdGeneration(String),

    /// Attempted to add a dependency that already exists.
    #[error("Dependency already exists: {from} -> {to}")]
    DuplicateDependency {
        /// The source issue.
        from: IssueId,
        /// The target issue.
        to: IssueId,
    },

    /// Invalid format encountered during parsing.
    #[error("Invalid format: {0}")]
    InvalidFormat(String),

    /// JSON serialization failed during storage operations.
    #[error("JSON serialization failed")]
    Serialization(#[source] serde_json::Error),

    /// Storage backend exists but is not yet implemented.
    #[error("Storage backend not yet implemented: {0}")]
    UnsupportedBackend(String),
}

/// The error type for rivets operations.
#[derive(Debug, Error)]
pub enum Error {
    /// IO error occurred.
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    /// Configuration error.
    #[error("{0}")]
    Config(#[from] ConfigError),

    /// Storage error.
    #[error("{0}")]
    Storage(#[from] StorageError),

    /// CLI input validation error.
    #[error("{reason}")]
    Validation {
        /// The field that failed validation (available for programmatic access).
        field: &'static str,
        /// Why the value was invalid.
        reason: String,
    },

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
