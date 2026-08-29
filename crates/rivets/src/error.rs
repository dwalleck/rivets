//! Error types for rivets CLI operations.

use crate::domain::{IssueId, ResourceError, StatusTransitionError};
use std::{fmt, io, path::PathBuf};
use thiserror::Error;

/// Configuration-related errors.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ConfigError {
    /// No rivets repository found in directory tree.
    #[error(
        "Not a rivets repository (or any of the parent directories). Run 'rivets init' to create a new repository."
    )]
    NotInitialized,

    /// Rivets is already initialized in the target directory.
    #[error("Rivets is already initialized in this directory. Found existing '{0}'")]
    AlreadyInitialized(String),

    /// Invalid issue ID prefix format.
    #[error("{0}")]
    InvalidPrefix(String),

    /// Failed to parse (deserialize) a YAML config file from disk.
    ///
    /// Carries the file path for diagnostic context. Contrast with
    /// [`YamlSerialization`](Self::YamlSerialization), which fires during
    /// serialization where no file path is relevant yet.
    #[error("Failed to parse config file '{path}': {source}")]
    Parse {
        /// Path to the config file that failed to parse.
        path: String,
        /// The underlying YAML parse error.
        source: serde_yaml::Error,
    },

    /// Failed to serialize config to YAML.
    ///
    /// This fires when converting an in-memory config struct to a YAML string
    /// (e.g., during `save()`). Unlike [`Parse`](Self::Parse), no file path
    /// is available because serialization precedes the write.
    #[error("YAML serialization error")]
    YamlSerialization(#[source] serde_yaml::Error),

    /// data_file path must be relative, not absolute.
    #[error("data_file must be a relative path")]
    AbsoluteDataPath,

    /// data_file path contains parent directory references.
    #[error("data_file must not contain parent directory references ('..')")]
    PathTraversal,

    /// Unknown storage backend specified in config.
    #[error("Unknown storage backend '{0}'. Supported backends: jsonl, postgresql")]
    UnknownBackend(String),

    /// Storage backend recognized but not yet implemented.
    ///
    /// Raised at both config-resolution time (e.g., `to_backend()`) and
    /// storage-creation time (e.g., `create_storage()`). A single variant
    /// here avoids duplication — the concept "this backend isn't ready"
    /// is a configuration-level concern regardless of which layer detects it.
    #[error("Storage backend not yet implemented: {0}")]
    UnsupportedBackend(String),
}

/// The reason one persisted Issue record was omitted during resilient loading.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum SkippedIssueRecordCause {
    /// The record could not be decoded from its JSONL representation.
    #[error("line {line_number}: malformed JSON ({error})")]
    MalformedJson {
        /// Physical 1-based line number in the JSONL file.
        line_number: usize,
        /// Decoder error returned by the JSONL adapter.
        error: String,
    },
    /// The decoded record violated an Issue invariant.
    #[error("line {line_number}: issue {issue_id} is invalid ({error})")]
    InvalidIssueData {
        /// Physical 1-based line number in the JSONL file.
        line_number: usize,
        /// Issue identifier decoded before validation failed.
        issue_id: IssueId,
        /// Domain validation error.
        error: String,
    },
    /// One of the decoded record's Associated Resources violated an invariant.
    #[error("line {line_number}: issue {issue_id} has an invalid Associated Resource ({source})")]
    InvalidResourceData {
        /// Physical 1-based line number in the JSONL file.
        line_number: usize,
        /// Issue identifier decoded before resource validation failed.
        issue_id: IssueId,
        /// Typed resource validation failure.
        #[source]
        source: ResourceError,
    },
}

impl SkippedIssueRecordCause {
    /// Returns the record's physical 1-based JSONL line number.
    #[must_use]
    pub const fn line_number(&self) -> usize {
        match self {
            Self::MalformedJson { line_number, .. }
            | Self::InvalidIssueData { line_number, .. }
            | Self::InvalidResourceData { line_number, .. } => *line_number,
        }
    }
}

/// A non-empty, typed account of Issue records omitted from a JSONL load.
#[derive(Debug)]
pub struct PartialLoadError {
    causes: Box<[SkippedIssueRecordCause]>,
}

impl PartialLoadError {
    pub(crate) fn new(mut causes: Vec<SkippedIssueRecordCause>) -> Option<Self> {
        if causes.is_empty() {
            None
        } else {
            causes.sort_by_key(SkippedIssueRecordCause::line_number);
            Some(Self {
                causes: causes.into_boxed_slice(),
            })
        }
    }

    /// Returns the number of Issue records omitted from the in-memory view.
    #[must_use]
    pub fn skipped_records(&self) -> usize {
        self.causes.len()
    }

    /// Returns the typed, source-ordered causes for every omitted Issue.
    #[must_use]
    pub fn causes(&self) -> &[SkippedIssueRecordCause] {
        &self.causes
    }
}

impl fmt::Display for PartialLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Refusing to modify storage after an incomplete JSONL load: {} issue record(s) were skipped: ",
            self.skipped_records()
        )?;
        let mut causes = self.causes.iter();
        if let Some(first) = causes.next() {
            write!(f, "{first}")?;
        }
        for cause in causes {
            write!(f, "; {cause}")?;
        }
        Ok(())
    }
}

impl std::error::Error for PartialLoadError {
    /// Exposes the first (lowest line number) cause on the standard error
    /// chain, so generic reporters like `anyhow` surface a typed cause without
    /// knowing about [`causes`](Self::causes). The full set remains available
    /// only through [`causes`](Self::causes).
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.causes
            .first()
            .map(|cause| cause as &(dyn std::error::Error + 'static))
    }
}

/// Storage-layer errors.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum StorageError {
    /// Issue data failed validation.
    ///
    /// Currently wraps the `String` returned by domain-level `validate()`.
    /// If `validate()` evolves to return a richer error type, this variant
    /// should be updated to wrap it via `#[source]` instead.
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

    /// A write was attempted after one or more Issue records were omitted
    /// during resilient JSONL loading.
    #[error(transparent)]
    UnsafePartialLoad(#[from] PartialLoadError),

    /// Persistent JSONL changed since this storage instance last synchronized.
    #[error("Persistent storage changed externally: {}", path.display())]
    ExternalChange {
        /// Path whose persisted revision no longer matches the cached revision.
        path: PathBuf,
    },

    /// JSON serialization failed during storage operations.
    #[error("JSON serialization failed")]
    Serialization(#[source] serde_json::Error),

    /// An Associated Resource invariant was violated.
    #[error(transparent)]
    Resource(#[from] crate::domain::ResourceError),

    /// A status change violated the domain transition rules.
    ///
    /// Transparent so every adapter surfaces the domain's message unchanged:
    /// CLI and MCP must reject a transition with the same observable error
    /// (ADR-0005).
    #[error(transparent)]
    InvalidStatusTransition(#[from] StatusTransitionError),
}

impl StorageError {
    /// Separates an Associated Resource invariant failure from other storage failures.
    ///
    /// This classification lives in the owning crate so adding a new
    /// [`StorageError`] variant forces an explicit decision here. External
    /// adapters can special-case resource input errors without a wildcard over
    /// this non-exhaustive enum.
    ///
    /// # Errors
    ///
    /// Returns the original error unchanged when it is not an Associated
    /// Resource invariant failure.
    pub fn try_into_resource_error(self) -> std::result::Result<ResourceError, Self> {
        match self {
            Self::Resource(source) => Ok(source),
            error @ (Self::Validation(_)
            | Self::IdGeneration(_)
            | Self::DuplicateDependency { .. }
            | Self::InvalidFormat(_)
            | Self::UnsafePartialLoad(_)
            | Self::ExternalChange { .. }
            | Self::Serialization(_)
            | Self::InvalidStatusTransition(_)) => Err(error),
        }
    }

    /// Separates a rejected status transition from other storage failures.
    ///
    /// Same rationale as [`try_into_resource_error`](Self::try_into_resource_error):
    /// the classification lives here so external adapters can surface the
    /// domain rejection first-class without a wildcard over this
    /// non-exhaustive enum.
    ///
    /// # Errors
    ///
    /// Returns the original error unchanged when it is not a rejected
    /// status transition.
    pub fn try_into_status_transition_error(
        self,
    ) -> std::result::Result<StatusTransitionError, Self> {
        match self {
            Self::InvalidStatusTransition(source) => Ok(source),
            error @ (Self::Validation(_)
            | Self::IdGeneration(_)
            | Self::DuplicateDependency { .. }
            | Self::InvalidFormat(_)
            | Self::UnsafePartialLoad(_)
            | Self::ExternalChange { .. }
            | Self::Serialization(_)
            | Self::Resource(_)) => Err(error),
        }
    }
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
    ///
    /// `field` uses `&'static str` because validation field names are known at
    /// compile time (e.g., `"prefix"`, `"title"`). This prevents accidental
    /// use with runtime-generated field names and avoids allocation. If dynamic
    /// field names are ever needed, this should be changed to `String`.
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
    #[error(
        "Cannot delete {issue_id}: {dependent_count} other issue(s) depend on it. Dependents: {dependents:?}"
    )]
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
    #[case::not_initialized(
        ConfigError::NotInitialized,
        "Not a rivets repository (or any of the parent directories). Run 'rivets init' to create a new repository."
    )]
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

    #[test]
    fn config_parse_display_includes_path_and_source() {
        let yaml_err = serde_yaml::from_str::<String>("invalid: [yaml").unwrap_err();
        let error = ConfigError::Parse {
            path: "config.yaml".to_string(),
            source: yaml_err,
        };
        let msg = error.to_string();
        assert!(msg.starts_with("Failed to parse config file 'config.yaml': "));
    }

    #[test]
    fn config_yaml_serialization_display() {
        let yaml_err = serde_yaml::from_str::<String>("invalid: [yaml").unwrap_err();
        let error = ConfigError::YamlSerialization(yaml_err);
        assert_eq!(error.to_string(), "YAML serialization error");
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
    #[case::invalid_format(
        StorageError::InvalidFormat("unexpected field".to_string()),
        "Invalid format: unexpected field"
    )]
    #[case::external_change(
        StorageError::ExternalChange {
            path: PathBuf::from("/workspace/.rivets/issues.jsonl"),
        },
        "Persistent storage changed externally: /workspace/.rivets/issues.jsonl"
    )]
    fn storage_error_display(#[case] error: StorageError, #[case] expected: &str) {
        assert_eq!(error.to_string(), expected);
    }

    #[rstest]
    #[case::malformed_json(
        SkippedIssueRecordCause::MalformedJson {
            line_number: 3,
            error: "unexpected end of input".to_string(),
        },
        "line 3: malformed JSON (unexpected end of input)"
    )]
    #[case::invalid_issue_data(
        SkippedIssueRecordCause::InvalidIssueData {
            line_number: 7,
            issue_id: IssueId::new("proj-abc"),
            error: "Priority exceeds maximum".to_string(),
        },
        "line 7: issue proj-abc is invalid (Priority exceeds maximum)"
    )]
    #[case::invalid_resource_data(
        SkippedIssueRecordCause::InvalidResourceData {
            line_number: 9,
            issue_id: IssueId::new("proj-def"),
            source: ResourceError::EmptyResourceId,
        },
        "line 9: issue proj-def has an invalid Associated Resource (Resource identifier cannot be empty)"
    )]
    fn skipped_issue_record_cause_display(
        #[case] cause: SkippedIssueRecordCause,
        #[case] expected: &str,
    ) {
        assert_eq!(cause.to_string(), expected);
    }

    #[test]
    fn partial_load_error_display_orders_causes_by_line() {
        let error = PartialLoadError::new(vec![
            SkippedIssueRecordCause::InvalidIssueData {
                line_number: 5,
                issue_id: IssueId::new("proj-abc"),
                error: "bad data".to_string(),
            },
            SkippedIssueRecordCause::MalformedJson {
                line_number: 2,
                error: "truncated".to_string(),
            },
        ])
        .expect("two causes should produce an error");
        assert_eq!(
            error.to_string(),
            "Refusing to modify storage after an incomplete JSONL load: \
             2 issue record(s) were skipped: \
             line 2: malformed JSON (truncated); \
             line 5: issue proj-abc is invalid (bad data)"
        );
    }

    #[test]
    fn partial_load_error_requires_at_least_one_cause() {
        assert!(
            PartialLoadError::new(Vec::new()).is_none(),
            "an empty cause list must not build a PartialLoadError"
        );
    }

    #[test]
    fn partial_load_error_source_chain_reaches_typed_resource_error() {
        let error = PartialLoadError::new(vec![SkippedIssueRecordCause::InvalidResourceData {
            line_number: 1,
            issue_id: IssueId::new("proj-abc"),
            source: ResourceError::EmptyResourceId,
        }])
        .expect("one cause should produce an error");

        let cause = error
            .source()
            .expect("PartialLoadError should expose its first cause as source");
        let skipped = cause
            .downcast_ref::<SkippedIssueRecordCause>()
            .expect("source should be a SkippedIssueRecordCause");
        let resource_error = skipped
            .source()
            .expect("InvalidResourceData should expose its typed source")
            .downcast_ref::<ResourceError>()
            .expect("cause source should be a ResourceError");
        assert!(matches!(resource_error, ResourceError::EmptyResourceId));
    }

    #[test]
    fn try_into_resource_error_extracts_resource_variant() {
        let error = StorageError::Resource(ResourceError::EmptyLabel);
        assert!(matches!(
            error.try_into_resource_error(),
            Ok(ResourceError::EmptyLabel)
        ));
    }

    #[test]
    fn try_into_resource_error_returns_other_variants_unchanged() {
        let error = StorageError::Validation("title is required".to_string());
        assert!(matches!(
            error.try_into_resource_error(),
            Err(StorageError::Validation(reason)) if reason == "title is required"
        ));
    }

    #[test]
    fn try_into_status_transition_error_extracts_transition_variant() {
        use crate::domain::IssueStatus;

        let error = StorageError::InvalidStatusTransition(StatusTransitionError::AlreadyClosed {
            current: IssueStatus::Closed,
        });
        assert!(matches!(
            error.try_into_status_transition_error(),
            Ok(StatusTransitionError::AlreadyClosed {
                current: IssueStatus::Closed
            })
        ));
    }

    #[test]
    fn try_into_status_transition_error_returns_other_variants_unchanged() {
        let error = StorageError::Validation("title is required".to_string());
        assert!(matches!(
            error.try_into_status_transition_error(),
            Err(StorageError::Validation(reason)) if reason == "title is required"
        ));
    }

    #[test]
    fn storage_serialization_display() {
        let json_err = serde_json::from_str::<String>("not json").unwrap_err();
        let error = StorageError::Serialization(json_err);
        assert_eq!(error.to_string(), "JSON serialization failed");
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
    fn config_yaml_serialization_error_has_source() {
        let yaml_err = serde_yaml::from_str::<String>("invalid: [yaml").unwrap_err();
        let error = ConfigError::YamlSerialization(yaml_err);
        assert!(
            error.source().is_some(),
            "ConfigError::YamlSerialization should expose a source"
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
