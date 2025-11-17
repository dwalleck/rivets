//! Error types for rivets CLI operations.

use std::io;
use thiserror::Error;

/// The error type for rivets CLI operations.
#[allow(dead_code)]
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
    IssueNotFound(String),
}

/// A specialized Result type for rivets operations.
#[allow(dead_code)]
pub type Result<T> = std::result::Result<T, Error>;
