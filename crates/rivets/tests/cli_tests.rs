//! Integration tests for the rivets CLI.
//!
//! These tests verify the end-to-end behavior of all CLI commands.

use rstest::{fixture, rstest};
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

mod common;
use common::{create_issue, get_rivets_binary, run_rivets_in_dir};

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
        stdout.contains("resource"),
        "Help should show 'resource' command"
    );
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
    assert!(stdout.contains("--kind"), "Create help should show --kind");
    assert!(
        !stdout.contains("--type"),
        "Create help should not expose removed --type"
    );
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
#[case::partial_load(true, 2, 2)]
#[case::schema_incompatible_zero_load(false, 1, 1)]
fn test_cli_refuses_to_save_after_skipped_issue_records(
    initialized_dir: TempDir,
    #[case] include_valid_issue: bool,
    #[case] skipped_line_number: usize,
    #[case] skipped_record_count: usize,
) {
    use std::io::Write as _;

    let data_path = initialized_dir.path().join(".rivets/issues.jsonl");
    if include_valid_issue {
        create_issue(initialized_dir.path(), "Preserved issue", &[]);
    }
    let skipped_record = if include_valid_issue {
        r#"{"id":"broken","notes":[}"#
    } else {
        r#"{"id":"broken","title":"Broken","description":"Schema mismatch","status":"open","priority":2,"issue_kind":"task","assignee":null,"labels":[],"design":null,"acceptance_criteria":null,"notes":42,"external_ref":null,"dependencies":[],"created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z","closed_at":null}"#
    };
    let mut data_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&data_path)
        .expect("test should open the JSONL data file");
    writeln!(data_file, "{skipped_record}").expect("test should append a skipped record");
    if include_valid_issue {
        writeln!(data_file, "not valid JSON").expect("test should append another skipped record");
    }
    drop(data_file);
    let before = std::fs::read(&data_path).expect("test should read the original JSONL bytes");

    let create = run_rivets_in_dir(
        initialized_dir.path(),
        &["create", "--title", "Must not be persisted"],
    );

    assert!(
        !create.status.success(),
        "mutation after a partial load must be rejected"
    );
    let stderr = String::from_utf8_lossy(&create.stderr);
    assert!(
        stderr.contains(&format!("{skipped_record_count} issue record"))
            && stderr.contains(&format!("line {skipped_line_number}")),
        "error should report the skipped-record count and cause: {stderr}"
    );
    let after = std::fs::read(&data_path).expect("test should reread the JSONL bytes");
    assert_eq!(
        after, before,
        "a refused save must not rewrite the JSONL file"
    );
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
            "--kind",
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

#[test]
fn test_cli_list_invalid_status_rejected() {
    // Regression fence: invalid enum strings must fail with clap's exit-2
    // "invalid value" error, listing the possible values.
    let dir = tempfile::tempdir().expect("tempdir");
    let output = run_rivets_in_dir(dir.path(), &["list", "--status", "bogus"]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("invalid value 'bogus' for '--status <STATUS>'"),
        "stderr: {stderr}"
    );
    assert!(stderr.contains("possible values: open, in_progress, blocked, closed"));
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
fn test_cli_create_issue_kinds(initialized_dir: TempDir, #[case] issue_kind: &str) {
    let issue_id = create_issue(initialized_dir.path(), "Kind test", &["--kind", issue_kind]);

    let show = run_rivets_in_dir(initialized_dir.path(), &["show", &issue_id]);
    assert!(
        show.status.success(),
        "Issue kind '{issue_kind}' should persist. Stderr: {}",
        String::from_utf8_lossy(&show.stderr)
    );
    let show_text = String::from_utf8_lossy(&show.stdout);
    assert!(show_text.contains("Kind:"));
    assert!(show_text.contains(issue_kind));

    let list = run_rivets_in_dir(initialized_dir.path(), &["list", "--kind", issue_kind]);
    assert!(list.status.success());
    assert!(String::from_utf8_lossy(&list.stdout).contains(&issue_id));

    let json_show = run_rivets_in_dir(initialized_dir.path(), &["--json", "show", &issue_id]);
    let json: serde_json::Value =
        serde_json::from_slice(&json_show.stdout).expect("show output should be JSON");
    assert_eq!(json[0]["issue_kind"], issue_kind);
    assert!(json[0].get("issue_type").is_none());
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

#[rstest]
fn test_cli_notes_append_and_survive_restart(initialized_dir: TempDir) {
    let issue_id = create_issue(
        initialized_dir.path(),
        "Note history",
        &["--notes", "Initial context"],
    );

    let initial = run_rivets_in_dir(initialized_dir.path(), &["--json", "show", &issue_id]);
    assert!(
        initial.status.success(),
        "Initial show failed: {}",
        String::from_utf8_lossy(&initial.stderr)
    );
    let initial: serde_json::Value =
        serde_json::from_slice(&initial.stdout).expect("initial show output should be JSON");
    let initial_issue = &initial[0];
    assert_eq!(initial_issue["notes"][0]["content"], "Initial context");
    assert_eq!(
        initial_issue["notes"][0]["created_at"],
        initial_issue["updated_at"]
    );

    let update = run_rivets_in_dir(
        initialized_dir.path(),
        &["update", &issue_id, "--notes", "Second finding"],
    );
    assert!(
        update.status.success(),
        "Note append failed: {}",
        String::from_utf8_lossy(&update.stderr)
    );

    let restarted = run_rivets_in_dir(initialized_dir.path(), &["--json", "show", &issue_id]);
    let restarted: serde_json::Value =
        serde_json::from_slice(&restarted.stdout).expect("restarted show output should be JSON");
    let restarted_issue = &restarted[0];
    assert_eq!(restarted_issue["notes"][0]["content"], "Initial context");
    assert_eq!(
        restarted_issue["notes"][0]["created_at"],
        initial_issue["notes"][0]["created_at"]
    );
    assert_eq!(restarted_issue["notes"][1]["content"], "Second finding");
    assert_eq!(
        restarted_issue["notes"][1]["created_at"],
        restarted_issue["updated_at"]
    );

    let human = run_rivets_in_dir(initialized_dir.path(), &["show", &issue_id]);
    let human = String::from_utf8_lossy(&human.stdout);
    for note in restarted_issue["notes"]
        .as_array()
        .expect("Notes should be an array")
    {
        let timestamp = note["created_at"]
            .as_str()
            .expect("Note timestamp should be a string");
        let human_timestamp = chrono::DateTime::parse_from_rfc3339(timestamp)
            .expect("JSON Note timestamp should be RFC 3339")
            .format("%Y-%m-%d %H:%M")
            .to_string();
        assert!(
            human.contains(&human_timestamp),
            "human output should include Note timestamp {human_timestamp}"
        );
    }
    let first = human
        .find("Initial context")
        .expect("human output should include initial Note");
    let second = human
        .find("Second finding")
        .expect("human output should include appended Note");
    assert!(first < second, "human output should preserve Note order");

    let persisted = std::fs::read_to_string(initialized_dir.path().join(".rivets/issues.jsonl"))
        .expect("persisted issues should be readable");
    let persisted: serde_json::Value =
        serde_json::from_str(persisted.lines().next().expect("one persisted issue"))
            .expect("persisted issue should be JSON");
    assert!(persisted["notes"].is_array());
    assert_eq!(persisted["notes"][0]["content"], "Initial context");
    assert_eq!(persisted["notes"][1]["content"], "Second finding");
}

#[rstest]
fn test_cli_rejects_empty_notes_on_create_and_update(initialized_dir: TempDir) {
    let create = run_rivets_in_dir(
        initialized_dir.path(),
        &["create", "--title", "Invalid Note", "--notes", " \n "],
    );
    assert!(!create.status.success());
    assert!(String::from_utf8_lossy(&create.stderr).contains("Note content cannot be empty"));
    let after_failed_create = run_rivets_in_dir(initialized_dir.path(), &["--json", "list"]);
    let after_failed_create: serde_json::Value =
        serde_json::from_slice(&after_failed_create.stdout).expect("list output should be JSON");
    assert_eq!(after_failed_create, serde_json::json!([]));

    let issue_id = create_issue(initialized_dir.path(), "Valid Issue", &[]);
    let update = run_rivets_in_dir(
        initialized_dir.path(),
        &["update", &issue_id, "--notes", ""],
    );
    assert!(!update.status.success());
    assert!(String::from_utf8_lossy(&update.stderr).contains("Note content cannot be empty"));

    let shown = run_rivets_in_dir(initialized_dir.path(), &["--json", "show", &issue_id]);
    let shown: serde_json::Value =
        serde_json::from_slice(&shown.stdout).expect("show output should be JSON");
    assert_eq!(shown[0]["notes"], serde_json::json!([]));
}

#[rstest]
fn test_cli_reclassifies_only_kind_and_persists(initialized_dir: TempDir) {
    let issue_id = create_issue(
        initialized_dir.path(),
        "Reclassify me",
        &[
            "--description",
            "Keep this description",
            "--priority",
            "1",
            "--kind",
            "task",
            "--assignee",
            "alice",
            "--labels",
            "backend,ready",
        ],
    );

    let before = run_rivets_in_dir(initialized_dir.path(), &["--json", "show", &issue_id]);
    let before: serde_json::Value =
        serde_json::from_slice(&before.stdout).expect("initial show output should be JSON");

    let update = run_rivets_in_dir(
        initialized_dir.path(),
        &["update", &issue_id, "--kind", "bug"],
    );
    assert!(
        update.status.success(),
        "Kind update failed: {}",
        String::from_utf8_lossy(&update.stderr)
    );

    let after = run_rivets_in_dir(initialized_dir.path(), &["--json", "show", &issue_id]);
    let after: serde_json::Value =
        serde_json::from_slice(&after.stdout).expect("restarted show output should be JSON");
    assert_eq!(after[0]["issue_kind"], "bug");
    assert!(after[0].get("issue_type").is_none());
    assert_ne!(before[0]["updated_at"], after[0]["updated_at"]);

    let mut before_fields = before[0]
        .as_object()
        .expect("issue should be an object")
        .clone();
    let mut after_fields = after[0]
        .as_object()
        .expect("issue should be an object")
        .clone();
    before_fields.remove("issue_kind");
    before_fields.remove("updated_at");
    after_fields.remove("issue_kind");
    after_fields.remove("updated_at");
    assert_eq!(before_fields, after_fields);

    let persisted = std::fs::read_to_string(initialized_dir.path().join(".rivets/issues.jsonl"))
        .expect("persisted issues should be readable");
    let record: serde_json::Value = persisted
        .lines()
        .map(|line| serde_json::from_str(line).expect("persisted record should be JSON"))
        .find(|record: &serde_json::Value| record["id"] == issue_id)
        .expect("updated issue should remain persisted");
    assert_eq!(record["issue_kind"], "bug");
    assert!(record.get("issue_type").is_none());
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

#[rstest]
fn test_cli_close_and_reopen_reasons_append_notes(initialized_dir: TempDir) {
    let issue_id = create_issue(initialized_dir.path(), "Lifecycle history", &[]);

    let close = run_rivets_in_dir(
        initialized_dir.path(),
        &["close", &issue_id, "--reason", "Fixed once"],
    );
    assert!(
        close.status.success(),
        "Close failed: {}",
        String::from_utf8_lossy(&close.stderr)
    );

    let reopen = run_rivets_in_dir(
        initialized_dir.path(),
        &["reopen", &issue_id, "--reason", "Regression found"],
    );
    assert!(
        reopen.status.success(),
        "Reopen failed: {}",
        String::from_utf8_lossy(&reopen.stderr)
    );

    let restarted = run_rivets_in_dir(initialized_dir.path(), &["--json", "show", &issue_id]);
    let restarted: serde_json::Value =
        serde_json::from_slice(&restarted.stdout).expect("show output should be JSON");
    let issue = &restarted[0];
    assert_eq!(issue["notes"].as_array().map(Vec::len), Some(2));
    assert_eq!(issue["notes"][0]["content"], "Closed: Fixed once");
    assert_eq!(issue["notes"][0]["created_at"], issue["closed_at"]);
    assert_eq!(issue["notes"][1]["content"], "Reopened: Regression found");
    assert_eq!(issue["notes"][1]["created_at"], issue["updated_at"]);
    assert_eq!(issue["status"], "open");
}

#[rstest]
fn test_cli_rejects_blank_lifecycle_reasons(initialized_dir: TempDir) {
    let issue_id = create_issue(initialized_dir.path(), "Blank reasons", &[]);

    let blank_close = run_rivets_in_dir(
        initialized_dir.path(),
        &["close", &issue_id, "--reason", ""],
    );
    assert!(!blank_close.status.success());
    assert!(
        String::from_utf8_lossy(&blank_close.stderr).contains("Note content cannot be empty"),
        "close with blank reason should reject empty Note content"
    );

    let close = run_rivets_in_dir(
        initialized_dir.path(),
        &["close", &issue_id, "--reason", "Fixed"],
    );
    assert!(close.status.success());

    let blank_reopen = run_rivets_in_dir(
        initialized_dir.path(),
        &["reopen", &issue_id, "--reason", "   "],
    );
    assert!(!blank_reopen.status.success());
    assert!(
        String::from_utf8_lossy(&blank_reopen.stderr).contains("Note content cannot be empty"),
        "reopen with blank reason should reject empty Note content"
    );

    let shown = run_rivets_in_dir(initialized_dir.path(), &["--json", "show", &issue_id]);
    let shown: serde_json::Value =
        serde_json::from_slice(&shown.stdout).expect("show output should be JSON");
    assert_eq!(shown[0]["notes"][0]["content"], "Closed: Fixed");
    assert_eq!(shown[0]["notes"].as_array().map(Vec::len), Some(1));
    assert_eq!(shown[0]["status"], "closed");
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

#[rstest]
fn test_cli_ready_filters_by_kind_and_label(initialized_dir: TempDir) {
    let expected_id = create_issue(
        initialized_dir.path(),
        "Ready agent task",
        &["--kind", "task", "--labels", "ready-for-agent"],
    );
    create_issue(
        initialized_dir.path(),
        "Ready task with another label",
        &["--kind", "task", "--labels", "needs-triage"],
    );
    create_issue(
        initialized_dir.path(),
        "Ready agent feature",
        &["--kind", "feature", "--labels", "ready-for-agent"],
    );

    let output = run_rivets_in_dir(
        initialized_dir.path(),
        &[
            "--json",
            "ready",
            "--kind",
            "task",
            "--label",
            "ready-for-agent",
        ],
    );

    assert!(
        output.status.success(),
        "Ready filtering failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let issues: Vec<serde_json::Value> =
        serde_json::from_slice(&output.stdout).expect("Ready output should be valid JSON");
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0]["id"], expected_id);
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
    // Simple text format shows blockers on indented line
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

    // Close both issues (--yes to skip confirmation)
    run_rivets_in_dir(initialized_dir.path(), &["--yes", "close", &id1, &id2]);

    // Reopen both at once (--yes to skip confirmation)
    let output = run_rivets_in_dir(initialized_dir.path(), &["--yes", "reopen", &id1, &id2]);

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

    // Try to reopen an already open issue - should fail since it's not closed
    let output = run_rivets_in_dir(initialized_dir.path(), &["reopen", &issue_id]);

    assert!(
        !output.status.success(),
        "Reopen should fail for non-closed issues"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not closed"),
        "Error should mention issue is not closed: {stderr}"
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
    assert!(stdout.contains(&id1), "should contain root issue ID");
    assert!(stdout.contains("Parent issue"), "should contain root title");
    assert!(stdout.contains(&id2), "should contain child issue ID");
    assert!(stdout.contains("blocks"), "should contain dep type");
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
    assert!(json["id"].is_string(), "should have 'id' field");
    assert!(json["title"].is_string(), "should have 'title' field");
    assert!(
        json["dependencies"].is_array(),
        "should have 'dependencies' array"
    );
    assert!(
        json["dependents"].is_array(),
        "should have 'dependents' array"
    );
}

#[rstest]
fn test_cli_dep_tree_no_dependencies(initialized_dir: TempDir) {
    let issue_id = create_issue(initialized_dir.path(), "Standalone issue", &[]);

    let output = run_rivets_in_dir(initialized_dir.path(), &["dep", "tree", &issue_id]);

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Root node is always displayed with ID and title
    assert!(
        stdout.contains(&issue_id),
        "should contain issue ID, got: {}",
        stdout
    );
    assert!(
        stdout.contains("Standalone issue"),
        "should contain issue title, got: {}",
        stdout
    );
    // No dependency tree connectors should appear
    assert!(
        !stdout.contains("├──") && !stdout.contains("└──"),
        "should not contain tree connectors for standalone issue, got: {}",
        stdout
    );
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
        &[
            "--yes",
            "close",
            &id1,
            &id2,
            &id3,
            "--reason",
            "Batch close",
        ],
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

// ============================================================================
// NO_COLOR Integration Tests
// ============================================================================

/// Run the rivets binary with a custom environment variable set.
fn run_rivets_with_env(
    dir: &Path,
    args: &[&str],
    env_key: &str,
    env_val: &str,
) -> std::process::Output {
    let binary = get_rivets_binary();
    match Command::new(&binary)
        .args(args)
        .current_dir(dir)
        .env(env_key, env_val)
        .output()
    {
        Ok(output) => output,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::thread::sleep(std::time::Duration::from_millis(500));
            Command::new(&binary)
                .args(args)
                .current_dir(dir)
                .env(env_key, env_val)
                .output()
                .expect("Failed to execute rivets binary after retry")
        }
        Err(e) => panic!("Failed to execute rivets binary: {e}"),
    }
}

/// Returns true if the string contains any ANSI escape sequences.
fn contains_ansi_escapes(s: &str) -> bool {
    s.contains("\x1b[")
}

#[rstest]
fn test_cli_no_color_env_disables_ansi(initialized_dir: TempDir) {
    create_issue(
        initialized_dir.path(),
        "Color test issue",
        &["--kind", "bug", "--priority", "1"],
    );

    // Without NO_COLOR, output may contain ANSI escapes (depends on terminal detection,
    // but we can at least verify the NO_COLOR path produces clean output)
    let output = run_rivets_with_env(initialized_dir.path(), &["list"], "NO_COLOR", "1");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !contains_ansi_escapes(&stdout),
        "NO_COLOR=1 should suppress ANSI escape sequences in output, got: {stdout}"
    );
}

#[rstest]
fn test_cli_rivets_color_zero_disables_ansi(initialized_dir: TempDir) {
    create_issue(
        initialized_dir.path(),
        "Color test issue",
        &["--kind", "feature"],
    );

    let output = run_rivets_with_env(initialized_dir.path(), &["list"], "RIVETS_COLOR", "0");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !contains_ansi_escapes(&stdout),
        "RIVETS_COLOR=0 should suppress ANSI escape sequences in output, got: {stdout}"
    );
}

// ============================================================================
// Associated Resource Commands
// ============================================================================

#[rstest]
fn resource_add_list_show_and_validation_survive_process_restart(initialized_dir: TempDir) {
    let issue_id = create_issue(initialized_dir.path(), "Resource owner", &[]);

    let first = run_rivets_in_dir(
        initialized_dir.path(),
        &[
            "resource",
            "add",
            &issue_id,
            "--url",
            "https://example.com/pr/123",
            "--role",
            "implementation",
            "--label",
            "Implementation PR",
        ],
    );
    assert!(
        first.status.success(),
        "first resource add failed: {}",
        String::from_utf8_lossy(&first.stderr)
    );

    let second = run_rivets_in_dir(
        initialized_dir.path(),
        &[
            "resource",
            "add",
            &issue_id,
            "--url",
            "https://example.com/pr/123",
            "--role",
            "documentation",
        ],
    );
    assert!(
        second.status.success(),
        "same target with distinct role should succeed: {}",
        String::from_utf8_lossy(&second.stderr)
    );

    let list = run_rivets_in_dir(
        initialized_dir.path(),
        &["--json", "resource", "list", &issue_id],
    );
    assert!(list.status.success());
    let resources: serde_json::Value =
        serde_json::from_slice(&list.stdout).expect("resource list should be JSON");
    let resources = resources
        .as_array()
        .expect("resource list should be an array");
    assert_eq!(resources.len(), 2);
    assert_eq!(resources[0]["id"], "r1");
    assert_eq!(resources[0]["target"]["type"], "web");
    assert_eq!(resources[0]["target"]["url"], "https://example.com/pr/123");
    assert_eq!(resources[0]["role"], "implementation");
    assert_eq!(resources[0]["label"], "Implementation PR");
    assert_eq!(resources[1]["id"], "r2");
    assert_eq!(resources[1]["role"], "documentation");
    assert!(resources[1]["label"].is_null());

    let show = run_rivets_in_dir(initialized_dir.path(), &["show", &issue_id]);
    assert!(show.status.success());
    let show_text = String::from_utf8_lossy(&show.stdout);
    let first_pos = show_text
        .find("[r1] https://example.com/pr/123 (implementation) — Implementation PR")
        .expect("show should render first resource");
    let second_pos = show_text
        .find("[r2] https://example.com/pr/123 (documentation)")
        .expect("show should render second resource");
    assert!(first_pos < second_pos, "show must preserve insertion order");

    let duplicate = run_rivets_in_dir(
        initialized_dir.path(),
        &[
            "resource",
            "add",
            &issue_id,
            "--url",
            "https://example.com/pr/123",
            "--role",
            "implementation",
        ],
    );
    assert!(!duplicate.status.success());
    assert!(
        String::from_utf8_lossy(&duplicate.stderr).contains("already exists"),
        "duplicate error should be explicit: {}",
        String::from_utf8_lossy(&duplicate.stderr)
    );

    let invalid_url = run_rivets_in_dir(
        initialized_dir.path(),
        &[
            "resource",
            "add",
            &issue_id,
            "--url",
            "docs/adr/0003-associated-resources.md",
            "--role",
            "reference",
        ],
    );
    assert!(!invalid_url.status.success());
    assert!(String::from_utf8_lossy(&invalid_url.stderr).contains("Invalid web URL"));

    let empty_label = run_rivets_in_dir(
        initialized_dir.path(),
        &[
            "resource",
            "add",
            &issue_id,
            "--url",
            "https://example.com/evidence",
            "--role",
            "evidence",
            "--label",
            "",
        ],
    );
    assert!(!empty_label.status.success());
    assert!(
        String::from_utf8_lossy(&empty_label.stderr).contains("Resource label cannot be empty")
    );

    let after_failures = run_rivets_in_dir(
        initialized_dir.path(),
        &["--json", "resource", "list", &issue_id],
    );
    let resources: serde_json::Value =
        serde_json::from_slice(&after_failures.stdout).expect("resource list should be JSON");
    assert_eq!(
        resources
            .as_array()
            .expect("resource list should be an array")
            .len(),
        2,
        "validation and duplicate failures must not persist"
    );

    let data = std::fs::read_to_string(initialized_dir.path().join(".rivets/issues.jsonl"))
        .expect("issues file should be readable");
    let record: serde_json::Value = data
        .lines()
        .map(|line| serde_json::from_str(line).expect("record should be JSON"))
        .find(|record: &serde_json::Value| record["id"] == issue_id)
        .expect("created Issue should be persisted");
    assert!(record.get("external_ref").is_none());
    assert_eq!(record["resources"].as_array().unwrap().len(), 2);
    assert_eq!(record["next_resource_id"], 3);
}

#[rstest]
fn legacy_web_external_ref_migrates_then_rewrites_canonically(initialized_dir: TempDir) {
    let issues_path = initialized_dir.path().join(".rivets/issues.jsonl");
    let legacy = r#"{"id":"test-legacy","title":"Legacy URL","description":"Test","status":"open","priority":2,"issue_type":"task","assignee":null,"labels":[],"design":null,"acceptance_criteria":null,"notes":null,"external_ref":"https://example.com/legacy","dependencies":[],"created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-02T00:00:00Z","closed_at":null}"#;
    std::fs::write(&issues_path, format!("{legacy}\n")).expect("legacy record should be seeded");

    let migrated = run_rivets_in_dir(
        initialized_dir.path(),
        &["--json", "resource", "list", "test-legacy"],
    );
    assert!(
        migrated.status.success(),
        "legacy list failed: {}",
        String::from_utf8_lossy(&migrated.stderr)
    );
    let resources: serde_json::Value =
        serde_json::from_slice(&migrated.stdout).expect("migrated resources should be JSON");
    assert_eq!(resources[0]["id"], "r1");
    assert_eq!(resources[0]["role"], "reference");
    assert_eq!(resources[0]["target"]["url"], "https://example.com/legacy");

    let add = run_rivets_in_dir(
        initialized_dir.path(),
        &[
            "resource",
            "add",
            "test-legacy",
            "--url",
            "https://example.com/new",
            "--role",
            "evidence",
        ],
    );
    assert!(
        add.status.success(),
        "canonicalizing mutation failed: {}",
        String::from_utf8_lossy(&add.stderr)
    );

    let restarted = run_rivets_in_dir(
        initialized_dir.path(),
        &["--json", "resource", "list", "test-legacy"],
    );
    let resources: serde_json::Value =
        serde_json::from_slice(&restarted.stdout).expect("restarted list should be JSON");
    assert_eq!(resources.as_array().unwrap().len(), 2);
    assert_eq!(resources[0]["id"], "r1");
    assert_eq!(resources[1]["id"], "r2");

    let canonical =
        std::fs::read_to_string(issues_path).expect("canonical file should be readable");
    let record: serde_json::Value =
        serde_json::from_str(canonical.trim()).expect("canonical record should be JSON");
    assert!(record.get("external_ref").is_none());
    assert!(record.get("issue_type").is_none());
    assert_eq!(record["issue_kind"], "task");
    assert_eq!(record["resources"].as_array().unwrap().len(), 2);
    assert_eq!(record["next_resource_id"], 3);
}

#[rstest]
fn resource_path_add_update_remove_and_error_cases(initialized_dir: TempDir) {
    let issue_id = create_issue(initialized_dir.path(), "Path owner", &[]);

    // Path resources: stored normalized, workspace-root-relative.
    let added = run_rivets_in_dir(
        initialized_dir.path(),
        &[
            "resource",
            "add",
            &issue_id,
            "--path",
            "docs/../docs/adr/0003.md",
            "--role",
            "documentation",
            "--label",
            "ADR 3",
        ],
    );
    assert!(
        added.status.success(),
        "path add failed: {}",
        String::from_utf8_lossy(&added.stderr)
    );
    // Unicode paths are stored as given (normalized).
    let unicode = run_rivets_in_dir(
        initialized_dir.path(),
        &[
            "resource",
            "add",
            &issue_id,
            "--path",
            "é/文件.md",
            "--role",
            "evidence",
        ],
    );
    assert!(
        unicode.status.success(),
        "unicode path add failed: {}",
        String::from_utf8_lossy(&unicode.stderr)
    );
    // Path inputs are workspace-root-relative even from a subdirectory.
    let subdir = initialized_dir.path().join("crates/x");
    std::fs::create_dir_all(&subdir).expect("subdir should be created");
    let from_subdir = run_rivets_in_dir(
        &subdir,
        &[
            "resource",
            "add",
            &issue_id,
            "--path",
            "crates/x/src/lib.rs",
            "--role",
            "implementation",
        ],
    );
    assert!(
        from_subdir.status.success(),
        "subdir path add failed: {}",
        String::from_utf8_lossy(&from_subdir.stderr)
    );

    let list = run_rivets_in_dir(
        initialized_dir.path(),
        &["--json", "resource", "list", &issue_id],
    );
    assert!(list.status.success());
    let resources: serde_json::Value =
        serde_json::from_slice(&list.stdout).expect("list should be JSON");
    let resources = resources.as_array().expect("array");
    assert_eq!(resources.len(), 3);
    assert_eq!(resources[0]["id"], "r1");
    assert_eq!(resources[0]["target"]["type"], "path");
    assert_eq!(resources[0]["target"]["path"], "docs/adr/0003.md");
    assert_eq!(resources[1]["target"]["path"], "é/文件.md");
    assert_eq!(resources[2]["target"]["path"], "crates/x/src/lib.rs");

    // Escape, absolute, and conflict errors are all rejected without persisting.
    let escape = run_rivets_in_dir(
        initialized_dir.path(),
        &[
            "resource",
            "add",
            &issue_id,
            "--path",
            "../escape.md",
            "--role",
            "reference",
        ],
    );
    assert!(!escape.status.success(), "escape must fail");
    assert!(String::from_utf8_lossy(&escape.stderr).contains("escapes the workspace root"));

    let absolute = run_rivets_in_dir(
        initialized_dir.path(),
        &[
            "resource",
            "add",
            &issue_id,
            "--path",
            "/etc/passwd",
            "--role",
            "reference",
        ],
    );
    assert!(!absolute.status.success(), "absolute must fail");

    let backslash = run_rivets_in_dir(
        initialized_dir.path(),
        &[
            "resource",
            "add",
            &issue_id,
            "--path",
            r"docs\readme.md",
            "--role",
            "reference",
        ],
    );
    assert!(!backslash.status.success(), "backslash must fail");
    assert!(
        String::from_utf8_lossy(&backslash.stderr).contains("use '/' as the separator"),
        "backslash rejection should name the portable separator"
    );

    let conflict = run_rivets_in_dir(
        initialized_dir.path(),
        &[
            "resource",
            "add",
            &issue_id,
            "--url",
            "https://example.com",
            "--path",
            "src/lib.rs",
            "--role",
            "reference",
        ],
    );
    assert!(!conflict.status.success(), "--url with --path must fail");

    // Normalized-equivalent duplicate is rejected with a typed message.
    let duplicate = run_rivets_in_dir(
        initialized_dir.path(),
        &[
            "resource",
            "add",
            &issue_id,
            "--path",
            "crates/../crates/x/src/lib.rs",
            "--role",
            "implementation",
        ],
    );
    assert!(
        !duplicate.status.success(),
        "normalized duplicate must fail"
    );
    assert!(String::from_utf8_lossy(&duplicate.stderr).contains("already exists"));

    // Update: role change + label clear keep id and position.
    let updated = run_rivets_in_dir(
        initialized_dir.path(),
        &[
            "resource",
            "update",
            &issue_id,
            "--resource",
            "r1",
            "--role",
            "reference",
            "--no-label",
        ],
    );
    assert!(
        updated.status.success(),
        "update failed: {}",
        String::from_utf8_lossy(&updated.stderr)
    );
    // Update: target change normalizes and preserves position.
    let target_update = run_rivets_in_dir(
        initialized_dir.path(),
        &[
            "resource",
            "update",
            &issue_id,
            "--resource",
            "r2",
            "--path",
            "specs/../specs/0x/0004.md",
        ],
    );
    assert!(target_update.status.success());
    let after_update = run_rivets_in_dir(
        initialized_dir.path(),
        &["--json", "resource", "list", &issue_id],
    );
    let resources: serde_json::Value =
        serde_json::from_slice(&after_update.stdout).expect("list should be JSON");
    let resources = resources.as_array().expect("array");
    assert_eq!(resources.len(), 3);
    assert_eq!(resources[0]["id"], "r1");
    assert_eq!(resources[0]["role"], "reference");
    assert!(resources[0]["label"].is_null(), "label must be cleared");
    assert_eq!(resources[1]["id"], "r2");
    assert_eq!(resources[1]["target"]["path"], "specs/0x/0004.md");

    // Unknown resource id is a typed error that does not persist.
    let unknown = run_rivets_in_dir(
        initialized_dir.path(),
        &["resource", "remove", &issue_id, "--resource", "r99"],
    );
    assert!(!unknown.status.success());
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("Resource not found: r99"));

    // An empty label is a typed error that does not persist.
    let empty_label = run_rivets_in_dir(
        initialized_dir.path(),
        &[
            "resource",
            "update",
            &issue_id,
            "--resource",
            "r2",
            "--label",
            "",
        ],
    );
    assert!(!empty_label.status.success(), "empty label must fail");
    assert!(String::from_utf8_lossy(&empty_label.stderr).contains("label cannot be empty"));

    // Remove: remaining resources keep ids and positions.
    let removed = run_rivets_in_dir(
        initialized_dir.path(),
        &["resource", "remove", &issue_id, "--resource", "r1"],
    );
    assert!(removed.status.success());
    let after_remove = run_rivets_in_dir(
        initialized_dir.path(),
        &["--json", "resource", "list", &issue_id],
    );
    let resources: serde_json::Value =
        serde_json::from_slice(&after_remove.stdout).expect("list should be JSON");
    let ids: Vec<_> = resources
        .as_array()
        .expect("array")
        .iter()
        .map(|r| r["id"].as_str().expect("id is a string").to_string())
        .collect();
    assert_eq!(
        ids,
        ["r2", "r3"],
        "remaining resources keep ids and positions"
    );

    // The persisted file is the same state, and next_resource_id never reuses.
    let issues_path = initialized_dir.path().join(".rivets/issues.jsonl");
    let canonical = std::fs::read_to_string(&issues_path).expect("canonical file readable");
    let record: serde_json::Value = canonical
        .lines()
        .map(|line| serde_json::from_str(line).expect("record"))
        .find(|record: &serde_json::Value| record["id"] == issue_id)
        .expect("record exists");
    assert_eq!(record["next_resource_id"], 4);
    let persisted_ids: Vec<_> = record["resources"]
        .as_array()
        .expect("resources should be an array")
        .iter()
        .map(|r| r["id"].as_str().expect("id is a string").to_string())
        .collect();
    assert_eq!(persisted_ids, ["r2", "r3"]);
}

#[rstest]
fn resource_update_requires_at_least_one_field(initialized_dir: TempDir) {
    let issue_id = create_issue(initialized_dir.path(), "No-op update", &[]);
    let output = run_rivets_in_dir(
        initialized_dir.path(),
        &["resource", "update", &issue_id, "--resource", "r1"],
    );
    assert!(
        !output.status.success(),
        "zero-field update must fail at parse time"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("required arguments"),
        "clap should reject before touching the workspace: {stderr}"
    );
}

#[rstest]
fn resource_mutations_persist_across_process_generations(initialized_dir: TempDir) {
    // Every run_rivets_in_dir call is a fresh process; generation boundaries
    // are the process exits between invocations.
    let issue_id = create_issue(initialized_dir.path(), "Generations", &[]);

    // Generation 1: add a web target and a path target, then update r1's role.
    run_ok(
        initialized_dir.path(),
        &[
            "resource",
            "add",
            &issue_id,
            "--url",
            "https://example.com/pr/1",
            "--role",
            "implementation",
        ],
    );
    run_ok(
        initialized_dir.path(),
        &[
            "resource",
            "add",
            &issue_id,
            "--path",
            "docs/../docs/guide.md",
            "--role",
            "documentation",
            "--label",
            "Guide",
        ],
    );
    run_ok(
        initialized_dir.path(),
        &[
            "resource",
            "update",
            &issue_id,
            "--resource",
            "r1",
            "--role",
            "successor",
        ],
    );

    // Generation 2: state survived the process boundary exactly.
    let resources = list_resources(initialized_dir.path(), &issue_id);
    assert_eq!(resources.len(), 2);
    assert_eq!(resources[0]["id"], "r1");
    assert_eq!(resources[0]["role"], "successor");
    assert_eq!(resources[1]["id"], "r2");
    assert_eq!(resources[1]["target"]["type"], "path");
    assert_eq!(resources[1]["target"]["path"], "docs/guide.md");

    // Generation 3: remove r1 and add a new resource; the new id is r3,
    // continuing the sequence rather than reusing r1.
    run_ok(
        initialized_dir.path(),
        &["resource", "remove", &issue_id, "--resource", "r1"],
    );
    run_ok(
        initialized_dir.path(),
        &[
            "resource",
            "add",
            &issue_id,
            "--path",
            "src/main.rs",
            "--role",
            "implementation",
        ],
    );

    // Generation 4: removal, order, and the continued sequence all persist.
    let resources = list_resources(initialized_dir.path(), &issue_id);
    let ids: Vec<_> = resources
        .iter()
        .map(|r| r["id"].as_str().expect("id is a string"))
        .collect();
    assert_eq!(ids, ["r2", "r3"]);
    assert_eq!(resources[1]["target"]["path"], "src/main.rs");

    let canonical = std::fs::read_to_string(initialized_dir.path().join(".rivets/issues.jsonl"))
        .expect("canonical file readable");
    let record: serde_json::Value = canonical
        .lines()
        .map(|line| serde_json::from_str(line).expect("record"))
        .find(|record: &serde_json::Value| record["id"] == issue_id)
        .expect("record exists");
    assert_eq!(record["next_resource_id"], 4);
}

fn run_ok(dir: &Path, args: &[&str]) {
    let output = run_rivets_in_dir(dir, args);
    assert!(
        output.status.success(),
        "rivets {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn list_resources(dir: &Path, issue_id: &str) -> Vec<serde_json::Value> {
    let output = run_rivets_in_dir(dir, &["--json", "resource", "list", issue_id]);
    assert!(
        output.status.success(),
        "resource list failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice::<serde_json::Value>(&output.stdout)
        .expect("resource list should be JSON")
        .as_array()
        .expect("resource list should be an array")
        .clone()
}
