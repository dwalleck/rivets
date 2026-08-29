//! Durable Workspace mutation-lock behavior.

use rivets::commands::init;
use rivets::error::Error;
use rivets::workspace_lock::{WORKSPACE_LOCK_FILE_NAME, WorkspaceMutationLock};
use std::env;
use std::fs;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use tempfile::TempDir;

async fn initialized_workspace() -> TempDir {
    let workspace = TempDir::new().expect("temporary Workspace should be created");
    init::init(workspace.path(), Some("test"))
        .await
        .expect("Workspace should initialize");
    workspace
}

#[tokio::test]
async fn workspace_lock_is_canonical_scoped_and_persistent() {
    let first_workspace = initialized_workspace().await;
    let second_workspace = initialized_workspace().await;
    #[cfg(unix)]
    let alias_parent = TempDir::new().expect("alias parent should be created");
    #[cfg(unix)]
    let alias = {
        let alias = alias_parent.path().join("Workspace λ alias");
        std::os::unix::fs::symlink(first_workspace.path(), &alias)
            .expect("Workspace symlink alias should be created");
        alias
    };
    #[cfg(not(unix))]
    let alias = first_workspace.path().join(".");

    let first = WorkspaceMutationLock::try_acquire(&alias)
        .expect("first canonical Workspace lock should succeed");
    let busy = WorkspaceMutationLock::try_acquire(first_workspace.path())
        .expect_err("canonical alias should contend on the same sidecar");
    assert!(matches!(
        busy,
        Error::WorkspaceBusy { ref workspace_root }
            if workspace_root == &first.workspace_root().canonicalize().unwrap()
    ));
    let second = WorkspaceMutationLock::try_acquire(second_workspace.path())
        .expect("different Workspace should acquire independently");
    let lock_path = first.lock_path().to_path_buf();
    assert!(lock_path.ends_with(WORKSPACE_LOCK_FILE_NAME));
    assert!(lock_path.exists());

    drop(first);
    let reacquired = WorkspaceMutationLock::try_acquire(first_workspace.path())
        .expect("drop should release the Workspace lock");
    drop(reacquired);
    assert!(lock_path.exists(), "release must not remove the sidecar");
    drop(second);
}

#[tokio::test]
async fn workspace_lock_contention_is_nonblocking() {
    let workspace = initialized_workspace().await;
    let holder = WorkspaceMutationLock::try_acquire(workspace.path())
        .expect("holder should acquire the lock");
    let contender_workspace = workspace.path().to_path_buf();
    let (sender, receiver) = std::sync::mpsc::channel();

    let started = Instant::now();
    let contender = thread::spawn(move || {
        let result = WorkspaceMutationLock::try_acquire(&contender_workspace);
        if sender.send(result).is_err() {
            eprintln!("contention receiver dropped after timeout");
        }
    });
    let result = receiver
        .recv_timeout(Duration::from_secs(1))
        .expect("contended acquisition must not wait for the holder");
    let elapsed = started.elapsed();
    eprintln!("contended Workspace lock acquisition: {elapsed:?}");
    assert!(matches!(result, Err(Error::WorkspaceBusy { .. })));
    drop(holder);
    contender.join().expect("contender thread should finish");
}

#[tokio::test]
async fn workspace_lock_scales_across_distinct_workspaces() {
    let mut workspaces = Vec::with_capacity(32);
    for _ in 0..32 {
        workspaces.push(initialized_workspace().await);
    }
    let guards: Vec<_> = workspaces
        .iter()
        .map(|workspace| {
            WorkspaceMutationLock::try_acquire(workspace.path())
                .expect("each distinct Workspace should acquire independently")
        })
        .collect();
    assert_eq!(guards.len(), 32);
}

#[tokio::test]
async fn workspace_lock_reports_causal_open_failure() {
    let missing = TempDir::new()
        .expect("temporary parent should be created")
        .path()
        .join("missing-workspace");
    let error = WorkspaceMutationLock::try_acquire(&missing)
        .expect_err("missing Workspace should return a causal lock error");
    assert!(matches!(error, Error::WorkspaceLock { .. }));
    assert!(std::error::Error::source(&error).is_some());
}

#[tokio::test]
async fn init_creates_ignored_workspace_lock_sidecar() {
    let workspace = initialized_workspace().await;
    let rivets_dir = workspace.path().join(".rivets");
    assert!(rivets_dir.join(WORKSPACE_LOCK_FILE_NAME).is_file());
    let ignore = fs::read_to_string(rivets_dir.join(".gitignore"))
        .expect("Rivets metadata ignore should be readable");
    assert!(ignore.lines().any(|line| line == WORKSPACE_LOCK_FILE_NAME));
}

#[test]
fn workspace_lock_child_holder() {
    let Ok(workspace) = env::var("RIVETS_WORKSPACE_LOCK_HOLDER") else {
        return;
    };
    let ready = env::var("RIVETS_WORKSPACE_LOCK_READY")
        .expect("child holder should receive a readiness path");
    let _guard = WorkspaceMutationLock::try_acquire(workspace.as_ref())
        .expect("child holder should acquire the Workspace lock");
    fs::write(ready, b"ready").expect("child holder should publish readiness");
    thread::sleep(Duration::from_secs(60));
}

#[tokio::test]
async fn workspace_lock_killed_holder_releases_without_stale_cleanup() {
    let workspace = initialized_workspace().await;
    let ready = workspace.path().join("holder-ready");
    let executable = env::current_exe().expect("test executable path should resolve");
    let mut child = Command::new(executable)
        .arg("--exact")
        .arg("workspace_lock_child_holder")
        .arg("--nocapture")
        .env("RIVETS_WORKSPACE_LOCK_HOLDER", workspace.path())
        .env("RIVETS_WORKSPACE_LOCK_READY", &ready)
        .stdout(Stdio::null())
        .spawn()
        .expect("lock-holder child should start");

    let deadline = Instant::now() + Duration::from_secs(5);
    while !ready.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(ready.exists(), "child holder should become ready");
    assert!(matches!(
        WorkspaceMutationLock::try_acquire(workspace.path()),
        Err(Error::WorkspaceBusy { .. })
    ));

    child.kill().expect("holder child should be killed");
    child.wait().expect("holder child should be reaped");
    let reacquired = WorkspaceMutationLock::try_acquire(workspace.path())
        .expect("killed holder should release without stale cleanup");
    drop(reacquired);
    assert!(
        workspace
            .path()
            .join(".rivets")
            .join(WORKSPACE_LOCK_FILE_NAME)
            .exists()
    );
}
