//! Common test utilities shared across integration tests.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Get the path to the rivets binary built by `cargo test`.
///
/// Uses `CARGO_BIN_EXE_rivets` which cargo sets at compile time for
/// integration tests in packages with a `[[bin]]` target. This avoids
/// running `cargo build` inside the test, which caused TOCTOU races
/// on macOS when multiple test threads built concurrently.
pub fn get_rivets_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_rivets"))
}

/// Run the rivets binary directly in the specified directory
pub fn run_rivets_in_dir(dir: &Path, args: &[&str]) -> Output {
    let binary = get_rivets_binary();

    Command::new(&binary)
        .args(args)
        .current_dir(dir)
        .output()
        .expect("Failed to execute rivets binary")
}

#[allow(dead_code)] // Used by cli_tests but not init_integration
/// Create an issue and return its ID.
///
/// This helper creates an issue using JSON output mode and parses
/// the resulting ID for use in subsequent commands.
///
/// # Arguments
/// * `dir` - The directory containing the initialized rivets repository
/// * `title` - The issue title
/// * `extra_args` - Optional additional arguments (e.g., ["--priority", "1"])
///
/// # Returns
/// The issue ID as a String
///
/// # Panics
/// Panics if the issue creation fails or if the output cannot be parsed
pub fn create_issue(dir: &Path, title: &str, extra_args: &[&str]) -> String {
    let mut args = vec!["--json", "create", "--title", title];
    args.extend(extra_args);

    let output = run_rivets_in_dir(dir, &args);
    assert!(
        output.status.success(),
        "Failed to create issue: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(&stdout).expect("Failed to parse JSON output");
    json["id"]
        .as_str()
        .expect("Issue ID not found in output")
        .to_string()
}
