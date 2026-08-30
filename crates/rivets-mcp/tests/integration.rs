//! Integration tests for rivets-mcp server.
//!
//! These tests exercise the MCP tools with real JSONL storage backends
//! to verify end-to-end behavior including:
//! - Complete issue lifecycle (create -> update -> close)
//! - Multi-workspace context switching
//! - Error response verification
//! - Real storage persistence

use chrono::{DateTime, Utc};
use rivets::domain::{
    AssignmentError, BlockingDependency, DiscoveryOrigin, Issue, IssueKind, IssueStatus,
    RelatedAssociation, ResourceTarget, StatusTransitionError, WorkspacePath,
};
use rivets_mcp::context::Context;
use rivets_mcp::error::Error;
use rivets_mcp::models::{
    BlockingDependencyListQuery, BlockingDependencyTreeResponse, CreateParams, IssueKindInput,
    ListParams, ReadyParams, UpdateParams,
};
use rivets_mcp::tools::Tools;
use rmcp::model::Content;
use rstest::rstest;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::sync::Arc;
use tempfile::TempDir;
use tokio::sync::RwLock;

#[path = "../../rivets/tests/common/mixed_legacy.rs"]
mod mixed_legacy;

use mixed_legacy::{
    CONFLICT_ID, LEGACY_NOTE_ID, LEGACY_OPAQUE_ID, LEGACY_URL_ID, MIXED_ISSUE_COUNT,
    assert_canonical_records, fixture_records, read_records, record, seed_mixed_workspace,
};

fn kind_input(value: Option<&str>) -> IssueKindInput {
    // Parse through the domain FromStr so tests exercise the real
    // vocabulary instead of a parallel arm table.
    let issue_kind = value.map(|value| value.parse().expect("valid test Issue Kind"));
    IssueKindInput::canonical(issue_kind)
}

fn ready_params(
    limit: Option<usize>,
    priority: Option<u8>,
    issue_kind: Option<&str>,
    assignee: Option<String>,
    label: Option<String>,
    workspace_root: Option<&str>,
) -> ReadyParams {
    ReadyParams {
        limit,
        priority,
        kind: kind_input(issue_kind),
        assignee,
        all_assignees: false,
        label,
        workspace_root: workspace_root.map(str::to_string),
    }
}

fn list_params(
    status: Option<&str>,
    priority: Option<u8>,
    issue_kind: Option<&str>,
    assignee: Option<String>,
    label: Option<String>,
    limit: Option<usize>,
    workspace_root: Option<&str>,
) -> ListParams {
    ListParams {
        status: status.map(str::to_string),
        priority,
        kind: kind_input(issue_kind),
        assignee,
        label,
        limit,
        workspace_root: workspace_root.map(str::to_string),
    }
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

#[allow(clippy::too_many_arguments, clippy::needless_pass_by_value)]
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
    serde_json::from_value(serde_json::json!({
        "issue_id": issue_id,
        "status": status,
        "priority": priority,
        "issue_kind": issue_kind,
        "title": title,
        "description": description,
        "design": design,
        "acceptance_criteria": acceptance_criteria,
        "labels": labels,
        "workspace_root": workspace_root,
    }))
    .expect("update parameters should deserialize")
}

mod helpers {
    use super::*;
    use std::path::Path;

    /// Create a temporary workspace with `.rivets/` directory and config file.
    pub fn create_temp_workspace() -> TempDir {
        let temp = TempDir::new().expect("Failed to create temp dir");
        let rivets_dir = temp.path().join(".rivets");
        std::fs::create_dir(&rivets_dir).expect("Failed to create .rivets dir");

        // Create config.yaml with default prefix
        let config_content = r"issue-prefix: test
storage:
  backend: jsonl
  data_file: .rivets/issues.jsonl
";
        std::fs::write(rivets_dir.join("config.yaml"), config_content)
            .expect("Failed to create config.yaml");

        temp
    }

    /// Create Tools instance with empty context.
    pub fn create_tools() -> Tools {
        let context = Arc::new(RwLock::new(Context::new()));
        Tools::new(context)
    }

    /// Set the tools context to the given workspace path.
    pub async fn set_context(tools: &Tools, path: &Path) {
        tools
            .set_context(&path.display().to_string())
            .await
            .expect("set_context should succeed");
    }

    /// Create an issue and return it.
    pub async fn create_issue(tools: &Tools, title: &str) -> Issue {
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

    // =========================================================================
    // Test Case Structs for Parameterized Testing
    // =========================================================================

    /// Describes an issue to create for test setup.
    #[derive(Debug, Clone)]
    pub struct IssueSetup {
        pub title: &'static str,
        pub priority: Option<u8>,
        pub issue_kind: Option<&'static str>,
        pub assignee: Option<&'static str>,
        pub labels: Option<Vec<&'static str>>,
        pub close_after_create: bool,
    }

    impl IssueSetup {
        pub fn new(title: &'static str) -> Self {
            Self {
                title,
                priority: None,
                issue_kind: None,
                assignee: None,
                labels: None,
                close_after_create: false,
            }
        }

        pub fn with_priority(mut self, p: u8) -> Self {
            self.priority = Some(p);
            self
        }

        pub fn with_issue_kind(mut self, t: &'static str) -> Self {
            self.issue_kind = Some(t);
            self
        }

        pub fn with_assignee(mut self, a: &'static str) -> Self {
            self.assignee = Some(a);
            self
        }

        pub fn with_labels(mut self, l: Vec<&'static str>) -> Self {
            self.labels = Some(l);
            self
        }

        pub fn closed(mut self) -> Self {
            self.close_after_create = true;
            self
        }
    }

    /// Filter parameters for list/ready tests.
    #[derive(Debug, Clone, Default)]
    pub struct FilterParams {
        pub status: Option<&'static str>,
        pub priority: Option<u8>,
        pub issue_kind: Option<&'static str>,
        pub assignee: Option<&'static str>,
        pub label: Option<&'static str>,
        pub limit: Option<usize>,
    }

    impl FilterParams {
        pub fn new() -> Self {
            Self::default()
        }

        pub fn with_status(mut self, s: &'static str) -> Self {
            self.status = Some(s);
            self
        }

        pub fn with_priority(mut self, p: u8) -> Self {
            self.priority = Some(p);
            self
        }

        pub fn with_issue_kind(mut self, t: &'static str) -> Self {
            self.issue_kind = Some(t);
            self
        }

        pub fn with_assignee(mut self, a: &'static str) -> Self {
            self.assignee = Some(a);
            self
        }

        pub fn with_label(mut self, l: &'static str) -> Self {
            self.label = Some(l);
            self
        }

        pub fn with_limit(mut self, n: usize) -> Self {
            self.limit = Some(n);
            self
        }
    }

    /// Test case for list filter tests.
    #[derive(Debug, Clone)]
    pub struct ListFilterCase {
        pub setup: Vec<IssueSetup>,
        pub filter: FilterParams,
        pub expected_count: usize,
        pub expected_titles: Option<Vec<&'static str>>,
    }

    /// Test case for ready filter tests.
    #[derive(Debug, Clone)]
    pub struct ReadyFilterCase {
        pub setup: Vec<IssueSetup>,
        pub filter: FilterParams,
        pub expected_count: usize,
        pub expected_titles: Option<Vec<&'static str>>,
    }

    /// Create an issue with full customization.
    pub async fn create_custom_issue(tools: &Tools, setup: &IssueSetup) -> Issue {
        let labels = setup
            .labels
            .as_ref()
            .map(|l| l.iter().copied().map(str::to_string).collect());

        let issue = tools
            .create(create_params(
                setup.title.to_string(),
                Some(format!("Description for {}", setup.title)),
                setup.priority,
                setup.issue_kind,
                setup.assignee.map(str::to_string),
                labels,
                None,
                None,
                None,
            ))
            .await
            .expect("create should succeed");

        if setup.close_after_create {
            tools
                .close(issue.id.as_str(), None, None)
                .await
                .expect("Failed to close issue during setup");
            // Fetch updated issue after closing
            tools
                .show(issue.id.as_str(), None)
                .await
                .expect("Failed to fetch closed issue")
        } else {
            issue
        }
    }
}

use helpers::*;

/// Wire fields that carry RFC 3339 timestamps in the canonical Issue shape.
fn is_timestamp_key(key: &str) -> bool {
    matches!(key, "created_at" | "updated_at" | "closed_at")
}

fn normalize_wire_timestamps(value: &mut Value) {
    match value {
        Value::Array(values) => {
            for value in values {
                normalize_wire_timestamps(value);
            }
        }
        Value::Object(fields) => {
            for (key, value) in fields {
                if is_timestamp_key(key) && value.is_string() {
                    *value = Value::String("<timestamp>".to_string());
                } else {
                    normalize_wire_timestamps(value);
                }
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn timestamp_as_utc(value: &Value, field: &str) -> DateTime<Utc> {
    let raw = value
        .as_str()
        .unwrap_or_else(|| panic!("{field} must serialize as an RFC 3339 string"));
    assert!(
        raw.ends_with('Z'),
        "{field} must use the canonical UTC Z suffix: {raw}"
    );
    DateTime::parse_from_rfc3339(raw)
        .unwrap_or_else(|error| panic!("{field} must parse as RFC 3339: {error}"))
        .with_timezone(&Utc)
}
fn mcp_content_json<T: serde::Serialize>(value: &T) -> Value {
    let content = Content::json(value).expect("MCP Content::json should serialize");
    let text = content.as_text().expect("MCP JSON should use text content");
    serde_json::from_str(&text.text).expect("MCP JSON content should parse")
}

fn assert_and_count_utc_timestamps(value: &Value) -> usize {
    match value {
        Value::Array(values) => values.iter().map(assert_and_count_utc_timestamps).sum(),
        Value::Object(fields) => fields
            .iter()
            .map(|(key, value)| {
                if is_timestamp_key(key) {
                    match value {
                        Value::Null => 0,
                        Value::String(_) => {
                            timestamp_as_utc(value, key);
                            1
                        }
                        _ => panic!("{key} must serialize as a string or null"),
                    }
                } else {
                    assert_and_count_utc_timestamps(value)
                }
            })
            .sum(),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => 0,
    }
}

async fn create_golden_issue(tools: &Tools) -> Issue {
    let mut issue = tools
        .create(CreateParams {
            title: "Golden wire Issue".to_string(),
            description: Some("Every serializable field is populated.".to_string()),
            priority: Some(1),
            kind: kind_input(Some("feature")),
            assignee: Some("golden-owner".to_string()),
            labels: Some(vec!["golden".to_string(), "wire".to_string()]),
            design: Some("Pin the canonical Issue wire shape.".to_string()),
            acceptance: Some("- [x] Exact fields\n- [x] Stable nested arrays".to_string()),
            initial_note: Some("Initial context".to_string()),
            workspace_root: None,
        })
        .await
        .expect("golden Issue should be created");

    for note in ["Second finding", "Third finding", "Fourth finding"] {
        issue = tools
            .add_note(issue.id.as_str(), note.to_string(), None)
            .await
            .expect("golden Note should append");
    }

    for (url, path, role, label) in [
        (
            Some("https://example.com/implementation"),
            None,
            "implementation",
            Some("Implementation source"),
        ),
        (
            None,
            Some("docs/space path.md"),
            "documentation",
            Some("Documentation path"),
        ),
        (
            Some("https://example.com/evidence"),
            None,
            "evidence",
            Some("Evidence source"),
        ),
        (None, Some("docs/successor.md"), "successor", None),
        (
            Some("https://example.com/reference"),
            None,
            "reference",
            Some("Reference source"),
        ),
    ] {
        issue = tools
            .resource_add(
                issue.id.as_str(),
                url.map(str::to_string),
                path.map(str::to_string),
                role,
                label.map(str::to_string),
                None,
            )
            .await
            .expect("golden resource should be added");
    }

    tools
        .close(issue.id.as_str(), None, None)
        .await
        .expect("golden Issue should close without an extra Note")
}
async fn reload_golden_issue(workspace: &TempDir, issue_id: &str) -> Issue {
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;
    tools
        .show(issue_id, None)
        .await
        .expect("reloaded golden Issue should exist")
}

fn cli_json_for_issue(issue: &Issue) -> Value {
    let mut output = Vec::new();
    rivets::output::print_issues_to(
        &mut output,
        std::slice::from_ref(issue),
        rivets::output::OutputMode::Json,
    )
    .expect("CLI list JSON should serialize");
    serde_json::from_slice(&output).expect("CLI list should emit JSON")
}
#[tokio::test]
async fn mcp_full_issue_json_golden() {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;
    let created = create_golden_issue(&tools).await;
    let issue = reload_golden_issue(&workspace, created.id.as_str()).await;

    let mut actual = mcp_content_json(&issue);
    normalize_wire_timestamps(&mut actual);

    let expected = json!({
        "id": issue.id,
        "title": "Golden wire Issue",
        "description": "Every serializable field is populated.",
        "status": "closed",
        "priority": 1,
        "issue_kind": "feature",
        "assignee": null,
        "labels": ["golden", "wire"],
        "design": "Pin the canonical Issue wire shape.",
        "acceptance_criteria": "- [x] Exact fields\n- [x] Stable nested arrays",
        "notes": [
            {"content": "Initial context", "created_at": "<timestamp>"},
            {"content": "Second finding", "created_at": "<timestamp>"},
            {"content": "Third finding", "created_at": "<timestamp>"},
            {"content": "Fourth finding", "created_at": "<timestamp>"},
        ],
        "resources": [
            {
                "id": "r1",
                "target": {"type": "web", "url": "https://example.com/implementation"},
                "role": "implementation",
                "label": "Implementation source",
            },
            {
                "id": "r2",
                "target": {"type": "path", "path": "docs/space path.md"},
                "role": "documentation",
                "label": "Documentation path",
            },
            {
                "id": "r3",
                "target": {"type": "web", "url": "https://example.com/evidence"},
                "role": "evidence",
                "label": "Evidence source",
            },
            {
                "id": "r4",
                "target": {"type": "path", "path": "docs/successor.md"},
                "role": "successor",
                "label": null,
            },
            {
                "id": "r5",
                "target": {"type": "web", "url": "https://example.com/reference"},
                "role": "reference",
                "label": "Reference source",
            },
        ],
        "created_at": "<timestamp>",
        "updated_at": "<timestamp>",
        "closed_at": "<timestamp>",
    });

    assert_eq!(actual, expected);
    assert!(actual.get("next_resource_id").is_none());
}

#[tokio::test]
async fn mcp_timestamps_use_z_suffix() {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;
    let created = create_golden_issue(&tools).await;
    let issue = reload_golden_issue(&workspace, created.id.as_str()).await;
    let wire = mcp_content_json(&issue);

    assert_eq!(assert_and_count_utc_timestamps(&wire), 7);
    assert_eq!(
        timestamp_as_utc(&wire["created_at"], "Issue.created_at"),
        issue.created_at
    );
    assert_eq!(
        timestamp_as_utc(&wire["updated_at"], "Issue.updated_at"),
        issue.updated_at
    );
    assert_eq!(
        timestamp_as_utc(&wire["closed_at"], "Issue.closed_at"),
        issue.closed_at.expect("golden Issue should be closed")
    );
    for (wire_note, note) in wire["notes"]
        .as_array()
        .expect("notes should be an array")
        .iter()
        .zip(issue.notes())
    {
        assert_eq!(
            timestamp_as_utc(&wire_note["created_at"], "Note.created_at"),
            *note.created_at()
        );
    }
}

#[tokio::test]
async fn cli_and_mcp_issue_json_shapes_match() {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;
    let created = create_golden_issue(&tools).await;
    let mcp = reload_golden_issue(&workspace, created.id.as_str()).await;
    let mcp_json = mcp_content_json(&mcp);
    let cli_json = cli_json_for_issue(&mcp);
    let cli_issues = cli_json
        .as_array()
        .expect("CLI list JSON should be an array");

    assert_eq!(cli_issues.len(), 1);
    assert_eq!(&cli_issues[0], &mcp_json);
}

// ============================================================================
// Issue Lifecycle Tests
// ============================================================================

/// ADR-0005: the domain rejects closing an already-closed Issue, and MCP
/// surfaces the identical observable message the CLI prints.
#[tokio::test]
async fn close_rejects_already_closed_issue_without_mutation() {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;
    let issue = create_issue(&tools, "Close twice").await;
    let closed = tools
        .close(issue.id.as_str(), Some("Done".to_string()), None)
        .await
        .expect("first close should succeed");

    let rejected = tools
        .close(issue.id.as_str(), Some("Again".to_string()), None)
        .await
        .expect_err("second close must be rejected");
    assert!(
        matches!(
            &rejected,
            Error::InvalidStatusTransition(StatusTransitionError::AlreadyClosed {
                current: IssueStatus::Closed
            })
        ),
        "unexpected error: {rejected:?}"
    );
    assert_eq!(
        rejected.to_string(),
        "Issue is already closed (status: closed)",
        "MCP must surface the domain message the CLI prints"
    );

    let unchanged = tools
        .show(issue.id.as_str(), None)
        .await
        .expect("show should succeed after a rejected close");
    assert_eq!(unchanged.status, IssueStatus::Closed);
    assert_eq!(
        unchanged.closed_at, closed.closed_at,
        "a rejected close must not touch closed_at"
    );
    assert_eq!(
        unchanged.notes().len(),
        closed.notes().len(),
        "a rejected close must not append its Note"
    );
}

/// ADR-0005: reopening an already Open Issue remains a typed rejection.
#[tokio::test]
async fn reopen_rejects_open_issue_without_mutation() {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;
    let issue = create_issue(&tools, "Already open").await;

    let rejected = tools
        .reopen(issue.id.as_str(), Some("Not done yet".to_string()), None)
        .await
        .expect_err("reopening an Open Issue must be rejected");
    assert!(matches!(
        &rejected,
        Error::InvalidStatusTransition(StatusTransitionError::NotClosed {
            current: IssueStatus::Open
        })
    ));
    assert_eq!(
        rejected.to_string(),
        "Issue is not closed (status: open)",
        "MCP must surface the domain message the CLI prints"
    );

    let unchanged = tools
        .show(issue.id.as_str(), None)
        .await
        .expect("show should succeed after a rejected reopen");
    assert_eq!(unchanged.status, IssueStatus::Open);
    assert_eq!(
        unchanged.notes().len(),
        0,
        "a rejected reopen must not append its Note"
    );
}

/// Dedicated Reopen is Closed-only; generic Update retains the active-to-Open path.
#[tokio::test]
async fn reopen_rejects_in_progress_while_generic_update_returns_to_open() {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;
    let issue = tools
        .create(create_params(
            "Active work".to_string(),
            None,
            None,
            None,
            Some("active-owner".to_string()),
            None,
            None,
            None,
            None,
        ))
        .await
        .expect("create should succeed");
    let active = tools
        .update(update_params(
            issue.id.as_str(),
            None,
            None,
            Some("in_progress"),
            None,
            None,
            None,
            None,
            None,
            None,
        ))
        .await
        .expect("status setup should succeed");

    let rejected = tools
        .reopen(issue.id.as_str(), Some("Paused".to_string()), None)
        .await
        .expect_err("dedicated Reopen must reject In Progress");
    assert!(matches!(
        rejected,
        Error::InvalidStatusTransition(StatusTransitionError::NotClosed {
            current: IssueStatus::InProgress
        })
    ));
    let unchanged = tools
        .show(issue.id.as_str(), None)
        .await
        .expect("rejected Reopen target should remain readable");
    assert_eq!(unchanged.status, IssueStatus::InProgress);
    assert_eq!(unchanged.assignee, active.assignee);
    assert!(unchanged.notes().is_empty());

    let returned = tools
        .update(update_params(
            issue.id.as_str(),
            None,
            None,
            Some("open"),
            None,
            None,
            None,
            None,
            None,
            None,
        ))
        .await
        .expect("generic Update should retain active-to-Open transition");
    assert_eq!(returned.status, IssueStatus::Open);
    assert_eq!(returned.assignee, active.assignee);
}
#[tokio::test]
async fn claim_release_contract_survives_context_restart() {
    let workspace = create_temp_workspace();
    let workspace_root = workspace.path().to_string_lossy().into_owned();
    let tools = create_tools();

    assert!(matches!(
        tools.claim("test-missing", "alice", None).await,
        Err(Error::NoContext)
    ));

    let created = tools
        .create(create_params(
            "MCP Claim target".to_string(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(&workspace_root),
        ))
        .await
        .expect("explicit Workspace create should succeed");
    let claimed = tools
        .claim(created.id.as_str(), "alice", Some(&workspace_root))
        .await
        .expect("explicit Workspace Claim should succeed");
    assert_eq!(claimed.assignee.as_deref(), Some("alice"));
    let claimed_at = claimed.updated_at;

    let recreated = create_tools();
    set_context(&recreated, workspace.path()).await;
    let durable = recreated
        .show(created.id.as_str(), None)
        .await
        .expect("recreated context should load durable Claim");
    assert_eq!(durable.assignee.as_deref(), Some("alice"));
    assert_eq!(
        recreated
            .claim(created.id.as_str(), "alice", None)
            .await
            .expect("owner retry should be idempotent")
            .updated_at,
        claimed_at
    );

    let conflict = recreated.claim(created.id.as_str(), "bob", None).await;
    assert!(matches!(
        &conflict,
        Err(Error::Assignment(AssignmentError::AlreadyClaimed {
            issue_id,
            assignee,
        })) if issue_id == &created.id && assignee == "alice"
    ));
    let mismatch = recreated.release(created.id.as_str(), "bob", None).await;
    assert!(matches!(
        &mismatch,
        Err(Error::Assignment(AssignmentError::AssigneeMismatch {
            issue_id,
            expected,
            actual,
        })) if issue_id == &created.id && expected == "bob" && actual == "alice"
    ));

    let released = recreated
        .release(created.id.as_str(), "alice", None)
        .await
        .expect("exact owner should release");
    assert_eq!(released.assignee, None);
    let final_context = create_tools();
    set_context(&final_context, workspace.path()).await;
    assert_eq!(
        final_context
            .show(created.id.as_str(), None)
            .await
            .expect("release should persist")
            .assignee,
        None
    );
}

#[tokio::test]
async fn claim_and_release_reject_blank_assignees_without_mutation() {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;
    let target = create_issue(&tools, "Blank Assignment target").await;

    for assignee in ["", " \t "] {
        for result in [
            tools.claim(target.id.as_str(), assignee, None).await,
            tools.release(target.id.as_str(), assignee, None).await,
        ] {
            assert!(matches!(
                result,
                Err(Error::Assignment(AssignmentError::BlankAssignee { ref issue_id }))
                    if issue_id == &target.id
            ));
        }
    }
    assert_eq!(
        tools
            .show(target.id.as_str(), None)
            .await
            .expect("blank-input target should remain readable")
            .assignee,
        None
    );
}

#[tokio::test]
async fn claim_release_mcp_state_matrix() {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;

    let missing = tools.claim("test-missing", "alice", None).await;
    assert!(matches!(
        &missing,
        Err(Error::IssueNotFound(issue_id)) if issue_id == "test-missing"
    ));

    let unassigned = create_issue(&tools, "Unassigned release").await;
    let not_claimed = tools.release(unassigned.id.as_str(), "alice", None).await;
    assert!(matches!(
        &not_claimed,
        Err(Error::Assignment(AssignmentError::NotClaimed { issue_id }))
            if issue_id == &unassigned.id
    ));

    let prerequisite = create_issue(&tools, "Open prerequisite").await;
    let blocked = create_issue(&tools, "Blocked Claim").await;
    tools
        .blocking_dependency_add(blocked.id.as_str(), prerequisite.id.as_str(), None)
        .await
        .expect("Blocking Dependency should be added");
    let blocked_claim = tools.claim(blocked.id.as_str(), "alice", None).await;
    assert!(matches!(
        &blocked_claim,
        Err(Error::Assignment(AssignmentError::Blocked { issue_id }))
            if issue_id == &blocked.id
    ));

    let releasable = create_issue(&tools, "Blocked release").await;
    tools
        .claim(releasable.id.as_str(), "alice", None)
        .await
        .expect("initial Claim should succeed");
    tools
        .blocking_dependency_add(releasable.id.as_str(), prerequisite.id.as_str(), None)
        .await
        .expect("Issue should become blocked after Claim");
    assert_eq!(
        tools
            .release(releasable.id.as_str(), "alice", None)
            .await
            .expect("blocked Open owner should release")
            .assignee,
        None
    );

    let active = tools
        .create(create_params(
            "Active target".to_string(),
            None,
            None,
            None,
            Some("active-owner".to_string()),
            None,
            None,
            None,
            None,
        ))
        .await
        .expect("assigned active target should be created");
    tools
        .update(update_params(
            active.id.as_str(),
            None,
            None,
            Some("in_progress"),
            None,
            None,
            None,
            None,
            None,
            None,
        ))
        .await
        .expect("assigned target should enter In Progress");
    for result in [
        tools.claim(active.id.as_str(), "active-owner", None).await,
        tools
            .release(active.id.as_str(), "active-owner", None)
            .await,
    ] {
        assert!(matches!(
            &result,
            Err(Error::Assignment(AssignmentError::NotOpen {
                issue_id,
                status: IssueStatus::InProgress,
            })) if issue_id == &active.id
        ));
    }
}
/// Test complete issue lifecycle: create -> update -> close
#[tokio::test]
async fn test_issue_lifecycle_create_update_close() {
    let workspace = create_temp_workspace();
    let tools = create_tools();

    // Set context
    set_context(&tools, workspace.path()).await;

    // Create issue
    let created = tools
        .create(create_params(
            "Lifecycle Test Issue".to_string(),
            None,
            None,
            None,
            Some("alice".to_string()),
            None,
            None,
            None,
            None,
        ))
        .await
        .expect("create should succeed");
    assert_eq!(created.status, IssueStatus::Open);

    // Update to in_progress
    let updated = tools
        .update(update_params(
            created.id.as_str(),
            None,
            None,
            Some("in_progress"),
            Some(1),
            None, // issue_kind
            None,
            None,
            None, // labels
            None, // workspace_root
        ))
        .await
        .expect("update should succeed");

    assert_eq!(updated.status, IssueStatus::InProgress);
    assert_eq!(updated.priority, 1);
    assert_eq!(updated.assignee, Some("alice".to_string()));

    // Close the issue
    let closed = tools
        .close(
            created.id.as_str(),
            Some("Completed successfully".to_string()),
            None,
        )
        .await
        .expect("close should succeed");

    assert_eq!(closed.status, IssueStatus::Closed);

    // Verify via show
    let shown = tools
        .show(created.id.as_str(), None)
        .await
        .expect("show should succeed");
    assert_eq!(shown.status, IssueStatus::Closed);
}

#[tokio::test]
async fn test_notes_create_append_validate_and_survive_context_restart() {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;

    let created = tools
        .create(CreateParams {
            title: "Note history".to_string(),
            description: None,
            priority: None,
            kind: kind_input(None),
            assignee: None,
            labels: None,
            design: None,
            acceptance: None,
            initial_note: Some("Initial context".to_string()),
            workspace_root: None,
        })
        .await
        .expect("create with an initial Note should succeed");
    assert_eq!(created.notes().len(), 1);
    assert_eq!(created.notes()[0].content(), "Initial context");
    assert_eq!(*created.notes()[0].created_at(), created.updated_at);

    let appended = tools
        .add_note(created.id.as_str(), "Second finding".to_string(), None)
        .await
        .expect("add_note should append");
    assert_eq!(appended.notes().len(), 2);
    assert_eq!(appended.notes()[0], created.notes()[0]);
    assert_eq!(appended.notes()[1].content(), "Second finding");
    assert_eq!(*appended.notes()[1].created_at(), appended.updated_at);

    let empty = tools
        .add_note(created.id.as_str(), " \n ".to_string(), None)
        .await;
    assert!(matches!(empty, Err(Error::InvalidNote(_))));

    let empty_close = tools
        .close(created.id.as_str(), Some(" \n ".to_string()), None)
        .await;
    assert!(matches!(empty_close, Err(Error::InvalidNote(_))));
    let unchanged = tools
        .show(created.id.as_str(), None)
        .await
        .expect("rejected close reason must leave the Issue unchanged");
    assert_eq!(unchanged.status, IssueStatus::Open);
    assert_eq!(unchanged.notes(), appended.notes());

    let closed = tools
        .close(created.id.as_str(), Some("Completed".to_string()), None)
        .await
        .expect("close reason should append a Note");
    assert_eq!(closed.notes().len(), 3);
    assert_eq!(closed.notes()[2].content(), "Closed: Completed");
    assert_eq!(*closed.notes()[2].created_at(), closed.updated_at);
    assert_eq!(
        *closed.notes()[2].created_at(),
        closed
            .closed_at
            .expect("closed Issue should have closed_at")
    );

    let reopened = tools
        .reopen(
            created.id.as_str(),
            Some("Needs more work".to_string()),
            None,
        )
        .await
        .expect("reopen reason should append a Note");
    assert_eq!(reopened.notes().len(), 4);
    assert_eq!(reopened.notes()[3].content(), "Reopened: Needs more work");
    assert_eq!(*reopened.notes()[3].created_at(), reopened.updated_at);

    let restarted = create_tools();
    set_context(&restarted, workspace.path()).await;
    let shown = restarted
        .show(created.id.as_str(), None)
        .await
        .expect("restarted context should load Notes");
    assert_eq!(shown.notes(), reopened.notes());
}

/// Test issue creation with all optional fields.
#[tokio::test]
async fn test_create_issue_with_all_fields() {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;

    let issue = tools
        .create(create_params(
            "Full Issue".to_string(),
            Some("Detailed description".to_string()),
            Some(0),
            Some("feature"),
            Some("bob".to_string()),
            Some(vec!["urgent".to_string(), "frontend".to_string()]),
            Some("Technical design notes".to_string()),
            Some("- [ ] Criteria 1\n- [ ] Criteria 2".to_string()),
            None,
        ))
        .await
        .expect("create should succeed");

    assert_eq!(issue.title, "Full Issue");
    assert_eq!(issue.description, "Detailed description");
    assert_eq!(issue.priority, 0);
    assert_eq!(issue.issue_kind, IssueKind::Feature);
    assert_eq!(issue.assignee, Some("bob".to_string()));
    assert_eq!(issue.design, Some("Technical design notes".to_string()));
    assert_eq!(
        issue.acceptance_criteria,
        Some("- [ ] Criteria 1\n- [ ] Criteria 2".to_string())
    );
}

/// Test creating, returning, filtering, and persisting every Issue Kind.
#[tokio::test]
async fn test_create_all_issue_kinds() {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;

    let kinds = ["bug", "feature", "task", "epic", "chore"];

    for issue_kind in kinds {
        let issue = tools
            .create(create_params(
                format!("A {issue_kind}"),
                None,
                None,
                Some(issue_kind),
                None,
                None,
                None,
                None,
                None,
            ))
            .await
            .expect("create should succeed");

        let expected_kind: IssueKind = issue_kind.parse().expect("valid Issue Kind");
        assert_eq!(issue.issue_kind, expected_kind);
        let response = serde_json::to_value(&issue).expect("MCP Issue should serialize");
        assert_eq!(response["issue_kind"], issue_kind);
        assert!(response.get("issue_type").is_none());

        let filtered = tools
            .list(list_params(
                None,
                None,
                Some(issue_kind),
                None,
                None,
                None,
                None,
            ))
            .await
            .expect("kind filter should succeed");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].issue_kind, expected_kind);
    }

    drop(tools);
    let restarted = create_tools();
    set_context(&restarted, workspace.path()).await;
    let list = restarted
        .list(list_params(None, None, None, None, None, None, None))
        .await
        .expect("list after restart should succeed");
    assert_eq!(list.len(), 5);
    for issue_kind in kinds {
        let expected_kind: IssueKind = issue_kind.parse().expect("valid Issue Kind");
        assert!(list.iter().any(|issue| issue.issue_kind == expected_kind));
    }
}

#[tokio::test]
async fn test_update_reclassifies_only_kind_and_persists_across_context_restart() {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;

    let created = create_custom_issue(
        &tools,
        &IssueSetup::new("Reclassify through MCP")
            .with_issue_kind("task")
            .with_priority(1)
            .with_assignee("agent")
            .with_labels(vec!["backend", "ready"]),
    )
    .await;

    let updated = tools
        .update(update_params(
            created.id.as_str(),
            None,
            None,
            None,
            None,
            Some("bug"),
            None,
            None,
            None,
            None,
        ))
        .await
        .expect("kind update should succeed");

    assert_eq!(updated.issue_kind, IssueKind::Bug);
    assert_ne!(created.updated_at, updated.updated_at);
    let mut before = serde_json::to_value(&created)
        .expect("MCP Issue should serialize")
        .as_object()
        .expect("MCP Issue should serialize as an object")
        .clone();
    let mut after = serde_json::to_value(&updated)
        .expect("MCP Issue should serialize")
        .as_object()
        .expect("MCP Issue should serialize as an object")
        .clone();
    before.remove("issue_kind");
    before.remove("updated_at");
    after.remove("issue_kind");
    after.remove("updated_at");
    assert_eq!(before, after);

    let persisted = std::fs::read_to_string(workspace.path().join(".rivets/issues.jsonl"))
        .expect("persisted issues should be readable");
    let record: serde_json::Value = persisted
        .lines()
        .map(|line| serde_json::from_str(line).expect("persisted record should be JSON"))
        .find(|record: &serde_json::Value| record["id"] == created.id.as_str())
        .expect("updated issue should remain persisted");
    assert_eq!(record["issue_kind"], "bug");
    assert!(record.get("issue_type").is_none());

    drop(tools);
    let restarted = create_tools();
    set_context(&restarted, workspace.path()).await;
    let reloaded = restarted
        .show(created.id.as_str(), None)
        .await
        .expect("reclassified issue should survive context restart");
    assert_eq!(reloaded.issue_kind, IssueKind::Bug);
}

// ============================================================================
// Multi-Workspace Tests
// ============================================================================

/// Test switching between multiple workspaces.
#[tokio::test]
async fn test_multi_workspace_context_switching() {
    let workspace_a = create_temp_workspace();
    let workspace_b = create_temp_workspace();
    let tools = create_tools();

    // Create issue in workspace A
    set_context(&tools, workspace_a.path()).await;
    create_issue(&tools, "Issue in Workspace A").await;

    // Switch to workspace B and create issue
    set_context(&tools, workspace_b.path()).await;
    create_issue(&tools, "Issue in Workspace B").await;

    // Verify workspace B has only one issue
    let issues_b = tools
        .list(list_params(None, None, None, None, None, None, None))
        .await
        .expect("list should succeed");
    assert_eq!(issues_b.len(), 1);
    assert_eq!(issues_b[0].title, "Issue in Workspace B");

    // Switch back to workspace A
    set_context(&tools, workspace_a.path()).await;
    let issues_a = tools
        .list(list_params(None, None, None, None, None, None, None))
        .await
        .expect("list should succeed");
    assert_eq!(issues_a.len(), 1);
    assert_eq!(issues_a[0].title, "Issue in Workspace A");
}

/// Test using `workspace_root` parameter to access different workspace without switching context.
#[tokio::test]
async fn test_workspace_root_parameter_override() {
    let workspace_a = create_temp_workspace();
    let workspace_b = create_temp_workspace();
    let tools = create_tools();

    // Set up both workspaces
    set_context(&tools, workspace_a.path()).await;
    create_issue(&tools, "Issue A").await;

    set_context(&tools, workspace_b.path()).await;
    create_issue(&tools, "Issue B").await;

    // Current context is B, but query A using workspace_root parameter
    let issues_a = tools
        .list(list_params(
            None,
            None,
            None,
            None,
            None,
            None,
            Some(&workspace_a.path().display().to_string()),
        ))
        .await
        .expect("list should succeed");

    assert_eq!(issues_a.len(), 1);
    assert_eq!(issues_a[0].title, "Issue A");
}

/// Test workspace isolation - issues in one workspace don't appear in another.
#[tokio::test]
async fn test_workspace_isolation() {
    let workspace_a = create_temp_workspace();
    let workspace_b = create_temp_workspace();
    let tools = create_tools();

    // Create 3 issues in workspace A
    set_context(&tools, workspace_a.path()).await;
    for i in 1..=3 {
        create_issue(&tools, &format!("A-Issue-{i}")).await;
    }

    // Create 2 issues in workspace B
    set_context(&tools, workspace_b.path()).await;
    for i in 1..=2 {
        create_issue(&tools, &format!("B-Issue-{i}")).await;
    }

    // Verify counts
    let issues_b = tools
        .list(list_params(None, None, None, None, None, None, None))
        .await
        .unwrap();
    assert_eq!(issues_b.len(), 2);

    set_context(&tools, workspace_a.path()).await;
    let issues_a = tools
        .list(list_params(None, None, None, None, None, None, None))
        .await
        .unwrap();
    assert_eq!(issues_a.len(), 3);
}

// ============================================================================
// Error Response Tests
// ============================================================================

/// Test error response for no context set.
#[tokio::test]
async fn test_error_no_context() {
    let tools = create_tools();

    let result = tools
        .list(list_params(None, None, None, None, None, None, None))
        .await;

    assert!(result.is_err());
    match result.unwrap_err() {
        Error::NoContext => {} // Expected
        e => panic!("Expected NoContext error, got: {e:?}"),
    }
}

/// Test error response for invalid status value.
#[tokio::test]
async fn test_error_invalid_status() {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;

    let result = tools
        .list(list_params(
            Some("not_a_status"),
            None,
            None,
            None,
            None,
            None,
            None,
        ))
        .await;

    assert!(result.is_err());
    match result.unwrap_err() {
        Error::InvalidArgument { field, value, .. } => {
            assert_eq!(field, "status");
            assert_eq!(value, "not_a_status");
        }
        e => panic!("Expected InvalidArgument error, got: {e:?}"),
    }
}

/// Test error response for issue not found.
#[tokio::test]
async fn test_error_issue_not_found() {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;

    let result = tools.show("nonexistent-123", None).await;

    assert!(result.is_err());
    match result.unwrap_err() {
        Error::IssueNotFound(id) => {
            assert_eq!(id, "nonexistent-123");
        }
        e => panic!("Expected IssueNotFound error, got: {e:?}"),
    }
}

/// Test error response for workspace not found.
#[tokio::test]
async fn test_error_workspace_not_found() {
    let tools = create_tools();

    let result = tools.set_context("/nonexistent/path/to/workspace").await;

    assert!(result.is_err());
    match result.unwrap_err() {
        Error::WorkspaceNotFound { path, .. } => {
            assert!(path.contains("nonexistent"));
        }
        e => panic!("Expected WorkspaceNotFound error, got: {e:?}"),
    }
}

/// Test per-call `workspace_root` rejects a workspace without a `.rivets` directory.
#[tokio::test]
async fn test_error_no_rivets_directory() {
    let temp = TempDir::new().expect("temporary workspace should be created");
    let tools = create_tools();
    let workspace_root = temp.path().display().to_string();
    let expected_workspace_root = temp
        .path()
        .canonicalize()
        .expect("temporary workspace should canonicalize")
        .display()
        .to_string();

    let result = tools
        .list(list_params(
            None,
            None,
            None,
            None,
            None,
            None,
            Some(&workspace_root),
        ))
        .await;

    match result {
        Err(Error::NoRivetsDirectory(path)) => assert_eq!(path, expected_workspace_root),
        Err(error) => panic!("Expected NoRivetsDirectory, got {error:?}"),
        Ok(_) => panic!("Expected error, got Ok"),
    }
}

/// Test `workspace_root` initializes a valid workspace without setting context.
#[tokio::test]
async fn test_workspace_root_initializes_without_context() {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    let workspace_root = workspace.path().display().to_string();

    let created = tools
        .create(create_params(
            "Created without context".to_string(),
            None,
            None,
            Some("task"),
            None,
            None,
            None,
            None,
            Some(&workspace_root),
        ))
        .await
        .expect("create should initialize workspace_root");

    assert_eq!(created.title, "Created without context");
    let issues = tools
        .list(list_params(
            None,
            None,
            None,
            None,
            None,
            None,
            Some(&workspace_root),
        ))
        .await
        .expect("list should reuse initialized workspace_root");
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].id, created.id);
    let context = tools
        .where_am_i()
        .await
        .expect("where_am_i should remain available");
    assert!(!context.context_set);
}

/// Test concurrent first use serializes uncached writers and preserves both Issues.
#[tokio::test]
async fn test_concurrent_workspace_root_initialization() {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    let workspace_root = workspace.path().display().to_string();
    let params = |title: &str| {
        create_params(
            title.to_string(),
            None,
            None,
            Some("task"),
            None,
            None,
            None,
            None,
            Some(&workspace_root),
        )
    };

    let (first, second) = tokio::join!(
        tools.create(params("First concurrent issue")),
        tools.create(params("Second concurrent issue"))
    );
    let _ = match (first, second) {
        (Ok(first), Ok(second)) => {
            assert_ne!(first.id, second.id);
            first
        }
        (
            Ok(winner),
            Err(Error::WorkspaceBusy {
                workspace_root: busy,
            }),
        )
        | (
            Err(Error::WorkspaceBusy {
                workspace_root: busy,
            }),
            Ok(winner),
        ) => {
            assert_eq!(
                busy,
                workspace
                    .path()
                    .canonicalize()
                    .expect("contended Workspace path should canonicalize")
            );
            let retry_title = if winner.title == "First concurrent issue" {
                "Second concurrent issue"
            } else {
                "First concurrent issue"
            };
            let retried = tools
                .create(params(retry_title))
                .await
                .expect("retry should succeed after the winner releases");
            assert_ne!(winner.id, retried.id);
            winner
        }
        (first, second) => {
            panic!(
                "expected two serialized creates or one WorkspaceBusy, got {first:?} and {second:?}"
            )
        }
    };
    let issues = tools
        .list(list_params(
            None,
            None,
            None,
            None,
            None,
            None,
            Some(&workspace_root),
        ))
        .await
        .expect("list should use the shared initialized storage");
    assert_eq!(issues.len(), 2);
}

/// Test explicit workspace caching cannot evict the active default context.
#[tokio::test]
async fn test_explicit_workspace_cache_eviction_preserves_current_context() {
    let current_workspace = create_temp_workspace();
    let explicit_workspaces: Vec<_> = (0..32).map(|_| create_temp_workspace()).collect();
    let tools = create_tools();
    set_context(&tools, current_workspace.path()).await;
    create_issue(&tools, "Current workspace issue").await;

    for workspace in &explicit_workspaces {
        let workspace_root = workspace.path().display().to_string();
        tools
            .list(list_params(
                None,
                None,
                None,
                None,
                None,
                None,
                Some(&workspace_root),
            ))
            .await
            .expect("explicit workspace should initialize");
    }

    let context = tools
        .where_am_i()
        .await
        .expect("where_am_i should report the current context");
    assert!(context.context_set);
    assert!(
        context.database_path.is_some(),
        "current workspace metadata should remain cached"
    );

    let current_issues = tools
        .list(list_params(None, None, None, None, None, None, None))
        .await
        .expect("current context should remain cached");
    assert_eq!(current_issues.len(), 1);
    assert_eq!(current_issues[0].title, "Current workspace issue");
}

// ============================================================================
// Dependency Tests
// ============================================================================
fn relationship_value(dependent: &Issue, prerequisite: &Issue) -> Value {
    json!({
        "dependent_id": dependent.id,
        "prerequisite_id": prerequisite.id
    })
}

fn assert_relationship_wire(actual: &BlockingDependency, dependent: &Issue, prerequisite: &Issue) {
    assert_eq!(actual.dependent_id(), &dependent.id);
    assert_eq!(actual.prerequisite_id(), &prerequisite.id);
    assert_eq!(
        serde_json::to_value(actual).expect("relationship should serialize"),
        relationship_value(dependent, prerequisite)
    );
}

fn assert_relationship_list_wire(
    actual: &[BlockingDependency],
    expected: &[(&Issue, &Issue)],
    sort_key: &str,
) {
    let mut expected = expected
        .iter()
        .map(|(dependent, prerequisite)| relationship_value(dependent, prerequisite))
        .collect::<Vec<_>>();
    expected.sort_by(|left, right| left[sort_key].as_str().cmp(&right[sort_key].as_str()));
    assert_eq!(
        serde_json::to_value(actual).expect("relationships should serialize"),
        Value::Array(expected)
    );
}

fn assert_tree_wire(
    actual: &BlockingDependencyTreeResponse,
    dependent: &Issue,
    prerequisites: [&Issue; 2],
) {
    let mut expected = prerequisites
        .map(|prerequisite| {
            let mut row = relationship_value(dependent, prerequisite);
            row["depth"] = json!(1);
            row
        })
        .to_vec();
    expected.sort_by(|left, right| {
        left["prerequisite_id"]
            .as_str()
            .cmp(&right["prerequisite_id"].as_str())
    });
    assert_eq!(
        serde_json::to_value(actual).expect("tree should serialize"),
        json!({
            "root_dependent_id": dependent.id,
            "prerequisites": expected
        })
    );
}

async fn assert_blocking_dependency_queries(
    tools: &Tools,
    dependent: &Issue,
    second_dependent: &Issue,
    prerequisite_a: &Issue,
    prerequisite_b: &Issue,
) -> Vec<BlockingDependency> {
    let prerequisites = tools
        .blocking_dependency_list(
            &BlockingDependencyListQuery::PrerequisitesOf {
                dependent_id: dependent.id.to_string(),
            },
            None,
        )
        .await
        .unwrap();
    let mut actual = prerequisites
        .iter()
        .map(|relationship| relationship.prerequisite_id().to_string())
        .collect::<Vec<_>>();
    actual.sort();
    let mut expected = vec![prerequisite_a.id.to_string(), prerequisite_b.id.to_string()];
    expected.sort();
    assert_eq!(actual, expected);
    assert_relationship_list_wire(
        &prerequisites,
        &[(dependent, prerequisite_a), (dependent, prerequisite_b)],
        "prerequisite_id",
    );

    let dependents = tools
        .blocking_dependency_list(
            &BlockingDependencyListQuery::DependentsOf {
                prerequisite_id: prerequisite_a.id.to_string(),
            },
            None,
        )
        .await
        .unwrap();
    assert!(
        dependents
            .iter()
            .any(|edge| edge.dependent_id() == &dependent.id)
    );
    assert!(
        dependents
            .iter()
            .any(|edge| edge.dependent_id() == &second_dependent.id)
    );
    assert_relationship_list_wire(
        &dependents,
        &[
            (dependent, prerequisite_a),
            (second_dependent, prerequisite_a),
        ],
        "dependent_id",
    );

    let tree = tools
        .blocking_dependency_tree(dependent.id.as_str(), Some(1), None)
        .await
        .unwrap();
    assert_eq!(tree.root_dependent_id, dependent.id.to_string());
    assert_eq!(tree.prerequisites.len(), 2);
    assert!(
        tree.prerequisites
            .iter()
            .all(|entry| entry.depth == 1 && entry.dependent_id == dependent.id.as_str())
    );
    assert_tree_wire(&tree, dependent, [prerequisite_a, prerequisite_b]);
    dependents
}

async fn assert_blocking_input_errors(tools: &Tools, dependent: &Issue) {
    let self_reference = tools
        .blocking_dependency_add(dependent.id.as_str(), dependent.id.as_str(), None)
        .await;
    assert!(matches!(
        self_reference,
        Err(Error::InvalidBlockingDependency(_))
    ));
    let missing = tools
        .blocking_dependency_add(dependent.id.as_str(), "test-missing", None)
        .await;
    assert!(matches!(
        missing,
        Err(Error::IssueNotFound(issue_id)) if issue_id == "test-missing"
    ));
}
/// Test adding dependencies between issues.
#[tokio::test]
async fn blocking_dependency_mcp_direction_and_context_recreation() {
    let workspace = create_temp_workspace();
    let issues_path = workspace.path().join(".rivets/issues.jsonl");
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;
    let prerequisite_a = create_issue(&tools, "Prerequisite A").await;
    let prerequisite_b = create_issue(&tools, "Prerequisite B").await;
    let dependent = create_issue(&tools, "Dependent").await;
    let second_dependent = create_issue(&tools, "Second dependent").await;
    let mut records = std::fs::read_to_string(&issues_path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    let second_record = records
        .iter_mut()
        .find(|record| record["id"] == second_dependent.id.as_str())
        .unwrap();
    second_record["dependencies"] = json!([
        {"depends_on_id": prerequisite_a.id, "dep_type": "related"}
    ]);
    let seeded = records
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&issues_path, format!("{seeded}\n")).unwrap();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;

    tools
        .blocking_dependency_add(dependent.id.as_str(), prerequisite_b.id.as_str(), None)
        .await
        .unwrap();
    tools
        .blocking_dependency_add(
            dependent.id.as_str(),
            prerequisite_a.id.as_str(),
            Some(workspace.path().to_str().unwrap()),
        )
        .await
        .unwrap();
    let added = tools
        .blocking_dependency_add(
            second_dependent.id.as_str(),
            prerequisite_a.id.as_str(),
            None,
        )
        .await
        .unwrap();
    assert_relationship_wire(&added, &second_dependent, &prerequisite_a);

    let dependents = assert_blocking_dependency_queries(
        &tools,
        &dependent,
        &second_dependent,
        &prerequisite_a,
        &prerequisite_b,
    )
    .await;

    let restarted = create_tools();
    set_context(&restarted, workspace.path()).await;
    let persisted = restarted
        .blocking_dependency_list(
            &BlockingDependencyListQuery::DependentsOf {
                prerequisite_id: prerequisite_a.id.to_string(),
            },
            None,
        )
        .await
        .unwrap();
    assert_eq!(persisted, dependents);

    let removed = restarted
        .blocking_dependency_remove(
            second_dependent.id.as_str(),
            prerequisite_a.id.as_str(),
            None,
        )
        .await
        .unwrap();
    assert_relationship_wire(&removed, &second_dependent, &prerequisite_a);
    let records = std::fs::read_to_string(issues_path).unwrap();
    let second_record: Value = records
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .find(|record: &Value| record["id"] == second_dependent.id.as_str())
        .unwrap();
    assert_eq!(
        second_record["dependencies"],
        json!([
            {"depends_on_id": prerequisite_a.id, "dep_type": "related"}
        ])
    );

    assert_blocking_input_errors(&restarted, &dependent).await;
}

async fn assert_relationship_mcp_errors(
    tools: &Tools,
    workspace: &std::path::Path,
    issue_a: &Issue,
    issue_c: &Issue,
) {
    let issues_path = workspace.join(".rivets/issues.jsonl");
    let before_errors = std::fs::read(&issues_path).unwrap();
    assert!(matches!(
        tools
            .related_add(issue_a.id.as_str(), issue_a.id.as_str(), None)
            .await,
        Err(Error::InvalidRelatedAssociation(_))
    ));
    assert!(matches!(
        tools
            .discovery_add(issue_c.id.as_str(), issue_c.id.as_str(), None)
            .await,
        Err(Error::InvalidDiscoveryOrigin(_))
    ));
    assert!(matches!(
        tools
            .discovery_add(issue_c.id.as_str(), issue_a.id.as_str(), None)
            .await,
        Err(Error::DuplicateDiscoveryOrigin { .. })
    ));
    assert!(matches!(
        tools
            .discovery_add(issue_a.id.as_str(), issue_c.id.as_str(), None)
            .await,
        Err(Error::CircularDiscoveryOrigin { .. })
    ));
    assert!(matches!(
        tools
            .related_add(issue_a.id.as_str(), "test-missing", None)
            .await,
        Err(Error::IssueNotFound(issue_id)) if issue_id == "test-missing"
    ));
    assert_eq!(std::fs::read(&issues_path).unwrap(), before_errors);
}

async fn assert_relationship_restart_and_removal(
    workspace: &std::path::Path,
    issue_a: &Issue,
    issue_b: &Issue,
    issue_c: &Issue,
    related: RelatedAssociation,
    discovery_a: DiscoveryOrigin,
    discovery_b: DiscoveryOrigin,
) {
    let mut expected_discovery = vec![discovery_a.clone(), discovery_b.clone()];
    expected_discovery.sort();
    let restarted = create_tools();
    set_context(&restarted, workspace).await;
    assert_eq!(
        restarted
            .related_list(issue_a.id.as_str(), None)
            .await
            .unwrap(),
        vec![related.clone()]
    );
    assert_eq!(
        restarted
            .discovery_list(issue_c.id.as_str(), None)
            .await
            .unwrap(),
        expected_discovery
    );
    assert_eq!(
        restarted
            .related_remove(issue_b.id.as_str(), issue_a.id.as_str(), None)
            .await
            .unwrap(),
        related
    );
    assert_eq!(
        restarted
            .discovery_remove(issue_c.id.as_str(), issue_a.id.as_str(), None)
            .await
            .unwrap(),
        discovery_a
    );
    assert!(
        restarted
            .related_list(issue_a.id.as_str(), None)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        restarted
            .discovery_list(issue_c.id.as_str(), None)
            .await
            .unwrap(),
        vec![discovery_b]
    );
}

#[tokio::test]
async fn related_and_discovery_mcp_direction_and_context_recreation() {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;
    let issue_a = create_issue(&tools, "Issue A").await;
    let issue_b = create_issue(&tools, "Issue B").await;
    let issue_c = create_issue(&tools, "Issue C").await;

    let related = tools
        .related_add(
            issue_b.id.as_str(),
            issue_a.id.as_str(),
            Some(workspace.path().to_str().unwrap()),
        )
        .await
        .unwrap();
    let mut expected_endpoints = [issue_a.id.clone(), issue_b.id.clone()];
    expected_endpoints.sort();
    assert_eq!(related.left_issue_id(), &expected_endpoints[0]);
    assert_eq!(related.right_issue_id(), &expected_endpoints[1]);
    assert_eq!(
        serde_json::to_value(&related).unwrap(),
        json!({
            "left_issue_id": expected_endpoints[0],
            "right_issue_id": expected_endpoints[1]
        })
    );

    let duplicate_related = tools
        .related_add(issue_a.id.as_str(), issue_b.id.as_str(), None)
        .await
        .unwrap();
    assert_eq!(duplicate_related, related);
    let related_from_right = tools
        .related_list(expected_endpoints[1].as_str(), None)
        .await
        .unwrap();
    assert_eq!(related_from_right, vec![related.clone()]);
    let restarted_after_related = create_tools();
    set_context(&restarted_after_related, workspace.path()).await;
    assert_eq!(
        restarted_after_related
            .related_list(expected_endpoints[1].as_str(), None)
            .await
            .unwrap(),
        vec![related.clone()],
        "a fresh MCP context must observe the persisted Related mutation"
    );

    let discovery_a = tools
        .discovery_add(issue_c.id.as_str(), issue_a.id.as_str(), None)
        .await
        .unwrap();
    let discovery_b = tools
        .discovery_add(issue_c.id.as_str(), issue_b.id.as_str(), None)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(&discovery_a).unwrap(),
        json!({
            "discovered_issue_id": issue_c.id,
            "source_issue_id": issue_a.id
        })
    );
    let listed_discovery = tools
        .discovery_list(issue_c.id.as_str(), None)
        .await
        .unwrap();
    let mut expected_discovery = vec![discovery_a.clone(), discovery_b.clone()];
    expected_discovery.sort();
    assert_eq!(listed_discovery, expected_discovery);

    assert_relationship_mcp_errors(&tools, workspace.path(), &issue_a, &issue_c).await;
    assert_relationship_restart_and_removal(
        workspace.path(),
        &issue_a,
        &issue_b,
        &issue_c,
        related,
        discovery_a,
        discovery_b,
    )
    .await;
}
async fn create_epic(tools: &Tools, title: &str) -> Issue {
    tools
        .create(create_params(
            title.to_string(),
            None,
            None,
            Some("epic"),
            None,
            None,
            None,
            None,
            None,
        ))
        .await
        .expect("Epic creation should succeed")
}

async fn assert_parentage_restart_and_clear(
    workspace: &TempDir,
    child: &Issue,
    first_parent: &Issue,
    second_parent: &Issue,
    moved: &rivets::domain::Parentage,
) {
    let workspace_root = workspace.path().to_str().unwrap();
    let issues_path = workspace.path().join(".rivets/issues.jsonl");
    let restarted = create_tools();
    set_context(&restarted, workspace.path()).await;
    assert_eq!(
        restarted
            .parent_show(child.id.as_str(), Some(workspace_root))
            .await
            .unwrap(),
        Some(moved.clone())
    );
    let records = std::fs::read_to_string(&issues_path).unwrap();
    let child_record: Value = records
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .find(|record: &Value| record["id"] == child.id.as_str())
        .unwrap();
    let dependencies = child_record["dependencies"].as_array().unwrap();
    assert!(dependencies.contains(&json!({
        "depends_on_id": first_parent.id,
        "dep_type": "blocks"
    })));
    assert!(dependencies.contains(&json!({
        "depends_on_id": second_parent.id,
        "dep_type": "parent-child"
    })));

    assert_eq!(
        restarted
            .parent_clear(child.id.as_str(), Some(workspace_root))
            .await
            .unwrap(),
        moved.clone()
    );
    assert_eq!(
        restarted
            .parent_show(child.id.as_str(), Some(workspace_root))
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        restarted
            .blocking_dependency_list(
                &BlockingDependencyListQuery::PrerequisitesOf {
                    dependent_id: child.id.to_string(),
                },
                Some(workspace_root),
            )
            .await
            .unwrap(),
        vec![BlockingDependency::new(child.id.clone(), first_parent.id.clone()).unwrap()]
    );
}

#[tokio::test]
async fn parentage_mcp_contract_context_recreation_and_locking() {
    let workspace = create_temp_workspace();
    let workspace_root = workspace.path().to_str().unwrap();
    let issues_path = workspace.path().join(".rivets/issues.jsonl");
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;
    let child = create_issue(&tools, "Child with spaces Ω").await;
    let non_epic = create_issue(&tools, "Not an Epic").await;
    let first_parent = create_epic(&tools, "First Epic").await;
    let second_parent = create_epic(&tools, "Second Epic").await;
    tools
        .blocking_dependency_add(child.id.as_str(), first_parent.id.as_str(), None)
        .await
        .unwrap();

    let set = tools
        .parent_set(child.id.as_str(), first_parent.id.as_str(), None)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(&set).unwrap(),
        json!({
            "child_id": child.id,
            "parent_id": first_parent.id
        })
    );
    assert_eq!(
        tools
            .parent_set(child.id.as_str(), first_parent.id.as_str(), None)
            .await
            .unwrap(),
        set
    );
    assert_eq!(
        tools.parent_show(child.id.as_str(), None).await.unwrap(),
        Some(set.clone())
    );

    let before_failed_move = std::fs::read(&issues_path).unwrap();
    let failed_move = tools
        .parent_move(
            child.id.as_str(),
            non_epic.id.as_str(),
            Some(workspace_root),
        )
        .await;
    assert!(matches!(
        failed_move,
        Err(Error::InvalidParentage(
            rivets::domain::ParentageError::ParentNotEpic { parent_id, .. }
        )) if parent_id == non_epic.id
    ));
    assert_eq!(std::fs::read(&issues_path).unwrap(), before_failed_move);
    assert_eq!(
        tools.parent_show(child.id.as_str(), None).await.unwrap(),
        Some(set)
    );

    let moved = tools
        .parent_move(
            child.id.as_str(),
            second_parent.id.as_str(),
            Some(workspace_root),
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(&moved).unwrap(),
        json!({
            "child_id": child.id,
            "parent_id": second_parent.id
        })
    );

    assert_parentage_restart_and_clear(&workspace, &child, &first_parent, &second_parent, &moved)
        .await;
}

#[tokio::test]
async fn ready_assignment_visibility() {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;
    let unassigned = create_issue(&tools, "Unassigned").await;
    let alice = tools
        .create(create_params(
            "Alice".to_string(),
            None,
            None,
            Some("task"),
            Some("alice".to_string()),
            None,
            None,
            None,
            None,
        ))
        .await
        .expect("assigned Issue creation should succeed");

    let ready_ids = |issues: Vec<Issue>| {
        issues
            .into_iter()
            .map(|issue| issue.id)
            .collect::<BTreeSet<_>>()
    };
    assert_eq!(
        ready_ids(
            tools
                .ready(ready_params(None, None, None, None, None, None))
                .await
                .expect("default Ready query should succeed")
        ),
        BTreeSet::from([unassigned.id.clone()])
    );
    assert_eq!(
        ready_ids(
            tools
                .ready(ready_params(
                    None,
                    None,
                    None,
                    Some("alice".to_string()),
                    None,
                    None,
                ))
                .await
                .expect("assignee Ready query should succeed")
        ),
        BTreeSet::from([alice.id.clone()])
    );

    let all = ReadyParams {
        all_assignees: true,
        ..ready_params(None, None, None, None, None, None)
    };
    assert_eq!(
        ready_ids(
            tools
                .ready(all.clone())
                .await
                .expect("all-assignees Ready query should succeed")
        ),
        BTreeSet::from([unassigned.id.clone(), alice.id.clone()])
    );
    let conflict = tools
        .ready(ReadyParams {
            assignee: Some("alice".to_string()),
            all_assignees: true,
            ..ready_params(None, None, None, None, None, None)
        })
        .await
        .unwrap_err();
    assert!(matches!(
        conflict,
        Error::InvalidArgument {
            field: "assignment selector",
            value,
            valid_values: "unassigned, assignee, all_assignees",
        } if value == "assignee and all_assignees"
    ));

    drop(tools);
    let restarted = create_tools();
    set_context(&restarted, workspace.path()).await;
    assert_eq!(
        ready_ids(
            restarted
                .ready(all)
                .await
                .expect("restarted Ready query should succeed")
        ),
        BTreeSet::from([unassigned.id, alice.id])
    );
}

/// Test ready-to-work excludes blocked issues.
#[tokio::test]
async fn test_ready_excludes_blocked() {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;

    // Create blocker and dependent
    let blocker = create_issue(&tools, "Blocker").await;
    let dependent = create_issue(&tools, "Dependent").await;

    // Add blocking dependency
    tools
        .blocking_dependency_add(dependent.id.as_str(), blocker.id.as_str(), None)
        .await
        .unwrap();

    // Ready should only return the blocker (dependent is blocked)
    let ready = tools
        .ready(ready_params(None, None, None, None, None, None))
        .await
        .expect("ready should succeed");

    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].id, blocker.id);
}

// ============================================================================
// Filter Tests (rstest parameterized)
// ============================================================================

/// Test list with status filter (open).
#[rstest]
#[case::status_open(
    ListFilterCase {
        setup: vec![
            IssueSetup::new("To Close").closed(),
            IssueSetup::new("Still Open"),
        ],
        filter: FilterParams::new().with_status("open"),
        expected_count: 1,
        expected_titles: Some(vec!["Still Open"]),
    }
)]
#[case::status_closed(
    ListFilterCase {
        setup: vec![
            IssueSetup::new("To Close").closed(),
            IssueSetup::new("Still Open"),
        ],
        filter: FilterParams::new().with_status("closed"),
        expected_count: 1,
        expected_titles: Some(vec!["To Close"]),
    }
)]
#[case::priority_filter(
    ListFilterCase {
        setup: vec![
            IssueSetup::new("High Priority").with_priority(0),
            IssueSetup::new("Low Priority").with_priority(4),
        ],
        filter: FilterParams::new().with_priority(0),
        expected_count: 1,
        expected_titles: Some(vec!["High Priority"]),
    }
)]
#[case::issue_kind_bug(
    ListFilterCase {
        setup: vec![
            IssueSetup::new("A Bug").with_issue_kind("bug"),
            IssueSetup::new("A Feature").with_issue_kind("feature"),
        ],
        filter: FilterParams::new().with_issue_kind("bug"),
        expected_count: 1,
        expected_titles: Some(vec!["A Bug"]),
    }
)]
#[case::issue_kind_feature(
    ListFilterCase {
        setup: vec![
            IssueSetup::new("A Bug").with_issue_kind("bug"),
            IssueSetup::new("A Feature").with_issue_kind("feature"),
        ],
        filter: FilterParams::new().with_issue_kind("feature"),
        expected_count: 1,
        expected_titles: Some(vec!["A Feature"]),
    }
)]
#[case::assignee_filter(
    ListFilterCase {
        setup: vec![
            IssueSetup::new("Alice's Task").with_assignee("alice"),
            IssueSetup::new("Bob's Task").with_assignee("bob"),
        ],
        filter: FilterParams::new().with_assignee("alice"),
        expected_count: 1,
        expected_titles: Some(vec!["Alice's Task"]),
    }
)]
#[case::label_filter(
    ListFilterCase {
        setup: vec![
            IssueSetup::new("Urgent Task").with_labels(vec!["urgent", "frontend"]),
            IssueSetup::new("Backend Task").with_labels(vec!["backend"]),
        ],
        filter: FilterParams::new().with_label("urgent"),
        expected_count: 1,
        expected_titles: Some(vec!["Urgent Task"]),
    }
)]
#[case::limit(
    ListFilterCase {
        setup: vec![
            IssueSetup::new("Issue 1"),
            IssueSetup::new("Issue 2"),
            IssueSetup::new("Issue 3"),
            IssueSetup::new("Issue 4"),
            IssueSetup::new("Issue 5"),
        ],
        filter: FilterParams::new().with_limit(2),
        expected_count: 2,
        expected_titles: None,  // Don't check titles since order may vary
    }
)]
// -------------------------------------------------------------------------
// Two-way filter combinations
// -------------------------------------------------------------------------
#[case::status_and_priority(
    ListFilterCase {
        setup: vec![
            IssueSetup::new("Open P0").with_priority(0),
            IssueSetup::new("Open P2").with_priority(2),
            IssueSetup::new("Closed P0").with_priority(0).closed(),
        ],
        filter: FilterParams::new().with_status("open").with_priority(0),
        expected_count: 1,
        expected_titles: Some(vec!["Open P0"]),
    }
)]
#[case::status_and_kind(
    ListFilterCase {
        setup: vec![
            IssueSetup::new("Open Bug").with_issue_kind("bug"),
            IssueSetup::new("Open Feature").with_issue_kind("feature"),
            IssueSetup::new("Closed Bug").with_issue_kind("bug").closed(),
        ],
        filter: FilterParams::new().with_status("open").with_issue_kind("bug"),
        expected_count: 1,
        expected_titles: Some(vec!["Open Bug"]),
    }
)]
#[case::status_and_assignee(
    ListFilterCase {
        setup: vec![
            IssueSetup::new("Open Alice").with_assignee("alice"),
            IssueSetup::new("Open Bob").with_assignee("bob"),
            IssueSetup::new("Closed Alice").with_assignee("alice").closed(),
        ],
        filter: FilterParams::new().with_status("open").with_assignee("alice"),
        expected_count: 1,
        expected_titles: Some(vec!["Open Alice"]),
    }
)]
#[case::status_and_label(
    ListFilterCase {
        setup: vec![
            IssueSetup::new("Open Urgent").with_labels(vec!["urgent"]),
            IssueSetup::new("Open Normal").with_labels(vec!["normal"]),
            IssueSetup::new("Closed Urgent").with_labels(vec!["urgent"]).closed(),
        ],
        filter: FilterParams::new().with_status("open").with_label("urgent"),
        expected_count: 1,
        expected_titles: Some(vec!["Open Urgent"]),
    }
)]
#[case::priority_and_kind(
    ListFilterCase {
        setup: vec![
            IssueSetup::new("P0 Bug").with_priority(0).with_issue_kind("bug"),
            IssueSetup::new("P0 Feature").with_priority(0).with_issue_kind("feature"),
            IssueSetup::new("P2 Bug").with_priority(2).with_issue_kind("bug"),
        ],
        filter: FilterParams::new().with_priority(0).with_issue_kind("bug"),
        expected_count: 1,
        expected_titles: Some(vec!["P0 Bug"]),
    }
)]
#[case::priority_and_assignee(
    ListFilterCase {
        setup: vec![
            IssueSetup::new("P0 Alice").with_priority(0).with_assignee("alice"),
            IssueSetup::new("P0 Bob").with_priority(0).with_assignee("bob"),
            IssueSetup::new("P2 Alice").with_priority(2).with_assignee("alice"),
        ],
        filter: FilterParams::new().with_priority(0).with_assignee("alice"),
        expected_count: 1,
        expected_titles: Some(vec!["P0 Alice"]),
    }
)]
#[case::priority_and_label(
    ListFilterCase {
        setup: vec![
            IssueSetup::new("P0 Urgent").with_priority(0).with_labels(vec!["urgent"]),
            IssueSetup::new("P0 Normal").with_priority(0).with_labels(vec!["normal"]),
            IssueSetup::new("P2 Urgent").with_priority(2).with_labels(vec!["urgent"]),
        ],
        filter: FilterParams::new().with_priority(0).with_label("urgent"),
        expected_count: 1,
        expected_titles: Some(vec!["P0 Urgent"]),
    }
)]
#[case::kind_and_assignee(
    ListFilterCase {
        setup: vec![
            IssueSetup::new("Bug Alice").with_issue_kind("bug").with_assignee("alice"),
            IssueSetup::new("Bug Bob").with_issue_kind("bug").with_assignee("bob"),
            IssueSetup::new("Feature Alice").with_issue_kind("feature").with_assignee("alice"),
        ],
        filter: FilterParams::new().with_issue_kind("bug").with_assignee("alice"),
        expected_count: 1,
        expected_titles: Some(vec!["Bug Alice"]),
    }
)]
#[case::kind_and_label(
    ListFilterCase {
        setup: vec![
            IssueSetup::new("Bug Urgent").with_issue_kind("bug").with_labels(vec!["urgent"]),
            IssueSetup::new("Bug Normal").with_issue_kind("bug").with_labels(vec!["normal"]),
            IssueSetup::new("Feature Urgent").with_issue_kind("feature").with_labels(vec!["urgent"]),
        ],
        filter: FilterParams::new().with_issue_kind("bug").with_label("urgent"),
        expected_count: 1,
        expected_titles: Some(vec!["Bug Urgent"]),
    }
)]
#[case::assignee_and_label(
    ListFilterCase {
        setup: vec![
            IssueSetup::new("Alice Urgent").with_assignee("alice").with_labels(vec!["urgent"]),
            IssueSetup::new("Alice Normal").with_assignee("alice").with_labels(vec!["normal"]),
            IssueSetup::new("Bob Urgent").with_assignee("bob").with_labels(vec!["urgent"]),
        ],
        filter: FilterParams::new().with_assignee("alice").with_label("urgent"),
        expected_count: 1,
        expected_titles: Some(vec!["Alice Urgent"]),
    }
)]
// -------------------------------------------------------------------------
// Three-way filter combinations
// -------------------------------------------------------------------------
#[case::status_priority_kind(
    ListFilterCase {
        setup: vec![
            IssueSetup::new("Open P0 Bug").with_priority(0).with_issue_kind("bug"),
            IssueSetup::new("Open P0 Feature").with_priority(0).with_issue_kind("feature"),
            IssueSetup::new("Open P2 Bug").with_priority(2).with_issue_kind("bug"),
            IssueSetup::new("Closed P0 Bug").with_priority(0).with_issue_kind("bug").closed(),
        ],
        filter: FilterParams::new().with_status("open").with_priority(0).with_issue_kind("bug"),
        expected_count: 1,
        expected_titles: Some(vec!["Open P0 Bug"]),
    }
)]
#[case::status_priority_assignee(
    ListFilterCase {
        setup: vec![
            IssueSetup::new("Open P0 Alice").with_priority(0).with_assignee("alice"),
            IssueSetup::new("Open P0 Bob").with_priority(0).with_assignee("bob"),
            IssueSetup::new("Open P2 Alice").with_priority(2).with_assignee("alice"),
            IssueSetup::new("Closed P0 Alice").with_priority(0).with_assignee("alice").closed(),
        ],
        filter: FilterParams::new().with_status("open").with_priority(0).with_assignee("alice"),
        expected_count: 1,
        expected_titles: Some(vec!["Open P0 Alice"]),
    }
)]
#[case::status_kind_label(
    ListFilterCase {
        setup: vec![
            IssueSetup::new("Open Bug Urgent").with_issue_kind("bug").with_labels(vec!["urgent"]),
            IssueSetup::new("Open Bug Normal").with_issue_kind("bug").with_labels(vec!["normal"]),
            IssueSetup::new("Open Feature Urgent").with_issue_kind("feature").with_labels(vec!["urgent"]),
            IssueSetup::new("Closed Bug Urgent").with_issue_kind("bug").with_labels(vec!["urgent"]).closed(),
        ],
        filter: FilterParams::new().with_status("open").with_issue_kind("bug").with_label("urgent"),
        expected_count: 1,
        expected_titles: Some(vec!["Open Bug Urgent"]),
    }
)]
#[case::priority_kind_assignee(
    ListFilterCase {
        setup: vec![
            IssueSetup::new("P0 Bug Alice").with_priority(0).with_issue_kind("bug").with_assignee("alice"),
            IssueSetup::new("P0 Bug Bob").with_priority(0).with_issue_kind("bug").with_assignee("bob"),
            IssueSetup::new("P0 Feature Alice").with_priority(0).with_issue_kind("feature").with_assignee("alice"),
            IssueSetup::new("P2 Bug Alice").with_priority(2).with_issue_kind("bug").with_assignee("alice"),
        ],
        filter: FilterParams::new().with_priority(0).with_issue_kind("bug").with_assignee("alice"),
        expected_count: 1,
        expected_titles: Some(vec!["P0 Bug Alice"]),
    }
)]
#[case::priority_assignee_label(
    ListFilterCase {
        setup: vec![
            IssueSetup::new("P0 Alice Urgent").with_priority(0).with_assignee("alice").with_labels(vec!["urgent"]),
            IssueSetup::new("P0 Alice Normal").with_priority(0).with_assignee("alice").with_labels(vec!["normal"]),
            IssueSetup::new("P0 Bob Urgent").with_priority(0).with_assignee("bob").with_labels(vec!["urgent"]),
            IssueSetup::new("P2 Alice Urgent").with_priority(2).with_assignee("alice").with_labels(vec!["urgent"]),
        ],
        filter: FilterParams::new().with_priority(0).with_assignee("alice").with_label("urgent"),
        expected_count: 1,
        expected_titles: Some(vec!["P0 Alice Urgent"]),
    }
)]
#[case::kind_assignee_label(
    ListFilterCase {
        setup: vec![
            IssueSetup::new("Bug Alice Urgent").with_issue_kind("bug").with_assignee("alice").with_labels(vec!["urgent"]),
            IssueSetup::new("Bug Alice Normal").with_issue_kind("bug").with_assignee("alice").with_labels(vec!["normal"]),
            IssueSetup::new("Bug Bob Urgent").with_issue_kind("bug").with_assignee("bob").with_labels(vec!["urgent"]),
            IssueSetup::new("Feature Alice Urgent").with_issue_kind("feature").with_assignee("alice").with_labels(vec!["urgent"]),
        ],
        filter: FilterParams::new().with_issue_kind("bug").with_assignee("alice").with_label("urgent"),
        expected_count: 1,
        expected_titles: Some(vec!["Bug Alice Urgent"]),
    }
)]
// -------------------------------------------------------------------------
// Four-way and five-way filter combinations
// -------------------------------------------------------------------------
#[case::four_way_status_priority_kind_assignee(
    ListFilterCase {
        setup: vec![
            IssueSetup::new("Target").with_priority(0).with_issue_kind("bug").with_assignee("alice"),
            IssueSetup::new("Wrong Priority").with_priority(2).with_issue_kind("bug").with_assignee("alice"),
            IssueSetup::new("Wrong Kind").with_priority(0).with_issue_kind("feature").with_assignee("alice"),
            IssueSetup::new("Wrong Assignee").with_priority(0).with_issue_kind("bug").with_assignee("bob"),
            IssueSetup::new("Closed Match").with_priority(0).with_issue_kind("bug").with_assignee("alice").closed(),
        ],
        filter: FilterParams::new().with_status("open").with_priority(0).with_issue_kind("bug").with_assignee("alice"),
        expected_count: 1,
        expected_titles: Some(vec!["Target"]),
    }
)]
#[case::five_way_all_filters(
    ListFilterCase {
        setup: vec![
            IssueSetup::new("Perfect Match").with_priority(0).with_issue_kind("bug").with_assignee("alice").with_labels(vec!["urgent"]),
            IssueSetup::new("Wrong Label").with_priority(0).with_issue_kind("bug").with_assignee("alice").with_labels(vec!["normal"]),
            IssueSetup::new("Wrong Assignee").with_priority(0).with_issue_kind("bug").with_assignee("bob").with_labels(vec!["urgent"]),
            IssueSetup::new("Wrong Kind").with_priority(0).with_issue_kind("feature").with_assignee("alice").with_labels(vec!["urgent"]),
            IssueSetup::new("Wrong Priority").with_priority(2).with_issue_kind("bug").with_assignee("alice").with_labels(vec!["urgent"]),
            IssueSetup::new("Closed Match").with_priority(0).with_issue_kind("bug").with_assignee("alice").with_labels(vec!["urgent"]).closed(),
        ],
        filter: FilterParams::new().with_status("open").with_priority(0).with_issue_kind("bug").with_assignee("alice").with_label("urgent"),
        expected_count: 1,
        expected_titles: Some(vec!["Perfect Match"]),
    }
)]
// -------------------------------------------------------------------------
// Edge cases
// -------------------------------------------------------------------------
#[case::no_matches(
    ListFilterCase {
        setup: vec![
            IssueSetup::new("Bug").with_issue_kind("bug"),
            IssueSetup::new("Feature").with_issue_kind("feature"),
        ],
        filter: FilterParams::new().with_issue_kind("epic"),
        expected_count: 0,
        expected_titles: None,
    }
)]
#[case::all_match(
    ListFilterCase {
        setup: vec![
            IssueSetup::new("Bug 1").with_issue_kind("bug"),
            IssueSetup::new("Bug 2").with_issue_kind("bug"),
            IssueSetup::new("Bug 3").with_issue_kind("bug"),
        ],
        filter: FilterParams::new().with_issue_kind("bug"),
        expected_count: 3,
        expected_titles: Some(vec!["Bug 1", "Bug 2", "Bug 3"]),
    }
)]
#[case::limit_with_filters(
    ListFilterCase {
        setup: vec![
            IssueSetup::new("Bug 1").with_issue_kind("bug"),
            IssueSetup::new("Bug 2").with_issue_kind("bug"),
            IssueSetup::new("Bug 3").with_issue_kind("bug"),
            IssueSetup::new("Feature 1").with_issue_kind("feature"),
        ],
        filter: FilterParams::new().with_issue_kind("bug").with_limit(2),
        expected_count: 2,
        expected_titles: None,
    }
)]
#[tokio::test]
async fn test_list_filters(#[case] test_case: ListFilterCase) {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;

    // Create all issues from setup
    for setup in &test_case.setup {
        create_custom_issue(&tools, setup).await;
    }

    // Apply filter
    let results = tools
        .list(list_params(
            test_case.filter.status,
            test_case.filter.priority,
            test_case.filter.issue_kind,
            test_case.filter.assignee.map(str::to_string),
            test_case.filter.label.map(str::to_string),
            test_case.filter.limit,
            None,
        ))
        .await
        .expect("list should succeed");

    // Verify count
    assert_eq!(
        results.len(),
        test_case.expected_count,
        "Expected {} issues, got {}",
        test_case.expected_count,
        results.len()
    );

    // Verify titles if specified (bidirectional check)
    if let Some(expected_titles) = test_case.expected_titles {
        let actual_titles: Vec<&str> = results.iter().map(|i| i.title.as_str()).collect();

        // Check all expected titles are present
        for title in &expected_titles {
            assert!(
                actual_titles.contains(title),
                "Expected title '{title}' not found in results: {actual_titles:?}"
            );
        }

        // Check no unexpected titles are present
        for actual in &actual_titles {
            assert!(
                expected_titles.contains(actual),
                "Unexpected title '{actual}' in results. Expected only: {expected_titles:?}"
            );
        }
    }
}

// ============================================================================
// where_am_i Tests
// ============================================================================

/// Test `where_am_i` returns correct workspace info.
#[tokio::test]
async fn test_where_am_i() {
    let workspace = create_temp_workspace();
    let tools = create_tools();

    // Before context is set
    let before = tools.where_am_i().await.expect("where_am_i should succeed");
    assert!(!before.context_set);
    assert!(before.workspace_root.is_none());

    // After context is set
    set_context(&tools, workspace.path()).await;
    let after = tools.where_am_i().await.expect("where_am_i should succeed");
    assert!(after.context_set);
    assert!(after.workspace_root.is_some());
    assert!(after.database_path.is_some());
}

// ============================================================================
// Persistence Tests
// ============================================================================

/// Test that issues persist across "sessions" (tool instances).
#[tokio::test]
async fn test_persistence_across_sessions() {
    let workspace = create_temp_workspace();

    // First "session" - create issue
    {
        let tools = create_tools();
        set_context(&tools, workspace.path()).await;
        create_issue(&tools, "Persistent Issue").await;
    }

    // Second "session" - should see the issue
    {
        let tools = create_tools();
        set_context(&tools, workspace.path()).await;

        let issues = tools
            .list(list_params(None, None, None, None, None, None, None))
            .await
            .expect("list should succeed");

        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].title, "Persistent Issue");
    }
}

/// Test that updates persist.
#[tokio::test]
async fn test_update_persistence() {
    let workspace = create_temp_workspace();
    let issue_id: String;

    // Create and update
    {
        let tools = create_tools();
        set_context(&tools, workspace.path()).await;
        let issue = tools
            .create(create_params(
                "To Update".to_string(),
                None,
                None,
                None,
                Some("active-owner".to_string()),
                None,
                None,
                None,
                None,
            ))
            .await
            .expect("create should succeed");
        issue_id = issue.id.as_str().to_string();

        tools
            .update(update_params(
                &issue_id,
                Some("Updated Title".to_string()),
                None,
                Some("in_progress"),
                None,
                None, // issue_kind
                None,
                None,
                None, // labels
                None, // workspace_root
            ))
            .await
            .unwrap();
    }

    // Verify persistence
    {
        let tools = create_tools();
        set_context(&tools, workspace.path()).await;

        let issue = tools.show(&issue_id, None).await.unwrap();
        assert_eq!(issue.title, "Updated Title");
        assert_eq!(issue.status, IssueStatus::InProgress);
    }
}

// ============================================================================
// Ready-to-Work Filter Tests (rstest parameterized)
// ============================================================================

#[rstest]
#[case::priority_filter(
    ReadyFilterCase {
        setup: vec![
            IssueSetup::new("P0 Issue").with_priority(0),
            IssueSetup::new("P2 Issue").with_priority(2),
        ],
        filter: FilterParams::new().with_priority(0),
        expected_count: 1,
        expected_titles: Some(vec!["P0 Issue"]),
    }
)]
#[case::assignee_filter(
    ReadyFilterCase {
        setup: vec![
            IssueSetup::new("Alice's Issue").with_assignee("alice"),
            IssueSetup::new("Unassigned Issue"),
        ],
        filter: FilterParams::new().with_assignee("alice"),
        expected_count: 1,
        expected_titles: Some(vec!["Alice's Issue"]),
    }
)]
#[case::issue_kind_filter(
    ReadyFilterCase {
        setup: vec![
            IssueSetup::new("Bug Issue").with_issue_kind("bug"),
            IssueSetup::new("Feature Issue").with_issue_kind("feature"),
        ],
        filter: FilterParams::new().with_issue_kind("bug"),
        expected_count: 1,
        expected_titles: Some(vec!["Bug Issue"]),
    }
)]
#[case::label_filter(
    ReadyFilterCase {
        setup: vec![
            IssueSetup::new("Critical Task").with_labels(vec!["critical"]),
            IssueSetup::new("Normal Task").with_labels(vec!["normal"]),
        ],
        filter: FilterParams::new().with_label("critical"),
        expected_count: 1,
        expected_titles: Some(vec!["Critical Task"]),
    }
)]
#[case::limit(
    ReadyFilterCase {
        setup: vec![
            IssueSetup::new("Ready Issue 1"),
            IssueSetup::new("Ready Issue 2"),
            IssueSetup::new("Ready Issue 3"),
            IssueSetup::new("Ready Issue 4"),
            IssueSetup::new("Ready Issue 5"),
        ],
        filter: FilterParams::new().with_limit(2),
        expected_count: 2,
        expected_titles: None,
    }
)]
// -------------------------------------------------------------------------
// Two-way filter combinations
// -------------------------------------------------------------------------
#[case::priority_and_kind(
    ReadyFilterCase {
        setup: vec![
            IssueSetup::new("P0 Bug").with_priority(0).with_issue_kind("bug"),
            IssueSetup::new("P0 Feature").with_priority(0).with_issue_kind("feature"),
            IssueSetup::new("P2 Bug").with_priority(2).with_issue_kind("bug"),
        ],
        filter: FilterParams::new().with_priority(0).with_issue_kind("bug"),
        expected_count: 1,
        expected_titles: Some(vec!["P0 Bug"]),
    }
)]
#[case::priority_and_assignee(
    ReadyFilterCase {
        setup: vec![
            IssueSetup::new("P0 Alice").with_priority(0).with_assignee("alice"),
            IssueSetup::new("P0 Bob").with_priority(0).with_assignee("bob"),
            IssueSetup::new("P2 Alice").with_priority(2).with_assignee("alice"),
        ],
        filter: FilterParams::new().with_priority(0).with_assignee("alice"),
        expected_count: 1,
        expected_titles: Some(vec!["P0 Alice"]),
    }
)]
#[case::priority_and_label(
    ReadyFilterCase {
        setup: vec![
            IssueSetup::new("P0 Urgent").with_priority(0).with_labels(vec!["urgent"]),
            IssueSetup::new("P0 Normal").with_priority(0).with_labels(vec!["normal"]),
            IssueSetup::new("P2 Urgent").with_priority(2).with_labels(vec!["urgent"]),
        ],
        filter: FilterParams::new().with_priority(0).with_label("urgent"),
        expected_count: 1,
        expected_titles: Some(vec!["P0 Urgent"]),
    }
)]
#[case::kind_and_assignee(
    ReadyFilterCase {
        setup: vec![
            IssueSetup::new("Bug Alice").with_issue_kind("bug").with_assignee("alice"),
            IssueSetup::new("Bug Bob").with_issue_kind("bug").with_assignee("bob"),
            IssueSetup::new("Feature Alice").with_issue_kind("feature").with_assignee("alice"),
        ],
        filter: FilterParams::new().with_issue_kind("bug").with_assignee("alice"),
        expected_count: 1,
        expected_titles: Some(vec!["Bug Alice"]),
    }
)]
#[case::kind_and_label(
    ReadyFilterCase {
        setup: vec![
            IssueSetup::new("Bug Urgent").with_issue_kind("bug").with_labels(vec!["urgent"]),
            IssueSetup::new("Bug Normal").with_issue_kind("bug").with_labels(vec!["normal"]),
            IssueSetup::new("Feature Urgent").with_issue_kind("feature").with_labels(vec!["urgent"]),
        ],
        filter: FilterParams::new().with_issue_kind("bug").with_label("urgent"),
        expected_count: 1,
        expected_titles: Some(vec!["Bug Urgent"]),
    }
)]
#[case::assignee_and_label(
    ReadyFilterCase {
        setup: vec![
            IssueSetup::new("Alice Urgent").with_assignee("alice").with_labels(vec!["urgent"]),
            IssueSetup::new("Alice Normal").with_assignee("alice").with_labels(vec!["normal"]),
            IssueSetup::new("Bob Urgent").with_assignee("bob").with_labels(vec!["urgent"]),
        ],
        filter: FilterParams::new().with_assignee("alice").with_label("urgent"),
        expected_count: 1,
        expected_titles: Some(vec!["Alice Urgent"]),
    }
)]
// -------------------------------------------------------------------------
// Three-way filter combinations
// -------------------------------------------------------------------------
#[case::priority_kind_assignee(
    ReadyFilterCase {
        setup: vec![
            IssueSetup::new("P0 Bug Alice").with_priority(0).with_issue_kind("bug").with_assignee("alice"),
            IssueSetup::new("P0 Bug Bob").with_priority(0).with_issue_kind("bug").with_assignee("bob"),
            IssueSetup::new("P0 Feature Alice").with_priority(0).with_issue_kind("feature").with_assignee("alice"),
            IssueSetup::new("P2 Bug Alice").with_priority(2).with_issue_kind("bug").with_assignee("alice"),
        ],
        filter: FilterParams::new().with_priority(0).with_issue_kind("bug").with_assignee("alice"),
        expected_count: 1,
        expected_titles: Some(vec!["P0 Bug Alice"]),
    }
)]
#[case::priority_kind_label(
    ReadyFilterCase {
        setup: vec![
            IssueSetup::new("P0 Bug Urgent").with_priority(0).with_issue_kind("bug").with_labels(vec!["urgent"]),
            IssueSetup::new("P0 Bug Normal").with_priority(0).with_issue_kind("bug").with_labels(vec!["normal"]),
            IssueSetup::new("P0 Feature Urgent").with_priority(0).with_issue_kind("feature").with_labels(vec!["urgent"]),
            IssueSetup::new("P2 Bug Urgent").with_priority(2).with_issue_kind("bug").with_labels(vec!["urgent"]),
        ],
        filter: FilterParams::new().with_priority(0).with_issue_kind("bug").with_label("urgent"),
        expected_count: 1,
        expected_titles: Some(vec!["P0 Bug Urgent"]),
    }
)]
#[case::priority_assignee_label(
    ReadyFilterCase {
        setup: vec![
            IssueSetup::new("P0 Alice Urgent").with_priority(0).with_assignee("alice").with_labels(vec!["urgent"]),
            IssueSetup::new("P0 Alice Normal").with_priority(0).with_assignee("alice").with_labels(vec!["normal"]),
            IssueSetup::new("P0 Bob Urgent").with_priority(0).with_assignee("bob").with_labels(vec!["urgent"]),
            IssueSetup::new("P2 Alice Urgent").with_priority(2).with_assignee("alice").with_labels(vec!["urgent"]),
        ],
        filter: FilterParams::new().with_priority(0).with_assignee("alice").with_label("urgent"),
        expected_count: 1,
        expected_titles: Some(vec!["P0 Alice Urgent"]),
    }
)]
#[case::kind_assignee_label(
    ReadyFilterCase {
        setup: vec![
            IssueSetup::new("Bug Alice Urgent").with_issue_kind("bug").with_assignee("alice").with_labels(vec!["urgent"]),
            IssueSetup::new("Bug Alice Normal").with_issue_kind("bug").with_assignee("alice").with_labels(vec!["normal"]),
            IssueSetup::new("Bug Bob Urgent").with_issue_kind("bug").with_assignee("bob").with_labels(vec!["urgent"]),
            IssueSetup::new("Feature Alice Urgent").with_issue_kind("feature").with_assignee("alice").with_labels(vec!["urgent"]),
        ],
        filter: FilterParams::new().with_issue_kind("bug").with_assignee("alice").with_label("urgent"),
        expected_count: 1,
        expected_titles: Some(vec!["Bug Alice Urgent"]),
    }
)]
// -------------------------------------------------------------------------
// Four-way filter combination (all ready filters)
// -------------------------------------------------------------------------
#[case::four_way_all_filters(
    ReadyFilterCase {
        setup: vec![
            IssueSetup::new("Perfect Match").with_priority(0).with_issue_kind("bug").with_assignee("alice").with_labels(vec!["urgent"]),
            IssueSetup::new("Wrong Label").with_priority(0).with_issue_kind("bug").with_assignee("alice").with_labels(vec!["normal"]),
            IssueSetup::new("Wrong Assignee").with_priority(0).with_issue_kind("bug").with_assignee("bob").with_labels(vec!["urgent"]),
            IssueSetup::new("Wrong Kind").with_priority(0).with_issue_kind("feature").with_assignee("alice").with_labels(vec!["urgent"]),
            IssueSetup::new("Wrong Priority").with_priority(2).with_issue_kind("bug").with_assignee("alice").with_labels(vec!["urgent"]),
        ],
        filter: FilterParams::new().with_priority(0).with_issue_kind("bug").with_assignee("alice").with_label("urgent"),
        expected_count: 1,
        expected_titles: Some(vec!["Perfect Match"]),
    }
)]
// -------------------------------------------------------------------------
// Edge cases
// -------------------------------------------------------------------------
#[case::no_matches(
    ReadyFilterCase {
        setup: vec![
            IssueSetup::new("Bug").with_issue_kind("bug"),
            IssueSetup::new("Feature").with_issue_kind("feature"),
        ],
        filter: FilterParams::new().with_issue_kind("epic"),
        expected_count: 0,
        expected_titles: None,
    }
)]
#[case::all_match(
    ReadyFilterCase {
        setup: vec![
            IssueSetup::new("Bug 1").with_issue_kind("bug"),
            IssueSetup::new("Bug 2").with_issue_kind("bug"),
            IssueSetup::new("Bug 3").with_issue_kind("bug"),
        ],
        filter: FilterParams::new().with_issue_kind("bug"),
        expected_count: 3,
        expected_titles: Some(vec!["Bug 1", "Bug 2", "Bug 3"]),
    }
)]
#[case::limit_with_filters(
    ReadyFilterCase {
        setup: vec![
            IssueSetup::new("Bug 1").with_issue_kind("bug"),
            IssueSetup::new("Bug 2").with_issue_kind("bug"),
            IssueSetup::new("Bug 3").with_issue_kind("bug"),
            IssueSetup::new("Feature 1").with_issue_kind("feature"),
        ],
        filter: FilterParams::new().with_issue_kind("bug").with_limit(2),
        expected_count: 2,
        expected_titles: None,
    }
)]
#[case::excludes_closed_issues(
    ReadyFilterCase {
        setup: vec![
            IssueSetup::new("Open Bug").with_issue_kind("bug"),
            IssueSetup::new("Closed Bug").with_issue_kind("bug").closed(),
        ],
        filter: FilterParams::new().with_issue_kind("bug"),
        expected_count: 1,
        expected_titles: Some(vec!["Open Bug"]),
    }
)]
#[tokio::test]
async fn test_ready_filters(#[case] test_case: ReadyFilterCase) {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;

    // Create all issues from setup
    for setup in &test_case.setup {
        create_custom_issue(&tools, setup).await;
    }

    // Apply filter
    let results = tools
        .ready(ready_params(
            test_case.filter.limit,
            test_case.filter.priority,
            test_case.filter.issue_kind,
            test_case.filter.assignee.map(str::to_string),
            test_case.filter.label.map(str::to_string),
            None,
        ))
        .await
        .expect("ready should succeed");

    // Verify count
    assert_eq!(
        results.len(),
        test_case.expected_count,
        "Expected {} issues, got {}",
        test_case.expected_count,
        results.len()
    );

    // Verify titles if specified (bidirectional check)
    if let Some(expected_titles) = test_case.expected_titles {
        let actual_titles: Vec<&str> = results.iter().map(|i| i.title.as_str()).collect();

        // Check all expected titles are present
        for title in &expected_titles {
            assert!(
                actual_titles.contains(title),
                "Expected title '{title}' not found in results: {actual_titles:?}"
            );
        }

        // Check no unexpected titles are present
        for actual in &actual_titles {
            assert!(
                expected_titles.contains(actual),
                "Unexpected title '{actual}' in results. Expected only: {expected_titles:?}"
            );
        }
    }
}

// ============================================================================
// Edge Case Filter Tests (rivets-8fe)
// ============================================================================

/// Test that empty filter results return an empty vec without errors.
#[tokio::test]
async fn test_empty_filter_results() {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;

    // Create some issues that won't match our filter
    let setup_issues = vec![
        IssueSetup::new("Bug Issue")
            .with_issue_kind("bug")
            .with_priority(2),
        IssueSetup::new("Feature Issue")
            .with_issue_kind("feature")
            .with_priority(3),
    ];

    for setup in &setup_issues {
        create_custom_issue(&tools, setup).await;
    }

    // Filter for a combination that doesn't exist
    let results = tools
        .list(list_params(
            Some("open"),
            Some(0),      // P0 priority - none of our issues have this
            Some("epic"), // Epic Kind - none of our issues have this
            Some("nonexistent-user".to_string()),
            Some("nonexistent-label".to_string()),
            None,
            None,
        ))
        .await
        .expect("list should succeed even with no matches");

    assert!(
        results.is_empty(),
        "Expected empty vec for non-matching filter, got {} results",
        results.len()
    );

    // Also test ready with non-matching filter
    let ready_results = tools
        .ready(ready_params(
            None,
            Some(0),
            Some("epic"),
            Some("nonexistent-user".to_string()),
            None,
            None,
        ))
        .await
        .expect("ready should succeed even with no matches");

    assert!(
        ready_results.is_empty(),
        "Expected empty vec for non-matching ready filter"
    );
}

/// Test filtering by a label when issue has multiple labels.
#[tokio::test]
async fn test_multiple_labels_filter() {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;

    // Create issue with multiple labels
    let multi_label_issue =
        IssueSetup::new("Multi-label Issue").with_labels(vec!["frontend", "backend", "urgent"]);
    let single_label_issue = IssueSetup::new("Single Label Issue").with_labels(vec!["frontend"]);
    let no_backend_issue =
        IssueSetup::new("No Backend Issue").with_labels(vec!["frontend", "urgent"]);

    create_custom_issue(&tools, &multi_label_issue).await;
    create_custom_issue(&tools, &single_label_issue).await;
    create_custom_issue(&tools, &no_backend_issue).await;

    // Filter by "backend" - should only find the multi-label issue
    let results = tools
        .list(list_params(
            None,
            None,
            None,
            None,
            Some("backend".to_string()),
            None,
            None,
        ))
        .await
        .expect("list should succeed");

    assert_eq!(results.len(), 1, "Expected 1 issue with 'backend' label");
    assert_eq!(results[0].title, "Multi-label Issue");
    assert!(results[0].labels.contains(&"backend".to_string()));
    assert!(results[0].labels.contains(&"frontend".to_string()));
    assert!(results[0].labels.contains(&"urgent".to_string()));

    // Filter by "frontend" - should find all three
    let frontend_results = tools
        .list(list_params(
            None,
            None,
            None,
            None,
            Some("frontend".to_string()),
            None,
            None,
        ))
        .await
        .expect("list should succeed");

    assert_eq!(
        frontend_results.len(),
        3,
        "Expected 3 issues with 'frontend' label"
    );

    // Filter by "urgent" - should find 2
    let urgent_results = tools
        .list(list_params(
            None,
            None,
            None,
            None,
            Some("urgent".to_string()),
            None,
            None,
        ))
        .await
        .expect("list should succeed");

    assert_eq!(
        urgent_results.len(),
        2,
        "Expected 2 issues with 'urgent' label"
    );
}

/// Test case sensitivity for assignee filter.
/// Documents that assignee filtering is case-sensitive.
#[tokio::test]
async fn test_assignee_case_sensitivity() {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;

    // Create issue with mixed-case assignee
    let issue = tools
        .create(create_params(
            "Alice's Task".to_string(),
            None,
            None,
            None,
            Some("Alice".to_string()), // Mixed case
            None,
            None,
            None,
            None,
        ))
        .await
        .expect("create should succeed");

    assert_eq!(issue.assignee, Some("Alice".to_string()));

    // Filter with exact case - should match
    let exact_match = tools
        .list(list_params(
            None,
            None,
            None,
            Some("Alice".to_string()),
            None,
            None,
            None,
        ))
        .await
        .expect("list should succeed");

    assert_eq!(
        exact_match.len(),
        1,
        "Exact case match should find the issue"
    );

    // Filter with different case - behavior documented here
    let lowercase_match = tools
        .list(list_params(
            None,
            None,
            None,
            Some("alice".to_string()),
            None,
            None,
            None,
        ))
        .await
        .expect("list should succeed");

    // NOTE: This documents the current behavior - assignee filtering is case-sensitive
    // "alice" does not match "Alice"
    assert_eq!(
        lowercase_match.len(),
        0,
        "Assignee filtering is case-sensitive: 'alice' does not match 'Alice'"
    );

    // Also test uppercase
    let uppercase_match = tools
        .list(list_params(
            None,
            None,
            None,
            Some("ALICE".to_string()),
            None,
            None,
            None,
        ))
        .await
        .expect("list should succeed");

    assert_eq!(
        uppercase_match.len(),
        0,
        "Assignee filtering is case-sensitive: 'ALICE' does not match 'Alice'"
    );
}

/// Test Unicode support in various fields.
#[tokio::test]
async fn test_unicode_support() {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;

    // Create issue with Japanese title
    let japanese_issue = tools
        .create(create_params(
            "バグ修正".to_string(),               // "Bug fix" in Japanese
            Some("これはテストです".to_string()), // "This is a test"
            Some(1),
            Some("bug"),
            None,
            None,
            None,
            None,
            None,
        ))
        .await
        .expect("create with Japanese title should succeed");

    assert_eq!(japanese_issue.title, "バグ修正");
    assert_eq!(japanese_issue.description, "これはテストです");

    // Verify we can retrieve it
    let shown = tools
        .show(japanese_issue.id.as_str(), None)
        .await
        .expect("show should work with unicode issue");
    assert_eq!(shown.title, "バグ修正");

    // Create issue with emoji label
    let emoji_issue = tools
        .create(create_params(
            "Hot Fix".to_string(),
            None,
            Some(0),
            Some("bug"),
            None,
            Some(vec!["🔥hotfix".to_string(), "critical".to_string()]),
            None,
            None,
            None,
        ))
        .await
        .expect("create with emoji label should succeed");

    assert!(emoji_issue.labels.contains(&"🔥hotfix".to_string()));

    // Filter by emoji label
    let emoji_filtered = tools
        .list(list_params(
            None,
            None,
            None,
            None,
            Some("🔥hotfix".to_string()),
            None,
            None,
        ))
        .await
        .expect("list with emoji label filter should succeed");

    assert_eq!(emoji_filtered.len(), 1);
    assert_eq!(emoji_filtered[0].title, "Hot Fix");

    // Create issue with accented assignee name
    let accented_issue = tools
        .create(create_params(
            "Accented Assignee Task".to_string(),
            None,
            None,
            None,
            Some("José García".to_string()),
            None,
            None,
            None,
            None,
        ))
        .await
        .expect("create with accented assignee should succeed");

    assert_eq!(accented_issue.assignee, Some("José García".to_string()));

    // Filter by accented assignee
    let accented_filtered = tools
        .list(list_params(
            None,
            None,
            None,
            Some("José García".to_string()),
            None,
            None,
            None,
        ))
        .await
        .expect("list with accented assignee filter should succeed");

    assert_eq!(accented_filtered.len(), 1);
    assert_eq!(accented_filtered[0].title, "Accented Assignee Task");

    // Verify all issues are in the list
    let all_issues = tools
        .list(list_params(None, None, None, None, None, None, None))
        .await
        .expect("list all should succeed");

    assert_eq!(all_issues.len(), 3, "Should have 3 unicode-related issues");
}

/// Test Unicode in title search/list.
#[tokio::test]
async fn test_unicode_titles_in_list() {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;

    // Create various unicode-titled issues
    let titles = vec![
        "バグ修正",            // Japanese
        "Ошибка исправлена",   // Russian
        "修复漏洞",            // Chinese
        "버그 수정",           // Korean
        "Résolution de bogue", // French with accents
    ];

    for title in &titles {
        tools
            .create(create_params(
                (*title).to_string(),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            ))
            .await
            .expect("create with unicode title should succeed");
    }

    // List all and verify they're all present
    let all_issues = tools
        .list(list_params(None, None, None, None, None, None, None))
        .await
        .expect("list should succeed");

    assert_eq!(
        all_issues.len(),
        5,
        "Should have all 5 unicode-titled issues"
    );

    for title in &titles {
        assert!(
            all_issues.iter().any(|i| i.title == *title),
            "Missing issue with title: {title}"
        );
    }
}

// ============================================================================
// Invalid Filter Values and Error Path Tests (rivets-2pn)
// ============================================================================

/// Test invalid status values return appropriate errors.
#[rstest]
#[case::invalid_status("invalid", "status")]
#[case::pending_status("pending", "status")]
#[case::done_status("done", "status")]
#[case::completed_status("completed", "status")]
#[case::active_status("active", "status")]
#[tokio::test]
async fn test_invalid_status_values(#[case] invalid_value: &str, #[case] expected_field: &str) {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;

    let result = tools
        .list(list_params(
            Some(invalid_value),
            None,
            None,
            None,
            None,
            None,
            None,
        ))
        .await;

    assert!(
        result.is_err(),
        "Expected error for invalid status: {invalid_value}"
    );
    match result.unwrap_err() {
        Error::InvalidArgument {
            field,
            value,
            valid_values,
        } => {
            assert_eq!(field, expected_field);
            assert_eq!(value, invalid_value);
            assert_eq!(valid_values, "open, in_progress, closed");
        }
        e => panic!("Expected InvalidArgument error, got: {e:?}"),
    }
}

/// Invalid Issue Kind values fail at every MCP parameter boundary.
#[rstest]
#[case::invalid_kind("invalid")]
#[case::story_kind("story")]
#[case::spike_kind("spike")]
#[case::enhancement_kind("enhancement")]
#[case::defect_kind("defect")]
fn test_invalid_issue_kind_values(#[case] invalid_value: &str) {
    let requests = [
        (
            "ready",
            serde_json::from_value::<ReadyParams>(serde_json::json!({
                "issue_kind": invalid_value
            }))
            .map(|_| ()),
        ),
        (
            "list",
            serde_json::from_value::<ListParams>(serde_json::json!({
                "issue_kind": invalid_value
            }))
            .map(|_| ()),
        ),
        (
            "create",
            serde_json::from_value::<CreateParams>(serde_json::json!({
                "title": "Test",
                "issue_kind": invalid_value
            }))
            .map(|_| ()),
        ),
        (
            "update",
            serde_json::from_value::<UpdateParams>(serde_json::json!({
                "issue_id": "test-issue",
                "issue_kind": invalid_value
            }))
            .map(|_| ()),
        ),
    ];

    for (tool, result) in requests {
        let error = result.expect_err("invalid Issue Kind should not reach the tool");
        let message = error.to_string();
        assert!(
            message.contains(invalid_value),
            "{tool} error should identify invalid value: {message}"
        );
        for valid_kind in ["bug", "feature", "task", "epic", "chore"] {
            assert!(
                message.contains(valid_kind),
                "{tool} error should name valid Kind {valid_kind}: {message}"
            );
        }
    }
}

/// Test invalid status in update returns appropriate error.
#[rstest]
#[case::invalid_update_status("invalid")]
#[case::pending_update_status("pending")]
#[case::done_update_status("done")]
#[tokio::test]
async fn test_invalid_status_in_update(#[case] invalid_value: &str) {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;

    let issue = create_issue(&tools, "Test Issue").await;

    let result = tools
        .update(update_params(
            issue.id.as_str(),
            None,
            None,
            Some(invalid_value),
            None,
            None, // issue_kind
            None,
            None,
            None, // labels
            None, // workspace_root
        ))
        .await;

    assert!(
        result.is_err(),
        "Expected error for invalid status in update: {invalid_value}"
    );
    match result.unwrap_err() {
        Error::InvalidArgument { field, value, .. } => {
            assert_eq!(field, "status");
            assert_eq!(value, invalid_value);
        }
        e => panic!("Expected InvalidArgument error, got: {e:?}"),
    }
}

/// Test error message formatting includes all relevant information.
#[tokio::test]
async fn test_error_message_format() {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;

    // Test status error message format
    let result = tools
        .list(list_params(
            Some("bogus_status"),
            None,
            None,
            None,
            None,
            None,
            None,
        ))
        .await;

    let error = result.unwrap_err();
    let error_msg = error.to_string();

    // Verify error message contains useful information
    assert!(
        error_msg.contains("status"),
        "Error message should contain field name 'status': {error_msg}"
    );
    assert!(
        error_msg.contains("bogus_status"),
        "Error message should contain invalid value 'bogus_status': {error_msg}"
    );
    assert!(
        error_msg.contains("open") || error_msg.contains("Valid"),
        "Error message should contain valid options: {error_msg}"
    );
}

// ============================================================================
// Additional Lifecycle and Integration Tests (rivets-d06)
// ============================================================================

/// Test complete issue lifecycle through multiple state transitions.
#[tokio::test]
async fn canonical_workflow_state_inputs() {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;

    // Create issue (starts as open)
    let created = tools
        .create(create_params(
            "Lifecycle Issue".to_string(),
            Some("Testing full lifecycle".to_string()),
            Some(2),
            Some("feature"),
            Some("developer".to_string()),
            Some(vec!["lifecycle-test".to_string()]),
            Some("Design notes here".to_string()),
            Some("- [ ] Criteria 1".to_string()),
            None,
        ))
        .await
        .expect("create should succeed");

    assert_eq!(created.status, IssueStatus::Open);
    assert!(created.closed_at.is_none());
    let rejected = tools
        .update(update_params(
            created.id.as_str(),
            None,
            None,
            Some("blocked"),
            None,
            None,
            None,
            None,
            None,
            None,
        ))
        .await
        .expect_err("Blocked is derived and must not be accepted as Workflow State");
    match rejected {
        Error::InvalidArgument {
            field,
            value,
            valid_values,
        } => {
            assert_eq!(field, "status");
            assert_eq!(value, "blocked");
            assert_eq!(valid_values, "open, in_progress, closed");
        }
        other => panic!("expected InvalidArgument, got {other:?}"),
    }
    assert_eq!(
        tools
            .show(created.id.as_str(), None)
            .await
            .expect("rejected update preserves Issue")
            .status,
        IssueStatus::Open
    );

    // Transition to in_progress
    let in_progress = tools
        .update(update_params(
            created.id.as_str(),
            None,
            None,
            Some("in_progress"),
            None,
            None, // issue_kind
            None,
            None,
            None, // labels
            None, // workspace_root
        ))
        .await
        .expect("update to in_progress should succeed");

    assert_eq!(in_progress.status, IssueStatus::InProgress);

    // Close the issue
    let closed = tools
        .close(
            created.id.as_str(),
            Some("Completed successfully".to_string()),
            None,
        )
        .await
        .expect("close should succeed");

    assert_eq!(closed.status, IssueStatus::Closed);
    assert!(closed.closed_at.is_some());

    // Verify final state via show
    let final_state = tools
        .show(created.id.as_str(), None)
        .await
        .expect("show should succeed");

    assert_eq!(final_state.status, IssueStatus::Closed);
    assert_eq!(final_state.title, "Lifecycle Issue");
    assert_eq!(final_state.description, "Testing full lifecycle");
    assert!(final_state.closed_at.is_some());
}

/// Test issue updates preserve unmodified fields.
#[tokio::test]
async fn test_update_preserves_unmodified_fields() {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;

    // Create issue with all fields
    let created = tools
        .create(create_params(
            "Original Title".to_string(),
            Some("Original Description".to_string()),
            Some(1),
            Some("bug"),
            Some("alice".to_string()),
            Some(vec!["label1".to_string(), "label2".to_string()]),
            Some("Original Design".to_string()),
            Some("Original Criteria".to_string()),
            None,
        ))
        .await
        .expect("create should succeed");

    // Update only the title
    let updated = tools
        .update(update_params(
            created.id.as_str(),
            Some("New Title".to_string()),
            None, // Don't update description
            None, // Don't update status
            None, // Don't update priority
            None, // issue_kind
            None, // Don't update design
            None, // Don't update acceptance
            None, // labels
            None, // workspace_root
        ))
        .await
        .expect("update should succeed");

    // Verify title changed
    assert_eq!(updated.title, "New Title");

    // Verify all other fields preserved
    assert_eq!(updated.description, "Original Description");
    assert_eq!(updated.status, IssueStatus::Open);
    assert_eq!(updated.priority, 1);
    assert_eq!(updated.assignee, Some("alice".to_string()));
    assert_eq!(updated.design, Some("Original Design".to_string()));
    assert_eq!(
        updated.acceptance_criteria,
        Some("Original Criteria".to_string())
    );
    assert_eq!(updated.labels, vec!["label1", "label2"]);
}

/// Test rapid context switching between workspaces.
#[tokio::test]
async fn test_rapid_workspace_context_switching() {
    let workspaces: Vec<_> = (0..5).map(|_| create_temp_workspace()).collect();
    let tools = create_tools();

    // Create issues in each workspace
    for (i, workspace) in workspaces.iter().enumerate() {
        set_context(&tools, workspace.path()).await;
        create_issue(&tools, &format!("Issue in workspace {i}")).await;
    }

    // Rapidly switch between workspaces and verify isolation
    for _ in 0..10 {
        for (i, workspace) in workspaces.iter().enumerate() {
            set_context(&tools, workspace.path()).await;

            let issues = tools
                .list(list_params(None, None, None, None, None, None, None))
                .await
                .expect("list should succeed");

            assert_eq!(issues.len(), 1, "Workspace {i} should have exactly 1 issue");
            assert_eq!(issues[0].title, format!("Issue in workspace {i}"));
        }
    }
}

/// Test error response format for various error types.
#[tokio::test]
async fn test_error_response_formats() {
    let workspace = create_temp_workspace();
    let tools = create_tools();

    // Test NoContext error format
    let no_context_err = tools
        .list(list_params(None, None, None, None, None, None, None))
        .await
        .unwrap_err();
    let no_context_msg = no_context_err.to_string();
    assert!(
        no_context_msg.contains("context") || no_context_msg.contains("Context"),
        "NoContext error should mention 'context': {no_context_msg}"
    );

    // Set context for remaining tests
    set_context(&tools, workspace.path()).await;

    // Test IssueNotFound error format
    let not_found_err = tools.show("nonexistent-xyz-123", None).await.unwrap_err();
    let not_found_msg = not_found_err.to_string();
    assert!(
        not_found_msg.contains("nonexistent-xyz-123"),
        "IssueNotFound error should contain the missing ID: {not_found_msg}"
    );

    // Test InvalidArgument error format
    let invalid_arg_err = tools
        .list(list_params(
            Some("bogus"),
            None,
            None,
            None,
            None,
            None,
            None,
        ))
        .await
        .unwrap_err();
    let invalid_arg_msg = invalid_arg_err.to_string();
    assert!(
        invalid_arg_msg.contains("bogus"),
        "InvalidArgument error should contain the invalid value: {invalid_arg_msg}"
    );
    assert!(
        invalid_arg_msg.contains("status"),
        "InvalidArgument error should contain the field name: {invalid_arg_msg}"
    );
}

/// Test all tools with real storage backend (comprehensive integration).
#[tokio::test]
async fn test_all_tools_with_storage_backend() {
    let workspace = create_temp_workspace();
    let tools = create_tools();

    // 1. set_context
    let ctx_response = tools
        .set_context(&workspace.path().display().to_string())
        .await
        .expect("set_context should succeed");
    assert!(ctx_response.message.contains("success") || ctx_response.message.contains("Context"));

    // 2. where_am_i
    let where_response = tools.where_am_i().await.expect("where_am_i should succeed");
    assert!(where_response.context_set);
    assert!(where_response.workspace_root.is_some());

    // 3. create
    let created = tools
        .create(create_params(
            "Integration Test Issue".to_string(),
            Some("Full integration test".to_string()),
            Some(1),
            Some("task"),
            Some("tester".to_string()),
            Some(vec!["integration".to_string()]),
            Some("Design".to_string()),
            Some("Criteria".to_string()),
            None,
        ))
        .await
        .expect("create should succeed");
    assert!(!created.id.as_str().is_empty());

    // 4. show
    let shown = tools
        .show(created.id.as_str(), None)
        .await
        .expect("show should succeed");
    assert_eq!(shown.id, created.id);
    assert_eq!(shown.title, "Integration Test Issue");

    // 5. list
    let listed = tools
        .list(list_params(None, None, None, None, None, None, None))
        .await
        .expect("list should succeed");
    assert!(!listed.is_empty());

    // 6. ready (the created Issue is assigned to tester)
    let ready = tools
        .ready(ready_params(
            None,
            None,
            None,
            Some("tester".to_string()),
            None,
            None,
        ))
        .await
        .expect("ready should succeed");
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].id, created.id);

    // 7. update
    let updated = tools
        .update(update_params(
            created.id.as_str(),
            Some("Updated Title".to_string()),
            None,
            Some("in_progress"),
            None,
            None, // issue_kind
            None,
            None,
            None, // labels
            None, // workspace_root
        ))
        .await
        .expect("update should succeed");
    assert_eq!(updated.title, "Updated Title");
    assert_eq!(updated.status, IssueStatus::InProgress);

    // 8. Create another issue for dependency testing
    let blocker = tools
        .create(create_params(
            "Blocker Issue".to_string(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        ))
        .await
        .expect("create blocker should succeed");

    // 9. add Blocking Dependency
    let dep_result = tools
        .blocking_dependency_add(created.id.as_str(), blocker.id.as_str(), None)
        .await
        .expect("Blocking Dependency should succeed");
    assert_eq!(dep_result.dependent_id(), &created.id);
    assert_eq!(dep_result.prerequisite_id(), &blocker.id);

    // 10. blocked
    let blocked_issues = tools.blocked(None).await.expect("blocked should succeed");
    assert!(!blocked_issues.is_empty());
    assert!(blocked_issues.iter().any(|b| b.issue.id == created.id));

    // 11. close
    let closed = tools
        .close(
            created.id.as_str(),
            Some("Test completed".to_string()),
            None,
        )
        .await
        .expect("close should succeed");
    assert_eq!(closed.status, IssueStatus::Closed);
}

/// Test dependency chains don't cause issues.
#[tokio::test]
async fn test_dependency_chain() {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;

    // Create chain: A -> B -> C (A depends on B, B depends on C)
    let issue_c = create_issue(&tools, "Issue C").await;
    let issue_b = create_issue(&tools, "Issue B").await;
    let issue_a = create_issue(&tools, "Issue A").await;

    // B depends on C
    tools
        .blocking_dependency_add(issue_b.id.as_str(), issue_c.id.as_str(), None)
        .await
        .expect("B->C dep should succeed");

    // A depends on B
    tools
        .blocking_dependency_add(issue_a.id.as_str(), issue_b.id.as_str(), None)
        .await
        .expect("A->B dep should succeed");

    // Check blocked issues
    let blocked = tools.blocked(None).await.expect("blocked should succeed");

    // Both A and B should be blocked
    assert!(blocked.len() >= 2, "At least 2 issues should be blocked");
    assert!(
        blocked.iter().any(|b| b.issue.id == issue_a.id),
        "Issue A should be blocked"
    );
    assert!(
        blocked.iter().any(|b| b.issue.id == issue_b.id),
        "Issue B should be blocked"
    );

    // Only C should be ready (not blocked)
    let ready = tools
        .ready(ready_params(None, None, None, None, None, None))
        .await
        .expect("ready should succeed");

    let ready_ids: Vec<_> = ready.iter().map(|i| i.id.as_str()).collect();
    assert!(
        ready_ids.contains(&issue_c.id.as_str()),
        "Issue C should be ready"
    );
    assert!(
        !ready_ids.contains(&issue_a.id.as_str()),
        "Issue A should not be ready"
    );
    assert!(
        !ready_ids.contains(&issue_b.id.as_str()),
        "Issue B should not be ready"
    );
}

/// Test closing a blocker unblocks dependent issues.
#[tokio::test]
async fn ready_and_blocked_survive_context_recreation() {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;

    // Create blocker and dependent
    let blocker = create_issue(&tools, "Blocker").await;
    let dependent = create_issue(&tools, "Dependent").await;

    // Add dependency
    tools
        .blocking_dependency_add(dependent.id.as_str(), blocker.id.as_str(), None)
        .await
        .expect("dep should succeed");

    // Verify dependent is blocked
    let blocked_before = tools.blocked(None).await.expect("blocked should succeed");
    assert!(
        blocked_before.iter().any(|b| b.issue.id == dependent.id),
        "Dependent should be blocked before"
    );

    // Close the blocker
    tools
        .close(blocker.id.as_str(), Some("Done".to_string()), None)
        .await
        .expect("close should succeed");

    // Verify dependent is no longer blocked
    let blocked_after = tools.blocked(None).await.expect("blocked should succeed");
    assert!(
        !blocked_after.iter().any(|b| b.issue.id == dependent.id),
        "Dependent should not be blocked after closing blocker"
    );

    // Verify dependent is now ready
    let ready = tools
        .ready(ready_params(None, None, None, None, None, None))
        .await
        .expect("ready should succeed");
    assert!(
        ready.iter().any(|i| i.id == dependent.id),
        "Dependent should be ready after blocker is closed"
    );

    let restarted = create_tools();
    set_context(&restarted, workspace.path()).await;
    let retained = restarted
        .blocking_dependency_list(
            &BlockingDependencyListQuery::PrerequisitesOf {
                dependent_id: dependent.id.to_string(),
            },
            None,
        )
        .await
        .expect("closed prerequisite relationship should reload");
    assert_eq!(retained.len(), 1);
    assert_eq!(retained[0].dependent_id(), &dependent.id);
    assert_eq!(retained[0].prerequisite_id(), &blocker.id);
    let ready_after_restart = restarted
        .ready(ready_params(None, None, None, None, None, None))
        .await
        .expect("Ready should be re-derived after context restart");
    assert!(
        ready_after_restart
            .iter()
            .any(|issue| issue.id == dependent.id),
        "Dependent should remain Ready after context restart"
    );
    let blocked_after_restart = restarted
        .blocked(None)
        .await
        .expect("Blocked should be re-derived after context restart");
    assert!(
        !blocked_after_restart
            .iter()
            .any(|entry| entry.issue.id == dependent.id),
        "Closed prerequisite must remain resolved after context restart"
    );
}

/// Test stats are accurate after various operations.
#[tokio::test]
async fn test_issue_counts_accurate() {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;

    // Create issues with various states
    let issue1 = create_issue(&tools, "Open Issue").await;
    let issue2 = tools
        .create(create_params(
            "In Progress Issue".to_string(),
            None,
            None,
            None,
            Some("active-owner".to_string()),
            None,
            None,
            None,
            None,
        ))
        .await
        .expect("create should succeed");
    let issue3 = create_issue(&tools, "To Close").await;

    tools
        .update(update_params(
            issue2.id.as_str(),
            None,
            None,
            Some("in_progress"),
            None,
            None, // issue_kind
            None,
            None,
            None, // labels
            None, // workspace_root
        ))
        .await
        .unwrap();

    tools
        .close(issue3.id.as_str(), Some("Done".to_string()), None)
        .await
        .unwrap();

    // Verify counts
    let all = tools
        .list(list_params(None, None, None, None, None, None, None))
        .await
        .unwrap();
    assert_eq!(all.len(), 3, "Should have 3 total issues");

    let open = tools
        .list(list_params(
            Some("open"),
            None,
            None,
            None,
            None,
            None,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(open.len(), 1, "Should have 1 open issue");
    assert_eq!(open[0].id, issue1.id);

    let in_progress = tools
        .list(list_params(
            Some("in_progress"),
            None,
            None,
            None,
            None,
            None,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(in_progress.len(), 1, "Should have 1 in_progress issue");
    assert_eq!(in_progress[0].id, issue2.id);

    let closed = tools
        .list(list_params(
            Some("closed"),
            None,
            None,
            None,
            None,
            None,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(closed.len(), 1, "Should have 1 closed issue");
    assert_eq!(closed[0].id, issue3.id);
}

// =============================================================================
// Associated Resource Tests
// =============================================================================

#[tokio::test]
async fn resource_add_list_and_context_recreation_use_real_storage() {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;
    let issue = create_issue(&tools, "Resource owner").await;

    let first = tools
        .resource_add(
            issue.id.as_str(),
            Some("https://example.com/pr/123".to_string()),
            None,
            "implementation",
            Some("Implementation PR".to_string()),
            None,
        )
        .await
        .expect("first resource should be added");
    assert_eq!(first.resources().len(), 1);
    assert_eq!(first.resources()[0].id().as_str(), "r1");

    let second = tools
        .resource_add(
            issue.id.as_str(),
            Some("https://example.com/pr/123".to_string()),
            None,
            "documentation",
            None,
            None,
        )
        .await
        .expect("same target with distinct role should be added");
    assert_eq!(second.resources().len(), 2);
    assert_eq!(second.resources()[0].role().to_string(), "implementation");
    assert_eq!(second.resources()[1].role().to_string(), "documentation");

    let resources = tools
        .resource_list(issue.id.as_str(), None)
        .await
        .expect("resource list should succeed");
    assert_eq!(resources.len(), 2);
    assert_eq!(resources[0].id().as_str(), "r1");
    assert_eq!(
        resources[0]
            .label()
            .map(rivets::domain::ResourceLabel::as_str),
        Some("Implementation PR")
    );
    assert_eq!(resources[1].id().as_str(), "r2");
    assert!(resources[1].label().is_none());
    match resources[0].target() {
        ResourceTarget::Web { url } => {
            assert_eq!(url.as_str(), "https://example.com/pr/123");
        }
        ResourceTarget::Path { .. } => panic!("web add must not produce a path target"),
    }

    let restarted = create_tools();
    set_context(&restarted, workspace.path()).await;
    let persisted = restarted
        .resource_list(issue.id.as_str(), None)
        .await
        .expect("resources should survive context recreation");
    assert_eq!(persisted, resources);

    let shown = restarted
        .show(issue.id.as_str(), None)
        .await
        .expect("full Issue response should include resources");
    assert_eq!(shown.resources(), persisted.as_slice());

    let data = std::fs::read_to_string(workspace.path().join(".rivets/issues.jsonl"))
        .expect("issues file should be readable");
    let record: serde_json::Value = data
        .lines()
        .map(|line| serde_json::from_str(line).expect("record should be JSON"))
        .find(|record: &serde_json::Value| record["id"] == issue.id.as_str())
        .expect("Issue should be persisted");
    assert!(record.get("external_ref").is_none());
    assert_eq!(record["resources"].as_array().unwrap().len(), 2);
    assert_eq!(record["next_resource_id"], 3);
}

#[tokio::test]
async fn resource_add_rejects_invalid_inputs_without_mutation() {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;
    let issue = create_issue(&tools, "Resource validation").await;

    tools
        .resource_add(
            issue.id.as_str(),
            Some("https://example.com/pr/123".to_string()),
            None,
            "implementation",
            None,
            None,
        )
        .await
        .expect("initial resource should be added");

    let duplicate = tools
        .resource_add(
            issue.id.as_str(),
            Some("https://example.com/pr/123".to_string()),
            None,
            "implementation",
            None,
            None,
        )
        .await
        .expect_err("exact target-and-role duplicate should fail");
    assert!(matches!(duplicate, Error::InvalidResource(_)));

    assert!(matches!(
        tools
            .resource_add(
                issue.id.as_str(),
                Some("docs/adr/0003-associated-resources.md".to_string()),
                None,
                "reference",
                None,
                None,
            )
            .await,
        Err(Error::InvalidResource(_))
    ));
    assert!(matches!(
        tools
            .resource_add(
                issue.id.as_str(),
                Some("https://example.com/evidence".to_string()),
                None,
                "evidence",
                Some("   ".to_string()),
                None,
            )
            .await,
        Err(Error::InvalidResource(_))
    ));
    assert!(matches!(
        tools
            .resource_add(
                issue.id.as_str(),
                Some("https://example.com/evidence".to_string()),
                None,
                "Evidence",
                None,
                None,
            )
            .await,
        Err(Error::InvalidArgument { field: "role", .. })
    ));
    assert!(matches!(
        tools
            .resource_add(
                "test-missing",
                Some("https://example.com/reference".to_string()),
                None,
                "reference",
                None,
                None,
            )
            .await,
        Err(Error::IssueNotFound(issue_id)) if issue_id == "test-missing"
    ));
    assert_eq!(
        tools
            .resource_list(issue.id.as_str(), None)
            .await
            .expect("failures must not mutate storage")
            .len(),
        1
    );
}

#[tokio::test]
async fn legacy_web_external_ref_migrates_through_mcp_and_persists() {
    let workspace = create_temp_workspace();
    let issues_path = workspace.path().join(".rivets/issues.jsonl");
    let legacy = r#"{"id":"test-legacy","title":"Legacy URL","description":"Test","status":"open","priority":2,"issue_type":"task","assignee":null,"labels":[],"design":null,"acceptance_criteria":null,"notes":null,"external_ref":"https://example.com/legacy","dependencies":[],"created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-02T00:00:00Z","closed_at":null}"#;
    std::fs::write(&issues_path, format!("{legacy}\n")).expect("legacy record should be seeded");

    let tools = create_tools();
    set_context(&tools, workspace.path()).await;
    let migrated = tools
        .resource_list("test-legacy", None)
        .await
        .expect("legacy resource should be visible");
    assert_eq!(migrated.len(), 1);
    assert_eq!(migrated[0].id().as_str(), "r1");
    assert_eq!(migrated[0].role().to_string(), "reference");

    let updated = tools
        .resource_add(
            "test-legacy",
            Some("https://example.com/new".to_string()),
            None,
            "evidence",
            None,
            None,
        )
        .await
        .expect("mutation should canonicalize and persist");
    assert_eq!(updated.resources().len(), 2);

    let restarted = create_tools();
    set_context(&restarted, workspace.path()).await;
    let persisted = restarted
        .resource_list("test-legacy", None)
        .await
        .expect("migrated resources should survive context recreation");
    assert_eq!(persisted.len(), 2);
    assert_eq!(persisted[0].id().as_str(), "r1");
    assert_eq!(persisted[1].id().as_str(), "r2");

    let canonical =
        std::fs::read_to_string(issues_path).expect("canonical record should be readable");
    let record: serde_json::Value =
        serde_json::from_str(canonical.trim()).expect("record should be JSON");
    assert!(record.get("external_ref").is_none());
    assert!(record.get("issue_type").is_none());
    assert_eq!(record["issue_kind"], "task");
    assert_eq!(record["resources"].as_array().unwrap().len(), 2);
    assert_eq!(record["next_resource_id"], 3);
}

async fn add_three_resources(tools: &Tools, issue_id: &str) {
    tools
        .resource_add(
            issue_id,
            Some("https://example.com/pr/123".to_string()),
            None,
            "implementation",
            Some("PR".to_string()),
            None,
        )
        .await
        .expect("web add should succeed");
    tools
        .resource_add(
            issue_id,
            None,
            Some("\u{e9}/\u{6587}\u{4ef6}.md".to_string()),
            "evidence",
            None,
            None,
        )
        .await
        .expect("unicode path add should succeed");
    tools
        .resource_add(
            issue_id,
            None,
            Some("docs/../docs/adr/0003.md".to_string()),
            "documentation",
            None,
            None,
        )
        .await
        .expect("path add should succeed");
}

#[tokio::test]
async fn resource_add_accepts_path_targets_and_normalizes() {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;
    let issue = create_issue(&tools, "Path owner").await;
    add_three_resources(&tools, issue.id.as_str()).await;

    let resources = tools
        .resource_list(issue.id.as_str(), None)
        .await
        .expect("list should succeed");
    assert_eq!(
        *resources[1].target(),
        ResourceTarget::path(
            WorkspacePath::new("\u{e9}/\u{6587}\u{4ef6}.md").expect("unicode path should be valid")
        ),
        "unicode path must be preserved"
    );
    assert_eq!(
        *resources[2].target(),
        ResourceTarget::path(
            WorkspacePath::new("docs/adr/0003.md").expect("normalized path should be valid")
        ),
        "persisted path target must be normalized"
    );
}

#[tokio::test]
async fn resource_update_remove_keep_identity_and_position() {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;
    let issue = create_issue(&tools, "Update owner").await;
    add_three_resources(&tools, issue.id.as_str()).await;

    // Update middle resource: role + label, id and position preserved.
    let updated = tools
        .resource_update(rivets_mcp::models::ResourceUpdateParams {
            issue_id: issue.id.as_str().to_string(),
            resource_id: "r2".to_string(),
            url: None,
            path: None,
            role: Some("reference".to_string()),
            label: Some("evidence note".to_string()),
            clear_label: false,
            workspace_root: None,
        })
        .await
        .expect("update should succeed");
    assert_eq!(updated.resources().len(), 3);
    assert_eq!(updated.resources()[1].id().as_str(), "r2");
    assert_eq!(updated.resources()[1].role().to_string(), "reference");
    assert_eq!(
        updated.resources()[1]
            .label()
            .map(rivets::domain::ResourceLabel::as_str),
        Some("evidence note")
    );

    // Update target web -> path, then clear the label.
    let retargeted = tools
        .resource_update(rivets_mcp::models::ResourceUpdateParams {
            issue_id: issue.id.as_str().to_string(),
            resource_id: "r1".to_string(),
            url: None,
            path: Some("crates/rivets/src/main.rs".to_string()),
            role: None,
            label: None,
            clear_label: true,
            workspace_root: None,
        })
        .await
        .expect("target change should succeed");
    assert_eq!(
        *retargeted.resources()[0].target(),
        ResourceTarget::path(
            WorkspacePath::new("crates/rivets/src/main.rs").expect("retarget path should be valid")
        )
    );
    assert!(
        retargeted.resources()[0].label().is_none(),
        "label must be cleared"
    );

    // Remove middle resource; remaining keep ids/positions.
    let removed = tools
        .resource_remove(issue.id.as_str(), "r2", None)
        .await
        .expect("remove should succeed");
    assert_eq!(
        removed
            .resources()
            .iter()
            .map(|resource| resource.id().as_str())
            .collect::<Vec<_>>(),
        ["r1", "r3"]
    );
}

#[tokio::test]
async fn resource_update_remove_errors_are_typed_without_mutation() {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;
    let issue = create_issue(&tools, "Error owner").await;
    tools
        .resource_add(
            issue.id.as_str(),
            None,
            Some("docs/adr/0003.md".to_string()),
            "documentation",
            None,
            None,
        )
        .await
        .expect("path add should succeed");

    // Duplicate target-role is a typed InvalidResource error.
    let duplicate = tools
        .resource_add(
            issue.id.as_str(),
            None,
            Some("docs/adr/0003.md".to_string()),
            "documentation",
            None,
            None,
        )
        .await;
    assert!(matches!(duplicate, Err(Error::InvalidResource(_))));

    // Unknown resource id is a typed InvalidResource error.
    let unknown = tools.resource_remove(issue.id.as_str(), "r99", None).await;
    assert!(matches!(unknown, Err(Error::InvalidResource(_))));

    // An escaping path is a typed InvalidResource error.
    let escaping = tools
        .resource_add(
            issue.id.as_str(),
            None,
            Some("../outside.md".to_string()),
            "reference",
            None,
            None,
        )
        .await;
    assert!(matches!(escaping, Err(Error::InvalidResource(_))));

    // Conflicting or missing target arguments are typed InvalidArgument errors.
    let both_targets = tools
        .resource_add(
            issue.id.as_str(),
            Some("https://example.com/x".to_string()),
            Some("src/y.rs".to_string()),
            "reference",
            None,
            None,
        )
        .await;
    assert!(matches!(
        both_targets,
        Err(Error::InvalidArgument {
            field: "target",
            ..
        })
    ));
    let neither = tools
        .resource_add(issue.id.as_str(), None, None, "reference", None, None)
        .await;
    assert!(matches!(
        neither,
        Err(Error::InvalidArgument {
            field: "target",
            ..
        })
    ));

    assert_eq!(
        tools
            .resource_list(issue.id.as_str(), None)
            .await
            .expect("list should succeed")
            .len(),
        1,
        "all failures must leave storage unmutated"
    );
}

#[tokio::test]
async fn resource_mutations_persist_across_context_restart() {
    let workspace = create_temp_workspace();
    let issues_path = workspace.path().join(".rivets/issues.jsonl");
    let issue_id: String;

    // Session 1: add, update, and remove in one context.
    {
        let tools = create_tools();
        set_context(&tools, workspace.path()).await;
        let issue = create_issue(&tools, "Restart owner").await;
        issue_id = issue.id.as_str().to_string();
        add_three_resources(&tools, &issue_id).await;
        tools
            .resource_update(rivets_mcp::models::ResourceUpdateParams {
                issue_id: issue_id.clone(),
                resource_id: "r1".to_string(),
                url: None,
                path: None,
                role: Some("successor".to_string()),
                label: None,
                clear_label: true,
                workspace_root: None,
            })
            .await
            .expect("update should succeed");
        tools
            .resource_remove(&issue_id, "r2", None)
            .await
            .expect("remove should succeed");
    }

    // Session 2: a fresh Tools (context restart) sees the exact same state.
    {
        let tools = create_tools();
        set_context(&tools, workspace.path()).await;
        let persisted = tools
            .resource_list(&issue_id, None)
            .await
            .expect("resources should survive context recreation");
        assert_eq!(
            persisted
                .iter()
                .map(|resource| resource.id().as_str())
                .collect::<Vec<_>>(),
            ["r1", "r3"],
            "removal keeps remaining ids and positions"
        );
        assert_eq!(persisted[0].role().to_string(), "successor");
        assert!(persisted[0].label().is_none(), "label clear must persist");
        assert_eq!(
            *persisted[1].target(),
            ResourceTarget::path(
                WorkspacePath::new("docs/adr/0003.md").expect("persisted path should be valid")
            ),
            "normalized path target must persist"
        );
    }

    // The raw JSONL records the same state; the sequence never reuses r2.
    let canonical =
        std::fs::read_to_string(issues_path).expect("canonical record should be readable");
    let record: serde_json::Value =
        serde_json::from_str(canonical.trim()).expect("record should be JSON");
    assert_eq!(record["next_resource_id"], 4);
    let persisted_ids: Vec<_> = record["resources"]
        .as_array()
        .expect("resources should be an array")
        .iter()
        .map(|r| r["id"].as_str().expect("id is a string").to_string())
        .collect();
    assert_eq!(persisted_ids, ["r1", "r3"]);
}

// ============================================================================
// Mixed legacy Workspace migration (rivets-c5e5)
// ============================================================================

fn assert_mixed_issue_kinds(issues: &[Issue]) {
    for (issue_id, expected_kind) in [
        ("test-missing", IssueKind::Bug),
        ("test-null", IssueKind::Feature),
        (LEGACY_NOTE_ID, IssueKind::Task),
        (LEGACY_URL_ID, IssueKind::Epic),
        (LEGACY_OPAQUE_ID, IssueKind::Chore),
        ("test-canonical", IssueKind::Bug),
        ("test-equal-kind", IssueKind::Task),
        (CONFLICT_ID, IssueKind::Feature),
    ] {
        let issue = issues
            .iter()
            .find(|issue| issue.id.as_str() == issue_id)
            .unwrap_or_else(|| panic!("MCP list should include Issue {issue_id}"));
        assert_eq!(issue.issue_kind, expected_kind);
    }
}

async fn assert_migrated_mcp_content(tools: &Tools, fixture: &[serde_json::Value]) -> String {
    let expected_long_note = record(fixture, LEGACY_NOTE_ID)["notes"]
        .as_str()
        .expect("legacy Note fixture should be a string")
        .to_owned();
    let long_note_issue = tools
        .show(LEGACY_NOTE_ID, None)
        .await
        .expect("MCP show should convert the legacy multiline Note");
    assert_eq!(long_note_issue.notes().len(), 1);
    assert_eq!(long_note_issue.notes()[0].content(), expected_long_note);
    assert_eq!(
        *long_note_issue.notes()[0].created_at(),
        long_note_issue.updated_at
    );

    let url_issue = tools
        .show(LEGACY_URL_ID, None)
        .await
        .expect("MCP show should convert a legacy Web resource");
    assert!(url_issue.notes().is_empty());
    assert_eq!(url_issue.resources().len(), 1);
    assert_eq!(url_issue.resources()[0].id().as_str(), "r1");
    assert_eq!(
        url_issue.resources()[0].target().to_string(),
        "https://example.com/legacy/pr/7"
    );
    assert_eq!(url_issue.resources()[0].role().to_string(), "reference");
    assert!(url_issue.resources()[0].label().is_none());

    let opaque_issue = tools
        .show(LEGACY_OPAQUE_ID, None)
        .await
        .expect("MCP show should preserve an opaque legacy reference as a Note");
    assert_eq!(opaque_issue.notes().len(), 1);
    assert_eq!(
        opaque_issue.notes()[0].content(),
        "Migrated legacy external reference:   GH-opaque-42  "
    );
    assert_eq!(
        *opaque_issue.notes()[0].created_at(),
        opaque_issue.updated_at
    );

    let canonical_issue = tools
        .show("test-canonical", None)
        .await
        .expect("MCP show should preserve canonical nested ordering");
    assert_eq!(canonical_issue.notes()[0].content(), "Canonical first Note");
    assert_eq!(
        canonical_issue.notes()[1].content(),
        "Canonical second Note"
    );
    assert_eq!(canonical_issue.resources()[0].id().as_str(), "r1");
    assert_eq!(canonical_issue.resources()[1].id().as_str(), "r2");
    assert_eq!(
        canonical_issue.resources()[0].role().to_string(),
        "evidence"
    );
    assert_eq!(
        canonical_issue.resources()[1].role().to_string(),
        "documentation"
    );

    expected_long_note
}

async fn assert_reloaded_mcp_content(tools: &Tools, expected_long_note: &str) {
    let reloaded = tools
        .list(list_params(None, None, None, None, None, None, None))
        .await
        .expect("fresh MCP context should reload every canonical record");
    assert_eq!(reloaded.len(), MIXED_ISSUE_COUNT);
    assert_mixed_issue_kinds(&reloaded);

    let reloaded_long_note = tools
        .show(LEGACY_NOTE_ID, None)
        .await
        .expect("fresh MCP context should reload the converted long Note");
    assert_eq!(reloaded_long_note.notes()[0].content(), expected_long_note);

    let reloaded_url = tools
        .show(LEGACY_URL_ID, None)
        .await
        .expect("fresh MCP context should reload the migrated URL resource");
    assert_eq!(reloaded_url.resources().len(), 1);
    assert_eq!(
        reloaded_url.resources()[0].target().to_string(),
        "https://example.com/legacy/pr/7"
    );

    let reloaded_opaque = tools
        .show(LEGACY_OPAQUE_ID, None)
        .await
        .expect("fresh MCP context should reload the opaque-reference Note");
    assert_eq!(
        reloaded_opaque.notes()[0].content(),
        "Migrated legacy external reference:   GH-opaque-42  "
    );

    let reloaded_canonical = tools
        .show("test-canonical", None)
        .await
        .expect("fresh MCP context should reload canonical nested ordering");
    assert_eq!(
        reloaded_canonical.notes()[0].content(),
        "Canonical first Note"
    );
    assert_eq!(
        reloaded_canonical.notes()[1].content(),
        "Canonical second Note"
    );
    assert_eq!(reloaded_canonical.resources()[0].id().as_str(), "r1");
    assert_eq!(reloaded_canonical.resources()[1].id().as_str(), "r2");

    let reloaded_conflict = tools
        .show(CONFLICT_ID, None)
        .await
        .expect("fresh MCP context should reload the mutated conflict Issue");
    assert_eq!(reloaded_conflict.title, "Canonical migration update");
    assert_eq!(reloaded_conflict.issue_kind, IssueKind::Feature);
}

#[tokio::test]
async fn mixed_legacy_fixture_migrates_through_mcp_and_context_recreation() {
    let workspace = create_temp_workspace();
    seed_mixed_workspace(workspace.path());
    let issues_path = workspace.path().join(".rivets/issues.jsonl");
    let atomic_temp_path = issues_path.with_extension("tmp");
    let fixture = fixture_records();

    let tools = create_tools();
    set_context(&tools, workspace.path()).await;
    let listed = tools
        .list(list_params(None, None, None, None, None, None, None))
        .await
        .expect("MCP list should load every mixed legacy record");
    assert_eq!(listed.len(), MIXED_ISSUE_COUNT);
    for fixture_record in &fixture {
        let issue_id = fixture_record["id"]
            .as_str()
            .expect("fixture Issue id should be a string");
        assert!(listed.iter().any(|issue| issue.id.as_str() == issue_id));
    }
    assert_mixed_issue_kinds(&listed);
    let expected_long_note = assert_migrated_mcp_content(&tools, &fixture).await;

    let updated = tools
        .update(update_params(
            CONFLICT_ID,
            Some("Canonical migration update".to_string()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        ))
        .await
        .expect("one deterministic MCP update should rewrite the mixed file");
    assert_eq!(updated.title, "Canonical migration update");
    assert_eq!(updated.issue_kind, IssueKind::Feature);

    let persisted_records = read_records(&issues_path);
    assert_canonical_records(&persisted_records);
    let persisted_ids: Vec<&str> = persisted_records
        .iter()
        .map(|record| {
            record["id"]
                .as_str()
                .expect("persisted Issue id should be a string")
        })
        .collect();
    let mut sorted_ids = persisted_ids.clone();
    sorted_ids.sort_unstable();
    assert_eq!(persisted_ids, sorted_ids);
    assert_eq!(
        record(&persisted_records, CONFLICT_ID)["title"],
        "Canonical migration update"
    );
    assert!(!atomic_temp_path.exists());

    let restarted = create_tools();
    set_context(&restarted, workspace.path()).await;
    assert_reloaded_mcp_content(&restarted, &expected_long_note).await;
}
