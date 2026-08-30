//! MCP mutation transactions under the durable Workspace lock.

use rivets::domain::{Issue, NewIssue};
use rivets::storage::{StorageBackend, create_storage};
use rivets::workspace_lock::WorkspaceMutationLock;
use rivets_mcp::context::Context;
use rivets_mcp::error::Error;
use rivets_mcp::models::{
    CreateParams, IssueKindInput, ListParams, ResourceUpdateParams, UpdateParams,
};
use rivets_mcp::tools::Tools;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Arc;
use std::time::Instant;
use tempfile::TempDir;
use tokio::sync::{Mutex, RwLock};

static WORKSPACE_LOCK_TESTS: Mutex<()> = Mutex::const_new(());

fn create_params(title: &str, workspace_root: Option<&str>) -> CreateParams {
    CreateParams {
        title: title.to_string(),
        description: None,
        priority: None,
        kind: IssueKindInput::canonical(Some(rivets::domain::IssueKind::Task)),
        assignee: None,
        labels: None,
        design: None,
        acceptance: None,
        initial_note: None,
        workspace_root: workspace_root.map(str::to_string),
    }
}

fn update_params(issue_id: &str, workspace_root: Option<&str>) -> UpdateParams {
    serde_json::from_value(serde_json::json!({
        "issue_id": issue_id,
        "title": "Updated title",
        "workspace_root": workspace_root,
    }))
    .expect("update parameters should deserialize")
}

fn list_params(workspace_root: Option<&str>) -> ListParams {
    ListParams {
        status: None,
        priority: None,
        kind: IssueKindInput::canonical(None),
        assignee: None,
        label: None,
        limit: Some(100),
        workspace_root: workspace_root.map(str::to_string),
    }
}

fn workspace() -> TempDir {
    let workspace = TempDir::new().expect("temporary Workspace should be created");
    let rivets_dir = workspace.path().join(".rivets");
    std::fs::create_dir(&rivets_dir).expect(".rivets should be created");
    std::fs::write(
        rivets_dir.join("config.yaml"),
        "issue-prefix: test\nstorage:\n  backend: jsonl\n  data_file: .rivets/issues.jsonl\n",
    )
    .expect("config should be written");
    std::fs::write(rivets_dir.join("issues.jsonl"), []).expect("source should be created");
    std::fs::write(rivets_dir.join("workspace.lock"), []).expect("sidecar should be created");
    workspace
}

fn tools() -> Tools {
    Tools::new(Arc::new(RwLock::new(Context::new())))
}

async fn set_context(tools: &Tools, path: &Path) {
    tools
        .set_context(&path.display().to_string())
        .await
        .expect("context should be set");
}

fn run_cli(workspace: &Path, args: &[&str]) -> Output {
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

fn persisted_issue(workspace: &Path, issue_id: &str) -> serde_json::Value {
    std::fs::read_to_string(workspace.join(".rivets/issues.jsonl"))
        .expect("issues source should be readable")
        .lines()
        .map(|line| serde_json::from_str(line).expect("Issue record should parse"))
        .find(|record: &serde_json::Value| record["id"] == issue_id)
        .expect("Issue record should exist")
}

async fn create_issue(tools: &Tools, title: &str) -> Issue {
    tools
        .create(create_params(title, None))
        .await
        .expect("setup Issue should be created")
}

async fn create_epic(tools: &Tools, title: &str) -> Issue {
    let mut params = create_params(title, None);
    params.kind = IssueKindInput::canonical(Some(rivets::domain::IssueKind::Epic));
    tools
        .create(params)
        .await
        .expect("setup Epic should be created")
}

fn assert_busy<T>(result: Result<T, Error>, workspace_root: &Path) {
    match result {
        Err(Error::WorkspaceBusy {
            workspace_root: actual,
        }) => assert_eq!(actual, workspace_root),
        Err(error) => panic!("expected WorkspaceBusy, got {error:?}"),
        Ok(_) => panic!("expected WorkspaceBusy, got success"),
    }
}

struct MutationFixture {
    _workspace: TempDir,
    root: std::path::PathBuf,
    root_string: String,
    tools: Tools,
    update_target: Issue,
    resource_target: Issue,
    lifecycle_target: Issue,
    dependent: Issue,
    prerequisite: Issue,
    parent_a: Issue,
    parent_b: Issue,
}

impl MutationFixture {
    async fn new() -> Self {
        let workspace = workspace();
        let root = workspace
            .path()
            .canonicalize()
            .expect("MCP workspace-lock test precondition should hold");
        let root_string = root.display().to_string();
        let tools = tools();
        set_context(&tools, &root).await;
        let update_target = create_issue(&tools, "Update target").await;
        let resource_target = create_issue(&tools, "Resource target").await;
        let lifecycle_target = create_issue(&tools, "Lifecycle target").await;
        let dependent = create_issue(&tools, "Dependent target").await;
        let prerequisite = create_issue(&tools, "Prerequisite target").await;
        let parent_a = create_epic(&tools, "Parent A").await;
        let parent_b = create_epic(&tools, "Parent B").await;
        tools
            .parent_set(lifecycle_target.id.as_str(), parent_a.id.as_str(), None)
            .await
            .expect("setup Parentage should be added");
        tools
            .resource_add(
                resource_target.id.as_str(),
                Some("https://example.com/original".to_string()),
                None,
                "reference",
                None,
                None,
            )
            .await
            .expect("setup Resource should be added");
        Self {
            _workspace: workspace,
            root,
            root_string,
            tools,
            update_target,
            resource_target,
            lifecycle_target,
            dependent,
            prerequisite,
            parent_a,
            parent_b,
        }
    }
}

async fn assert_issue_mutators_busy(fixture: &MutationFixture) {
    assert_busy(
        fixture
            .tools
            .create(create_params("Blocked create", None))
            .await,
        &fixture.root,
    );
    assert_busy(
        fixture
            .tools
            .update(update_params(
                fixture.update_target.id.as_str(),
                Some(&fixture.root_string),
            ))
            .await,
        &fixture.root,
    );
    assert_busy(
        fixture
            .tools
            .add_note(
                fixture.update_target.id.as_str(),
                "Blocked Note".to_string(),
                None,
            )
            .await,
        &fixture.root,
    );
}

async fn assert_resource_mutators_busy(fixture: &MutationFixture) {
    assert_busy(
        fixture
            .tools
            .resource_add(
                fixture.resource_target.id.as_str(),
                Some("https://example.com/blocked".to_string()),
                None,
                "reference",
                None,
                Some(&fixture.root_string),
            )
            .await,
        &fixture.root,
    );
    assert_busy(
        fixture
            .tools
            .resource_update(ResourceUpdateParams {
                issue_id: fixture.resource_target.id.to_string(),
                resource_id: "r1".to_string(),
                url: Some("https://example.com/updated".to_string()),
                path: None,
                role: None,
                label: None,
                clear_label: false,
                workspace_root: None,
            })
            .await,
        &fixture.root,
    );
    assert_busy(
        fixture
            .tools
            .resource_remove(
                fixture.resource_target.id.as_str(),
                "r1",
                Some(&fixture.root_string),
            )
            .await,
        &fixture.root,
    );
}

async fn assert_lifecycle_relationship_and_label_mutators_busy(fixture: &MutationFixture) {
    assert_busy(
        fixture
            .tools
            .close(fixture.lifecycle_target.id.as_str(), None, None)
            .await,
        &fixture.root,
    );
    assert_busy(
        fixture
            .tools
            .reopen(
                fixture.lifecycle_target.id.as_str(),
                None,
                Some(&fixture.root_string),
            )
            .await,
        &fixture.root,
    );
    assert_busy(
        fixture
            .tools
            .blocking_dependency_add(
                fixture.dependent.id.as_str(),
                fixture.prerequisite.id.as_str(),
                None,
            )
            .await,
        &fixture.root,
    );
    assert_busy(
        fixture
            .tools
            .blocking_dependency_remove(
                fixture.dependent.id.as_str(),
                fixture.prerequisite.id.as_str(),
                Some(&fixture.root_string),
            )
            .await,
        &fixture.root,
    );
    assert_busy(
        fixture
            .tools
            .parent_set(
                fixture.update_target.id.as_str(),
                fixture.parent_a.id.as_str(),
                None,
            )
            .await,
        &fixture.root,
    );
    assert_busy(
        fixture
            .tools
            .parent_clear(
                fixture.lifecycle_target.id.as_str(),
                Some(&fixture.root_string),
            )
            .await,
        &fixture.root,
    );
    assert_busy(
        fixture
            .tools
            .parent_move(
                fixture.lifecycle_target.id.as_str(),
                fixture.parent_b.id.as_str(),
                None,
            )
            .await,
        &fixture.root,
    );
    assert_busy(
        fixture
            .tools
            .label_add(fixture.update_target.id.as_str(), "blocked-label", None)
            .await,
        &fixture.root,
    );
    assert_busy(
        fixture
            .tools
            .label_remove(
                fixture.update_target.id.as_str(),
                "missing-label",
                Some(&fixture.root_string),
            )
            .await,
        &fixture.root,
    );
}

#[tokio::test]
async fn workspace_lock_blocks_every_mcp_mutator_but_not_queries() {
    let _serial = WORKSPACE_LOCK_TESTS.lock().await;
    let fixture = MutationFixture::new().await;
    let source_path = fixture.root.join(".rivets/issues.jsonl");
    let before =
        std::fs::read(&source_path).expect("MCP workspace-lock test precondition should hold");
    let holder = WorkspaceMutationLock::try_acquire(&fixture.root)
        .expect("MCP workspace-lock test precondition should hold");
    assert_issue_mutators_busy(&fixture).await;
    assert_resource_mutators_busy(&fixture).await;
    assert_lifecycle_relationship_and_label_mutators_busy(&fixture).await;
    assert_eq!(
        fixture
            .tools
            .parent_show(
                fixture.lifecycle_target.id.as_str(),
                Some(&fixture.root_string),
            )
            .await
            .unwrap()
            .expect("setup child should remain parented")
            .parent_id(),
        &fixture.parent_a.id
    );
    assert_eq!(std::fs::read(&source_path).unwrap(), before);
    assert_eq!(
        std::fs::read(&source_path).expect("source file should remain readable"),
        before
    );
    assert_eq!(
        fixture.tools.list(list_params(None)).await.unwrap().len(),
        7
    );
    drop(holder);
}

#[tokio::test]
async fn claim_and_release_require_workspace_lock() {
    let _serial = WORKSPACE_LOCK_TESTS.lock().await;
    let fixture = MutationFixture::new().await;
    let holder = WorkspaceMutationLock::try_acquire(&fixture.root)
        .expect("MCP workspace-lock test precondition should hold");
    assert_busy(
        fixture
            .tools
            .claim(fixture.update_target.id.as_str(), "alice", None)
            .await,
        &fixture.root,
    );
    drop(holder);

    fixture
        .tools
        .claim(fixture.update_target.id.as_str(), "alice", None)
        .await
        .expect("Claim should succeed after lock release");
    let holder = WorkspaceMutationLock::try_acquire(&fixture.root)
        .expect("MCP workspace-lock test precondition should hold");
    assert_busy(
        fixture
            .tools
            .release(fixture.update_target.id.as_str(), "alice", None)
            .await,
        &fixture.root,
    );
    drop(holder);
    fixture
        .tools
        .release(fixture.update_target.id.as_str(), "alice", None)
        .await
        .expect("Release should succeed after lock release");
}

#[tokio::test]
async fn mixed_cli_mcp_mutation_preserves_atomic_claim() {
    let _serial = WORKSPACE_LOCK_TESTS.lock().await;
    let fixture = MutationFixture::new().await;
    let issue_id = fixture.update_target.id.as_str();

    let holder = WorkspaceMutationLock::try_acquire(&fixture.root)
        .expect("MCP workspace-lock test precondition should hold");
    let contended_claim = run_cli(&fixture.root, &["claim", issue_id, "--assignee", "alice"]);
    assert!(!contended_claim.status.success());
    assert!(String::from_utf8_lossy(&contended_claim.stderr).contains("Workspace is busy"));
    drop(holder);
    assert!(
        run_cli(&fixture.root, &["claim", issue_id, "--assignee", "alice"],)
            .status
            .success()
    );

    let holder = WorkspaceMutationLock::try_acquire(&fixture.root)
        .expect("MCP workspace-lock test precondition should hold");
    assert_busy(
        fixture.tools.update(update_params(issue_id, None)).await,
        &fixture.root,
    );
    drop(holder);
    fixture
        .tools
        .update(update_params(issue_id, None))
        .await
        .expect("MCP retry should reload the CLI Claim before updating");
    let persisted = persisted_issue(&fixture.root, issue_id);
    assert_eq!(persisted["assignee"], "alice");
    assert_eq!(persisted["title"], "Updated title");

    let close_target = fixture
        .tools
        .create(create_params("MCP Claim then CLI close", None))
        .await
        .expect("second target should be created");
    fixture
        .tools
        .claim(close_target.id.as_str(), "bob", None)
        .await
        .expect("MCP Claim should persist");
    let holder = WorkspaceMutationLock::try_acquire(&fixture.root)
        .expect("MCP workspace-lock test precondition should hold");
    let contended_close = run_cli(&fixture.root, &["close", close_target.id.as_str()]);
    assert!(!contended_close.status.success());
    assert!(String::from_utf8_lossy(&contended_close.stderr).contains("Workspace is busy"));
    drop(holder);
    assert!(
        run_cli(&fixture.root, &["close", close_target.id.as_str()])
            .status
            .success()
    );
    let closed = persisted_issue(&fixture.root, close_target.id.as_str());
    assert_eq!(closed["status"], "closed");
    assert_eq!(closed["assignee"], serde_json::Value::Null);
}

#[tokio::test]
async fn workspace_lock_cache_miss_resolves_before_durable_lock() {
    let _serial = WORKSPACE_LOCK_TESTS.lock().await;
    let workspace = workspace();
    let root = workspace
        .path()
        .canonicalize()
        .expect("MCP workspace-lock test precondition should hold");
    let root_string = root.display().to_string();
    let holder = WorkspaceMutationLock::try_acquire(&root)
        .expect("MCP workspace-lock test precondition should hold");
    std::fs::write(root.join(".rivets/config.yaml"), "not: [valid")
        .expect("MCP workspace-lock test precondition should hold");
    let uncached = tools();
    match uncached
        .create(create_params("Must not initialize", Some(&root_string)))
        .await
    {
        Err(Error::ConfigLoad { .. }) => {}
        Err(error) => panic!("expected config loading to precede lock acquisition, got {error:?}"),
        Ok(_) => panic!("expected malformed config to fail before lock acquisition"),
    }
    drop(holder);

    std::fs::write(
        root.join(".rivets/config.yaml"),
        "issue-prefix: test\nstorage:\n  backend: jsonl\n  data_file: .rivets/issues.jsonl\n",
    )
    .expect("MCP workspace-lock test precondition should hold");
    let cached = tools();
    set_context(&cached, &root).await;
    let issue = create_issue(&cached, "Cached target").await;
    let holder = WorkspaceMutationLock::try_acquire(&root)
        .expect("MCP workspace-lock test precondition should hold");
    std::fs::write(root.join(".rivets/issues.jsonl"), b"{malformed\n")
        .expect("MCP workspace-lock test precondition should hold");
    assert_busy(
        cached
            .add_note(issue.id.as_str(), "Must not reload".to_string(), None)
            .await,
        &root,
    );
    drop(holder);
}

#[tokio::test]
async fn same_server_mutations_serialize_and_both_persist() {
    let workspace = workspace();
    let root = workspace
        .path()
        .canonicalize()
        .expect("MCP workspace-lock test precondition should hold");
    let source_path = root.join(".rivets/issues.jsonl");
    let tools = tools();
    set_context(&tools, &root).await;

    let (first, second) = tokio::join!(
        tools.create(create_params("Concurrent first", None)),
        tools.create(create_params("Concurrent second", None)),
    );
    assert!(
        first.is_ok(),
        "first same-server mutation should succeed: {first:?}"
    );
    assert!(
        second.is_ok(),
        "second same-server mutation should succeed: {second:?}"
    );

    let persisted = std::fs::read_to_string(source_path)
        .expect("MCP workspace-lock test precondition should hold");
    assert_eq!(persisted.lines().count(), 2);
    assert!(persisted.contains("Concurrent first"));
    assert!(persisted.contains("Concurrent second"));
}
#[tokio::test]
async fn workspace_lock_does_not_serialize_distinct_mcp_workspaces() {
    let _serial = WORKSPACE_LOCK_TESTS.lock().await;
    let workspace_a = workspace();
    let workspace_b = workspace();
    let root_a = workspace_a
        .path()
        .canonicalize()
        .expect("MCP workspace-lock test precondition should hold");
    let root_b = workspace_b
        .path()
        .canonicalize()
        .expect("MCP workspace-lock test precondition should hold");
    let root_b_string = root_b.display().to_string();
    let holder = WorkspaceMutationLock::try_acquire(&root_a)
        .expect("MCP workspace-lock test precondition should hold");
    let tools = tools();

    let created = tools
        .create(create_params(
            "Independent Workspace issue",
            Some(&root_b_string),
        ))
        .await
        .expect("Workspace B should mutate while Workspace A is held");
    assert_eq!(created.title, "Independent Workspace issue");
    assert!(
        std::fs::read_to_string(root_a.join(".rivets/issues.jsonl"))
            .expect("held Workspace source file should remain readable")
            .is_empty()
    );
    drop(holder);
}

#[tokio::test]
#[ignore = "production-scale durable-lock checkpoint"]
async fn workspace_lock_10k_mcp_mutation_preserves_records() {
    const ISSUE_COUNT: usize = 10_000;
    let _serial = WORKSPACE_LOCK_TESTS.lock().await;
    let workspace = workspace();
    let root = workspace
        .path()
        .canonicalize()
        .expect("MCP workspace-lock test precondition should hold");
    let source_path = root.join(".rivets/issues.jsonl");
    let mut storage = create_storage(StorageBackend::Jsonl(source_path.clone()), "scale".into())
        .await
        .expect("scale storage should open");
    let mut first_id = None;
    for index in 0..ISSUE_COUNT {
        let issue = storage
            .create(NewIssue {
                title: format!("Scale Issue {index} λ"),
                description: "multiline\ncontext".to_string(),
                ..Default::default()
            })
            .await
            .expect("scale Issue should be created");
        if first_id.is_none() {
            first_id = Some(issue.id);
        }
    }
    storage.save().await.expect("scale source should persist");
    drop(storage);

    let tools = tools();
    set_context(&tools, &root).await;
    let started = Instant::now();
    tools
        .claim(
            first_id
                .as_ref()
                .expect("scale fixture should have a first Issue")
                .as_str(),
            "scale-owner",
            None,
        )
        .await
        .expect("guarded scale Claim should succeed");
    let elapsed = started.elapsed();
    eprintln!("10k MCP guarded mutation elapsed: {elapsed:?}");

    assert_eq!(
        std::fs::read_to_string(source_path)
            .expect("scale source should be readable")
            .lines()
            .count(),
        ISSUE_COUNT
    );
}
