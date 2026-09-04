//! Real CLI mutation transactions under the durable Workspace lock.

mod common;

use rivets::commands::init;
use rivets::workspace_lock::WorkspaceMutationLock;
use serde_json::Value;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, Command, Output, Stdio};
use std::sync::{Arc, Barrier};
use std::thread;
use tempfile::TempDir;

async fn workspace() -> TempDir {
    let workspace = TempDir::new().expect("temporary Workspace should be created");
    init::init(workspace.path(), Some("test"))
        .await
        .expect("Workspace should initialize");
    workspace
}

fn run(workspace: &Path, args: &[&str]) -> Output {
    common::run_rivets_in_dir(workspace, args)
}

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

fn spawn_prompted(workspace: &Path, args: &[&str]) -> ChildGuard {
    ChildGuard(
        Command::new(common::get_rivets_binary())
            .args(args)
            .current_dir(workspace)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("prompted Rivets process should start"),
    )
}

fn wait_for_prompt(child: &mut ChildGuard, expected_fragment: &str) {
    let stderr = child
        .0
        .stderr
        .as_mut()
        .expect("prompted process stderr should be piped");
    let mut seen = Vec::new();
    let mut byte = [0_u8; 1];
    while !String::from_utf8_lossy(&seen).contains(expected_fragment) {
        stderr
            .read_exact(&mut byte)
            .expect("prompted process should emit the expected prompt");
        seen.push(byte[0]);
        assert!(seen.len() < 1024, "prompt output exceeded safety bound");
    }
}

fn answer_prompt(child: &mut ChildGuard, answer: &[u8]) {
    child
        .0
        .stdin
        .take()
        .expect("prompted process stdin should be piped")
        .write_all(answer)
        .expect("prompt response should be sent");
    assert!(
        child
            .0
            .wait()
            .expect("prompted process should exit")
            .success()
    );
}

fn issue_id_by_title(workspace: &Path, title: &str) -> String {
    issue_records(workspace)
        .into_iter()
        .find(|record| record["title"] == title)
        .and_then(|record| record["id"].as_str().map(str::to_string))
        .expect("seeded Issue should persist with a string ID")
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
    assert_eq!(
        std::fs::read(&source_path).expect("Issue source bytes should remain readable"),
        before
    );

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
async fn claim_and_release_require_workspace_mutation_lock() {
    let workspace = workspace().await;
    let created = run(
        workspace.path(),
        &["--json", "create", "--title", "Lock target", "--yes"],
    );
    let created: Value =
        serde_json::from_slice(&created.stdout).expect("create output should be JSON");
    let issue_id = created["id"].as_str().expect("Issue ID");

    let holder = WorkspaceMutationLock::try_acquire(workspace.path())
        .expect("test holder should acquire Workspace lock");
    let claim = run(
        workspace.path(),
        &["claim", issue_id, "--assignee", "alice"],
    );
    assert!(!claim.status.success());
    assert!(String::from_utf8_lossy(&claim.stderr).contains("Workspace is busy"));
    assert_eq!(issue_records(workspace.path())[0]["assignee"], Value::Null);
    drop(holder);

    assert!(
        run(
            workspace.path(),
            &["claim", issue_id, "--assignee", "alice"],
        )
        .status
        .success()
    );

    let holder = WorkspaceMutationLock::try_acquire(workspace.path())
        .expect("test holder should reacquire Workspace lock");
    let release = run(
        workspace.path(),
        &["release", issue_id, "--assignee", "alice"],
    );
    assert!(!release.status.success());
    assert!(String::from_utf8_lossy(&release.stderr).contains("Workspace is busy"));
    assert_eq!(issue_records(workspace.path())[0]["assignee"], "alice");
    drop(holder);

    assert!(
        run(
            workspace.path(),
            &["release", issue_id, "--assignee", "alice"],
        )
        .status
        .success()
    );
    assert_eq!(issue_records(workspace.path())[0]["assignee"], Value::Null);
}

#[tokio::test]
async fn interactive_title_prompt_precedes_mutation_lock_and_preserves_writes() {
    let workspace = workspace().await;
    let mut first = spawn_prompted(workspace.path(), &["create", "--yes"]);
    wait_for_prompt(&mut first, "Title: ");

    let second = run(
        workspace.path(),
        &["create", "--title", "Second issue", "--yes"],
    );
    assert!(
        second.status.success(),
        "a writer must complete while the first process awaits human input: {}",
        String::from_utf8_lossy(&second.stderr)
    );

    answer_prompt(&mut first, b"First issue\n");

    let mut titles: Vec<_> = issue_records(workspace.path())
        .into_iter()
        .map(|record| {
            record["title"]
                .as_str()
                .expect("persisted title should be a string")
                .to_string()
        })
        .collect();
    titles.sort();
    assert_eq!(titles, ["First issue", "Second issue"]);
}

#[tokio::test]
async fn lifecycle_confirmation_prompts_precede_mutation_lock() {
    let workspace = workspace().await;
    for title in ["Close A", "Close B", "Delete target"] {
        let created = run(workspace.path(), &["create", "--title", title, "--yes"]);
        assert!(
            created.status.success(),
            "seed issue should be created: {}",
            String::from_utf8_lossy(&created.stderr)
        );
    }
    let close_a = issue_id_by_title(workspace.path(), "Close A");
    let close_b = issue_id_by_title(workspace.path(), "Close B");
    let delete_target = issue_id_by_title(workspace.path(), "Delete target");

    let mut close = spawn_prompted(
        workspace.path(),
        &["close", close_a.as_str(), close_b.as_str()],
    );
    wait_for_prompt(&mut close, "Close 2 issues?");
    assert!(
        run(
            workspace.path(),
            &["create", "--title", "During close prompt", "--yes"],
        )
        .status
        .success(),
        "writer should complete while Close awaits confirmation"
    );
    answer_prompt(&mut close, b"n\n");

    assert!(
        run(
            workspace.path(),
            &["close", close_a.as_str(), close_b.as_str(), "--yes"],
        )
        .status
        .success(),
        "seed issues should close before Reopen prompt"
    );
    let mut reopen = spawn_prompted(
        workspace.path(),
        &["reopen", close_a.as_str(), close_b.as_str()],
    );
    wait_for_prompt(&mut reopen, "Reopen 2 issues?");
    assert!(
        run(
            workspace.path(),
            &["create", "--title", "During reopen prompt", "--yes"],
        )
        .status
        .success(),
        "writer should complete while Reopen awaits confirmation"
    );
    answer_prompt(&mut reopen, b"n\n");

    let mut delete = spawn_prompted(workspace.path(), &["delete", delete_target.as_str()]);
    wait_for_prompt(&mut delete, "Delete issue");
    assert!(
        run(
            workspace.path(),
            &["create", "--title", "During delete prompt", "--yes"],
        )
        .status
        .success(),
        "writer should complete while Delete awaits confirmation"
    );
    answer_prompt(&mut delete, b"n\n");

    for title in [
        "During close prompt",
        "During reopen prompt",
        "During delete prompt",
        "Delete target",
    ] {
        assert!(
            issue_records(workspace.path())
                .iter()
                .any(|record| record["title"] == title),
            "{title} should survive prompt-time concurrency"
        );
    }
}

#[tokio::test]
async fn synchronized_claims_have_one_durable_winner_and_terminal_retry() {
    const CLAIMANT_COUNT: usize = 16;

    let workspace = workspace().await;
    let created = run(
        workspace.path(),
        &["--json", "create", "--title", "Claim race", "--yes"],
    );
    assert!(created.status.success());
    let created: Value =
        serde_json::from_slice(&created.stdout).expect("create output should be JSON");
    let issue_id = created["id"].as_str().expect("Issue ID").to_string();
    let claimants = (0..CLAIMANT_COUNT)
        .map(|index| format!("claimant-{index}"))
        .collect::<Vec<_>>();

    let barrier = Arc::new(Barrier::new(CLAIMANT_COUNT + 1));
    let outcomes = thread::scope(|scope| {
        let handles = claimants
            .iter()
            .map(|claimant| {
                let claimant_barrier = Arc::clone(&barrier);
                let claimant_id = issue_id.clone();
                let workspace_path = workspace.path();
                scope.spawn(move || {
                    claimant_barrier.wait();
                    run(
                        workspace_path,
                        &["claim", &claimant_id, "--assignee", claimant],
                    )
                })
            })
            .collect::<Vec<_>>();
        barrier.wait();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("claimant should join"))
            .collect::<Vec<_>>()
    });
    assert_eq!(
        outcomes
            .iter()
            .filter(|output| output.status.success())
            .count(),
        1,
        "exactly one synchronized claimant should mutate the Issue"
    );

    let record = issue_records(workspace.path())
        .into_iter()
        .find(|record| record["id"] == issue_id)
        .expect("claimed Issue should persist");
    let winner = record["assignee"]
        .as_str()
        .expect("winner should be persisted")
        .to_string();
    let claimed_at = record["updated_at"].clone();

    for claimant in &claimants {
        let retry = run(
            workspace.path(),
            &["claim", &issue_id, "--assignee", claimant],
        );
        if claimant == &winner {
            assert!(retry.status.success(), "owner retry should be idempotent");
        } else {
            assert!(!retry.status.success(), "loser retry must be terminal");
            assert!(String::from_utf8_lossy(&retry.stderr).contains("already claimed"));
        }
    }

    let record = issue_records(workspace.path())
        .into_iter()
        .find(|record| record["id"] == issue_id)
        .expect("claimed Issue should remain");
    assert_eq!(record["assignee"], winner);
    assert_eq!(record["updated_at"], claimed_at);
}

#[tokio::test]
async fn synchronized_same_claimant_is_idempotent_after_retry() {
    let workspace = workspace().await;
    let created = run(
        workspace.path(),
        &["--json", "create", "--title", "Same claimant race", "--yes"],
    );
    let created: Value =
        serde_json::from_slice(&created.stdout).expect("create output should be JSON");
    let issue_id = created["id"].as_str().expect("Issue ID").to_string();

    let barrier = Arc::new(Barrier::new(3));
    let (first, second) = thread::scope(|scope| {
        let workspace_path = workspace.path();
        let first_barrier = Arc::clone(&barrier);
        let first_id = issue_id.clone();
        let first = scope.spawn(move || {
            first_barrier.wait();
            run(workspace_path, &["claim", &first_id, "--assignee", "alice"])
        });
        let second_barrier = Arc::clone(&barrier);
        let second_id = issue_id.clone();
        let second = scope.spawn(move || {
            second_barrier.wait();
            run(
                workspace_path,
                &["claim", &second_id, "--assignee", "alice"],
            )
        });
        barrier.wait();
        (
            first.join().expect("first join"),
            second.join().expect("second join"),
        )
    });
    assert!(
        first.status.success() || second.status.success(),
        "one same-owner claimant must acquire the lock"
    );

    let claimed = issue_records(workspace.path())
        .into_iter()
        .find(|record| record["id"] == issue_id)
        .expect("claimed Issue should persist");
    assert_eq!(claimed["assignee"], "alice");
    let claimed_at = claimed["updated_at"].clone();

    let retry = run(
        workspace.path(),
        &["claim", &issue_id, "--assignee", "alice"],
    );
    assert!(retry.status.success());
    let retried = issue_records(workspace.path())
        .into_iter()
        .find(|record| record["id"] == issue_id)
        .expect("claimed Issue should persist");
    assert_eq!(retried["updated_at"], claimed_at);
}
