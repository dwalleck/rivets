//! Error types for the rivets MCP server.

use rivets::error::{Error as RivetsError, StorageError};
use thiserror::Error;

/// Errors that can occur in the rivets MCP server.
#[derive(Debug, Error)]
pub enum Error {
    /// No workspace context has been set.
    #[error("No workspace context set. Call set_context first.")]
    NoContext,

    /// Invalid argument value provided.
    #[error("Invalid {field}: '{value}'. Valid values: {valid_values}")]
    InvalidArgument {
        /// The field name that had an invalid value.
        field: &'static str,
        /// The invalid value that was provided.
        value: String,
        /// Description of valid values.
        valid_values: &'static str,
    },

    /// Note content failed domain validation.
    #[error("Invalid note: {0}")]
    InvalidNote(#[from] rivets::domain::NoteError),

    /// Associated Resource input failed domain validation.
    #[error("Invalid resource: {0}")]
    InvalidResource(#[from] rivets::domain::ResourceError),

    /// The requested issue was not found.
    #[error("Issue not found: {0}")]
    IssueNotFound(String),

    /// The specified workspace was not found or path is invalid.
    #[error("Workspace not found: {path}")]
    WorkspaceNotFound {
        /// The path that was not found.
        path: String,
        /// The underlying IO error, if any.
        #[source]
        source: Option<std::io::Error>,
    },

    /// Workspace exists but was not initialized via `set_context`.
    #[error("Workspace not initialized: {0}. Call set_context first.")]
    WorkspaceNotInitialized(String),

    /// Failed to discover a rivets workspace.
    #[error("No .rivets directory found in {0} or parent directories")]
    NoRivetsDirectory(String),

    /// Failed to load workspace configuration.
    #[error(
        "Failed to load config from '{path}': {reason}. Run 'rivets init' to create a valid configuration."
    )]
    ConfigLoad {
        /// The path to the config file.
        path: String,
        /// The reason for the failure.
        reason: String,
    },

    /// An error from the rivets storage layer.
    #[error("Storage error: {0}")]
    Storage(#[source] RivetsError),

    /// An I/O error occurred.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization/deserialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Result type for rivets MCP operations.
pub type Result<T> = std::result::Result<T, Error>;

impl From<RivetsError> for Error {
    fn from(error: RivetsError) -> Self {
        match error {
            RivetsError::IssueNotFound(issue_id) => Self::IssueNotFound(issue_id.to_string()),
            RivetsError::Storage(StorageError::Resource(source)) => Self::InvalidResource(source),
            error @ (RivetsError::Io(_)
            | RivetsError::Config(_)
            | RivetsError::Storage(_)
            | RivetsError::Validation { .. }
            | RivetsError::HasDependents { .. }
            | RivetsError::CircularDependency { .. }
            | RivetsError::InvalidIssueId(_)
            | RivetsError::InvalidPriority(_)
            | RivetsError::DependencyNotFound { .. }
            | RivetsError::IssueAlreadyExists(_)
            | RivetsError::Json(_)) => Self::Storage(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivets::domain::{IssueId, ResourceError};

    #[test]
    fn core_issue_not_found_maps_to_mcp_issue_not_found() {
        let error = Error::from(RivetsError::IssueNotFound(IssueId::new("test-missing")));
        assert!(matches!(
            error,
            Error::IssueNotFound(issue_id) if issue_id == "test-missing"
        ));
    }

    #[test]
    fn storage_resource_error_maps_to_invalid_resource() {
        let error = Error::from(RivetsError::Storage(StorageError::Resource(
            ResourceError::EmptyLabel,
        )));
        assert!(matches!(
            error,
            Error::InvalidResource(ResourceError::EmptyLabel)
        ));
    }
}
