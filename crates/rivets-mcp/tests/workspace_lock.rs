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
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use tempfile::TempDir;
use tokio::sync::RwLock;

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
    UpdateParams {
        issue_id: issue_id.to_string(),
        status: None,
        priority: None,
        kind: IssueKindInput::canonical(None),
        title: Some("Updated title".to_string()),
        description: None,
        design: None,
        acceptance_criteria: None,
        labels: None,
        workspace_root: workspace_root.map(str::to_string),
    }
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

async fn create_issue(tools: &Tools, title: &str) -> Issue {
    tools
        .create(create_params(title, None))
        .await
        .expect("setup Issue should be created")
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
}

impl MutationFixture {
    async fn new() -> Self {
        let workspace = workspace();
        let root = workspace.path().canonicalize().unwrap();
        let root_string = root.display().to_string();
        let tools = tools();
        set_context(&tools, &root).await;
        let update_target = create_issue(&tools, "Update target").await;
        let resource_target = create_issue(&tools, "Resource target").await;
        let lifecycle_target = create_issue(&tools, "Lifecycle target").await;
        let dependent = create_issue(&tools, "Dependent target").await;
        let prerequisite = create_issue(&tools, "Prerequisite target").await;
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
    let fixture = MutationFixture::new().await;
    let source_path = fixture.root.join(".rivets/issues.jsonl");
    let before = std::fs::read(&source_path).unwrap();
    let holder = WorkspaceMutationLock::try_acquire(&fixture.root).unwrap();
    assert_issue_mutators_busy(&fixture).await;
    assert_resource_mutators_busy(&fixture).await;
    assert_lifecycle_relationship_and_label_mutators_busy(&fixture).await;
    assert_eq!(std::fs::read(&source_path).unwrap(), before);
    assert_eq!(
        fixture.tools.list(list_params(None)).await.unwrap().len(),
        5
    );
    drop(holder);
    fixture
        .tools
        .label_add(fixture.update_target.id.as_str(), "after-release", None)
        .await
        .expect("mutation should succeed after lock release");
}

#[tokio::test]
async fn workspace_lock_precedes_mcp_cache_miss_and_reload() {
    let workspace = workspace();
    let root = workspace.path().canonicalize().unwrap();
    let root_string = root.display().to_string();
    let holder = WorkspaceMutationLock::try_acquire(&root).unwrap();
    std::fs::write(root.join(".rivets/config.yaml"), "not: [valid").unwrap();
    let uncached = tools();
    assert_busy(
        uncached
            .create(create_params("Must not initialize", Some(&root_string)))
            .await,
        &root,
    );
    drop(holder);

    std::fs::write(
        root.join(".rivets/config.yaml"),
        "issue-prefix: test\nstorage:\n  backend: jsonl\n  data_file: .rivets/issues.jsonl\n",
    )
    .unwrap();
    let cached = tools();
    set_context(&cached, &root).await;
    let issue = create_issue(&cached, "Cached target").await;
    let holder = WorkspaceMutationLock::try_acquire(&root).unwrap();
    std::fs::write(root.join(".rivets/issues.jsonl"), b"{malformed\n").unwrap();
    assert_busy(
        cached
            .add_note(issue.id.as_str(), "Must not reload".to_string(), None)
            .await,
        &root,
    );
    drop(holder);
}

#[tokio::test]
async fn workspace_lock_does_not_serialize_distinct_mcp_workspaces() {
    let workspace_a = workspace();
    let workspace_b = workspace();
    let root_a = workspace_a.path().canonicalize().unwrap();
    let root_b = workspace_b.path().canonicalize().unwrap();
    let root_b_string = root_b.display().to_string();
    let holder = WorkspaceMutationLock::try_acquire(&root_a).unwrap();
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
            .unwrap()
            .is_empty()
    );
    drop(holder);
}

#[tokio::test]
#[ignore = "production-scale durable-lock checkpoint"]
async fn workspace_lock_10k_mcp_mutation_preserves_records() {
    const ISSUE_COUNT: usize = 10_000;
    let workspace = workspace();
    let root = workspace.path().canonicalize().unwrap();
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
        .label_add(
            first_id
                .as_ref()
                .expect("scale fixture should have a first Issue")
                .as_str(),
            "guarded",
            None,
        )
        .await
        .expect("guarded scale mutation should succeed");
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
