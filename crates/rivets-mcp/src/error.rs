//! Error types for the rivets MCP server.

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

    /// An error from the rivets storage layer.
    #[error("Storage error: {0}")]
    Storage(#[from] rivets::error::Error),

    /// An I/O error occurred.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization/deserialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// MCP protocol error.
    #[error("MCP error: {0}")]
    Mcp(String),
}

/// Result type for rivets MCP operations.
pub type Result<T> = std::result::Result<T, Error>;
