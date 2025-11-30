//! Integration tests for the rivets CLI.
//!
//! These tests verify the end-to-end behavior of all CLI commands.

use rstest::{fixture, rstest};
use std::process::Command;
use tempfile::TempDir;

mod common;
use common::{create_issue, run_rivets_in_dir};

// ============================================================================
// Test Fixtures
// ============================================================================

/// Provides a fresh temporary directory for each test
#[fixture]
fn temp_dir() -> TempDir {
    TempDir::new().expect("Failed to create temp directory")
}

/// Provides a temporary directory with an initialized rivets repository
#[fixture]
fn initialized_dir() -> TempDir {
    let temp = TempDir::new().expect("Failed to create temp directory");
    let output = run_rivets_in_dir(temp.path(), &["init", "--prefix", "test", "--quiet"]);
    assert!(
        output.status.success(),
        "Failed to initialize rivets: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    temp
}

// ============================================================================
// Help and Version Tests
// ============================================================================

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
// Init Command Tests
// ============================================================================

#[rstest]
fn test_cli_init_command(temp_dir: TempDir) {
    let output = run_rivets_in_dir(temp_dir.path(), &["init"]);

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Initializing"));
}

#[rstest]
fn test_cli_init_with_prefix(temp_dir: TempDir) {
    let output = run_rivets_in_dir(temp_dir.path(), &["init", "--prefix", "myproj"]);

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("myproj"));
}

#[rstest]
fn test_cli_init_invalid_prefix(temp_dir: TempDir) {
    let output = run_rivets_in_dir(temp_dir.path(), &["init", "--prefix", "a"]);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("at least 2") || stderr.contains("error"),
        "Should show error for prefix too short"
    );
}

// ============================================================================
// Create Command Tests
// ============================================================================

#[rstest]
fn test_cli_create_with_title(initialized_dir: TempDir) {
    let output = run_rivets_in_dir(initialized_dir.path(), &["create", "--title", "Test Issue"]);

    assert!(
        output.status.success(),
        "Create failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Created issue:"));
}

#[rstest]
fn test_cli_create_with_full_options(initialized_dir: TempDir) {
    let output = run_rivets_in_dir(
        initialized_dir.path(),
        &[
            "create",
            "--title",
            "Bug fix",
            "--priority",
            "1",
            "--type",
            "bug",
            "--assignee",
            "alice",
            "--labels",
            "urgent,backend",
        ],
    );

    assert!(
        output.status.success(),
        "Create failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Created issue:"));
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

    // Should fail because priority > 4 is invalid (at argument parsing level)
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("5") || stderr.contains("invalid") || stderr.contains("error"),
        "Should show error for invalid priority"
    );
}

#[test]
fn test_cli_show_invalid_issue_id_format() {
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

// ============================================================================
// List Command Tests
// ============================================================================

#[rstest]
fn test_cli_list_empty_repository(initialized_dir: TempDir) {
    let output = run_rivets_in_dir(initialized_dir.path(), &["list"]);

    assert!(
        output.status.success(),
        "List failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No issues found"));
}

#[rstest]
fn test_cli_list_with_issues(initialized_dir: TempDir) {
    // Create some issues first
    run_rivets_in_dir(
        initialized_dir.path(),
        &["create", "--title", "First issue", "--priority", "1"],
    );
    run_rivets_in_dir(
        initialized_dir.path(),
        &["create", "--title", "Second issue", "--priority", "2"],
    );

    let output = run_rivets_in_dir(initialized_dir.path(), &["list"]);

    assert!(
        output.status.success(),
        "List failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("2 issue(s)"));
    assert!(stdout.contains("First issue"));
    assert!(stdout.contains("Second issue"));
}

#[rstest]
fn test_cli_list_with_filters(initialized_dir: TempDir) {
    // Create issues with different priorities
    run_rivets_in_dir(
        initialized_dir.path(),
        &["create", "--title", "High priority", "--priority", "0"],
    );
    run_rivets_in_dir(
        initialized_dir.path(),
        &["create", "--title", "Low priority", "--priority", "3"],
    );

    let output = run_rivets_in_dir(initialized_dir.path(), &["list", "--priority", "0"]);

    assert!(
        output.status.success(),
        "List with filter failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("High priority"));
    assert!(!stdout.contains("Low priority"));
}

#[rstest]
#[case::open("open")]
#[case::in_progress("in_progress")]
#[case::in_progress_alias("in-progress")]
#[case::blocked("blocked")]
#[case::closed("closed")]
fn test_cli_list_status_filter_parsing(initialized_dir: TempDir, #[case] status: &str) {
    // Verify all status filter values are accepted by the CLI parser
    let output = run_rivets_in_dir(initialized_dir.path(), &["list", "--status", status]);
    assert!(
        output.status.success(),
        "Status filter '{}' should be valid. Stderr: {}",
        status,
        String::from_utf8_lossy(&output.stderr)
    );
}

#[rstest]
fn test_cli_list_status_filters_match_issues(initialized_dir: TempDir) {
    // Create issues with different statuses
    let open_id = create_issue(initialized_dir.path(), "Open issue", &[]);
    let in_progress_id = create_issue(initialized_dir.path(), "In progress issue", &[]);

    // Update one to in_progress
    run_rivets_in_dir(
        initialized_dir.path(),
        &["update", &in_progress_id, "--status", "in_progress"],
    );

    // List open - should only show open issue
    let output = run_rivets_in_dir(initialized_dir.path(), &["list", "--status", "open"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Open issue"));
    assert!(!stdout.contains("In progress issue"));

    // List in_progress - should only show in_progress issue
    let output = run_rivets_in_dir(initialized_dir.path(), &["list", "--status", "in_progress"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains(&open_id));
    assert!(stdout.contains("In progress issue"));
}

#[rstest]
#[case::bug("bug")]
#[case::feature("feature")]
#[case::task("task")]
#[case::epic("epic")]
#[case::chore("chore")]
fn test_cli_create_issue_types(initialized_dir: TempDir, #[case] issue_type: &str) {
    let output = run_rivets_in_dir(
        initialized_dir.path(),
        &["create", "--title", "Type test", "--type", issue_type],
    );
    assert!(
        output.status.success(),
        "Issue type '{}' should be valid. Stderr: {}",
        issue_type,
        String::from_utf8_lossy(&output.stderr)
    );
}

#[rstest]
#[case::priority_0(0)]
#[case::priority_1(1)]
#[case::priority_2(2)]
#[case::priority_3(3)]
#[case::priority_4(4)]
fn test_cli_create_valid_priorities(initialized_dir: TempDir, #[case] priority: u8) {
    let output = run_rivets_in_dir(
        initialized_dir.path(),
        &[
            "create",
            "--title",
            "Priority test",
            "--priority",
            &priority.to_string(),
        ],
    );
    assert!(
        output.status.success(),
        "Priority {} should be valid. Stderr: {}",
        priority,
        String::from_utf8_lossy(&output.stderr)
    );
}

// ============================================================================
// Show Command Tests
// ============================================================================

#[rstest]
fn test_cli_show_existing_issue(initialized_dir: TempDir) {
    let issue_id = create_issue(
        initialized_dir.path(),
        "Test show",
        &["--description", "Details here"],
    );

    let output = run_rivets_in_dir(initialized_dir.path(), &["show", &issue_id]);

    assert!(
        output.status.success(),
        "Show failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Test show"));
    assert!(stdout.contains("Details here"));
}

#[rstest]
fn test_cli_show_nonexistent_issue(initialized_dir: TempDir) {
    let output = run_rivets_in_dir(initialized_dir.path(), &["show", "test-notfound"]);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.to_lowercase().contains("not found"));
}

// ============================================================================
// Update Command Tests
// ============================================================================

#[rstest]
fn test_cli_update_issue(initialized_dir: TempDir) {
    let issue_id = create_issue(initialized_dir.path(), "Original title", &[]);

    let output = run_rivets_in_dir(
        initialized_dir.path(),
        &[
            "update",
            &issue_id,
            "--title",
            "Updated title",
            "--status",
            "in_progress",
        ],
    );

    assert!(
        output.status.success(),
        "Update failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Updated issue:"));

    // Verify the update
    let show_output = run_rivets_in_dir(initialized_dir.path(), &["show", &issue_id]);
    let show_stdout = String::from_utf8_lossy(&show_output.stdout);
    assert!(show_stdout.contains("Updated title"));
    assert!(show_stdout.contains("in_progress"));
}

// ============================================================================
// Close Command Tests
// ============================================================================

#[rstest]
fn test_cli_close_issue(initialized_dir: TempDir) {
    let issue_id = create_issue(initialized_dir.path(), "To be closed", &[]);

    let output = run_rivets_in_dir(
        initialized_dir.path(),
        &["close", &issue_id, "--reason", "Fixed in PR #42"],
    );

    assert!(
        output.status.success(),
        "Close failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Closed issue:"));
    assert!(stdout.contains("Fixed in PR #42"));
}

// ============================================================================
// Delete Command Tests
// ============================================================================

#[rstest]
fn test_cli_delete_with_force(initialized_dir: TempDir) {
    let issue_id = create_issue(initialized_dir.path(), "To be deleted", &[]);

    let output = run_rivets_in_dir(initialized_dir.path(), &["delete", &issue_id, "--force"]);

    assert!(
        output.status.success(),
        "Delete failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Deleted issue:"));

    // Verify it's gone
    let show_output = run_rivets_in_dir(initialized_dir.path(), &["show", &issue_id]);
    assert!(!show_output.status.success());
}

// ============================================================================
// Ready Command Tests
// ============================================================================

#[rstest]
fn test_cli_ready_empty(initialized_dir: TempDir) {
    let output = run_rivets_in_dir(initialized_dir.path(), &["ready"]);

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No ready issues found"));
}

#[rstest]
fn test_cli_ready_with_issues(initialized_dir: TempDir) {
    // Create some issues
    run_rivets_in_dir(
        initialized_dir.path(),
        &["create", "--title", "Ready issue 1", "--priority", "1"],
    );
    run_rivets_in_dir(
        initialized_dir.path(),
        &["create", "--title", "Ready issue 2", "--priority", "2"],
    );

    let output = run_rivets_in_dir(
        initialized_dir.path(),
        &["ready", "--sort", "priority", "--limit", "10"],
    );

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Ready to work"));
    assert!(stdout.contains("Ready issue 1"));
    assert!(stdout.contains("Ready issue 2"));
}

// ============================================================================
// Dependency Command Tests
// ============================================================================

#[rstest]
fn test_cli_dep_add_and_list(initialized_dir: TempDir) {
    let id1 = create_issue(initialized_dir.path(), "Dependent issue", &[]);
    let id2 = create_issue(initialized_dir.path(), "Blocking issue", &[]);

    // Add dependency: id1 depends on (is blocked by) id2
    let output = run_rivets_in_dir(
        initialized_dir.path(),
        &["dep", "add", &id1, &id2, "-t", "blocks"],
    );

    assert!(
        output.status.success(),
        "Dep add failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Added dependency"));

    // List dependencies
    let list_output = run_rivets_in_dir(initialized_dir.path(), &["dep", "list", &id1]);
    assert!(list_output.status.success());
    let list_stdout = String::from_utf8_lossy(&list_output.stdout);
    assert!(list_stdout.contains(&id2));
}

#[rstest]
fn test_cli_dep_remove(initialized_dir: TempDir) {
    let id1 = create_issue(initialized_dir.path(), "Issue 1", &[]);
    let id2 = create_issue(initialized_dir.path(), "Issue 2", &[]);

    // Add and then remove dependency
    run_rivets_in_dir(
        initialized_dir.path(),
        &["dep", "add", &id1, &id2, "-t", "blocks"],
    );

    let output = run_rivets_in_dir(initialized_dir.path(), &["dep", "remove", &id1, &id2]);

    assert!(
        output.status.success(),
        "Dep remove failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Removed dependency"));
}

// ============================================================================
// Blocked Command Tests
// ============================================================================

#[rstest]
fn test_cli_blocked_empty(initialized_dir: TempDir) {
    let output = run_rivets_in_dir(initialized_dir.path(), &["blocked"]);

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No blocked issues found"));
}

#[rstest]
fn test_cli_blocked_with_dependencies(initialized_dir: TempDir) {
    let id1 = create_issue(initialized_dir.path(), "Blocked issue", &[]);
    let id2 = create_issue(initialized_dir.path(), "Blocker", &[]);

    // Add blocking dependency
    run_rivets_in_dir(
        initialized_dir.path(),
        &["dep", "add", &id1, &id2, "-t", "blocks"],
    );

    let output = run_rivets_in_dir(initialized_dir.path(), &["blocked"]);

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Blocked issue"));
    assert!(stdout.contains("Blocked by:"));
}

// ============================================================================
// Stats Command Tests
// ============================================================================

#[rstest]
fn test_cli_stats_empty(initialized_dir: TempDir) {
    let output = run_rivets_in_dir(initialized_dir.path(), &["stats"]);

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Project Statistics"));
    assert!(stdout.contains("Total Issues:"));
}

#[rstest]
fn test_cli_stats_with_issues(initialized_dir: TempDir) {
    // Create some issues with different statuses
    run_rivets_in_dir(
        initialized_dir.path(),
        &["create", "--title", "Open issue 1"],
    );
    run_rivets_in_dir(
        initialized_dir.path(),
        &["create", "--title", "Open issue 2"],
    );

    let output = run_rivets_in_dir(initialized_dir.path(), &["stats", "--detailed"]);

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Total Issues:"));
    assert!(stdout.contains("By Priority:"));
}

// ============================================================================
// JSON Output Tests
// ============================================================================

#[rstest]
fn test_cli_json_output_list(initialized_dir: TempDir) {
    // Create an issue
    run_rivets_in_dir(
        initialized_dir.path(),
        &["create", "--title", "JSON test issue"],
    );

    let output = run_rivets_in_dir(initialized_dir.path(), &["--json", "list"]);

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should be valid JSON
    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("Output should be valid JSON");
    assert!(json.is_array());
}

#[rstest]
fn test_cli_json_output_stats(initialized_dir: TempDir) {
    let output = run_rivets_in_dir(initialized_dir.path(), &["--json", "stats"]);

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should be valid JSON
    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("Output should be valid JSON");
    assert!(json["total"].is_number());
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[rstest]
fn test_cli_requires_initialized_repository(temp_dir: TempDir) {
    // Try to run a command that requires storage without initializing
    let output = run_rivets_in_dir(temp_dir.path(), &["list"]);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Not a rivets repository") || stderr.contains("rivets init"),
        "Should show error about uninitialized repository. Got: {}",
        stderr
    );
}
