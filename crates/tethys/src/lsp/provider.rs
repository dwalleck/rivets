//! LSP provider trait and implementations for different language servers.

use serde_json::Value;

/// Trait for configuring LSP server providers.
///
/// Implementations define how to spawn and configure a specific LSP server.
///
/// # Example
///
/// ```rust
/// use tethys::lsp::LspProvider;
///
/// struct MyCustomLsp;
///
/// impl LspProvider for MyCustomLsp {
///     fn command(&self) -> &'static str { "my-lsp" }
///     fn args(&self) -> Vec<&str> { vec!["--stdio"] }
/// }
/// ```
pub trait LspProvider: Send + Sync {
    /// The command to spawn (e.g., "rust-analyzer", "csharp-ls").
    fn command(&self) -> &'static str;

    /// Additional command-line arguments for the LSP server.
    fn args(&self) -> Vec<&str> {
        vec![]
    }

    /// Language-specific initialization options for the LSP server.
    ///
    /// These are passed in the `initializationOptions` field of `InitializeParams`.
    fn initialize_options(&self) -> Option<Value> {
        None
    }

    /// Installation hint shown when the LSP server is not found.
    fn install_hint(&self) -> &'static str {
        "Please install the language server and ensure it's in your PATH."
    }
}

/// LSP provider for rust-analyzer.
///
/// rust-analyzer is the official LSP implementation for Rust.
/// Install via: `rustup component add rust-analyzer`
///
/// # Example
///
/// ```rust
/// use tethys::lsp::{LspClient, RustAnalyzerProvider};
/// use std::path::Path;
///
/// let provider = RustAnalyzerProvider;
/// // let client = LspClient::start(&provider, Path::new("."))?;
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct RustAnalyzerProvider;

impl LspProvider for RustAnalyzerProvider {
    fn command(&self) -> &'static str {
        "rust-analyzer"
    }

    fn install_hint(&self) -> &'static str {
        "Install rust-analyzer: https://rust-analyzer.github.io/manual.html#installation"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_analyzer_provider_has_correct_command() {
        let provider = RustAnalyzerProvider;
        assert_eq!(provider.command(), "rust-analyzer");
    }

    #[test]
    fn rust_analyzer_provider_has_no_args_by_default() {
        let provider = RustAnalyzerProvider;
        assert!(provider.args().is_empty());
    }

    #[test]
    fn rust_analyzer_provider_has_no_init_options_by_default() {
        let provider = RustAnalyzerProvider;
        assert!(provider.initialize_options().is_none());
    }

    #[test]
    fn rust_analyzer_provider_has_install_hint() {
        let provider = RustAnalyzerProvider;
        assert!(provider.install_hint().contains("rust-analyzer"));
    }
}
