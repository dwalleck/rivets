//! Focused cross-adapter fences for the canonical Update and dedicated intents.
//!
//! These tests deliberately use literal before/after records and raw JSONL bytes
//! rather than deriving an oracle from the adapter under test.

use rivets::domain::{Issue, IssueKind, IssueStatus};
use rivets_mcp::context::Context;
use rivets_mcp::models::{CreateParams, IssueKindInput, LifecycleParams, UpdateParams};
use rivets_mcp::tools::Tools;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Arc;
use tempfile::TempDir;
use tokio::sync::RwLock;

fn workspace() -> TempDir {
    let workspace = TempDir::new().expect("temporary workspace should be created");
    let rivets_dir = workspace.path().join(".rivets");
    std::fs::create_dir(&rivets_dir).expect(".rivets should be created");
    std::fs::write(
        rivets_dir.join("config.yaml"),
        "issue-prefix: test\nstorage:\n  backend: jsonl\n  data_file: .rivets/issues.jsonl\n",
    )
    .expect("config should be written");
    std::fs::write(rivets_dir.join("issues.jsonl"), []).expect("JSONL source should be created");
    workspace
}

fn tools() -> Tools {
    Tools::new(Arc::new(RwLock::new(Context::new())))
}

async fn set_context(tools: &Tools, root: &Path) {
    tools
        .set_context(root.to_str().expect("workspace path should be UTF-8"))
        .await
        .expect("MCP context should be set");
}

fn create_params(title: &str) -> CreateParams {
    CreateParams {
        title: title.to_string(),
        description: Some("Original description".to_string()),
        priority: Some(2),
        kind: IssueKindInput::canonical(Some(IssueKind::Task)),
        assignee: None,
        labels: Some(vec!["keep-me".to_string()]),
        design: Some("Original design".to_string()),
        acceptance: Some("Original acceptance".to_string()),
        initial_note: Some("Existing note".to_string()),
        workspace_root: None,
    }
}

async fn create_issue(tools: &Tools, title: &str) -> Issue {
    tools
        .create(create_params(title))
        .await
        .expect("fixture Issue should be created")
}

fn update_cli_args(issue_ids: &[&str], title: &str) -> Vec<String> {
    let mut args = vec!["update".to_string()];
    args.extend(issue_ids.iter().map(|issue_id| (*issue_id).to_string()));
    args.extend(["--title".to_string(), title.to_string()]);
    args
}

fn run_cli(workspace: &Path, args: &[String]) -> Output {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("Workspace manifest parent")
        .join("Cargo.toml");
    Command::new(env!("CARGO"))
        .arg("run")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(manifest)
        .args(["-p", "rivets", "--"])
        .args(args)
        .current_dir(workspace)
        .output()
        .expect("real Rivets CLI should run")
}

fn jsonl_path(workspace: &Path) -> PathBuf {
    workspace.join(".rivets/issues.jsonl")
}

fn read_bytes(workspace: &Path) -> Vec<u8> {
    std::fs::read(jsonl_path(workspace)).expect("JSONL source should be readable")
}

fn read_records(workspace: &Path) -> Vec<Value> {
    std::fs::read_to_string(jsonl_path(workspace))
        .expect("JSONL source should be readable")
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("JSONL record should parse"))
        .collect()
}

fn read_record(workspace: &Path, issue_id: &str) -> Value {
    read_records(workspace)
        .into_iter()
        .find(|record| record["id"] == issue_id)
        .expect("requested record should exist")
}

#[tokio::test]
async fn canonical_update_accepts_all_nonempty_field_masks_and_preserves_omissions() {
    let workspace = workspace();
    let tools = tools();
    set_context(&tools, workspace.path()).await;
    let issue = create_issue(&tools, "Mask fixture").await;

    let before_empty = read_bytes(workspace.path());
    let empty = tools
        .update(UpdateParams {
            issue_id: (issue.id.as_str()).to_string(),
            ..Default::default()
        })
        .await;
    assert!(empty.is_err(), "the zero presence mask must be rejected");
    assert_eq!(read_bytes(workspace.path()), before_empty);

    let mut previous = issue;
    for mask in 1_u8..=63 {
        let title = (mask & 1 != 0).then(|| format!("Mask title {mask}"));
        let description = (mask & 2 != 0).then(|| format!("Mask description {mask}"));
        let priority = (mask & 4 != 0).then_some(mask % 5);
        let issue_kind = (mask & 8 != 0).then(|| {
            [
                IssueKind::Bug,
                IssueKind::Feature,
                IssueKind::Task,
                IssueKind::Epic,
                IssueKind::Chore,
            ][usize::from(mask) % 5]
        });
        let design = (mask & 16 != 0).then(|| format!("Mask design {mask}"));
        let acceptance_criteria = (mask & 32 != 0).then(|| format!("Mask acceptance {mask}"));
        let updated = tools
            .update(UpdateParams {
                issue_id: previous.id.to_string(),
                title: title.clone(),
                description: description.clone(),
                priority,
                issue_kind,
                design: design.clone(),
                acceptance_criteria: acceptance_criteria.clone(),
                workspace_root: None,
            })
            .await
            .expect("every nonempty canonical presence mask should update");

        if let Some(title) = title {
            assert_eq!(updated.title, title);
        } else {
            assert_eq!(updated.title, previous.title);
        }
        if let Some(description) = description {
            assert_eq!(updated.description, description);
        } else {
            assert_eq!(updated.description, previous.description);
        }
        if let Some(priority) = priority {
            assert_eq!(updated.priority, priority);
        } else {
            assert_eq!(updated.priority, previous.priority);
        }
        if let Some(issue_kind) = issue_kind {
            assert_eq!(updated.issue_kind, issue_kind);
        } else {
            assert_eq!(updated.issue_kind, previous.issue_kind);
        }
        if let Some(design) = design {
            assert_eq!(updated.design.as_deref(), Some(design.as_str()));
        } else {
            assert_eq!(updated.design, previous.design);
        }
        if let Some(acceptance) = acceptance_criteria {
            assert_eq!(
                updated.acceptance_criteria.as_deref(),
                Some(acceptance.as_str())
            );
        } else {
            assert_eq!(updated.acceptance_criteria, previous.acceptance_criteria);
        }
        assert_eq!(updated.assignee, previous.assignee);
        assert_eq!(updated.labels, previous.labels);
        assert_eq!(updated.notes(), previous.notes());
        previous = updated;
    }
}

#[tokio::test]
async fn rejected_update_requests_preserve_state_and_obsolete_cli_flags_are_not_accepted() {
    let workspace = workspace();
    let tools = tools();
    set_context(&tools, workspace.path()).await;
    let issue = create_issue(&tools, "Strict fixture").await;
    let issue_id = issue.id.to_string();

    for key in [
        "status",
        "labels",
        "notes",
        "note",
        "assignee",
        "relationships",
        "resources",
        "issue_type",
        "unknown",
    ] {
        let alone = json!({"issue_id": issue_id, key: Value::Null});
        assert!(
            serde_json::from_value::<UpdateParams>(alone).is_err(),
            "obsolete/unknown key {key} must be rejected alone"
        );
        let mixed = json!({"issue_id": issue_id, "title": "Valid title", key: Value::Null});
        assert!(
            serde_json::from_value::<UpdateParams>(mixed).is_err(),
            "obsolete/unknown key {key} must be rejected beside a valid title"
        );
    }

    let before_bytes = read_bytes(workspace.path());
    let before = tools
        .show(&issue_id, None)
        .await
        .expect("fixture should be readable");
    let empty = tools
        .update(UpdateParams {
            issue_id: issue_id.clone(),
            ..Default::default()
        })
        .await;
    assert!(empty.is_err(), "empty MCP Update must fail");
    assert_eq!(read_bytes(workspace.path()), before_bytes);
    assert_eq!(
        serde_json::to_value(tools.show(&issue_id, None).await.unwrap()).unwrap(),
        serde_json::to_value(before).unwrap()
    );

    let cli_empty = run_cli(
        workspace.path(),
        &["update".into(), issue_id.clone(), "--json".into()],
    );
    assert!(!cli_empty.status.success(), "empty CLI Update must fail");
    assert_eq!(read_bytes(workspace.path()), before_bytes);

    for (flag, value) in [("--status", "closed"), ("--notes", "obsolete note")] {
        let cli_obsolete = run_cli(
            workspace.path(),
            &[
                "update".into(),
                issue_id.clone(),
                "--title".into(),
                "Should not apply".into(),
                flag.into(),
                value.into(),
            ],
        );
        assert!(
            !cli_obsolete.status.success(),
            "removed {flag} must be rejected"
        );
        assert_eq!(read_bytes(workspace.path()), before_bytes);
    }

    let positive = run_cli(
        workspace.path(),
        &[
            "update".into(),
            issue_id.clone(),
            "--title".into(),
            "CLI title".into(),
            "--json".into(),
        ],
    );
    assert!(
        positive.status.success(),
        "valid CLI Update should succeed: {}",
        String::from_utf8_lossy(&positive.stderr)
    );
    assert_eq!(
        read_record(workspace.path(), &issue_id)["title"],
        "CLI title"
    );
}

#[tokio::test]
async fn explicit_empty_text_is_distinct_from_omission() {
    let workspace = workspace();
    let tools = tools();
    set_context(&tools, workspace.path()).await;
    let issue = create_issue(&tools, "Text fixture").await;

    let cleared = tools
        .update(UpdateParams {
            issue_id: (issue.id.as_str()).to_string(),
            description: Some(String::new()),
            design: Some(String::new()),
            acceptance_criteria: Some(String::new()),
            ..Default::default()
        })
        .await
        .expect("explicit empty descriptive text should be accepted");
    assert_eq!(cleared.description, "");
    assert_eq!(cleared.design.as_deref(), Some(""));
    assert_eq!(cleared.acceptance_criteria.as_deref(), Some(""));

    let omitted = tools
        .update(UpdateParams {
            issue_id: (issue.id.as_str()).to_string(),
            title: Some(("Only title changed").to_string()),
            ..Default::default()
        })
        .await
        .expect("title-only update should succeed");
    assert_eq!(omitted.title, "Only title changed");
    assert_eq!(omitted.description, "");
    assert_eq!(omitted.design.as_deref(), Some(""));
    assert_eq!(omitted.acceptance_criteria.as_deref(), Some(""));
}

#[tokio::test]
async fn dedicated_intents_preserve_state_assignment_and_note_history() {
    let workspace = workspace();
    let tools = tools();
    set_context(&tools, workspace.path()).await;
    let issue = create_issue(&tools, "Lifecycle fixture").await;
    let id = issue.id.to_string();

    let unassigned_start = tools
        .start(LifecycleParams {
            issue_id: id.clone(),
            workspace_root: None,
        })
        .await;
    assert!(unassigned_start.is_err(), "Start must require Assignment");

    let claimed = tools
        .claim(&id, "alice", None)
        .await
        .expect("fixture should be claimed");
    assert_eq!(claimed.assignee.as_deref(), Some("alice"));
    let started = tools
        .start(LifecycleParams {
            issue_id: id.clone(),
            workspace_root: None,
        })
        .await
        .expect("assigned Issue should start");
    assert_eq!(started.status, IssueStatus::InProgress);
    assert_eq!(started.assignee.as_deref(), Some("alice"));

    let returned = tools
        .return_to_open(LifecycleParams {
            issue_id: id.clone(),
            workspace_root: None,
        })
        .await
        .expect("In Progress Issue should return to Open");
    assert_eq!(returned.status, IssueStatus::Open);
    assert_eq!(returned.assignee.as_deref(), Some("alice"));

    let with_note = tools
        .add_note(&id, "repeated note".to_string(), None)
        .await
        .expect("Append Note should succeed");
    let repeated = tools
        .add_note(&id, "repeated note".to_string(), None)
        .await
        .expect("duplicate Note content should remain append-only");
    assert_eq!(repeated.notes().len(), with_note.notes().len() + 1);
    assert_eq!(repeated.notes().last().unwrap().content(), "repeated note");

    let closed = tools
        .close(&id, Some("completed".to_string()), None)
        .await
        .expect("Open Issue should close");
    assert_eq!(closed.status, IssueStatus::Closed);
    assert_eq!(closed.assignee, None);
    assert_eq!(
        closed.notes().last().unwrap().content(),
        "Closed: completed"
    );

    let closed_return = tools
        .return_to_open(LifecycleParams {
            issue_id: id.clone(),
            workspace_root: None,
        })
        .await;
    assert!(
        closed_return.is_err(),
        "ReturnToOpen must not reopen Closed"
    );
    let still_closed = tools
        .show(&id, None)
        .await
        .expect("Issue should remain closed");
    assert_eq!(still_closed.status, IssueStatus::Closed);
    assert_eq!(still_closed.notes(), closed.notes());

    let reopened = tools
        .reopen(&id, Some("needs more work".to_string()), None)
        .await
        .expect("Closed Issue should reopen");
    assert_eq!(reopened.status, IssueStatus::Open);
    assert_eq!(reopened.assignee, None);
    assert_eq!(
        reopened.notes().last().unwrap().content(),
        "Reopened: needs more work"
    );
}

#[tokio::test]
async fn update_field_boundaries_match_cli_and_mcp() {
    for (length, accepted) in [(199_usize, true), (200, true), (201, false)] {
        let title = "x".repeat(length);

        let mcp_workspace = workspace();
        let mcp_tools = tools();
        set_context(&mcp_tools, mcp_workspace.path()).await;
        let mcp_issue = create_issue(&mcp_tools, "MCP boundary fixture").await;
        let mcp_before = read_bytes(mcp_workspace.path());
        let mcp_result = mcp_tools
            .update(UpdateParams {
                issue_id: (mcp_issue.id.as_str()).to_string(),
                title: Some(title.clone()),
                ..Default::default()
            })
            .await;
        assert_eq!(mcp_result.is_ok(), accepted, "MCP title length {length}");
        if accepted {
            assert_eq!(mcp_result.expect("accepted MCP title").title, title);
        } else {
            assert_eq!(read_bytes(mcp_workspace.path()), mcp_before);
        }

        let cli_workspace = workspace();
        let cli_tools = tools();
        set_context(&cli_tools, cli_workspace.path()).await;
        let cli_issue = create_issue(&cli_tools, "CLI boundary fixture").await;
        let cli_before = read_bytes(cli_workspace.path());
        let cli = run_cli(
            cli_workspace.path(),
            &update_cli_args(&[cli_issue.id.as_str()], &title),
        );
        assert_eq!(cli.status.success(), accepted, "CLI title length {length}");
        if accepted {
            assert_eq!(
                read_record(cli_workspace.path(), cli_issue.id.as_str())["title"],
                title
            );
        } else {
            assert_eq!(read_bytes(cli_workspace.path()), cli_before);
        }
    }
}

fn assert_expected_batch_update(record: &Value, expected_title: &str) {
    assert_eq!(record["title"], expected_title);
    assert_eq!(record["description"], "Original description");
    assert_eq!(record["priority"], 2);
    assert_eq!(record["labels"], json!(["keep-me"]));
    assert_eq!(record["notes"][0]["content"], "Existing note");
}

#[tokio::test]
async fn cli_update_batch_matches_ordered_single_target_mcp_calls() {
    let missing = "test-missing";
    let cli_workspace = workspace();
    let cli_tools = tools();
    set_context(&cli_tools, cli_workspace.path()).await;
    let cli_first = create_issue(&cli_tools, "Batch first").await;
    let cli_second = create_issue(&cli_tools, "Batch second").await;
    let cli_first_id = cli_first.id.to_string();
    let cli_second_id = cli_second.id.to_string();

    let cli = run_cli(
        cli_workspace.path(),
        &update_cli_args(
            &[
                cli_first_id.as_str(),
                missing,
                cli_second_id.as_str(),
                cli_first_id.as_str(),
            ],
            "Batch update",
        ),
    );
    assert!(
        !cli.status.success(),
        "mixed Update batch must report the missing target"
    );
    assert_expected_batch_update(
        &read_record(cli_workspace.path(), &cli_first_id),
        "Batch update",
    );
    assert_expected_batch_update(
        &read_record(cli_workspace.path(), &cli_second_id),
        "Batch update",
    );

    let mcp_workspace = workspace();
    let mcp_tools = tools();
    set_context(&mcp_tools, mcp_workspace.path()).await;
    let mcp_first = create_issue(&mcp_tools, "Batch first").await;
    let mcp_second = create_issue(&mcp_tools, "Batch second").await;
    let mcp_first_id = mcp_first.id.to_string();
    let mcp_second_id = mcp_second.id.to_string();
    for target in [
        mcp_first_id.as_str(),
        missing,
        mcp_second_id.as_str(),
        mcp_first_id.as_str(),
    ] {
        let result = mcp_tools
            .update(UpdateParams {
                issue_id: (target).to_string(),
                title: Some(("Batch update").to_string()),
                ..Default::default()
            })
            .await;
        if target == missing {
            assert!(result.is_err(), "missing target should fail independently");
        } else {
            result.expect("valid repeated target should update");
        }
    }
    assert_expected_batch_update(
        &read_record(mcp_workspace.path(), &mcp_first_id),
        "Batch update",
    );
    assert_expected_batch_update(
        &read_record(mcp_workspace.path(), &mcp_second_id),
        "Batch update",
    );
}

#[tokio::test]
async fn cli_note_batch_matches_ordered_single_target_mcp_calls() {
    let cli_workspace = workspace();
    let cli_tools = tools();
    set_context(&cli_tools, cli_workspace.path()).await;
    let cli_first = create_issue(&cli_tools, "Batch first").await;
    let cli_second = create_issue(&cli_tools, "Batch second").await;
    let cli_first_id = cli_first.id.to_string();
    let cli_second_id = cli_second.id.to_string();
    let missing = "test-missing".to_string();

    let cli = run_cli(
        cli_workspace.path(),
        &[
            "note".into(),
            "append".into(),
            cli_first_id.clone(),
            missing.clone(),
            cli_second_id.clone(),
            cli_first_id.clone(),
            "--content".into(),
            "batch note".into(),
        ],
    );
    assert!(
        !cli.status.success(),
        "mixed batch must report the missing target"
    );
    let cli_reloaded = tools();
    set_context(&cli_reloaded, cli_workspace.path()).await;
    let cli_first_record = cli_reloaded.show(&cli_first_id, None).await.unwrap();
    let cli_second_record = cli_reloaded.show(&cli_second_id, None).await.unwrap();
    assert_eq!(
        cli_first_record.notes().last().unwrap().content(),
        "batch note"
    );
    assert_eq!(cli_first_record.notes().len(), 3);
    assert_eq!(
        cli_second_record.notes().last().unwrap().content(),
        "batch note"
    );
    assert_eq!(cli_second_record.notes().len(), 2);

    let mcp_workspace = workspace();
    let mcp_tools = tools();
    set_context(&mcp_tools, mcp_workspace.path()).await;
    let mcp_first = create_issue(&mcp_tools, "Batch first").await;
    let mcp_second = create_issue(&mcp_tools, "Batch second").await;
    let mcp_first_id = mcp_first.id.to_string();
    let mcp_second_id = mcp_second.id.to_string();
    for target in [
        mcp_first_id.as_str(),
        missing.as_str(),
        mcp_second_id.as_str(),
        mcp_first_id.as_str(),
    ] {
        let result = mcp_tools
            .add_note(target, "batch note".to_string(), None)
            .await;
        if target == missing {
            assert!(result.is_err(), "missing target should fail independently");
        } else {
            result.expect("valid repeated target should append");
        }
    }
    let mcp_first_record = mcp_tools.show(&mcp_first_id, None).await.unwrap();
    let mcp_second_record = mcp_tools.show(&mcp_second_id, None).await.unwrap();
    assert_eq!(
        mcp_first_record.notes().len(),
        cli_first_record.notes().len()
    );
    assert_eq!(
        mcp_second_record.notes().len(),
        cli_second_record.notes().len()
    );
    assert_eq!(
        mcp_first_record.notes().last().unwrap().content(),
        "batch note"
    );
    assert_eq!(
        mcp_second_record.notes().last().unwrap().content(),
        "batch note"
    );
}

#[test]
#[ignore = "large persistence fixture is an explicit scale fence"]
fn scale_fixture_updates_100_targets_in_10000_issue_workspace() {
    let workspace = workspace();
    let source = jsonl_path(workspace.path());
    let mut bytes = Vec::new();
    for index in 0..10_000 {
        let id = format!("test-scale-{index:05}");
        let record = json!({
            "id": id,
            "title": format!("Scale fixture {index}"),
            "description": "scale description",
            "status": "open",
            "priority": 2,
            "issue_kind": "task",
            "assignee": null,
            "labels": [],
            "design": null,
            "acceptance_criteria": null,
            "notes": [],
            "resources": [],
            "dependencies": [],
            "created_at": "2020-01-01T00:00:00Z",
            "updated_at": "2020-01-01T00:00:00Z",
            "closed_at": null,
        });
        serde_json::to_writer(&mut bytes, &record).expect("scale record should serialize");
        bytes.push(b'\n');
    }
    std::fs::write(&source, bytes).expect("scale JSONL should be written");
    let before_records = read_records(workspace.path());
    assert_eq!(before_records.len(), 10_000);

    let mut args = vec!["update".to_string()];
    args.extend((0..100).map(|index| format!("test-scale-{index:05}")));
    args.extend(["--title".to_string(), "Batch scale update".to_string()]);
    let output = run_cli(workspace.path(), &args);
    assert!(
        output.status.success(),
        "100-target scale batch should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let after_records = read_records(workspace.path());
    assert_eq!(after_records.len(), 10_000);
    let without_timestamp = |mut record: Value| {
        record
            .as_object_mut()
            .expect("scale record should be an object")
            .remove("updated_at");
        record
    };
    for index in 0..100 {
        let mut expected = before_records[index].clone();
        expected["title"] = json!("Batch scale update");
        assert_eq!(
            without_timestamp(after_records[index].clone()),
            without_timestamp(expected),
            "target record {index} should change only in the requested fields and timestamp"
        );
    }
    for index in 100..10_000 {
        assert_eq!(
            after_records[index], before_records[index],
            "unrelated record {index} must remain byte-equivalent after the batch"
        );
    }
}
