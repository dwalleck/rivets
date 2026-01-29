//! Error types for LSP operations.

use thiserror::Error;

/// Errors that can occur during LSP operations.
#[derive(Debug, Error)]
pub enum LspError {
    /// Failed to spawn the LSP server process.
    #[error("failed to spawn LSP server '{command}': {source}")]
    SpawnFailed {
        /// The command that failed to spawn.
        command: String,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// LSP server executable not found.
    #[error("{command} not found\n\nLSP refinement was requested but the language server is not available.\n{install_hint}\n\nTo index without LSP refinement, omit the --lsp flag.")]
    NotFound {
        /// The command that was not found.
        command: String,
        /// Installation instructions for the missing command.
        install_hint: String,
    },

    /// I/O error communicating with the LSP server.
    #[error("LSP communication error: {0}")]
    Io(#[from] std::io::Error),

    /// Failed to serialize request to JSON.
    #[error("failed to serialize LSP request: {0}")]
    Serialize(#[source] serde_json::Error),

    /// Failed to deserialize response from JSON.
    #[error("failed to deserialize LSP response: {0}")]
    Deserialize(#[source] serde_json::Error),

    /// Invalid Content-Length header in response.
    #[error("invalid Content-Length header: {0}")]
    InvalidHeader(String),

    /// Invalid file path for LSP operation.
    #[error("invalid path: {0}")]
    InvalidPath(String),

    /// LSP server returned an error response.
    #[error("LSP error {code}: {message}")]
    ServerError {
        /// The JSON-RPC error code.
        code: i64,
        /// The error message from the server.
        message: String,
    },

    /// Response ID doesn't match request ID.
    #[error("response ID mismatch: expected {expected}, got {actual}")]
    IdMismatch {
        /// The expected request ID.
        expected: i64,
        /// The actual response ID received.
        actual: i64,
    },

    /// LSP server process exited unexpectedly.
    #[error("LSP server exited unexpectedly")]
    ServerExited,

    /// Initialize handshake failed.
    #[error("LSP initialize handshake failed: {0}")]
    InitializeFailed(String),
}

impl LspError {
    /// Create a "not found" error with an install hint.
    #[must_use]
    pub fn not_found(command: &str, install_hint: &str) -> Self {
        Self::NotFound {
            command: command.to_string(),
            install_hint: install_hint.to_string(),
        }
    }

    /// Create a spawn failed error.
    #[must_use]
    pub fn spawn_failed(command: &str, source: std::io::Error) -> Self {
        Self::SpawnFailed {
            command: command.to_string(),
            source,
        }
    }

    /// Create a server error from JSON-RPC error response.
    #[must_use]
    pub fn server_error(code: i64, message: impl Into<String>) -> Self {
        Self::ServerError {
            code,
            message: message.into(),
        }
    }
}
