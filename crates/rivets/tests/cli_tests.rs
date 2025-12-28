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
    assert!(stdout.contains("Updated 1 issue(s):"));

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
    assert!(stdout.contains("Closed 1 issue(s):"));
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

// ============================================================================
// Reopen Command Tests
// ============================================================================

#[rstest]
fn test_cli_reopen_issue(initialized_dir: TempDir) {
    let issue_id = create_issue(initialized_dir.path(), "Issue to reopen", &[]);

    // Close the issue first
    run_rivets_in_dir(initialized_dir.path(), &["close", &issue_id]);

    // Reopen it
    let output = run_rivets_in_dir(
        initialized_dir.path(),
        &["reopen", &issue_id, "--reason", "Needs more work"],
    );

    assert!(
        output.status.success(),
        "Reopen failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Reopened 1 issue(s):"));

    // Verify status is now open
    let show_output = run_rivets_in_dir(initialized_dir.path(), &["show", &issue_id]);
    let show_stdout = String::from_utf8_lossy(&show_output.stdout);
    assert!(show_stdout.contains("open"));
}

#[rstest]
fn test_cli_reopen_multiple_issues(initialized_dir: TempDir) {
    let id1 = create_issue(initialized_dir.path(), "Issue 1", &[]);
    let id2 = create_issue(initialized_dir.path(), "Issue 2", &[]);

    // Close both issues
    run_rivets_in_dir(initialized_dir.path(), &["close", &id1, &id2]);

    // Reopen both at once
    let output = run_rivets_in_dir(initialized_dir.path(), &["reopen", &id1, &id2]);

    assert!(
        output.status.success(),
        "Reopen multiple failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(&id1));
    assert!(stdout.contains(&id2));
}

#[rstest]
fn test_cli_reopen_already_open_issue(initialized_dir: TempDir) {
    let issue_id = create_issue(initialized_dir.path(), "Open issue", &[]);

    // Try to reopen an already open issue
    let output = run_rivets_in_dir(initialized_dir.path(), &["reopen", &issue_id]);

    assert!(
        output.status.success(),
        "Reopen should succeed even for open issues: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// ============================================================================
// Info Command Tests
// ============================================================================

#[rstest]
fn test_cli_info_command(initialized_dir: TempDir) {
    let output = run_rivets_in_dir(initialized_dir.path(), &["info"]);

    assert!(
        output.status.success(),
        "Info failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Rivets Repository Information"));
    assert!(stdout.contains("Database:"));
    assert!(stdout.contains("Issue prefix:"));
    assert!(stdout.contains("Issues:"));
}

#[rstest]
fn test_cli_info_with_issues(initialized_dir: TempDir) {
    // Create some issues with different statuses
    create_issue(initialized_dir.path(), "Open issue", &[]);
    let id2 = create_issue(initialized_dir.path(), "In progress issue", &[]);
    let id3 = create_issue(initialized_dir.path(), "Closed issue", &[]);

    run_rivets_in_dir(
        initialized_dir.path(),
        &["update", &id2, "--status", "in_progress"],
    );
    run_rivets_in_dir(initialized_dir.path(), &["close", &id3]);

    let output = run_rivets_in_dir(initialized_dir.path(), &["info"]);

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("3 total"));
    assert!(stdout.contains("1 open"));
    assert!(stdout.contains("1 in progress"));
    assert!(stdout.contains("1 closed"));
}

#[rstest]
fn test_cli_info_json_output(initialized_dir: TempDir) {
    create_issue(initialized_dir.path(), "Test issue", &[]);

    let output = run_rivets_in_dir(initialized_dir.path(), &["--json", "info"]);

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("Output should be valid JSON");
    assert!(json["database_path"].is_string());
    assert!(json["issue_prefix"].is_string());
    assert!(json["issues"]["total"].is_number());
}

#[rstest]
fn test_cli_info_with_blocked_status(initialized_dir: TempDir) {
    // Create issues with all statuses including blocked
    create_issue(initialized_dir.path(), "Open issue", &[]);
    let id2 = create_issue(initialized_dir.path(), "In progress issue", &[]);
    let id3 = create_issue(initialized_dir.path(), "Blocked issue", &[]);
    let id4 = create_issue(initialized_dir.path(), "Closed issue", &[]);

    run_rivets_in_dir(
        initialized_dir.path(),
        &["update", &id2, "--status", "in_progress"],
    );
    run_rivets_in_dir(
        initialized_dir.path(),
        &["update", &id3, "--status", "blocked"],
    );
    run_rivets_in_dir(initialized_dir.path(), &["close", &id4]);

    let output = run_rivets_in_dir(initialized_dir.path(), &["info"]);

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("4 total"));
    assert!(stdout.contains("1 open"));
    assert!(stdout.contains("1 in progress"));
    assert!(stdout.contains("1 blocked"));
    assert!(stdout.contains("1 closed"));
}

#[rstest]
fn test_cli_info_json_includes_blocked_count(initialized_dir: TempDir) {
    // Create issues with all statuses
    create_issue(initialized_dir.path(), "Open issue", &[]);
    let id2 = create_issue(initialized_dir.path(), "Blocked issue", &[]);

    run_rivets_in_dir(
        initialized_dir.path(),
        &["update", &id2, "--status", "blocked"],
    );

    let output = run_rivets_in_dir(initialized_dir.path(), &["--json", "info"]);

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("Output should be valid JSON");
    assert_eq!(json["issues"]["total"], 2, "Should have 2 total issues");
    assert_eq!(json["issues"]["open"], 1, "Should have 1 open issue");
    assert_eq!(json["issues"]["blocked"], 1, "Should have 1 blocked issue");
    assert_eq!(json["issues"]["closed"], 0, "Should have 0 closed issues");
}

// ============================================================================
// Label Command Tests
// ============================================================================

#[rstest]
fn test_cli_label_add(initialized_dir: TempDir) {
    let issue_id = create_issue(initialized_dir.path(), "Issue for labeling", &[]);

    let output = run_rivets_in_dir(
        initialized_dir.path(),
        &["label", "add", "urgent", &issue_id],
    );

    assert!(
        output.status.success(),
        "Label add failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Added label"));

    // Verify the label was added
    let show_output = run_rivets_in_dir(initialized_dir.path(), &["show", &issue_id]);
    let show_stdout = String::from_utf8_lossy(&show_output.stdout);
    assert!(show_stdout.contains("urgent"));
}

#[rstest]
fn test_cli_label_add_multiple_issues(initialized_dir: TempDir) {
    let id1 = create_issue(initialized_dir.path(), "Issue 1", &[]);
    let id2 = create_issue(initialized_dir.path(), "Issue 2", &[]);

    let output = run_rivets_in_dir(
        initialized_dir.path(),
        &["label", "add", "backend", "--ids", &id1, &id2],
    );

    assert!(
        output.status.success(),
        "Label add multiple failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(&id1));
    assert!(stdout.contains(&id2));
}

#[rstest]
fn test_cli_label_remove(initialized_dir: TempDir) {
    let issue_id = create_issue(
        initialized_dir.path(),
        "Labeled issue",
        &["--labels", "bug"],
    );

    let output = run_rivets_in_dir(
        initialized_dir.path(),
        &["label", "remove", "bug", &issue_id],
    );

    assert!(
        output.status.success(),
        "Label remove failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Removed label"));
}

#[rstest]
fn test_cli_label_list(initialized_dir: TempDir) {
    let issue_id = create_issue(
        initialized_dir.path(),
        "Multi-label issue",
        &["--labels", "bug,urgent,backend"],
    );

    let output = run_rivets_in_dir(initialized_dir.path(), &["label", "list", &issue_id]);

    assert!(
        output.status.success(),
        "Label list failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("bug"));
    assert!(stdout.contains("urgent"));
    assert!(stdout.contains("backend"));
}

#[rstest]
fn test_cli_label_list_all(initialized_dir: TempDir) {
    create_issue(
        initialized_dir.path(),
        "Issue 1",
        &["--labels", "bug,frontend"],
    );
    create_issue(
        initialized_dir.path(),
        "Issue 2",
        &["--labels", "feature,backend"],
    );

    let output = run_rivets_in_dir(initialized_dir.path(), &["label", "list-all"]);

    assert!(
        output.status.success(),
        "Label list-all failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("bug"));
    assert!(stdout.contains("frontend"));
    assert!(stdout.contains("feature"));
    assert!(stdout.contains("backend"));
}

#[rstest]
fn test_cli_label_add_duplicate(initialized_dir: TempDir) {
    let issue_id = create_issue(initialized_dir.path(), "Issue", &["--labels", "existing"]);

    // Try to add the same label again
    let output = run_rivets_in_dir(
        initialized_dir.path(),
        &["label", "add", "existing", &issue_id],
    );

    // Should succeed but not duplicate
    assert!(output.status.success());
}

// ============================================================================
// Stale Command Tests
// ============================================================================

#[rstest]
fn test_cli_stale_empty(initialized_dir: TempDir) {
    let output = run_rivets_in_dir(initialized_dir.path(), &["stale"]);

    assert!(
        output.status.success(),
        "Stale failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No stale issues found"));
}

#[rstest]
fn test_cli_stale_with_days_option(initialized_dir: TempDir) {
    create_issue(initialized_dir.path(), "Recent issue", &[]);

    // Look for issues stale for 0 days (should find all open issues)
    let output = run_rivets_in_dir(initialized_dir.path(), &["stale", "--days", "0"]);

    assert!(
        output.status.success(),
        "Stale with days failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    // With 0 days, all open issues are considered stale
    assert!(stdout.contains("Recent issue"));
}

#[rstest]
fn test_cli_stale_with_status_filter(initialized_dir: TempDir) {
    create_issue(initialized_dir.path(), "Open issue", &[]);
    let id2 = create_issue(initialized_dir.path(), "In progress issue", &[]);

    run_rivets_in_dir(
        initialized_dir.path(),
        &["update", &id2, "--status", "in_progress"],
    );

    // Look for stale open issues only
    let output = run_rivets_in_dir(
        initialized_dir.path(),
        &["stale", "--days", "0", "--status", "open"],
    );

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Open issue"));
    assert!(!stdout.contains("In progress issue"));
}

#[rstest]
fn test_cli_stale_with_limit(initialized_dir: TempDir) {
    create_issue(initialized_dir.path(), "Issue 1", &[]);
    create_issue(initialized_dir.path(), "Issue 2", &[]);
    create_issue(initialized_dir.path(), "Issue 3", &[]);

    let output = run_rivets_in_dir(
        initialized_dir.path(),
        &["stale", "--days", "0", "--limit", "2"],
    );

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should show "Stale issues (2 not updated in 0 days):" in the output
    assert!(
        stdout.contains("Stale issues (2 not updated"),
        "Should show 2 stale issues due to limit. Got: {}",
        stdout
    );
}

#[rstest]
fn test_cli_stale_json_output(initialized_dir: TempDir) {
    create_issue(initialized_dir.path(), "Test issue", &[]);

    let output = run_rivets_in_dir(initialized_dir.path(), &["--json", "stale", "--days", "0"]);

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("Output should be valid JSON");
    assert!(json.is_array());
}

// ============================================================================
// Dep Tree Command Tests
// ============================================================================

#[rstest]
fn test_cli_dep_tree(initialized_dir: TempDir) {
    let id1 = create_issue(initialized_dir.path(), "Parent issue", &[]);
    let id2 = create_issue(initialized_dir.path(), "Child issue", &[]);

    // Create dependency
    run_rivets_in_dir(
        initialized_dir.path(),
        &["dep", "add", &id1, &id2, "-t", "blocks"],
    );

    let output = run_rivets_in_dir(initialized_dir.path(), &["dep", "tree", &id1]);

    assert!(
        output.status.success(),
        "Dep tree failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Dependency tree for:"));
    assert!(stdout.contains("Parent issue"));
    assert!(stdout.contains(&id2));
    assert!(stdout.contains("blocks"));
}

#[rstest]
fn test_cli_dep_tree_shows_dependents(initialized_dir: TempDir) {
    let id1 = create_issue(initialized_dir.path(), "Dependent issue", &[]);
    let id2 = create_issue(initialized_dir.path(), "Blocker issue", &[]);

    // id1 depends on id2 (id1 is blocked by id2)
    run_rivets_in_dir(
        initialized_dir.path(),
        &["dep", "add", &id1, &id2, "-t", "blocks"],
    );

    // Check tree from blocker's perspective
    let output = run_rivets_in_dir(initialized_dir.path(), &["dep", "tree", &id2]);

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Depended on by"));
    assert!(stdout.contains(&id1));
}

#[rstest]
fn test_cli_dep_tree_with_depth_limit(initialized_dir: TempDir) {
    let id1 = create_issue(initialized_dir.path(), "Level 1", &[]);
    let id2 = create_issue(initialized_dir.path(), "Level 2", &[]);
    let id3 = create_issue(initialized_dir.path(), "Level 3", &[]);

    // Create chain: id1 -> id2 -> id3
    run_rivets_in_dir(
        initialized_dir.path(),
        &["dep", "add", &id1, &id2, "-t", "blocks"],
    );
    run_rivets_in_dir(
        initialized_dir.path(),
        &["dep", "add", &id2, &id3, "-t", "blocks"],
    );

    // Tree with depth 1 should only show immediate dependencies
    let output = run_rivets_in_dir(
        initialized_dir.path(),
        &["dep", "tree", &id1, "--depth", "1"],
    );

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(&id2));
    // id3 might not be shown due to depth limit
}

#[rstest]
fn test_cli_dep_tree_json_output(initialized_dir: TempDir) {
    let id1 = create_issue(initialized_dir.path(), "Parent", &[]);
    let id2 = create_issue(initialized_dir.path(), "Child", &[]);

    run_rivets_in_dir(
        initialized_dir.path(),
        &["dep", "add", &id1, &id2, "-t", "blocks"],
    );

    let output = run_rivets_in_dir(initialized_dir.path(), &["--json", "dep", "tree", &id1]);

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("Output should be valid JSON");
    assert!(json["issue_id"].is_string());
    assert!(json["title"].is_string());
    assert!(json["dependencies"].is_array());
    assert!(json["dependents"].is_array());
}

#[rstest]
fn test_cli_dep_tree_no_dependencies(initialized_dir: TempDir) {
    let issue_id = create_issue(initialized_dir.path(), "Standalone issue", &[]);

    let output = run_rivets_in_dir(initialized_dir.path(), &["dep", "tree", &issue_id]);

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No issues depend on this"));
    assert!(stdout.contains("No dependencies"));
}

// ============================================================================
// Multi-ID Support Tests
// ============================================================================

#[rstest]
fn test_cli_show_multiple_issues(initialized_dir: TempDir) {
    let id1 = create_issue(initialized_dir.path(), "Issue One", &[]);
    let id2 = create_issue(initialized_dir.path(), "Issue Two", &[]);

    let output = run_rivets_in_dir(initialized_dir.path(), &["show", &id1, &id2]);

    assert!(
        output.status.success(),
        "Show multiple failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Issue One"));
    assert!(stdout.contains("Issue Two"));
}

#[rstest]
fn test_cli_update_multiple_issues(initialized_dir: TempDir) {
    let id1 = create_issue(initialized_dir.path(), "Issue 1", &[]);
    let id2 = create_issue(initialized_dir.path(), "Issue 2", &[]);

    let output = run_rivets_in_dir(
        initialized_dir.path(),
        &["update", &id1, &id2, "--priority", "0"],
    );

    assert!(
        output.status.success(),
        "Update multiple failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify both were updated
    let show1 = run_rivets_in_dir(initialized_dir.path(), &["show", &id1]);
    let show2 = run_rivets_in_dir(initialized_dir.path(), &["show", &id2]);
    assert!(String::from_utf8_lossy(&show1.stdout).contains("P0"));
    assert!(String::from_utf8_lossy(&show2.stdout).contains("P0"));
}

#[rstest]
fn test_cli_update_no_assignee_flag(initialized_dir: TempDir) {
    // Create an issue with an assignee
    let issue_id = create_issue(
        initialized_dir.path(),
        "Issue with assignee",
        &["--assignee", "alice"],
    );

    // Verify the assignee is set
    let show_before = run_rivets_in_dir(initialized_dir.path(), &["show", &issue_id]);
    let stdout_before = String::from_utf8_lossy(&show_before.stdout);
    assert!(
        stdout_before.contains("Assignee: alice"),
        "Assignee should be set initially"
    );

    // Update with --no-assignee to remove the assignee
    let update_output = run_rivets_in_dir(
        initialized_dir.path(),
        &["update", &issue_id, "--no-assignee"],
    );
    assert!(
        update_output.status.success(),
        "Update with --no-assignee failed: {:?}",
        String::from_utf8_lossy(&update_output.stderr)
    );

    // Verify the assignee was removed
    let show_after = run_rivets_in_dir(initialized_dir.path(), &["show", &issue_id]);
    let stdout_after = String::from_utf8_lossy(&show_after.stdout);
    assert!(
        !stdout_after.contains("Assignee:"),
        "Assignee should be removed after --no-assignee"
    );
}

#[rstest]
fn test_cli_close_multiple_issues(initialized_dir: TempDir) {
    let id1 = create_issue(initialized_dir.path(), "Issue 1", &[]);
    let id2 = create_issue(initialized_dir.path(), "Issue 2", &[]);
    let id3 = create_issue(initialized_dir.path(), "Issue 3", &[]);

    let output = run_rivets_in_dir(
        initialized_dir.path(),
        &["close", &id1, &id2, &id3, "--reason", "Batch close"],
    );

    assert!(
        output.status.success(),
        "Close multiple failed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify all were closed
    let list_output = run_rivets_in_dir(initialized_dir.path(), &["list", "--status", "closed"]);
    let stdout = String::from_utf8_lossy(&list_output.stdout);
    assert!(stdout.contains("3 issue(s)"));
}

#[rstest]
fn test_cli_show_multiple_json_output(initialized_dir: TempDir) {
    let id1 = create_issue(initialized_dir.path(), "Issue 1", &[]);
    let id2 = create_issue(initialized_dir.path(), "Issue 2", &[]);

    let output = run_rivets_in_dir(initialized_dir.path(), &["--json", "show", &id1, &id2]);

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("Output should be valid JSON");
    assert!(json.is_array());
    assert_eq!(json.as_array().unwrap().len(), 2);
}
