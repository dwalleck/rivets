//! Same-process MCP cache fences for out-of-band JSONL changes.

use rivets::domain::Issue;
use rivets_mcp::context::Context;
use rivets_mcp::models::{CreateParams, IssueKindInput, UpdateParams};
use rivets_mcp::tools::Tools;
use serde_json::Value;
use std::path::Path;
use std::sync::Arc;
use tempfile::TempDir;
use tokio::sync::RwLock;

fn kind_input(value: Option<&str>) -> IssueKindInput {
    let issue_kind = value.map(|value| value.parse().expect("valid test Issue Kind"));
    IssueKindInput::canonical(issue_kind)
}

#[allow(clippy::too_many_arguments)]
fn create_params(
    title: String,
    description: Option<String>,
    priority: Option<u8>,
    issue_kind: Option<&str>,
    assignee: Option<String>,
    labels: Option<Vec<String>>,
    design: Option<String>,
    acceptance: Option<String>,
    workspace_root: Option<&str>,
) -> CreateParams {
    CreateParams {
        title,
        description,
        priority,
        kind: kind_input(issue_kind),
        assignee,
        labels,
        design,
        acceptance,
        initial_note: None,
        workspace_root: workspace_root.map(str::to_string),
    }
}

#[allow(clippy::too_many_arguments)]
fn update_params(
    issue_id: &str,
    title: Option<String>,
    description: Option<String>,
    status: Option<&str>,
    priority: Option<u8>,
    issue_kind: Option<&str>,
    design: Option<String>,
    acceptance_criteria: Option<String>,
    labels: Option<Vec<String>>,
    workspace_root: Option<&str>,
) -> UpdateParams {
    UpdateParams {
        issue_id: issue_id.to_string(),
        status: status.map(str::to_string),
        priority,
        kind: kind_input(issue_kind),
        title,
        description,
        design,
        acceptance_criteria,
        labels,
        workspace_root: workspace_root.map(str::to_string),
    }
}

fn create_temp_workspace() -> TempDir {
    let temp = TempDir::new().expect("temporary workspace should be created");
    let rivets_dir = temp.path().join(".rivets");
    std::fs::create_dir(&rivets_dir).expect(".rivets directory should be created");
    std::fs::write(
        rivets_dir.join("config.yaml"),
        "issue-prefix: test\nstorage:\n  backend: jsonl\n  data_file: .rivets/issues.jsonl\n",
    )
    .expect("workspace config should be written");
    temp
}

fn create_tools() -> Tools {
    Tools::new(Arc::new(RwLock::new(Context::new())))
}

async fn set_context(tools: &Tools, path: &Path) {
    tools
        .set_context(&path.display().to_string())
        .await
        .expect("set_context should succeed");
}

async fn create_issue(tools: &Tools, title: &str) -> Issue {
    tools
        .create(create_params(
            title.to_string(),
            Some(format!("Description for {title}")),
            Some(2),
            Some("task"),
            None,
            None,
            None,
            None,
            None,
        ))
        .await
        .expect("create should succeed")
}
fn write_external_label(path: &std::path::Path, issue_id: &str, label: &str) {
    let source = std::fs::read_to_string(path).expect("JSONL source should be readable");
    let mut found = false;
    let records: Vec<Value> = source
        .lines()
        .map(|line| {
            let mut record: Value =
                serde_json::from_str(line).expect("canonical JSONL line should parse");
            if record["id"] == issue_id {
                record["labels"]
                    .as_array_mut()
                    .expect("canonical labels should be an array")
                    .push(Value::String(label.to_string()));
                found = true;
            }
            record
        })
        .collect();
    assert!(found, "sentinel Issue should exist before external edit");
    let rewritten = records
        .iter()
        .map(|record| serde_json::to_string(record).expect("record should serialize"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    std::fs::write(path, rewritten).expect("external JSONL edit should succeed");
    assert_persisted_label(path, issue_id, label);
}

fn assert_persisted_label(path: &std::path::Path, issue_id: &str, label: &str) {
    let source = std::fs::read_to_string(path).expect("JSONL source should be readable");
    let found = source.lines().any(|line| {
        let record: Value = serde_json::from_str(line).expect("canonical JSONL line should parse");
        record["id"] == issue_id
            && record["labels"]
                .as_array()
                .expect("canonical labels should be an array")
                .iter()
                .any(|persisted| persisted == label)
    });
    assert!(
        found,
        "external label {label} must survive mutation for {issue_id}"
    );
}

struct MutationFixture {
    _workspace: TempDir,
    issues_path: std::path::PathBuf,
    workspace_root: String,
    tools: Tools,
    sentinel: Issue,
    update_target: Issue,
    resource_target: Issue,
    lifecycle_target: Issue,
    dependent: Issue,
    prerequisite: Issue,
}

impl MutationFixture {
    async fn new() -> Self {
        let workspace = create_temp_workspace();
        let issues_path = workspace.path().join(".rivets/issues.jsonl");
        let workspace_root = workspace.path().display().to_string();
        let tools = create_tools();
        set_context(&tools, workspace.path()).await;
        let sentinel = create_issue(&tools, "External sentinel").await;
        let update_target = create_issue(&tools, "Update target").await;
        let resource_target = create_issue(&tools, "Resource target").await;
        let lifecycle_target = create_issue(&tools, "Lifecycle target").await;
        let dependent = create_issue(&tools, "Dependent target").await;
        let prerequisite = create_issue(&tools, "Prerequisite target").await;
        Self {
            _workspace: workspace,
            issues_path,
            workspace_root,
            tools,
            sentinel,
            update_target,
            resource_target,
            lifecycle_target,
            dependent,
            prerequisite,
        }
    }

    fn external_edit(&self, label: &str) {
        write_external_label(&self.issues_path, self.sentinel.id.as_str(), label);
    }

    fn assert_external_edit(&self, label: &str) {
        assert_persisted_label(&self.issues_path, self.sentinel.id.as_str(), label);
    }
}

async fn exercise_issue_mutations(fixture: &MutationFixture) {
    fixture.external_edit("external-create");
    fixture
        .tools
        .create(create_params(
            "Created after external edit".to_string(),
            None,
            None,
            Some("task"),
            None,
            None,
            None,
            None,
            None,
        ))
        .await
        .expect("create should refresh stale JSONL");
    fixture.assert_external_edit("external-create");

    fixture.external_edit("external-update");
    fixture
        .tools
        .update(update_params(
            fixture.update_target.id.as_str(),
            Some("Updated after external edit".to_string()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(&fixture.workspace_root),
        ))
        .await
        .expect("update should refresh stale JSONL");
    fixture.assert_external_edit("external-update");

    fixture.external_edit("external-note");
    fixture
        .tools
        .add_note(
            fixture.update_target.id.as_str(),
            "Note after external edit".to_string(),
            None,
        )
        .await
        .expect("add_note should refresh stale JSONL");
    fixture.assert_external_edit("external-note");
}

async fn exercise_resource_mutations(fixture: &MutationFixture) {
    fixture.external_edit("external-resource-add");
    fixture
        .tools
        .resource_add(
            fixture.resource_target.id.as_str(),
            Some("https://example.com/original".to_string()),
            None,
            "reference",
            None,
            Some(&fixture.workspace_root),
        )
        .await
        .expect("resource_add should refresh stale JSONL");
    fixture.assert_external_edit("external-resource-add");

    fixture.external_edit("external-resource-update");
    fixture
        .tools
        .resource_update(rivets_mcp::models::ResourceUpdateParams {
            issue_id: fixture.resource_target.id.to_string(),
            resource_id: "r1".to_string(),
            url: Some("https://example.com/updated".to_string()),
            path: None,
            role: None,
            label: None,
            clear_label: false,
            workspace_root: None,
        })
        .await
        .expect("resource_update should refresh stale JSONL");
    fixture.assert_external_edit("external-resource-update");

    fixture.external_edit("external-resource-remove");
    fixture
        .tools
        .resource_remove(
            fixture.resource_target.id.as_str(),
            "r1",
            Some(&fixture.workspace_root),
        )
        .await
        .expect("resource_remove should refresh stale JSONL");
    fixture.assert_external_edit("external-resource-remove");
}

async fn exercise_workflow_mutations(fixture: &MutationFixture) {
    fixture.external_edit("external-close");
    fixture
        .tools
        .close(fixture.lifecycle_target.id.as_str(), None, None)
        .await
        .expect("close should refresh stale JSONL");
    fixture.assert_external_edit("external-close");

    fixture.external_edit("external-reopen");
    fixture
        .tools
        .reopen(
            fixture.lifecycle_target.id.as_str(),
            None,
            Some(&fixture.workspace_root),
        )
        .await
        .expect("reopen should refresh stale JSONL");
    fixture.assert_external_edit("external-reopen");
}

async fn exercise_relationship_and_label_mutations(fixture: &MutationFixture) {
    fixture.external_edit("external-dependency-add");
    fixture
        .tools
        .blocking_dependency_add(
            fixture.dependent.id.as_str(),
            fixture.prerequisite.id.as_str(),
            None,
        )
        .await
        .expect("blocking_dependency_add should refresh stale JSONL");
    fixture.assert_external_edit("external-dependency-add");

    fixture.external_edit("external-dependency-remove");
    fixture
        .tools
        .blocking_dependency_remove(
            fixture.dependent.id.as_str(),
            fixture.prerequisite.id.as_str(),
            Some(&fixture.workspace_root),
        )
        .await
        .expect("blocking_dependency_remove should refresh stale JSONL");
    fixture.assert_external_edit("external-dependency-remove");

    fixture.external_edit("external-label-add");
    fixture
        .tools
        .label_add(fixture.update_target.id.as_str(), "mcp-added", None)
        .await
        .expect("label_add should refresh stale JSONL");
    fixture.assert_external_edit("external-label-add");

    fixture.external_edit("external-label-remove");
    fixture
        .tools
        .label_remove(
            fixture.update_target.id.as_str(),
            "mcp-added",
            Some(&fixture.workspace_root),
        )
        .await
        .expect("label_remove should refresh stale JSONL");
    fixture.assert_external_edit("external-label-remove");
}

#[tokio::test]
async fn stale_cache_mutations_preserve_external_jsonl_changes() {
    let fixture = MutationFixture::new().await;
    exercise_issue_mutations(&fixture).await;
    exercise_resource_mutations(&fixture).await;
    exercise_workflow_mutations(&fixture).await;
    exercise_relationship_and_label_mutations(&fixture).await;
}
