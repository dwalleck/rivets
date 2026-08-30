//! Real CLI mutation transactions under the durable Workspace lock.

use rivets::commands::init;
use rivets::error::Error;
use rivets::workspace_lock::WorkspaceMutationLock;
use serde_json::Value;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use tempfile::TempDir;

async fn workspace() -> TempDir {
    let workspace = TempDir::new().expect("temporary Workspace should be created");
    init::init(workspace.path(), Some("test"))
        .await
        .expect("Workspace should initialize");
    workspace
}

fn run(workspace: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rivets"))
        .args(args)
        .current_dir(workspace)
        .output()
        .expect("Rivets CLI should run")
}

fn issue_records(workspace: &Path) -> Vec<Value> {
    std::fs::read_to_string(workspace.join(".rivets/issues.jsonl"))
        .expect("issues source should be readable")
        .lines()
        .map(|line| serde_json::from_str(line).expect("canonical Issue should parse"))
        .collect()
}

#[tokio::test]
async fn workspace_mutation_lock_blocks_cli_writes_but_not_reads() {
    let workspace = workspace().await;
    let source_path = workspace.path().join(".rivets/issues.jsonl");
    let before = std::fs::read(&source_path).expect("source bytes should be readable");
    let holder = WorkspaceMutationLock::try_acquire(workspace.path())
        .expect("test holder should acquire Workspace lock");

    let create = run(
        workspace.path(),
        &["create", "--title", "Blocked create", "--yes"],
    );
    assert!(!create.status.success());
    let stderr = String::from_utf8_lossy(&create.stderr);
    assert!(stderr.contains("Workspace is busy"));
    assert!(stderr.contains("retry the operation"));
    assert_eq!(std::fs::read(&source_path).unwrap(), before);

    let list = run(workspace.path(), &["list", "--json"]);
    assert!(
        list.status.success(),
        "read should not acquire mutation lock"
    );

    drop(holder);
    let retry = run(
        workspace.path(),
        &["create", "--title", "Retried create", "--yes"],
    );
    assert!(retry.status.success(), "retry should succeed after release");
    assert_eq!(issue_records(workspace.path()).len(), 1);
}

#[tokio::test]
async fn workspace_mutation_lock_precedes_cli_config_load() {
    let workspace = workspace().await;
    let holder = WorkspaceMutationLock::try_acquire(workspace.path())
        .expect("test holder should acquire Workspace lock");
    std::fs::write(workspace.path().join(".rivets/config.yaml"), "not: [valid")
        .expect("malformed config should be written");

    let create = run(
        workspace.path(),
        &["create", "--title", "Must not load", "--yes"],
    );
    assert!(!create.status.success());
    assert!(String::from_utf8_lossy(&create.stderr).contains("Workspace is busy"));
    drop(holder);
}

#[tokio::test]
async fn workspace_mutation_lock_does_not_serialize_distinct_cli_workspaces() {
    let workspace_a = workspace().await;
    let workspace_b = workspace().await;
    let holder = WorkspaceMutationLock::try_acquire(workspace_a.path())
        .expect("Workspace A holder should acquire");

    let create_b = run(
        workspace_b.path(),
        &["create", "--title", "Workspace B issue", "--yes"],
    );
    assert!(create_b.status.success());
    assert_eq!(issue_records(workspace_b.path()).len(), 1);
    assert!(issue_records(workspace_a.path()).is_empty());
    drop(holder);
}

#[tokio::test]
async fn workspace_mutation_lock_retry_preserves_both_cli_writes() {
    let workspace = workspace().await;
    let mut first = Command::new(env!("CARGO_BIN_EXE_rivets"))
        .args(["create", "--yes"])
        .current_dir(workspace.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("first writer should start");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match WorkspaceMutationLock::try_acquire(workspace.path()) {
            Err(Error::WorkspaceBusy { .. }) => break,
            Ok(probe) => drop(probe),
            Err(error) => panic!("lock readiness probe failed: {error}"),
        }
        assert!(
            Instant::now() < deadline,
            "first writer should acquire before timeout"
        );
        thread::sleep(Duration::from_millis(10));
    }

    let second = run(
        workspace.path(),
        &["create", "--title", "Second issue", "--yes"],
    );
    assert!(!second.status.success());
    assert!(String::from_utf8_lossy(&second.stderr).contains("Workspace is busy"));

    first
        .stdin
        .take()
        .expect("first writer stdin should be piped")
        .write_all(b"First issue\n")
        .expect("first writer title should be sent");
    assert!(first.wait().expect("first writer should exit").success());

    let retry = run(
        workspace.path(),
        &["create", "--title", "Second issue", "--yes"],
    );
    assert!(retry.status.success());
    let mut titles: Vec<_> = issue_records(workspace.path())
        .into_iter()
        .map(|record| record["title"].as_str().unwrap().to_string())
        .collect();
    titles.sort();
    assert_eq!(titles, ["First issue", "Second issue"]);
}
