//! Error types for rivets-jsonl operations.

use std::io;
use thiserror::Error;

/// The error type for rivets-jsonl operations.
#[derive(Debug, Error)]
pub enum Error {
    /// IO error occurred while reading or writing.
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    /// JSON parsing or serialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Invalid JSONL format.
    #[error("Invalid JSONL format: {0}")]
    InvalidFormat(String),
}

/// A specialized Result type for rivets-jsonl operations.
pub type Result<T> = std::result::Result<T, Error>;
