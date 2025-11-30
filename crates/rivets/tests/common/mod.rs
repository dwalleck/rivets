//! Common test utilities shared across integration tests.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Get the workspace root directory
pub fn workspace_root() -> PathBuf {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    // Go up from crates/rivets to workspace root
    manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

/// Helper that builds the binary once and runs it directly
pub fn get_rivets_binary() -> PathBuf {
    let workspace = workspace_root();

    // Build the binary first (this should be quick if already built)
    let status = Command::new("cargo")
        .args(["build", "--package", "rivets", "--quiet"])
        .current_dir(&workspace)
        .status()
        .expect("Failed to build rivets");

    assert!(status.success(), "Failed to build rivets binary");

    workspace.join("target/debug/rivets")
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
