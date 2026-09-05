//! Consumer-level List/Stale contracts against real JSONL, CLI processes and MCP Tools.

use rivets_mcp::context::Context;
use rivets_mcp::models::{ListParams, StaleParams};
use rivets_mcp::tools::Tools;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Arc;
use tempfile::TempDir;
use tokio::sync::RwLock;

struct Fixture {
    workspace: TempDir,
    tools: Tools,
    records: Vec<Value>,
    original: Vec<u8>,
}

impl Fixture {
    async fn new() -> Self {
        let workspace = TempDir::new().expect("query Workspace");
        let dir = workspace.path().join(".rivets");
        std::fs::create_dir(&dir).expect("Workspace directory");
        std::fs::write(
            dir.join("config.yaml"),
            "issue-prefix: test\nstorage:\n  backend: jsonl\n  data_file: .rivets/issues.jsonl\n",
        )
        .expect("Workspace config");
        // Insertion order deliberately disagrees with both canonical orders.
        let rows = [
            (
                "test-d",
                "open",
                4,
                "chore",
                Some("owner"),
                false,
                "2020-03-01T00:00:00Z",
                "2020-04-01T00:00:00Z",
            ),
            (
                "test-c",
                "open",
                2,
                "task",
                None,
                true,
                "2020-04-01T00:00:00Z",
                "2020-03-01T00:00:00Z",
            ),
            (
                "test-b",
                "in_progress",
                0,
                "feature",
                Some("owner"),
                false,
                "2020-04-01T00:00:00Z",
                "2020-02-01T00:00:00Z",
            ),
            (
                "test-a",
                "open",
                2,
                "task",
                Some("Åda Space"),
                true,
                "2020-05-01T00:00:00Z",
                "2020-03-01T00:00:00Z",
            ),
            (
                "test-z",
                "closed",
                1,
                "bug",
                None,
                true,
                "2020-06-01T00:00:00Z",
                "2020-01-01T00:00:00Z",
            ),
            (
                "test-future",
                "open",
                2,
                "epic",
                None,
                true,
                "2099-01-01T00:00:00Z",
                "2099-01-01T00:00:00Z",
            ),
        ];
        let records: Vec<Value> = rows.into_iter().map(|(id, status, priority, kind, assignee, label, created, updated)| json!({
            "id": id, "title": format!("Query fixture {id}"), "description": "Unicode Ω, spaces, and complete wire records",
            "status": status, "priority": priority, "issue_kind": kind, "assignee": assignee,
            "labels": if label { vec!["triage"] } else { vec!["other"] },
            "design": null, "acceptance_criteria": null, "notes": [], "resources": [], "dependencies": [],
            "created_at": created, "updated_at": updated,
            "closed_at": if status == "closed" { Some(updated) } else { None },
        })).collect();
        let mut original = Vec::new();
        for record in &records {
            serde_json::to_writer(&mut original, record).expect("fixture record JSON");
            original.push(b'\n');
        }
        std::fs::write(dir.join("issues.jsonl"), &original).expect("query records");
        let tools = Tools::new(Arc::new(RwLock::new(Context::new())));
        tools
            .set_context(workspace.path().to_str().expect("UTF-8 Workspace"))
            .await
            .expect("MCP context");
        Self {
            workspace,
            tools,
            records,
            original,
        }
    }

    fn expected(&self, ids: &[&str]) -> Value {
        Value::Array(
            ids.iter()
                .map(|id| {
                    let mut record = self
                        .records
                        .iter()
                        .find(|record| record["id"] == *id)
                        .expect("literal oracle ID")
                        .clone();
                    record
                        .as_object_mut()
                        .expect("record object")
                        .remove("dependencies");
                    record
                })
                .collect(),
        )
    }

    fn cli(&self, intent: &str, params: &Value) -> Output {
        let mut args = vec![intent.to_string(), "--json".to_string()];
        for (field, value) in params.as_object().expect("query request") {
            if field == "workspace_root" || value.is_null() {
                continue;
            }
            let cli_field = if field == "issue_kind" { "kind" } else { field };
            args.push(format!("--{cli_field}"));
            args.push(
                value
                    .as_str()
                    .map_or_else(|| value.to_string(), str::to_string),
            );
        }
        run_cli(self.workspace.path(), &args)
    }

    async fn mcp(&self, intent: &str, params: Value) -> Result<Value, String> {
        let issues = match intent {
            "list" => {
                self.tools
                    .list(serde_json::from_value::<ListParams>(params).map_err(|e| e.to_string())?)
                    .await
            }
            "stale" => {
                let params: StaleParams =
                    serde_json::from_value(params).map_err(|e| e.to_string())?;
                self.tools
                    .stale(
                        params.days,
                        params.status.as_deref(),
                        params.limit,
                        params.workspace_root.as_deref(),
                    )
                    .await
            }
            _ => panic!("test intent must be List or Stale"),
        }
        .map_err(|e| e.to_string())?;
        serde_json::to_value(issues).map_err(|e| e.to_string())
    }

    async fn assert_query(&self, intent: &str, params: Value, ids: &[&str]) {
        let output = self.cli(intent, &params);
        assert!(
            output.status.success(),
            "{intent} {params}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let cli: Value = serde_json::from_slice(&output.stdout).expect("CLI query JSON");
        let mcp = self.mcp(intent, params.clone()).await.expect("MCP query");
        let expected = self.expected(ids);
        assert_eq!(
            cli, expected,
            "CLI {intent} {params} differs from literal oracle"
        );
        assert_eq!(
            mcp, expected,
            "MCP {intent} {params} differs from literal oracle"
        );
        assert_eq!(
            std::fs::read(self.workspace.path().join(".rivets/issues.jsonl"))
                .expect("persisted records"),
            self.original,
            "queries must remain read-only"
        );
    }
}

// Same real-CLI launch seam as workspace_lock.rs, not a duplicate query implementation.
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
        .expect("real CLI process")
}

#[tokio::test]
async fn list_and_stale_semantics() {
    let fixture = Fixture::new().await;
    fixture
        .assert_query(
            "list",
            json!({"limit": 100}),
            &[
                "test-future",
                "test-z",
                "test-a",
                "test-b",
                "test-c",
                "test-d",
            ],
        )
        .await;
    fixture
        .assert_query(
            "list",
            json!({"limit": 4}),
            &["test-future", "test-z", "test-a", "test-b"],
        )
        .await;
    fixture
        .assert_query("stale", json!({"limit": 1}), &["test-b"])
        .await;
    fixture
        .assert_query(
            "stale",
            json!({"limit": 3}),
            &["test-b", "test-a", "test-c"],
        )
        .await;
    // Explicit Workspace selection is an MCP mechanic, not a result-policy change.
    fixture
        .assert_query(
            "stale",
            json!({"limit": 2, "workspace_root": fixture.workspace.path().to_str().unwrap()}),
            &["test-b", "test-a"],
        )
        .await;
}

#[tokio::test]
async fn stale_contract() {
    let fixture = Fixture::new().await;
    fixture
        .assert_query(
            "stale",
            json!({"limit": 100}),
            &["test-b", "test-a", "test-c", "test-d"],
        )
        .await;
    fixture
        .assert_query(
            "stale",
            json!({"limit": 100, "days": 0}),
            &["test-b", "test-a", "test-c", "test-d"],
        )
        .await;
    fixture
        .assert_query(
            "stale",
            json!({"limit": 100, "status": "closed"}),
            &["test-z"],
        )
        .await;
    fixture
        .assert_query(
            "stale",
            json!({"limit": 100, "status": "in_progress"}),
            &["test-b"],
        )
        .await;
    fixture
        .assert_query(
            "stale",
            json!({"limit": 100, "status": "open", "days": 30}),
            &["test-a", "test-c", "test-d"],
        )
        .await;
    for days in [json!(-1), json!(u32::MAX), json!(u64::MAX), json!(1.5)] {
        let params = json!({"limit": 1, "days": days});
        assert!(
            !fixture.cli("stale", &params).status.success(),
            "invalid age {days}"
        );
        assert!(
            fixture.mcp("stale", params).await.is_err(),
            "invalid age {days}"
        );
    }
}

#[tokio::test]
async fn canonical_workflow_filters() {
    let fixture = Fixture::new().await;
    for (state, ids) in [
        ("open", vec!["test-future", "test-a", "test-c", "test-d"]),
        ("in_progress", vec!["test-b"]),
        ("closed", vec!["test-z"]),
    ] {
        fixture
            .assert_query("list", json!({"limit": 100, "status": state}), &ids)
            .await;
    }
    for intent in ["list", "stale"] {
        for state in ["blocked", "in-progress", "", "OPEN", "unknown"] {
            let params = json!({"limit": 100, "status": state});
            let cli = fixture.cli(intent, &params);
            assert!(!cli.status.success(), "{intent} accepted state {state:?}");
            assert!(String::from_utf8_lossy(&cli.stderr).contains("status"));
            assert!(
                fixture
                    .mcp(intent, params)
                    .await
                    .expect_err("invalid state")
                    .to_lowercase()
                    .contains("status")
            );
        }
    }
}

#[tokio::test]
async fn explicit_limits() {
    let fixture = Fixture::new().await;
    for intent in ["list", "stale"] {
        for params in [
            json!({}),
            json!({"limit": 0}),
            json!({"limit": -1}),
            json!({"limit": 1.5}),
            json!({"limit": "not-a-limit"}),
        ] {
            let cli = fixture.cli(intent, &params);
            assert!(!cli.status.success(), "CLI {intent} accepted {params}");
            assert!(String::from_utf8_lossy(&cli.stderr).contains("limit"));
            assert!(
                fixture.mcp(intent, params.clone()).await.is_err(),
                "MCP {intent} accepted {params}"
            );
        }
    }
    fixture
        .assert_query("list", json!({"limit": 1}), &["test-future"])
        .await;
    fixture
        .assert_query("stale", json!({"limit": 1}), &["test-b"])
        .await;
    fixture
        .assert_query(
            "list",
            json!({"limit": usize::MAX}),
            &[
                "test-future",
                "test-z",
                "test-a",
                "test-b",
                "test-c",
                "test-d",
            ],
        )
        .await;
    assert!(
        !fixture
            .cli("list", &json!({"limit": 1, "sort": "priority"}))
            .status
            .success(),
        "alternate list ordering must be retired"
    );
}

#[tokio::test]
async fn shared_filter_contract() {
    let fixture = Fixture::new().await;
    // Truth masks are handwritten membership facts, not the production predicate.
    let ordered_masks = [
        ("test-future", 19_u8),
        ("test-z", 16),
        ("test-a", 31),
        ("test-b", 0),
        ("test-c", 23),
        ("test-d", 1),
    ];
    let selected = [
        ("status", json!("open")),
        ("priority", json!(2)),
        ("issue_kind", json!("task")),
        ("assignee", json!("Åda Space")),
        ("label", json!("triage")),
    ];
    for mask in 0..32_u8 {
        let mut params = json!({"limit": 2});
        for (bit, (field, value)) in selected.iter().enumerate() {
            if mask & (1 << bit) != 0 {
                params[field] = value.clone();
            }
        }
        let ids: Vec<&str> = ordered_masks
            .iter()
            .filter(|(_, membership)| membership & mask == mask)
            .take(2)
            .map(|(id, _)| *id)
            .collect();
        fixture.assert_query("list", params, &ids).await;
    }
    for (kind, ids) in [
        ("task", vec!["test-a", "test-c"]),
        ("feature", vec!["test-b"]),
        ("bug", vec!["test-z"]),
        ("chore", vec!["test-d"]),
        ("epic", vec!["test-future"]),
    ] {
        fixture
            .assert_query("list", json!({"limit": 100, "issue_kind": kind}), &ids)
            .await;
    }
    for (priority, ids) in [
        (0, vec!["test-b"]),
        (1, vec!["test-z"]),
        (2, vec!["test-future", "test-a", "test-c"]),
        (3, vec![]),
        (4, vec!["test-d"]),
    ] {
        fixture
            .assert_query("list", json!({"limit": 100, "priority": priority}), &ids)
            .await;
    }
    fixture
        .assert_query("list", json!({"limit": 100, "assignee": ""}), &[])
        .await;
    fixture
        .assert_query("list", json!({"limit": 100, "assignee": "nobody"}), &[])
        .await;
    for (field, value) in [
        ("priority", json!(5)),
        ("priority", json!(255)),
        ("priority", json!(-1)),
        ("priority", json!(256)),
        ("issue_kind", json!("invalid")),
        ("issue_kind", json!("")),
        ("label", json!("UPPER")),
        ("label", json!("")),
        ("label", json!("two--parts")),
        ("label", json!("é")),
        ("label", json!("x".repeat(51))),
    ] {
        let mut params = json!({"limit": 1});
        params[field] = value;
        assert!(
            !fixture.cli("list", &params).status.success(),
            "CLI accepted {params}"
        );
        assert!(
            fixture.mcp("list", params.clone()).await.is_err(),
            "MCP accepted {params}"
        );
    }
    assert!(
        fixture
            .mcp("list", json!({"limit": 1, "issue_type": "task"}))
            .await
            .is_err(),
        "hidden kind alias must not silently broaden the query"
    );
    assert_eq!(
        std::fs::read(fixture.workspace.path().join(".rivets/issues.jsonl")).expect("records"),
        fixture.original,
        "invalid queries must not mutate"
    );
}
