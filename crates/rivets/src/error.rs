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
    #[error("{0}")]
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

    /// data_file path contains parent directory references.
    #[error("data_file must not contain parent directory references ('..')")]
    PathTraversal,

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

    /// JSON parsing error (e.g., loading corrupt JSONL files).
    ///
    /// Note: Storage-layer serialization failures use [`StorageError::Serialization`]
    /// instead, to distinguish internal bugs from external data problems.
    /// Because this variant has `#[from]`, bare `?` on `serde_json::Error` will
    /// route here — use `.map_err(StorageError::Serialization)` explicitly in
    /// storage code.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

/// A specialized Result type for rivets operations.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use std::error::Error as StdError;

    // ========== Display Formatting Tests ==========

    #[rstest]
    #[case::invalid_prefix(
        ConfigError::InvalidPrefix("Prefix must be at least 2 characters".to_string()),
        "Prefix must be at least 2 characters"
    )]
    #[case::already_initialized(
        ConfigError::AlreadyInitialized(".rivets".to_string()),
        "Rivets is already initialized in this directory. Found existing '.rivets'"
    )]
    #[case::unsupported_backend(
        ConfigError::UnsupportedBackend("PostgreSQL".to_string()),
        "Storage backend not yet implemented: PostgreSQL"
    )]
    #[case::absolute_data_path(ConfigError::AbsoluteDataPath, "data_file must be a relative path")]
    #[case::path_traversal(
        ConfigError::PathTraversal,
        "data_file must not contain parent directory references ('..')"
    )]
    #[case::unknown_backend(
        ConfigError::UnknownBackend("redis".to_string()),
        "Unknown storage backend 'redis'. Supported backends: jsonl, postgresql"
    )]
    fn config_error_display(#[case] error: ConfigError, #[case] expected: &str) {
        assert_eq!(error.to_string(), expected);
    }

    #[rstest]
    #[case::validation(
        StorageError::Validation("title is required".to_string()),
        "Validation failed: title is required"
    )]
    #[case::duplicate_dependency(
        StorageError::DuplicateDependency {
            from: IssueId::new("proj-abc"),
            to: IssueId::new("proj-def"),
        },
        "Dependency already exists: proj-abc -> proj-def"
    )]
    #[case::id_generation(
        StorageError::IdGeneration("exhausted retries".to_string()),
        "ID generation failed: exhausted retries"
    )]
    #[case::unsupported_backend(
        StorageError::UnsupportedBackend("PostgreSQL".to_string()),
        "Storage backend not yet implemented: PostgreSQL"
    )]
    fn storage_error_display(#[case] error: StorageError, #[case] expected: &str) {
        assert_eq!(error.to_string(), expected);
    }

    #[test]
    fn validation_error_display() {
        let error = Error::Validation {
            field: "priority",
            reason: "must be between 0 and 4".to_string(),
        };
        assert_eq!(error.to_string(), "must be between 0 and 4");
    }

    // ========== Source Chain Tests ==========

    #[test]
    fn config_parse_error_has_source() {
        let yaml_err = serde_yaml::from_str::<String>("invalid: [yaml").unwrap_err();
        let error = ConfigError::Parse {
            path: "config.yaml".to_string(),
            source: yaml_err,
        };
        assert!(
            error.source().is_some(),
            "ConfigError::Parse should expose a source"
        );
    }

    #[test]
    fn config_yaml_error_has_source() {
        let yaml_err = serde_yaml::from_str::<String>("invalid: [yaml").unwrap_err();
        let error = ConfigError::Yaml(yaml_err);
        assert!(
            error.source().is_some(),
            "ConfigError::Yaml should expose a source"
        );
    }

    #[test]
    fn storage_serialization_error_has_source() {
        let json_err = serde_json::from_str::<String>("not json").unwrap_err();
        let error = StorageError::Serialization(json_err);
        assert!(
            error.source().is_some(),
            "StorageError::Serialization should expose a source"
        );
    }

    #[test]
    fn invalid_prefix_has_no_source() {
        let error = ConfigError::InvalidPrefix("too short".to_string());
        assert!(
            error.source().is_none(),
            "ConfigError::InvalidPrefix should not have a source"
        );
    }

    // ========== From Conversion Tests ==========

    #[test]
    fn config_error_converts_to_error() {
        let config_err = ConfigError::InvalidPrefix("bad prefix".to_string());
        let error: Error = config_err.into();
        assert!(
            matches!(error, Error::Config(ConfigError::InvalidPrefix(_))),
            "ConfigError should convert to Error::Config"
        );
    }

    #[test]
    fn storage_error_converts_to_error() {
        let storage_err = StorageError::Validation("missing field".to_string());
        let error: Error = storage_err.into();
        assert!(
            matches!(error, Error::Storage(StorageError::Validation(_))),
            "StorageError should convert to Error::Storage"
        );
    }

    #[test]
    fn io_error_converts_to_error() {
        let io_err = io::Error::new(io::ErrorKind::NotFound, "file missing");
        let error: Error = io_err.into();
        assert!(
            matches!(error, Error::Io(_)),
            "io::Error should convert to Error::Io"
        );
    }

    // ========== Validation Field Access Test ==========

    #[test]
    fn validation_error_exposes_field_for_matching() {
        let error = Error::Validation {
            field: "prefix",
            reason: "too short".to_string(),
        };
        match &error {
            Error::Validation { field, .. } => assert_eq!(*field, "prefix"),
            other => panic!("Expected Error::Validation, got: {other:?}"),
        }
    }
}
