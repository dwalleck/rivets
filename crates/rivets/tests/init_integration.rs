//! Integration tests for the `init` command.
//!
//! These tests verify the end-to-end behavior of the init command,
//! including the CLI interface and file system operations.

use tempfile::TempDir;

mod common;
use common::run_rivets_in_dir;

// ============================================================================
// Init Command Integration Tests
// ============================================================================

#[test]
fn test_init_creates_rivets_directory() {
    let temp_dir = TempDir::new().unwrap();

    let output = run_rivets_in_dir(temp_dir.path(), &["init", "--quiet"]);

    assert!(output.status.success(), "Init command should succeed");

    // Verify .rivets directory was created
    let rivets_dir = temp_dir.path().join(".rivets");
    assert!(rivets_dir.exists(), ".rivets directory should exist");
    assert!(rivets_dir.is_dir(), ".rivets should be a directory");
}

#[test]
fn test_init_creates_config_file() {
    let temp_dir = TempDir::new().unwrap();

    let output = run_rivets_in_dir(temp_dir.path(), &["init", "--quiet"]);
    assert!(output.status.success());

    // Verify config.yaml exists and has expected content
    let config_path = temp_dir.path().join(".rivets/config.yaml");
    assert!(config_path.exists(), "config.yaml should exist");

    let content = std::fs::read_to_string(&config_path).unwrap();
    assert!(
        content.contains("issue-prefix:"),
        "Config should contain issue-prefix"
    );
    assert!(
        content.contains("backend: memory"),
        "Config should specify memory backend"
    );
    assert!(
        content.contains("data_file:"),
        "Config should specify data_file"
    );
}

#[test]
fn test_init_creates_issues_file() {
    let temp_dir = TempDir::new().unwrap();

    let output = run_rivets_in_dir(temp_dir.path(), &["init", "--quiet"]);
    assert!(output.status.success());

    // Verify issues.jsonl exists and is empty
    let issues_path = temp_dir.path().join(".rivets/issues.jsonl");
    assert!(issues_path.exists(), "issues.jsonl should exist");

    let content = std::fs::read_to_string(&issues_path).unwrap();
    assert!(content.is_empty(), "issues.jsonl should be empty initially");
}

#[test]
fn test_init_creates_gitignore() {
    let temp_dir = TempDir::new().unwrap();

    let output = run_rivets_in_dir(temp_dir.path(), &["init", "--quiet"]);
    assert!(output.status.success());

    // Verify .gitignore exists
    let gitignore_path = temp_dir.path().join(".rivets/.gitignore");
    assert!(gitignore_path.exists(), ".gitignore should exist");
}

#[test]
fn test_init_with_custom_prefix() {
    let temp_dir = TempDir::new().unwrap();

    let output = run_rivets_in_dir(temp_dir.path(), &["init", "--prefix", "myproj", "--quiet"]);
    assert!(output.status.success());

    // Verify config has the custom prefix
    let config_path = temp_dir.path().join(".rivets/config.yaml");
    let content = std::fs::read_to_string(&config_path).unwrap();
    assert!(
        content.contains("issue-prefix: myproj"),
        "Config should contain custom prefix 'myproj'"
    );
}

#[test]
fn test_init_with_default_prefix() {
    let temp_dir = TempDir::new().unwrap();

    let output = run_rivets_in_dir(temp_dir.path(), &["init", "--quiet"]);
    assert!(output.status.success());

    // Verify config has the default prefix
    let config_path = temp_dir.path().join(".rivets/config.yaml");
    let content = std::fs::read_to_string(&config_path).unwrap();
    assert!(
        content.contains("issue-prefix: proj"),
        "Config should contain default prefix 'proj'"
    );
}

#[test]
fn test_init_fails_if_already_initialized() {
    let temp_dir = TempDir::new().unwrap();

    // First init should succeed
    let output1 = run_rivets_in_dir(temp_dir.path(), &["init", "--quiet"]);
    assert!(output1.status.success(), "First init should succeed");

    // Second init should fail
    let output2 = run_rivets_in_dir(temp_dir.path(), &["init", "--quiet"]);
    assert!(
        !output2.status.success(),
        "Second init should fail because already initialized"
    );

    let stderr = String::from_utf8_lossy(&output2.stderr);
    assert!(
        stderr.to_lowercase().contains("already initialized")
            || stderr.to_lowercase().contains("already")
            || stderr.to_lowercase().contains("exists"),
        "Error message should indicate already initialized. Got: {}",
        stderr
    );
}

#[test]
fn test_init_fails_with_invalid_prefix_too_short() {
    let temp_dir = TempDir::new().unwrap();

    let output = run_rivets_in_dir(temp_dir.path(), &["init", "--prefix", "a"]);

    assert!(
        !output.status.success(),
        "Init should fail with prefix too short"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.to_lowercase().contains("at least 2")
            || stderr.to_lowercase().contains("characters"),
        "Error should mention minimum characters. Got: {}",
        stderr
    );
}

#[test]
fn test_init_fails_with_invalid_prefix_special_chars() {
    let temp_dir = TempDir::new().unwrap();

    let output = run_rivets_in_dir(temp_dir.path(), &["init", "--prefix", "proj-test"]);

    assert!(
        !output.status.success(),
        "Init should fail with hyphen in prefix"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.to_lowercase().contains("alphanumeric") || stderr.to_lowercase().contains("invalid"),
        "Error should mention alphanumeric requirement. Got: {}",
        stderr
    );
}

#[test]
fn test_init_output_without_quiet_flag() {
    let temp_dir = TempDir::new().unwrap();

    let output = run_rivets_in_dir(temp_dir.path(), &["init"]);
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should show initialization message
    assert!(
        stdout.contains("Initializing") || stdout.contains("rivets"),
        "Should show initialization message. Got: {}",
        stdout
    );

    // Should show the created directory
    assert!(
        stdout.contains(".rivets") || stdout.contains("Initialized"),
        "Should mention .rivets directory. Got: {}",
        stdout
    );
}

#[test]
fn test_init_quiet_flag_suppresses_output() {
    let temp_dir = TempDir::new().unwrap();

    let output = run_rivets_in_dir(temp_dir.path(), &["init", "-q"]);
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);

    // With quiet flag, stdout should be empty
    assert!(
        stdout.is_empty(),
        "Quiet mode should suppress output. Got: {}",
        stdout
    );

    // But the directory should still be created
    assert!(temp_dir.path().join(".rivets").exists());
}

#[test]
fn test_init_with_long_quiet_flag() {
    let temp_dir = TempDir::new().unwrap();

    let output = run_rivets_in_dir(temp_dir.path(), &["init", "--quiet"]);
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.is_empty(), "Long quiet flag should also work");
}

#[test]
fn test_init_complete_directory_structure() {
    let temp_dir = TempDir::new().unwrap();

    let output = run_rivets_in_dir(temp_dir.path(), &["init", "--prefix", "test", "--quiet"]);
    assert!(output.status.success());

    let rivets_dir = temp_dir.path().join(".rivets");

    // Verify complete structure
    assert!(rivets_dir.exists(), ".rivets/ should exist");
    assert!(
        rivets_dir.join("config.yaml").exists(),
        "config.yaml should exist"
    );
    assert!(
        rivets_dir.join("issues.jsonl").exists(),
        "issues.jsonl should exist"
    );
    assert!(
        rivets_dir.join(".gitignore").exists(),
        ".gitignore should exist"
    );

    // Verify no extra files were created (no database files)
    let entries: Vec<_> = std::fs::read_dir(&rivets_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();

    assert_eq!(
        entries.len(),
        3,
        "Should have exactly 3 files: config.yaml, issues.jsonl, .gitignore. Found: {:?}",
        entries.iter().map(|e| e.file_name()).collect::<Vec<_>>()
    );
}

#[test]
fn test_init_prefix_validation_boundary_2_chars() {
    let temp_dir = TempDir::new().unwrap();

    // Exactly 2 characters should work
    let output = run_rivets_in_dir(temp_dir.path(), &["init", "--prefix", "ab", "--quiet"]);
    assert!(output.status.success(), "2-char prefix should be valid");

    let config_path = temp_dir.path().join(".rivets/config.yaml");
    let content = std::fs::read_to_string(&config_path).unwrap();
    assert!(content.contains("issue-prefix: ab"));
}

#[test]
fn test_init_prefix_validation_boundary_20_chars() {
    let temp_dir = TempDir::new().unwrap();

    // Exactly 20 characters should work
    let prefix = "a1b2c3d4e5f6g7h8i9j0"; // 20 chars
    let output = run_rivets_in_dir(temp_dir.path(), &["init", "--prefix", prefix, "--quiet"]);
    assert!(output.status.success(), "20-char prefix should be valid");

    let config_path = temp_dir.path().join(".rivets/config.yaml");
    let content = std::fs::read_to_string(&config_path).unwrap();
    assert!(content.contains(&format!("issue-prefix: {}", prefix)));
}
