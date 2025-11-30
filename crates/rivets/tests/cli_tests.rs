//! Integration tests for the rivets CLI.

use std::process::Command;
use tempfile::TempDir;

mod common;
use common::run_rivets_in_dir;

#[test]
fn test_cli_help() {
    let output = Command::new("cargo")
        .args(["run", "--package", "rivets", "--", "--help"])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("rivets"));
    assert!(stdout.contains("Usage:"));
}

#[test]
fn test_cli_version() {
    let output = Command::new("cargo")
        .args(["run", "--package", "rivets", "--", "--version"])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("0.1.0"));
}

#[test]
fn test_cli_no_args() {
    let output = Command::new("cargo")
        .args(["run", "--package", "rivets", "--quiet"])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
}

#[test]
fn test_cli_init_command() {
    let temp_dir = TempDir::new().unwrap();

    let output = run_rivets_in_dir(temp_dir.path(), &["init"]);

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Initializing"));
}

#[test]
fn test_cli_list_command() {
    let output = Command::new("cargo")
        .args(["run", "--package", "rivets", "--", "list"])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Listing"));
}

#[test]
fn test_cli_create_command() {
    let output = Command::new("cargo")
        .args(["run", "--package", "rivets", "--", "create"])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Creating"));
}

// ============================================================================
// Help Text Tests
// ============================================================================

#[test]
fn test_cli_help_shows_all_commands() {
    let output = Command::new("cargo")
        .args(["run", "--package", "rivets", "--", "--help"])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Verify all main commands are listed
    assert!(stdout.contains("init"), "Help should show 'init' command");
    assert!(
        stdout.contains("create"),
        "Help should show 'create' command"
    );
    assert!(stdout.contains("list"), "Help should show 'list' command");
    assert!(stdout.contains("show"), "Help should show 'show' command");
    assert!(
        stdout.contains("update"),
        "Help should show 'update' command"
    );
    assert!(stdout.contains("close"), "Help should show 'close' command");
    assert!(
        stdout.contains("delete"),
        "Help should show 'delete' command"
    );
    assert!(stdout.contains("ready"), "Help should show 'ready' command");
    assert!(stdout.contains("dep"), "Help should show 'dep' command");
    assert!(
        stdout.contains("blocked"),
        "Help should show 'blocked' command"
    );
    assert!(stdout.contains("stats"), "Help should show 'stats' command");
}

#[test]
fn test_cli_create_help() {
    let output = Command::new("cargo")
        .args(["run", "--package", "rivets", "--", "create", "--help"])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Verify create command shows its options
    assert!(
        stdout.contains("--title"),
        "Create help should show --title"
    );
    assert!(
        stdout.contains("--priority"),
        "Create help should show --priority"
    );
    assert!(stdout.contains("--type"), "Create help should show --type");
    assert!(
        stdout.contains("--assignee"),
        "Create help should show --assignee"
    );
}

#[test]
fn test_cli_list_help() {
    let output = Command::new("cargo")
        .args(["run", "--package", "rivets", "--", "list", "--help"])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Verify list command shows its options
    assert!(
        stdout.contains("--status"),
        "List help should show --status"
    );
    assert!(
        stdout.contains("--priority"),
        "List help should show --priority"
    );
    assert!(stdout.contains("--limit"), "List help should show --limit");
    assert!(stdout.contains("--sort"), "List help should show --sort");
}

// ============================================================================
// Argument Validation Tests
// ============================================================================

#[test]
fn test_cli_create_with_title() {
    let output = Command::new("cargo")
        .args([
            "run",
            "--package",
            "rivets",
            "--",
            "create",
            "--title",
            "Test Issue",
        ])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Test Issue"));
}

#[test]
fn test_cli_create_with_priority() {
    let output = Command::new("cargo")
        .args([
            "run",
            "--package",
            "rivets",
            "--",
            "create",
            "--title",
            "Bug fix",
            "--priority",
            "1",
            "--type",
            "bug",
        ])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
}

#[test]
fn test_cli_create_invalid_priority() {
    let output = Command::new("cargo")
        .args([
            "run",
            "--package",
            "rivets",
            "--",
            "create",
            "--priority",
            "5",
        ])
        .output()
        .expect("Failed to execute command");

    // Should fail because priority > 4 is invalid
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("5") || stderr.contains("invalid") || stderr.contains("error"),
        "Should show error for invalid priority"
    );
}

#[test]
fn test_cli_show_valid_issue_id() {
    let output = Command::new("cargo")
        .args(["run", "--package", "rivets", "--", "show", "proj-abc"])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("proj-abc"));
}

#[test]
fn test_cli_show_invalid_issue_id() {
    let output = Command::new("cargo")
        .args(["run", "--package", "rivets", "--", "show", "invalid"])
        .output()
        .expect("Failed to execute command");

    // Should fail because "invalid" doesn't have prefix-suffix format
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Invalid") || stderr.contains("format"),
        "Should show error for invalid issue ID format"
    );
}

#[test]
fn test_cli_update_with_status() {
    let output = Command::new("cargo")
        .args([
            "run",
            "--package",
            "rivets",
            "--",
            "update",
            "proj-abc",
            "--status",
            "in_progress",
        ])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("proj-abc"));
}

#[test]
fn test_cli_list_with_filters() {
    let output = Command::new("cargo")
        .args([
            "run",
            "--package",
            "rivets",
            "--",
            "list",
            "--status",
            "open",
            "--priority",
            "1",
            "--limit",
            "10",
        ])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
}

#[test]
fn test_cli_ready_with_sort() {
    let output = Command::new("cargo")
        .args([
            "run",
            "--package",
            "rivets",
            "--",
            "ready",
            "--sort",
            "priority",
            "--limit",
            "5",
        ])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
}

#[test]
fn test_cli_close_with_reason() {
    let output = Command::new("cargo")
        .args([
            "run",
            "--package",
            "rivets",
            "--",
            "close",
            "proj-abc",
            "--reason",
            "Fixed in PR #42",
        ])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Fixed in PR #42"));
}

#[test]
fn test_cli_delete_with_force() {
    let output = Command::new("cargo")
        .args([
            "run",
            "--package",
            "rivets",
            "--",
            "delete",
            "proj-abc",
            "--force",
        ])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("forced"));
}

#[test]
fn test_cli_init_with_prefix() {
    let temp_dir = TempDir::new().unwrap();

    let output = run_rivets_in_dir(temp_dir.path(), &["init", "--prefix", "myproj"]);

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("myproj"));
}

#[test]
fn test_cli_init_invalid_prefix() {
    let temp_dir = TempDir::new().unwrap();

    let output = run_rivets_in_dir(temp_dir.path(), &["init", "--prefix", "a"]);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("at least 2") || stderr.contains("error"),
        "Should show error for prefix too short"
    );
}

// ============================================================================
// Global Flag Tests
// ============================================================================

#[test]
fn test_cli_global_json_flag() {
    let output = Command::new("cargo")
        .args(["run", "--package", "rivets", "--", "--json", "list"])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
}

// ============================================================================
// Dependency Command Tests
// ============================================================================

#[test]
fn test_cli_dep_add() {
    let output = Command::new("cargo")
        .args([
            "run",
            "--package",
            "rivets",
            "--",
            "dep",
            "add",
            "proj-abc",
            "proj-xyz",
            "-t",
            "blocks",
        ])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("proj-abc"));
    assert!(stdout.contains("proj-xyz"));
}

#[test]
fn test_cli_dep_remove() {
    let output = Command::new("cargo")
        .args([
            "run",
            "--package",
            "rivets",
            "--",
            "dep",
            "remove",
            "proj-abc",
            "proj-xyz",
        ])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
}

#[test]
fn test_cli_dep_list() {
    let output = Command::new("cargo")
        .args([
            "run",
            "--package",
            "rivets",
            "--",
            "dep",
            "list",
            "proj-abc",
            "--reverse",
        ])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("dependents"));
}

// ============================================================================
// Status Alias Tests
// ============================================================================

#[test]
fn test_cli_status_in_progress_underscore() {
    let output = Command::new("cargo")
        .args([
            "run",
            "--package",
            "rivets",
            "--",
            "list",
            "--status",
            "in_progress",
        ])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
}

#[test]
fn test_cli_status_in_progress_hyphen() {
    let output = Command::new("cargo")
        .args([
            "run",
            "--package",
            "rivets",
            "--",
            "list",
            "--status",
            "in-progress",
        ])
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());
}
