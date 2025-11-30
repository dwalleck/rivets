//! Common test utilities shared across integration tests.

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
