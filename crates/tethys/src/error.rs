//! Error types for Tethys operations.
//!
//! Errors are categorized into two main types:
//!
//! - **`Error`**: Top-level errors that halt operations (database failures, etc.)
//! - **`IndexError`**: File-level errors that are collected but don't halt indexing
//!
//! ## Error Philosophy
//!
//! Tethys follows a "best effort" approach for indexing:
//! - A single malformed file shouldn't prevent indexing the rest
//! - Errors are collected and reported, not thrown
//! - Only infrastructure failures (database, I/O) cause early termination
//!
//! ## Error Categorization
//!
//! `IndexErrorKind` uses a 4xx/5xx style categorization:
//! - Input problems (user's fault): parse errors, unsupported languages
//! - Internal problems (our fault): I/O errors, database errors

use std::path::PathBuf;
use thiserror::Error;

/// Result type for Tethys operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Top-level error type for Tethys operations.
///
/// These errors represent infrastructure failures that prevent
/// the operation from completing.
#[derive(Debug, Error)]
pub enum Error {
    /// Database operation failed
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    /// File system operation failed
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Tree-sitter parsing infrastructure failed
    #[error("parser error: {0}")]
    Parser(String),

    /// Invalid configuration or arguments
    #[error("configuration error: {0}")]
    Config(String),
}

/// Error encountered while indexing a specific file.
///
/// These errors are collected during indexing but don't halt the operation.
/// The indexer continues with remaining files and reports all errors at the end.
#[derive(Debug, Clone)]
pub struct IndexError {
    /// Path to the file that failed
    pub path: PathBuf,
    /// Category of the error
    pub kind: IndexErrorKind,
    /// Human-readable error message
    pub message: String,
}

impl std::fmt::Display for IndexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: {} ({})",
            self.path.display(),
            self.message,
            self.kind
        )
    }
}

impl std::error::Error for IndexError {}

/// Categorization of indexing errors.
///
/// Uses a 4xx/5xx style pattern:
/// - Input problems are issues with the source files (user can fix)
/// - Internal problems are issues with Tethys itself (we need to fix)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexErrorKind {
    // === Input Problems (analogous to HTTP 4xx) ===
    /// Source file has syntax errors that prevent parsing
    ParseFailed,

    /// File type is not supported (unknown extension)
    UnsupportedLanguage,

    /// File content is not valid UTF-8
    EncodingError,

    // === Internal Problems (analogous to HTTP 5xx) ===
    /// Could not read the file from disk
    IoError,

    /// Database operation failed for this file
    DatabaseError,
}

impl std::fmt::Display for IndexErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ParseFailed => write!(f, "parse failed"),
            Self::UnsupportedLanguage => write!(f, "unsupported language"),
            Self::EncodingError => write!(f, "encoding error"),
            Self::IoError => write!(f, "I/O error"),
            Self::DatabaseError => write!(f, "database error"),
        }
    }
}

impl IndexErrorKind {
    /// Returns `true` if this is an input problem (4xx-style).
    ///
    /// Input problems are issues with the source files that the user can fix.
    #[must_use]
    pub fn is_input_error(&self) -> bool {
        matches!(
            self,
            Self::ParseFailed | Self::UnsupportedLanguage | Self::EncodingError
        )
    }

    /// Returns `true` if this is an internal problem (5xx-style).
    ///
    /// Internal problems are issues with Tethys infrastructure.
    #[must_use]
    pub fn is_internal_error(&self) -> bool {
        matches!(self, Self::IoError | Self::DatabaseError)
    }
}

impl IndexError {
    /// Create a new indexing error.
    #[must_use]
    pub fn new(path: PathBuf, kind: IndexErrorKind, message: impl Into<String>) -> Self {
        Self {
            path,
            kind,
            message: message.into(),
        }
    }

    /// Create a parse error for a file.
    #[must_use]
    pub fn parse_failed(path: PathBuf, message: impl Into<String>) -> Self {
        Self::new(path, IndexErrorKind::ParseFailed, message)
    }

    /// Create an unsupported language error.
    #[must_use]
    pub fn unsupported_language(path: PathBuf) -> Self {
        let ext = path
            .extension()
            .map_or_else(|| "none".to_string(), |e| e.to_string_lossy().to_string());
        Self::new(
            path,
            IndexErrorKind::UnsupportedLanguage,
            format!("unsupported extension: {ext}"),
        )
    }

    /// Create an encoding error for a file.
    #[must_use]
    pub fn encoding_error(path: PathBuf) -> Self {
        Self::new(
            path,
            IndexErrorKind::EncodingError,
            "file is not valid UTF-8",
        )
    }

    /// Create an I/O error for a file.
    #[must_use]
    pub fn io_error(path: PathBuf, error: &std::io::Error) -> Self {
        Self::new(path, IndexErrorKind::IoError, error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_error_kind_categorization() {
        // Input errors (4xx-style)
        assert!(IndexErrorKind::ParseFailed.is_input_error());
        assert!(IndexErrorKind::UnsupportedLanguage.is_input_error());
        assert!(IndexErrorKind::EncodingError.is_input_error());
        assert!(!IndexErrorKind::ParseFailed.is_internal_error());

        // Internal errors (5xx-style)
        assert!(IndexErrorKind::IoError.is_internal_error());
        assert!(IndexErrorKind::DatabaseError.is_internal_error());
        assert!(!IndexErrorKind::IoError.is_input_error());
    }

    #[test]
    fn index_error_display_includes_path_and_kind() {
        let error = IndexError::parse_failed(PathBuf::from("src/main.rs"), "unexpected token");

        let display = error.to_string();
        assert!(display.contains("src/main.rs"));
        assert!(display.contains("unexpected token"));
        assert!(display.contains("parse failed"));
    }

    #[test]
    fn unsupported_language_includes_extension() {
        let error = IndexError::unsupported_language(PathBuf::from("script.py"));

        assert!(error.message.contains(".py") || error.message.contains("py"));
    }
}
