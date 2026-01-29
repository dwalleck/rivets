//! LSP client infrastructure for cross-file reference resolution.
//!
//! This module provides a thin JSON-RPC transport layer for communicating with
//! Language Server Protocol (LSP) servers like rust-analyzer. The design is
//! extensible to support additional language servers (e.g., csharp-ls).
//!
//! ## Usage
//!
//! ```no_run
//! use tethys::lsp::{LspClient, RustAnalyzerProvider};
//! use std::path::Path;
//!
//! let provider = RustAnalyzerProvider;
//! let mut client = LspClient::start(&provider, Path::new("/path/to/workspace"))?;
//!
//! // Query goto definition
//! let location = client.goto_definition(
//!     Path::new("src/main.rs"),
//!     10,  // line (0-indexed)
//!     5,   // column (0-indexed)
//! )?;
//!
//! // Clean shutdown
//! client.shutdown()?;
//! # Ok::<(), tethys::lsp::LspError>(())
//! ```
//!
//! ## Design Notes
//!
//! - Uses `lsp-types` for all protocol types
//! - JSON-RPC format: `Content-Length: N\r\n\r\n{json}`
//! - Request IDs are incrementing integers
//! - Supports both successful responses and error responses

mod error;
mod provider;
mod transport;

pub use error::LspError;
pub use provider::{AnyProvider, CSharpLsProvider, LspProvider, RustAnalyzerProvider};
pub use transport::LspClient;

/// Result type for LSP operations.
pub type Result<T> = std::result::Result<T, LspError>;
