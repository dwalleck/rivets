//! Parent-scoped Ready behavior through the CLI and MCP adapters.

use rivets::domain::{Issue, ParentageError};
use rivets_mcp::{context::Context, error::Error, models::ReadyParams, tools::Tools};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::Arc,
};
use tokio::sync::RwLock;

fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".rivets")).unwrap();
    std::fs::write(
        root.path().join(".rivets/config.yaml"),
        "issue-prefix: test\nstorage:\n  backend: jsonl\n  data_file: .rivets/issues.jsonl\n",
    )
    .unwrap();
    let rows = [
        ("test-parent", "epic", 4, None, None, "open"),
        ("test-other", "epic", 4, None, None, "open"),
        ("test-empty", "epic", 4, None, None, "open"),
        ("test-outside", "task", 0, Some("test-other"), None, "open"),
        ("test-child", "task", 1, Some("test-parent"), None, "open"),
        (
            "test-assigned",
            "task",
            2,
            Some("test-parent"),
            Some("alice"),
            "open",
        ),
        ("test-nested", "epic", 3, Some("test-parent"), None, "open"),
        (
            "test-grandchild",
            "task",
            3,
            Some("test-nested"),
            None,
            "open",
        ),
        ("test-blocked", "task", 0, Some("test-parent"), None, "open"),
        (
            "test-closed",
            "task",
            0,
            Some("test-parent"),
            None,
            "closed",
        ),
        (
            "test-active",
            "task",
            0,
            Some("test-parent"),
            Some("alice"),
            "in_progress",
        ),
    ];
    let mut bytes = Vec::new();
    for (id, kind, priority, parent, assignee, status) in rows {
        let mut dependencies = Vec::new();
        if let Some(parent) = parent {
            dependencies.push(json!({"depends_on_id": parent, "dep_type": "parent-child"}));
        }
        if id == "test-blocked" || id == "test-parent" {
            dependencies.push(json!({"depends_on_id": "test-outside", "dep_type": "blocks"}));
        }
        serde_json::to_writer(&mut bytes, &json!({
            "id": id, "title": id, "description": "", "status": status, "priority": priority,
            "issue_kind": kind, "assignee": assignee, "labels": ["focus"], "design": null,
            "acceptance_criteria": null, "notes": [], "resources": [], "dependencies": dependencies,
            "created_at": format!("2020-01-01T00:00:0{priority}Z"), "updated_at": "2020-01-02T00:00:00Z",
            "closed_at": (status == "closed").then_some("2020-01-02T00:00:00Z"),
        })).unwrap();
        bytes.push(b'\n');
    }
    std::fs::write(root.path().join(".rivets/issues.jsonl"), bytes).unwrap();
    root
}

fn cli(root: &Path, args: &[&str]) -> Output {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("Cargo.toml");
    Command::new(env!("CARGO"))
        .args(["run", "--quiet", "--manifest-path"])
        .arg(manifest)
        .args(["-p", "rivets", "--"])
        .args(args)
        .current_dir(root)
        .output()
        .unwrap()
}

fn ids(issues: Vec<Issue>) -> Vec<String> {
    issues
        .into_iter()
        .map(|issue| issue.id.to_string())
        .collect()
}

fn cli_ids(root: &Path, args: &[&str]) -> Vec<String> {
    let output = cli(root, args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let issues: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
    issues
        .iter()
        .map(|issue| issue["id"].as_str().unwrap().to_owned())
        .collect()
}

fn params(value: Value) -> ReadyParams {
    serde_json::from_value(value).unwrap()
}

#[tokio::test]
async fn ready_parent_scope_contract() {
    let root = fixture();
    let tools = Tools::new(Arc::new(RwLock::new(Context::new())));
    tools
        .set_context(root.path().to_str().unwrap())
        .await
        .unwrap();
    let path = root.path().join(".rivets/issues.jsonl");
    let before = std::fs::read(&path).unwrap();
    assert_eq!(
        ids(tools
            .ready(params(json!({"parent_id":"test-parent"})))
            .await
            .unwrap()),
        ["test-child", "test-nested"]
    );
    let scoped =
        json!({"parent_id":"test-parent", "issue_kind":"task", "label":"focus", "limit":1});
    assert_eq!(
        ids(tools.ready(params(scoped.clone())).await.unwrap()),
        ["test-child"]
    );
    assert_eq!(
        cli_ids(
            root.path(),
            &[
                "ready",
                "--parent",
                "test-parent",
                "--kind",
                "task",
                "--label",
                "focus",
                "--limit",
                "1",
                "--json"
            ]
        ),
        ["test-child"]
    );
    assert_eq!(
        cli_ids(root.path(), &["ready", "--kind", "task", "--json"]),
        ["test-outside", "test-child", "test-grandchild"]
    );
    assert_eq!(
        ids(tools
            .ready(params(json!({"issue_kind":"task"})))
            .await
            .unwrap()),
        ["test-outside", "test-child", "test-grandchild"]
    );
    assert_eq!(
        ids(tools
            .ready(params(
                json!({"parent_id":"test-parent", "issue_kind":"task", "all_assignees":true})
            ))
            .await
            .unwrap()),
        ["test-child", "test-assigned"]
    );
    assert_eq!(
        cli_ids(
            root.path(),
            &[
                "ready",
                "--parent",
                "test-parent",
                "--kind",
                "task",
                "--assignee",
                "alice",
                "--json"
            ]
        ),
        ["test-assigned"]
    );
    assert_eq!(
        ids(tools
            .ready(params(json!({"parent_id":"test-empty"})))
            .await
            .unwrap()),
        Vec::<String>::new()
    );
    assert_eq!(
        cli_ids(root.path(), &["ready", "--parent", "test-empty", "--json"]),
        Vec::<String>::new()
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[tokio::test]
async fn ready_parent_tracks_parentage_changes() {
    let root = fixture();
    let tools = Tools::new(Arc::new(RwLock::new(Context::new())));
    tools
        .set_context(root.path().to_str().unwrap())
        .await
        .unwrap();
    let path = root.path().join(".rivets/issues.jsonl");
    let before = std::fs::read(&path).unwrap();
    let scoped =
        json!({"parent_id":"test-parent", "issue_kind":"task", "label":"focus", "limit":1});
    tools
        .parent_move("test-child", "test-other", None)
        .await
        .unwrap();
    let moved = std::fs::read(&path).unwrap();
    assert_ne!(moved, before, "fixture mutation is a positive control");
    assert_eq!(
        ids(tools.ready(params(scoped.clone())).await.unwrap()),
        Vec::<String>::new()
    );
    assert_eq!(
        cli_ids(
            root.path(),
            &[
                "ready",
                "--parent",
                "test-other",
                "--kind",
                "task",
                "--json"
            ]
        ),
        ["test-outside", "test-child"]
    );
    assert_eq!(std::fs::read(&path).unwrap(), moved);
    tools.parent_clear("test-child", None).await.unwrap();
    let cleared = std::fs::read(&path).unwrap();
    assert_eq!(
        ids(tools
            .ready(params(
                json!({"parent_id":"test-other", "issue_kind":"task"})
            ))
            .await
            .unwrap()),
        ["test-outside"]
    );
    assert_eq!(
        cli_ids(root.path(), &["ready", "--kind", "task", "--json"]),
        ["test-outside", "test-child", "test-grandchild"]
    );
    assert_eq!(std::fs::read(&path).unwrap(), cleared);
}

#[tokio::test]
async fn ready_parent_scope_errors() {
    let root = fixture();
    let tools = Tools::new(Arc::new(RwLock::new(Context::new())));
    tools
        .set_context(root.path().to_str().unwrap())
        .await
        .unwrap();
    let path = root.path().join(".rivets/issues.jsonl");
    let before = std::fs::read(&path).unwrap();
    for invalid in ["invalid", "", "test-par ent", "test-Ω"] {
        assert!(matches!(
            tools.ready(params(json!({"parent_id":invalid}))).await,
            Err(Error::InvalidIssueId(_))
        ));
        assert!(
            !cli(root.path(), &["ready", "--parent", invalid, "--json"])
                .status
                .success()
        );
    }
    assert!(
        matches!(tools.ready(params(json!({"parent_id":"test-missing", "limit":0}))).await, Err(Error::IssueNotFound(id)) if id == "test-missing")
    );
    assert!(matches!(
        tools
            .ready(params(json!({"parent_id":"test-child", "limit":0})))
            .await,
        Err(Error::InvalidParentage(
            ParentageError::ParentNotEpic { .. }
        ))
    ));
    for rejected in ["test-missing", "test-child"] {
        assert!(
            !cli(
                root.path(),
                &["ready", "--parent", rejected, "--limit", "0", "--json"]
            )
            .status
            .success()
        );
    }
    assert_eq!(std::fs::read(&path).unwrap(), before);
}
