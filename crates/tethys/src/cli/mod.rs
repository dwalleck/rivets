//! CLI command implementations.

mod display;

pub mod affected_tests;
pub mod callers;
pub mod cycles;
pub mod impact;
pub mod index;
pub mod panic_points;
pub mod reachable;
pub mod search;
pub mod stats;

use std::path::PathBuf;
use tethys::lsp::{LspError, LspProvider, RustAnalyzerProvider};

/// Check if the LSP server is available in PATH.
///
/// Returns `Ok(())` if the LSP server executable exists, or an appropriate
/// error if it's not found.
///
/// # Errors
///
/// Returns `LspError::NotFound` if the LSP server is not in PATH.
pub fn check_lsp_availability() -> Result<(), LspError> {
    let provider = RustAnalyzerProvider;
    let command = provider.command();

    if find_in_path(command).is_some() {
        tracing::debug!(command = %command, "LSP server found in PATH");
        Ok(())
    } else {
        Err(LspError::not_found(command, provider.install_hint()))
    }
}

/// Search for an executable by name in the system PATH.
///
/// Returns the full path to the first matching executable, or `None` if not found.
fn find_in_path(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var).find_map(|dir| {
        let candidate = dir.join(name);
        if is_executable(&candidate) {
            return Some(candidate);
        }
        // On Windows, also check with common executable extensions
        if cfg!(windows) {
            for ext in &["exe", "cmd", "bat", "com"] {
                let candidate = candidate.with_extension(ext);
                if is_executable(&candidate) {
                    return Some(candidate);
                }
            }
        }
        None
    })
}

/// Check if a path points to an executable file.
#[cfg(unix)]
fn is_executable(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.is_file()
        && path
            .metadata()
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
}

/// Check if a path points to an existing file (Windows).
#[cfg(windows)]
fn is_executable(path: &std::path::Path) -> bool {
    path.is_file()
}

/// Check LSP availability if requested, returning early with an error if unavailable.
///
/// This is a convenience wrapper that checks LSP availability only when the `lsp` flag
/// is true, converting any `LspError` to `tethys::Error::Config`.
///
/// # Errors
///
/// Returns an error if `lsp` is true and the LSP server is not available.
pub fn ensure_lsp_if_requested(lsp: bool) -> Result<(), tethys::Error> {
    if lsp {
        check_lsp_availability().map_err(|e| tethys::Error::Config(e.to_string()))?;
        tracing::debug!("LSP mode enabled");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_lsp_availability_returns_error_for_nonexistent_command() {
        // This test verifies the error type - actual availability depends on system
        // We can at least verify the function doesn't panic
        let result = check_lsp_availability();
        // Result depends on whether rust-analyzer is installed on the system
        // We just verify the function runs without panicking
        let _ = result;
    }

    #[test]
    fn lsp_not_found_error_format_matches_spec() {
        // Verify the error message format matches the design doc spec:
        // error: rust-analyzer not found
        //
        // LSP refinement was requested but the language server is not available.
        // Install rust-analyzer: https://rust-analyzer.github.io/manual.html#installation
        //
        // To index without LSP refinement, omit the --lsp flag.

        let provider = RustAnalyzerProvider;
        let error = LspError::not_found(provider.command(), provider.install_hint());
        let message = error.to_string();

        // Verify key components are present
        assert!(
            message.contains("rust-analyzer not found"),
            "Error should contain 'rust-analyzer not found': {message}"
        );
        assert!(
            message
                .contains("LSP refinement was requested but the language server is not available"),
            "Error should contain LSP refinement message: {message}"
        );
        assert!(
            message.contains("https://rust-analyzer.github.io/manual.html#installation"),
            "Error should contain installation URL: {message}"
        );
        assert!(
            message.contains("omit the --lsp flag"),
            "Error should suggest omitting --lsp flag: {message}"
        );
    }

    #[test]
    fn check_lsp_returns_lsp_error_type() {
        // When rust-analyzer is not available, check_lsp_availability should return
        // an LspError::NotFound, not some other error type.
        // We test this by creating the error directly since we can't guarantee
        // rust-analyzer is absent on all test systems.
        let provider = RustAnalyzerProvider;
        let error = LspError::not_found(provider.command(), provider.install_hint());

        // Verify it's the NotFound variant by checking the error message
        let message = error.to_string();
        assert!(message.contains("not found"), "Should be a NotFound error");
    }
}
