//! Reporting parity through the real CLI and registered MCP protocol tools.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Output, Stdio};
use tempfile::TempDir;

struct Fixture {
    workspace: TempDir,
    config_path: PathBuf,
    data_path: PathBuf,
    original: Vec<u8>,
}

impl Fixture {
    fn mixed() -> Self {
        let mut parent = issue_record("rep-parent", "open", 3, None, &[]);
        parent["issue_kind"] = json!("epic");
        Self::new(vec![
            issue_record("rep-open", "open", 0, None, &[]),
            issue_record("rep-assigned", "open", 1, Some("alice"), &[]),
            issue_record("rep-active", "in_progress", 2, Some("bob"), &[]),
            issue_record("rep-closed", "closed", 3, None, &[]),
            issue_record(
                "rep-dependent",
                "open",
                4,
                None,
                &[("rep-prereq", "blocks")],
            ),
            issue_record("rep-prereq", "open", 0, None, &[]),
            issue_record(
                "rep-related",
                "open",
                2,
                None,
                &[
                    ("rep-prereq", "related"),
                    ("rep-parent", "parent-child"),
                    ("rep-prereq", "discovered-from"),
                ],
            ),
            parent,
            issue_record("rep-discovery", "open", 4, None, &[]),
        ])
    }

    fn empty() -> Self {
        Self::new(Vec::new())
    }

    fn new(records: Vec<Value>) -> Self {
        let workspace = tempfile::Builder::new()
            .prefix("reporting ü ")
            .tempdir()
            .expect("reporting workspace");
        let root = workspace.path();
        let rivets_dir = root.join(".rivets");
        let data_dir = root.join("data");
        std::fs::create_dir(&rivets_dir).expect(".rivets directory");
        std::fs::create_dir(&data_dir).expect("configured data parent");

        let config_path = rivets_dir.join("config.yaml");
        std::fs::write(
            &config_path,
            "issue-prefix: rep\nstorage:\n  backend: jsonl\n  data_file: data/issues.jsonl\n",
        )
        .expect("workspace config");

        let data_path = data_dir.join("issues.jsonl");
        let mut original = Vec::new();
        for record in records {
            serde_json::to_writer(&mut original, &record).expect("fixture record JSON");
            original.push(b'\n');
        }
        std::fs::write(&data_path, &original).expect("fixture records");

        Self {
            workspace,
            config_path,
            data_path,
            original,
        }
    }

    fn root(&self) -> &Path {
        self.workspace.path()
    }

    fn expected_info(&self) -> Value {
        json!({
            "workspace_root": self.root().canonicalize().expect("canonical root").display().to_string(),
            "database_path": self.data_path.canonicalize().expect("canonical data path").display().to_string(),
            "config_path": self.config_path.canonicalize().expect("canonical config path").display().to_string(),
            "storage_backend": "jsonl",
            "issue_prefix": "rep",
        })
    }

    fn assert_source_unchanged(&self) {
        assert_eq!(
            std::fs::read(&self.data_path).expect("persisted records"),
            self.original,
            "read-only reports must not rewrite persisted records",
        );
    }
}

fn issue_record(
    id: &str,
    status: &str,
    priority: u8,
    assignee: Option<&str>,
    dependencies: &[(&str, &str)],
) -> Value {
    let closed_at = (status == "closed").then_some("2020-01-02T00:00:00Z");
    json!({
        "id": id,
        "title": format!("Reporting fixture {id}"),
        "description": "Independent reporting fixture",
        "status": status,
        "priority": priority,
        "issue_kind": "task",
        "assignee": assignee,
        "labels": [],
        "design": null,
        "acceptance_criteria": null,
        "notes": [],
        "resources": [],
        "dependencies": dependencies
            .iter()
            .map(|(depends_on_id, dep_type)| json!({
                "depends_on_id": depends_on_id,
                "dep_type": dep_type,
            }))
            .collect::<Vec<_>>(),
        "created_at": "2020-01-01T00:00:00Z",
        "updated_at": "2020-01-02T00:00:00Z",
        "closed_at": closed_at,
    })
}

fn expected_mixed_initial() -> Value {
    json!({
        "total": 9,
        "by_status": {"open": 7, "in_progress": 1, "closed": 1},
        "ready": 6,
        "blocked_by_dependencies": 1,
        "by_priority": {
            "p0_critical": 2,
            "p1_high": 1,
            "p2_medium": 2,
            "p3_low": 2,
            "p4_backlog": 2,
        },
    })
}

fn expected_mixed_after_close() -> Value {
    json!({
        "total": 9,
        "by_status": {"open": 6, "in_progress": 1, "closed": 2},
        "ready": 6,
        "blocked_by_dependencies": 0,
        "by_priority": {
            "p0_critical": 2,
            "p1_high": 1,
            "p2_medium": 2,
            "p3_low": 2,
            "p4_backlog": 2,
        },
    })
}

fn expected_empty() -> Value {
    json!({
        "total": 0,
        "by_status": {"open": 0, "in_progress": 0, "closed": 0},
        "ready": 0,
        "blocked_by_dependencies": 0,
        "by_priority": {
            "p0_critical": 0,
            "p1_high": 0,
            "p2_medium": 0,
            "p3_low": 0,
            "p4_backlog": 0,
        },
    })
}

fn run_cli(workspace: &Path, args: &[&str]) -> Output {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace manifest parent")
        .join("Cargo.toml");
    Command::new(env!("CARGO"))
        .args(["run", "--quiet", "--manifest-path"])
        .arg(manifest)
        .args(["-p", "rivets", "--"])
        .args(args)
        .current_dir(workspace)
        .output()
        .expect("real CLI process")
}

struct McpProcess {
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
    next_id: u64,
}

impl McpProcess {
    fn new(workspace: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_rivets-mcp"))
            .current_dir(workspace)
            .env("RUST_LOG", "error")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("real MCP process");
        let stdin = child.stdin.take().expect("MCP stdin");
        let stdout = child.stdout.take().expect("MCP stdout");
        let mut process = Self {
            child,
            stdin,
            reader: BufReader::new(stdout),
            next_id: 0,
        };

        process.request(
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "reporting-parity-test", "version": "1"},
            }),
        );
        process.notify("notifications/initialized", json!({}));
        process
    }

    fn notify(&mut self, method: &str, params: Value) {
        let mut request = json!({"jsonrpc": "2.0", "method": method});
        request["params"] = params;
        writeln!(self.stdin, "{request}").expect("MCP notification");
        self.stdin.flush().expect("MCP notification flush");
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        let mut request = json!({"jsonrpc": "2.0", "id": id, "method": method});
        request["params"] = params;
        writeln!(self.stdin, "{request}").expect("MCP request");
        self.stdin.flush().expect("MCP request flush");

        loop {
            let mut line = String::new();
            let read = self.reader.read_line(&mut line).expect("MCP response");
            assert!(read > 0, "MCP process exited before response to {method}");
            if line.trim().is_empty() {
                continue;
            }
            let response: Value = serde_json::from_str(&line).expect("MCP response JSON");
            if response.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = response.get("error") {
                panic!("MCP {method} returned protocol error: {error}");
            }
            return response
                .get("result")
                .cloned()
                .expect("MCP result envelope");
        }
    }

    fn call(&mut self, name: &str, arguments: Value) -> Value {
        let mut params = json!({"name": name});
        params["arguments"] = arguments;
        let result = self.request("tools/call", params);
        let text = result["content"]
            .as_array()
            .and_then(|content| content.first())
            .and_then(|content| content["text"].as_str())
            .unwrap_or_else(|| panic!("MCP {name} did not return text JSON: {result}"));
        serde_json::from_str(text).expect("MCP tool JSON payload")
    }
}

impl Drop for McpProcess {
    fn drop(&mut self) {
        if let Err(error) = self.child.kill()
            && error.kind() != std::io::ErrorKind::InvalidInput
        {
            eprintln!("failed to stop MCP process: {error}");
        }
        if let Err(error) = self.child.wait() {
            eprintln!("failed to reap MCP process: {error}");
        }
    }
}

fn assert_cli_json(workspace: &Path, command: &str, expected: &Value) {
    let output = run_cli(workspace, &["--json", command]);
    assert!(
        output.status.success(),
        "CLI {command} failed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    let actual: Value = serde_json::from_slice(&output.stdout).expect("CLI report JSON");
    assert_eq!(
        &actual, expected,
        "CLI {command} differs from literal oracle"
    );
}

#[test]
fn reporting_matches_literal_oracle_through_cli_and_registered_mcp() {
    let fixture = Fixture::mixed();
    let root = fixture.root().display().to_string();
    let expected_info = fixture.expected_info();
    let expected_stats = expected_mixed_initial();
    let mut mcp = McpProcess::new(fixture.root());

    let context = mcp.call("set_context", json!({"workspace_root": root.clone()}));
    assert_eq!(context["workspace_root"], expected_info["workspace_root"]);
    assert_eq!(context["database_path"], expected_info["database_path"]);

    let where_am_i = mcp.call("where_am_i", json!({}));
    assert_eq!(
        where_am_i,
        json!({
            "workspace_root": expected_info["workspace_root"],
            "database_path": expected_info["database_path"],
            "context_set": true,
            "issue_prefix": "rep",
        }),
        "where_am_i must remain a separate context inspection route",
    );

    assert_cli_json(fixture.root(), "info", &expected_info);
    assert_cli_json(fixture.root(), "stats", &expected_stats);
    assert_eq!(mcp.call("info", json!({})), expected_info);
    assert_eq!(
        mcp.call("info", json!({"workspace_root": root.clone()})),
        expected_info,
        "explicit info root must use the same initialized workspace",
    );
    assert_eq!(mcp.call("stats", json!({})), expected_stats);
    assert_eq!(
        mcp.call("stats", json!({"workspace_root": root.clone()})),
        expected_stats,
        "explicit stats root must use the same initialized workspace",
    );
    fixture.assert_source_unchanged();

    mcp.call(
        "close",
        json!({"issue_id": "rep-prereq", "workspace_root": root.clone()}),
    );
    let expected_after_close = expected_mixed_after_close();
    let after_close = std::fs::read(&fixture.data_path).expect("closed records");
    assert_cli_json(fixture.root(), "stats", &expected_after_close);
    assert_eq!(mcp.call("stats", json!({})), expected_after_close);
    assert_eq!(
        std::fs::read(&fixture.data_path).expect("closed records"),
        after_close,
        "stats reports must not rewrite records after a lifecycle transition",
    );

    mcp.call(
        "reopen",
        json!({"issue_id": "rep-prereq", "workspace_root": root.clone()}),
    );
    let after_reopen = std::fs::read(&fixture.data_path).expect("reopened records");
    assert_cli_json(fixture.root(), "stats", &expected_stats);
    assert_eq!(mcp.call("stats", json!({})), expected_stats);
    assert_eq!(
        std::fs::read(&fixture.data_path).expect("reopened records"),
        after_reopen,
        "stats reports must remain read-only after reopening a prerequisite",
    );
}

#[test]
fn empty_reporting_contains_every_zero_bucket() {
    let fixture = Fixture::empty();
    let expected_info = fixture.expected_info();
    let expected_stats = expected_empty();
    let root = fixture.root().display().to_string();
    let mut mcp = McpProcess::new(fixture.root());

    assert_eq!(
        mcp.call("info", json!({"workspace_root": root.clone()})),
        expected_info,
        "explicit info initializes without changing the current context",
    );
    assert_eq!(
        mcp.call("stats", json!({"workspace_root": root})),
        expected_stats,
        "explicit stats initializes without changing the current context",
    );
    assert_eq!(
        mcp.call("where_am_i", json!({})),
        json!({
            "workspace_root": null,
            "database_path": null,
            "context_set": false,
            "issue_prefix": null,
        }),
        "explicit report roots must not set current context",
    );
    assert_cli_json(fixture.root(), "info", &expected_info);
    assert_cli_json(fixture.root(), "stats", &expected_stats);
    fixture.assert_source_unchanged();
}

#[test]
fn cached_mcp_report_keeps_initialized_configuration_snapshot() {
    let fixture = Fixture::mixed();
    let expected_info = fixture.expected_info();
    let expected_stats = expected_mixed_initial();
    let mut mcp = McpProcess::new(fixture.root());
    mcp.call(
        "set_context",
        json!({"workspace_root": fixture.root().display().to_string()}),
    );

    std::fs::write(
        &fixture.config_path,
        "issue-prefix: newer\nstorage:\n  backend: jsonl\n  data_file: data/replaced.jsonl\n",
    )
    .expect("changed config");

    assert_eq!(
        mcp.call("info", json!({})),
        expected_info,
        "cached info must describe the configuration used to initialize storage",
    );
    assert_eq!(mcp.call("stats", json!({})), expected_stats);
    let where_am_i = mcp.call("where_am_i", json!({}));
    assert_eq!(
        where_am_i["workspace_root"],
        expected_info["workspace_root"]
    );
    assert_eq!(where_am_i["database_path"], expected_info["database_path"]);
    fixture.assert_source_unchanged();
}

#[tokio::test]
async fn information_survives_concurrent_cache_eviction() {
    use rivets_mcp::{context::Context, tools::Tools};
    use std::sync::Arc;
    use tokio::sync::{Barrier, RwLock};
    use tokio::task::JoinSet;

    // More simultaneous explicit roots than the 32-entry cache, with no
    // protected current context. Every caller must retain its own report.
    let fixtures: Vec<_> = (0..64).map(|_| Fixture::empty()).collect();
    let tools = Arc::new(Tools::new(Arc::new(RwLock::new(Context::new()))));
    let start = Arc::new(Barrier::new(fixtures.len()));
    let mut tasks = JoinSet::new();
    for fixture in &fixtures {
        let tools = Arc::clone(&tools);
        let start = Arc::clone(&start);
        let root = fixture.root().display().to_string();
        let expected = fixture.expected_info();
        tasks.spawn(async move {
            start.wait().await;
            let report = tools
                .info(Some(root))
                .await
                .expect("valid Workspace report");
            assert_eq!(serde_json::to_value(report).unwrap(), expected);
        });
    }
    while let Some(result) = tasks.join_next().await {
        result.expect("report task");
    }
}
