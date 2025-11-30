//! Integration tests for rivets-mcp server.
//!
//! These tests exercise the MCP tools with real JSONL storage backends
//! to verify end-to-end behavior including:
//! - Complete issue lifecycle (create -> update -> close)
//! - Multi-workspace context switching
//! - Error response verification
//! - Real storage persistence

use rivets_mcp::context::Context;
use rivets_mcp::error::Error;
use rivets_mcp::models::McpIssue;
use rivets_mcp::tools::Tools;
use rstest::rstest;
use std::sync::Arc;
use tempfile::TempDir;
use tokio::sync::RwLock;

mod helpers {
    use super::*;
    use std::path::Path;

    /// Create a temporary workspace with `.rivets/` directory.
    pub fn create_temp_workspace() -> TempDir {
        let temp = TempDir::new().expect("Failed to create temp dir");
        std::fs::create_dir(temp.path().join(".rivets")).expect("Failed to create .rivets dir");
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
    pub async fn create_issue(tools: &Tools, title: &str) -> McpIssue {
        tools
            .create(
                title.to_string(),
                Some(format!("Description for {title}")),
                Some(2),
                Some("task"),
                None,
                None,
                None,
                None,
                None,
            )
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
        pub issue_type: Option<&'static str>,
        pub assignee: Option<&'static str>,
        pub labels: Option<Vec<&'static str>>,
        pub close_after_create: bool,
    }

    impl IssueSetup {
        pub fn new(title: &'static str) -> Self {
            Self {
                title,
                priority: None,
                issue_type: None,
                assignee: None,
                labels: None,
                close_after_create: false,
            }
        }

        pub fn with_priority(mut self, p: u8) -> Self {
            self.priority = Some(p);
            self
        }

        pub fn with_issue_type(mut self, t: &'static str) -> Self {
            self.issue_type = Some(t);
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
        pub issue_type: Option<&'static str>,
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

        pub fn with_issue_type(mut self, t: &'static str) -> Self {
            self.issue_type = Some(t);
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
    pub async fn create_custom_issue(tools: &Tools, setup: &IssueSetup) -> McpIssue {
        let labels = setup
            .labels
            .as_ref()
            .map(|l| l.iter().copied().map(str::to_string).collect());

        let issue = tools
            .create(
                setup.title.to_string(),
                Some(format!("Description for {}", setup.title)),
                setup.priority,
                setup.issue_type,
                setup.assignee.map(str::to_string),
                labels,
                None,
                None,
                None,
            )
            .await
            .expect("create should succeed");

        if setup.close_after_create {
            tools
                .close(&issue.id, None, None)
                .await
                .expect("Failed to close issue during setup");
            // Fetch updated issue after closing
            tools
                .show(&issue.id, None)
                .await
                .expect("Failed to fetch closed issue")
        } else {
            issue
        }
    }
}

use helpers::*;

// ============================================================================
// Issue Lifecycle Tests
// ============================================================================

/// Test complete issue lifecycle: create -> update -> close
#[tokio::test]
async fn test_issue_lifecycle_create_update_close() {
    let workspace = create_temp_workspace();
    let tools = create_tools();

    // Set context
    set_context(&tools, workspace.path()).await;

    // Create issue
    let created = create_issue(&tools, "Lifecycle Test Issue").await;
    assert_eq!(created.status, "open");

    // Update to in_progress
    let updated = tools
        .update(
            &created.id,
            None,
            None,
            Some("in_progress"),
            Some(1),
            Some(Some("alice".to_string())),
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .expect("update should succeed");

    assert_eq!(updated.status, "in_progress");
    assert_eq!(updated.priority, 1);
    assert_eq!(updated.assignee, Some("alice".to_string()));

    // Close the issue
    let closed = tools
        .close(
            &created.id,
            Some("Completed successfully".to_string()),
            None,
        )
        .await
        .expect("close should succeed");

    assert_eq!(closed.status, "closed");

    // Verify via show
    let shown = tools
        .show(&created.id, None)
        .await
        .expect("show should succeed");
    assert_eq!(shown.status, "closed");
}

/// Test issue creation with all optional fields.
#[tokio::test]
async fn test_create_issue_with_all_fields() {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;

    let issue = tools
        .create(
            "Full Issue".to_string(),
            Some("Detailed description".to_string()),
            Some(0),
            Some("feature"),
            Some("bob".to_string()),
            Some(vec!["urgent".to_string(), "frontend".to_string()]),
            Some("Technical design notes".to_string()),
            Some("- [ ] Criteria 1\n- [ ] Criteria 2".to_string()),
            None,
        )
        .await
        .expect("create should succeed");

    assert_eq!(issue.title, "Full Issue");
    assert_eq!(issue.description, "Detailed description");
    assert_eq!(issue.priority, 0);
    assert_eq!(issue.issue_type, "feature");
    assert_eq!(issue.assignee, Some("bob".to_string()));
    assert_eq!(issue.design, Some("Technical design notes".to_string()));
    assert_eq!(
        issue.acceptance_criteria,
        Some("- [ ] Criteria 1\n- [ ] Criteria 2".to_string())
    );
}

/// Test creating multiple issue types.
#[tokio::test]
async fn test_create_all_issue_types() {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;

    let types = ["bug", "feature", "task", "epic", "chore"];

    for issue_type in types {
        let issue = tools
            .create(
                format!("A {issue_type}"),
                None,
                None,
                Some(issue_type),
                None,
                None,
                None,
                None,
                None,
            )
            .await
            .expect("create should succeed");

        assert_eq!(issue.issue_type, issue_type);
    }

    let list = tools
        .list(None, None, None, None, None, None, None)
        .await
        .expect("list should succeed");
    assert_eq!(list.len(), 5);
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
        .list(None, None, None, None, None, None, None)
        .await
        .expect("list should succeed");
    assert_eq!(issues_b.len(), 1);
    assert_eq!(issues_b[0].title, "Issue in Workspace B");

    // Switch back to workspace A
    set_context(&tools, workspace_a.path()).await;
    let issues_a = tools
        .list(None, None, None, None, None, None, None)
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
        .list(
            None,
            None,
            None,
            None,
            None,
            None,
            Some(&workspace_a.path().display().to_string()),
        )
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
        .list(None, None, None, None, None, None, None)
        .await
        .unwrap();
    assert_eq!(issues_b.len(), 2);

    set_context(&tools, workspace_a.path()).await;
    let issues_a = tools
        .list(None, None, None, None, None, None, None)
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

    let result = tools.list(None, None, None, None, None, None, None).await;

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
        .list(Some("not_a_status"), None, None, None, None, None, None)
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

/// Test error response for invalid issue type.
#[tokio::test]
async fn test_error_invalid_issue_type() {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;

    let result = tools
        .create(
            "Test".to_string(),
            None,
            None,
            Some("invalid_type"),
            None,
            None,
            None,
            None,
            None,
        )
        .await;

    assert!(result.is_err());
    match result.unwrap_err() {
        Error::InvalidArgument { field, value, .. } => {
            assert_eq!(field, "issue_type");
            assert_eq!(value, "invalid_type");
        }
        e => panic!("Expected InvalidArgument error, got: {e:?}"),
    }
}

/// Test error response for invalid dependency type.
#[tokio::test]
async fn test_error_invalid_dep_type() {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;

    let issue1 = create_issue(&tools, "Issue 1").await;
    let issue2 = create_issue(&tools, "Issue 2").await;

    let result = tools
        .dep(&issue1.id, &issue2.id, Some("invalid_dep"), None)
        .await;

    assert!(result.is_err());
    match result.unwrap_err() {
        Error::InvalidArgument { field, value, .. } => {
            assert_eq!(field, "dep_type");
            assert_eq!(value, "invalid_dep");
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

/// Test error response for workspace without .rivets directory.
#[tokio::test]
async fn test_error_no_rivets_directory() {
    let temp = TempDir::new().unwrap();
    // Don't create .rivets directory
    let tools = create_tools();

    let result = tools.set_context(&temp.path().display().to_string()).await;

    assert!(result.is_err());
    match result.unwrap_err() {
        Error::NoRivetsDirectory(path) => {
            assert!(!path.is_empty());
        }
        e => panic!("Expected NoRivetsDirectory error, got: {e:?}"),
    }
}

/// Test error for accessing uninitialized workspace via `workspace_root` parameter.
#[tokio::test]
async fn test_error_workspace_not_initialized() {
    let workspace_a = create_temp_workspace();
    let workspace_b = create_temp_workspace();
    let tools = create_tools();

    // Only initialize workspace A
    set_context(&tools, workspace_a.path()).await;

    // Try to access B without initializing it
    let result = tools
        .list(
            None,
            None,
            None,
            None,
            None,
            None,
            Some(&workspace_b.path().display().to_string()),
        )
        .await;

    assert!(result.is_err());
    match result.unwrap_err() {
        Error::WorkspaceNotInitialized(_) => {} // Expected
        e => panic!("Expected WorkspaceNotInitialized error, got: {e:?}"),
    }
}

// ============================================================================
// Dependency Tests
// ============================================================================

/// Test adding dependencies between issues.
#[tokio::test]
async fn test_dependency_management() {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;

    // Create two issues
    let blocker = create_issue(&tools, "Blocker Issue").await;
    let dependent = create_issue(&tools, "Dependent Issue").await;

    // Add dependency
    let result = tools
        .dep(&dependent.id, &blocker.id, Some("blocks"), None)
        .await
        .expect("dep should succeed");

    // Verify dependency was added
    assert!(result.contains("Added dependency"));
    assert!(result.contains(&dependent.id));
    assert!(result.contains(&blocker.id));

    // Check blocked issues
    let blocked_issues = tools.blocked(None).await.expect("blocked should succeed");
    assert_eq!(blocked_issues.len(), 1);
    assert_eq!(blocked_issues[0].issue.id, dependent.id);
    assert_eq!(blocked_issues[0].blockers.len(), 1);
    assert_eq!(blocked_issues[0].blockers[0].id, blocker.id);
}

/// Test all dependency types.
#[tokio::test]
async fn test_all_dependency_types() {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;

    let dep_types = ["blocks", "related", "parent-child", "discovered-from"];

    for dep_type in dep_types {
        let issue1 = create_issue(&tools, &format!("Issue for {dep_type} 1")).await;
        let issue2 = create_issue(&tools, &format!("Issue for {dep_type} 2")).await;

        let result = tools
            .dep(&issue1.id, &issue2.id, Some(dep_type), None)
            .await
            .expect("dep should succeed");

        assert!(result.contains(dep_type));
    }
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
        .dep(&dependent.id, &blocker.id, Some("blocks"), None)
        .await
        .unwrap();

    // Ready should only return the blocker (dependent is blocked)
    let ready = tools
        .ready(None, None, None, None, None, None)
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
#[case::issue_type_bug(
    ListFilterCase {
        setup: vec![
            IssueSetup::new("A Bug").with_issue_type("bug"),
            IssueSetup::new("A Feature").with_issue_type("feature"),
        ],
        filter: FilterParams::new().with_issue_type("bug"),
        expected_count: 1,
        expected_titles: Some(vec!["A Bug"]),
    }
)]
#[case::issue_type_feature(
    ListFilterCase {
        setup: vec![
            IssueSetup::new("A Bug").with_issue_type("bug"),
            IssueSetup::new("A Feature").with_issue_type("feature"),
        ],
        filter: FilterParams::new().with_issue_type("feature"),
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
#[case::status_and_type(
    ListFilterCase {
        setup: vec![
            IssueSetup::new("Open Bug").with_issue_type("bug"),
            IssueSetup::new("Open Feature").with_issue_type("feature"),
            IssueSetup::new("Closed Bug").with_issue_type("bug").closed(),
        ],
        filter: FilterParams::new().with_status("open").with_issue_type("bug"),
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
#[case::priority_and_type(
    ListFilterCase {
        setup: vec![
            IssueSetup::new("P0 Bug").with_priority(0).with_issue_type("bug"),
            IssueSetup::new("P0 Feature").with_priority(0).with_issue_type("feature"),
            IssueSetup::new("P2 Bug").with_priority(2).with_issue_type("bug"),
        ],
        filter: FilterParams::new().with_priority(0).with_issue_type("bug"),
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
#[case::type_and_assignee(
    ListFilterCase {
        setup: vec![
            IssueSetup::new("Bug Alice").with_issue_type("bug").with_assignee("alice"),
            IssueSetup::new("Bug Bob").with_issue_type("bug").with_assignee("bob"),
            IssueSetup::new("Feature Alice").with_issue_type("feature").with_assignee("alice"),
        ],
        filter: FilterParams::new().with_issue_type("bug").with_assignee("alice"),
        expected_count: 1,
        expected_titles: Some(vec!["Bug Alice"]),
    }
)]
#[case::type_and_label(
    ListFilterCase {
        setup: vec![
            IssueSetup::new("Bug Urgent").with_issue_type("bug").with_labels(vec!["urgent"]),
            IssueSetup::new("Bug Normal").with_issue_type("bug").with_labels(vec!["normal"]),
            IssueSetup::new("Feature Urgent").with_issue_type("feature").with_labels(vec!["urgent"]),
        ],
        filter: FilterParams::new().with_issue_type("bug").with_label("urgent"),
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
#[case::status_priority_type(
    ListFilterCase {
        setup: vec![
            IssueSetup::new("Open P0 Bug").with_priority(0).with_issue_type("bug"),
            IssueSetup::new("Open P0 Feature").with_priority(0).with_issue_type("feature"),
            IssueSetup::new("Open P2 Bug").with_priority(2).with_issue_type("bug"),
            IssueSetup::new("Closed P0 Bug").with_priority(0).with_issue_type("bug").closed(),
        ],
        filter: FilterParams::new().with_status("open").with_priority(0).with_issue_type("bug"),
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
#[case::status_type_label(
    ListFilterCase {
        setup: vec![
            IssueSetup::new("Open Bug Urgent").with_issue_type("bug").with_labels(vec!["urgent"]),
            IssueSetup::new("Open Bug Normal").with_issue_type("bug").with_labels(vec!["normal"]),
            IssueSetup::new("Open Feature Urgent").with_issue_type("feature").with_labels(vec!["urgent"]),
            IssueSetup::new("Closed Bug Urgent").with_issue_type("bug").with_labels(vec!["urgent"]).closed(),
        ],
        filter: FilterParams::new().with_status("open").with_issue_type("bug").with_label("urgent"),
        expected_count: 1,
        expected_titles: Some(vec!["Open Bug Urgent"]),
    }
)]
#[case::priority_type_assignee(
    ListFilterCase {
        setup: vec![
            IssueSetup::new("P0 Bug Alice").with_priority(0).with_issue_type("bug").with_assignee("alice"),
            IssueSetup::new("P0 Bug Bob").with_priority(0).with_issue_type("bug").with_assignee("bob"),
            IssueSetup::new("P0 Feature Alice").with_priority(0).with_issue_type("feature").with_assignee("alice"),
            IssueSetup::new("P2 Bug Alice").with_priority(2).with_issue_type("bug").with_assignee("alice"),
        ],
        filter: FilterParams::new().with_priority(0).with_issue_type("bug").with_assignee("alice"),
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
#[case::type_assignee_label(
    ListFilterCase {
        setup: vec![
            IssueSetup::new("Bug Alice Urgent").with_issue_type("bug").with_assignee("alice").with_labels(vec!["urgent"]),
            IssueSetup::new("Bug Alice Normal").with_issue_type("bug").with_assignee("alice").with_labels(vec!["normal"]),
            IssueSetup::new("Bug Bob Urgent").with_issue_type("bug").with_assignee("bob").with_labels(vec!["urgent"]),
            IssueSetup::new("Feature Alice Urgent").with_issue_type("feature").with_assignee("alice").with_labels(vec!["urgent"]),
        ],
        filter: FilterParams::new().with_issue_type("bug").with_assignee("alice").with_label("urgent"),
        expected_count: 1,
        expected_titles: Some(vec!["Bug Alice Urgent"]),
    }
)]
// -------------------------------------------------------------------------
// Four-way and five-way filter combinations
// -------------------------------------------------------------------------
#[case::four_way_status_priority_type_assignee(
    ListFilterCase {
        setup: vec![
            IssueSetup::new("Target").with_priority(0).with_issue_type("bug").with_assignee("alice"),
            IssueSetup::new("Wrong Priority").with_priority(2).with_issue_type("bug").with_assignee("alice"),
            IssueSetup::new("Wrong Type").with_priority(0).with_issue_type("feature").with_assignee("alice"),
            IssueSetup::new("Wrong Assignee").with_priority(0).with_issue_type("bug").with_assignee("bob"),
            IssueSetup::new("Closed Match").with_priority(0).with_issue_type("bug").with_assignee("alice").closed(),
        ],
        filter: FilterParams::new().with_status("open").with_priority(0).with_issue_type("bug").with_assignee("alice"),
        expected_count: 1,
        expected_titles: Some(vec!["Target"]),
    }
)]
#[case::five_way_all_filters(
    ListFilterCase {
        setup: vec![
            IssueSetup::new("Perfect Match").with_priority(0).with_issue_type("bug").with_assignee("alice").with_labels(vec!["urgent"]),
            IssueSetup::new("Wrong Label").with_priority(0).with_issue_type("bug").with_assignee("alice").with_labels(vec!["normal"]),
            IssueSetup::new("Wrong Assignee").with_priority(0).with_issue_type("bug").with_assignee("bob").with_labels(vec!["urgent"]),
            IssueSetup::new("Wrong Type").with_priority(0).with_issue_type("feature").with_assignee("alice").with_labels(vec!["urgent"]),
            IssueSetup::new("Wrong Priority").with_priority(2).with_issue_type("bug").with_assignee("alice").with_labels(vec!["urgent"]),
            IssueSetup::new("Closed Match").with_priority(0).with_issue_type("bug").with_assignee("alice").with_labels(vec!["urgent"]).closed(),
        ],
        filter: FilterParams::new().with_status("open").with_priority(0).with_issue_type("bug").with_assignee("alice").with_label("urgent"),
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
            IssueSetup::new("Bug").with_issue_type("bug"),
            IssueSetup::new("Feature").with_issue_type("feature"),
        ],
        filter: FilterParams::new().with_issue_type("epic"),
        expected_count: 0,
        expected_titles: None,
    }
)]
#[case::all_match(
    ListFilterCase {
        setup: vec![
            IssueSetup::new("Bug 1").with_issue_type("bug"),
            IssueSetup::new("Bug 2").with_issue_type("bug"),
            IssueSetup::new("Bug 3").with_issue_type("bug"),
        ],
        filter: FilterParams::new().with_issue_type("bug"),
        expected_count: 3,
        expected_titles: Some(vec!["Bug 1", "Bug 2", "Bug 3"]),
    }
)]
#[case::limit_with_filters(
    ListFilterCase {
        setup: vec![
            IssueSetup::new("Bug 1").with_issue_type("bug"),
            IssueSetup::new("Bug 2").with_issue_type("bug"),
            IssueSetup::new("Bug 3").with_issue_type("bug"),
            IssueSetup::new("Feature 1").with_issue_type("feature"),
        ],
        filter: FilterParams::new().with_issue_type("bug").with_limit(2),
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
        .list(
            test_case.filter.status,
            test_case.filter.priority,
            test_case.filter.issue_type,
            test_case.filter.assignee.map(str::to_string),
            test_case.filter.label.map(str::to_string),
            test_case.filter.limit,
            None,
        )
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
// Assignee Tests
// ============================================================================

/// Test assignee clearing with empty string.
#[tokio::test]
async fn test_assignee_clearing() {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;

    // Create issue with assignee
    let created = tools
        .create(
            "Assigned Issue".to_string(),
            None,
            None,
            None,
            Some("alice".to_string()),
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();

    assert_eq!(created.assignee, Some("alice".to_string()));

    // Clear assignee by passing Some(None) - which comes from empty string in server layer
    let updated = tools
        .update(
            &created.id,
            None,
            None,
            None,
            None,
            Some(None), // This clears the assignee
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();

    assert!(updated.assignee.is_none(), "Assignee should be cleared");
}

/// Test assignee update vs no-op.
#[tokio::test]
async fn test_assignee_update_vs_noop() {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;

    let created = tools
        .create(
            "Test".to_string(),
            None,
            None,
            None,
            Some("original".to_string()),
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();

    // Update with None (no change)
    let unchanged = tools
        .update(
            &created.id,
            None,
            None,
            None,
            None,
            None, // None means don't update
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(unchanged.assignee, Some("original".to_string()));

    // Update with Some(Some("new"))
    let changed = tools
        .update(
            &created.id,
            None,
            None,
            None,
            None,
            Some(Some("new".to_string())),
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(changed.assignee, Some("new".to_string()));
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
            .list(None, None, None, None, None, None, None)
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
        let issue = create_issue(&tools, "To Update").await;
        issue_id = issue.id.clone();

        tools
            .update(
                &issue_id,
                Some("Updated Title".to_string()),
                None,
                Some("in_progress"),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();
    }

    // Verify persistence
    {
        let tools = create_tools();
        set_context(&tools, workspace.path()).await;

        let issue = tools.show(&issue_id, None).await.unwrap();
        assert_eq!(issue.title, "Updated Title");
        assert_eq!(issue.status, "in_progress");
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
#[case::issue_type_filter(
    ReadyFilterCase {
        setup: vec![
            IssueSetup::new("Bug Issue").with_issue_type("bug"),
            IssueSetup::new("Feature Issue").with_issue_type("feature"),
        ],
        filter: FilterParams::new().with_issue_type("bug"),
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
#[case::priority_and_type(
    ReadyFilterCase {
        setup: vec![
            IssueSetup::new("P0 Bug").with_priority(0).with_issue_type("bug"),
            IssueSetup::new("P0 Feature").with_priority(0).with_issue_type("feature"),
            IssueSetup::new("P2 Bug").with_priority(2).with_issue_type("bug"),
        ],
        filter: FilterParams::new().with_priority(0).with_issue_type("bug"),
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
#[case::type_and_assignee(
    ReadyFilterCase {
        setup: vec![
            IssueSetup::new("Bug Alice").with_issue_type("bug").with_assignee("alice"),
            IssueSetup::new("Bug Bob").with_issue_type("bug").with_assignee("bob"),
            IssueSetup::new("Feature Alice").with_issue_type("feature").with_assignee("alice"),
        ],
        filter: FilterParams::new().with_issue_type("bug").with_assignee("alice"),
        expected_count: 1,
        expected_titles: Some(vec!["Bug Alice"]),
    }
)]
#[case::type_and_label(
    ReadyFilterCase {
        setup: vec![
            IssueSetup::new("Bug Urgent").with_issue_type("bug").with_labels(vec!["urgent"]),
            IssueSetup::new("Bug Normal").with_issue_type("bug").with_labels(vec!["normal"]),
            IssueSetup::new("Feature Urgent").with_issue_type("feature").with_labels(vec!["urgent"]),
        ],
        filter: FilterParams::new().with_issue_type("bug").with_label("urgent"),
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
#[case::priority_type_assignee(
    ReadyFilterCase {
        setup: vec![
            IssueSetup::new("P0 Bug Alice").with_priority(0).with_issue_type("bug").with_assignee("alice"),
            IssueSetup::new("P0 Bug Bob").with_priority(0).with_issue_type("bug").with_assignee("bob"),
            IssueSetup::new("P0 Feature Alice").with_priority(0).with_issue_type("feature").with_assignee("alice"),
            IssueSetup::new("P2 Bug Alice").with_priority(2).with_issue_type("bug").with_assignee("alice"),
        ],
        filter: FilterParams::new().with_priority(0).with_issue_type("bug").with_assignee("alice"),
        expected_count: 1,
        expected_titles: Some(vec!["P0 Bug Alice"]),
    }
)]
#[case::priority_type_label(
    ReadyFilterCase {
        setup: vec![
            IssueSetup::new("P0 Bug Urgent").with_priority(0).with_issue_type("bug").with_labels(vec!["urgent"]),
            IssueSetup::new("P0 Bug Normal").with_priority(0).with_issue_type("bug").with_labels(vec!["normal"]),
            IssueSetup::new("P0 Feature Urgent").with_priority(0).with_issue_type("feature").with_labels(vec!["urgent"]),
            IssueSetup::new("P2 Bug Urgent").with_priority(2).with_issue_type("bug").with_labels(vec!["urgent"]),
        ],
        filter: FilterParams::new().with_priority(0).with_issue_type("bug").with_label("urgent"),
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
#[case::type_assignee_label(
    ReadyFilterCase {
        setup: vec![
            IssueSetup::new("Bug Alice Urgent").with_issue_type("bug").with_assignee("alice").with_labels(vec!["urgent"]),
            IssueSetup::new("Bug Alice Normal").with_issue_type("bug").with_assignee("alice").with_labels(vec!["normal"]),
            IssueSetup::new("Bug Bob Urgent").with_issue_type("bug").with_assignee("bob").with_labels(vec!["urgent"]),
            IssueSetup::new("Feature Alice Urgent").with_issue_type("feature").with_assignee("alice").with_labels(vec!["urgent"]),
        ],
        filter: FilterParams::new().with_issue_type("bug").with_assignee("alice").with_label("urgent"),
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
            IssueSetup::new("Perfect Match").with_priority(0).with_issue_type("bug").with_assignee("alice").with_labels(vec!["urgent"]),
            IssueSetup::new("Wrong Label").with_priority(0).with_issue_type("bug").with_assignee("alice").with_labels(vec!["normal"]),
            IssueSetup::new("Wrong Assignee").with_priority(0).with_issue_type("bug").with_assignee("bob").with_labels(vec!["urgent"]),
            IssueSetup::new("Wrong Type").with_priority(0).with_issue_type("feature").with_assignee("alice").with_labels(vec!["urgent"]),
            IssueSetup::new("Wrong Priority").with_priority(2).with_issue_type("bug").with_assignee("alice").with_labels(vec!["urgent"]),
        ],
        filter: FilterParams::new().with_priority(0).with_issue_type("bug").with_assignee("alice").with_label("urgent"),
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
            IssueSetup::new("Bug").with_issue_type("bug"),
            IssueSetup::new("Feature").with_issue_type("feature"),
        ],
        filter: FilterParams::new().with_issue_type("epic"),
        expected_count: 0,
        expected_titles: None,
    }
)]
#[case::all_match(
    ReadyFilterCase {
        setup: vec![
            IssueSetup::new("Bug 1").with_issue_type("bug"),
            IssueSetup::new("Bug 2").with_issue_type("bug"),
            IssueSetup::new("Bug 3").with_issue_type("bug"),
        ],
        filter: FilterParams::new().with_issue_type("bug"),
        expected_count: 3,
        expected_titles: Some(vec!["Bug 1", "Bug 2", "Bug 3"]),
    }
)]
#[case::limit_with_filters(
    ReadyFilterCase {
        setup: vec![
            IssueSetup::new("Bug 1").with_issue_type("bug"),
            IssueSetup::new("Bug 2").with_issue_type("bug"),
            IssueSetup::new("Bug 3").with_issue_type("bug"),
            IssueSetup::new("Feature 1").with_issue_type("feature"),
        ],
        filter: FilterParams::new().with_issue_type("bug").with_limit(2),
        expected_count: 2,
        expected_titles: None,
    }
)]
#[case::excludes_closed_issues(
    ReadyFilterCase {
        setup: vec![
            IssueSetup::new("Open Bug").with_issue_type("bug"),
            IssueSetup::new("Closed Bug").with_issue_type("bug").closed(),
        ],
        filter: FilterParams::new().with_issue_type("bug"),
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
        .ready(
            test_case.filter.limit,
            test_case.filter.priority,
            test_case.filter.issue_type,
            test_case.filter.assignee.map(str::to_string),
            test_case.filter.label.map(str::to_string),
            None,
        )
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
            .with_issue_type("bug")
            .with_priority(2),
        IssueSetup::new("Feature Issue")
            .with_issue_type("feature")
            .with_priority(3),
    ];

    for setup in &setup_issues {
        create_custom_issue(&tools, setup).await;
    }

    // Filter for a combination that doesn't exist
    let results = tools
        .list(
            Some("open"),
            Some(0),      // P0 priority - none of our issues have this
            Some("epic"), // Epic type - none of our issues have this
            Some("nonexistent-user".to_string()),
            Some("nonexistent-label".to_string()),
            None,
            None,
        )
        .await
        .expect("list should succeed even with no matches");

    assert!(
        results.is_empty(),
        "Expected empty vec for non-matching filter, got {} results",
        results.len()
    );

    // Also test ready with non-matching filter
    let ready_results = tools
        .ready(
            None,
            Some(0),
            Some("epic"),
            Some("nonexistent-user".to_string()),
            None,
            None,
        )
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
        .list(
            None,
            None,
            None,
            None,
            Some("backend".to_string()),
            None,
            None,
        )
        .await
        .expect("list should succeed");

    assert_eq!(results.len(), 1, "Expected 1 issue with 'backend' label");
    assert_eq!(results[0].title, "Multi-label Issue");
    assert!(results[0].labels.contains(&"backend".to_string()));
    assert!(results[0].labels.contains(&"frontend".to_string()));
    assert!(results[0].labels.contains(&"urgent".to_string()));

    // Filter by "frontend" - should find all three
    let frontend_results = tools
        .list(
            None,
            None,
            None,
            None,
            Some("frontend".to_string()),
            None,
            None,
        )
        .await
        .expect("list should succeed");

    assert_eq!(
        frontend_results.len(),
        3,
        "Expected 3 issues with 'frontend' label"
    );

    // Filter by "urgent" - should find 2
    let urgent_results = tools
        .list(
            None,
            None,
            None,
            None,
            Some("urgent".to_string()),
            None,
            None,
        )
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
        .create(
            "Alice's Task".to_string(),
            None,
            None,
            None,
            Some("Alice".to_string()), // Mixed case
            None,
            None,
            None,
            None,
        )
        .await
        .expect("create should succeed");

    assert_eq!(issue.assignee, Some("Alice".to_string()));

    // Filter with exact case - should match
    let exact_match = tools
        .list(
            None,
            None,
            None,
            Some("Alice".to_string()),
            None,
            None,
            None,
        )
        .await
        .expect("list should succeed");

    assert_eq!(
        exact_match.len(),
        1,
        "Exact case match should find the issue"
    );

    // Filter with different case - behavior documented here
    let lowercase_match = tools
        .list(
            None,
            None,
            None,
            Some("alice".to_string()),
            None,
            None,
            None,
        )
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
        .list(
            None,
            None,
            None,
            Some("ALICE".to_string()),
            None,
            None,
            None,
        )
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
        .create(
            "バグ修正".to_string(),               // "Bug fix" in Japanese
            Some("これはテストです".to_string()), // "This is a test"
            Some(1),
            Some("bug"),
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .expect("create with Japanese title should succeed");

    assert_eq!(japanese_issue.title, "バグ修正");
    assert_eq!(japanese_issue.description, "これはテストです");

    // Verify we can retrieve it
    let shown = tools
        .show(&japanese_issue.id, None)
        .await
        .expect("show should work with unicode issue");
    assert_eq!(shown.title, "バグ修正");

    // Create issue with emoji label
    let emoji_issue = tools
        .create(
            "Hot Fix".to_string(),
            None,
            Some(0),
            Some("bug"),
            None,
            Some(vec!["🔥hotfix".to_string(), "critical".to_string()]),
            None,
            None,
            None,
        )
        .await
        .expect("create with emoji label should succeed");

    assert!(emoji_issue.labels.contains(&"🔥hotfix".to_string()));

    // Filter by emoji label
    let emoji_filtered = tools
        .list(
            None,
            None,
            None,
            None,
            Some("🔥hotfix".to_string()),
            None,
            None,
        )
        .await
        .expect("list with emoji label filter should succeed");

    assert_eq!(emoji_filtered.len(), 1);
    assert_eq!(emoji_filtered[0].title, "Hot Fix");

    // Create issue with accented assignee name
    let accented_issue = tools
        .create(
            "Accented Assignee Task".to_string(),
            None,
            None,
            None,
            Some("José García".to_string()),
            None,
            None,
            None,
            None,
        )
        .await
        .expect("create with accented assignee should succeed");

    assert_eq!(accented_issue.assignee, Some("José García".to_string()));

    // Filter by accented assignee
    let accented_filtered = tools
        .list(
            None,
            None,
            None,
            Some("José García".to_string()),
            None,
            None,
            None,
        )
        .await
        .expect("list with accented assignee filter should succeed");

    assert_eq!(accented_filtered.len(), 1);
    assert_eq!(accented_filtered[0].title, "Accented Assignee Task");

    // Verify all issues are in the list
    let all_issues = tools
        .list(None, None, None, None, None, None, None)
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
            .create(
                (*title).to_string(),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .await
            .expect("create with unicode title should succeed");
    }

    // List all and verify they're all present
    let all_issues = tools
        .list(None, None, None, None, None, None, None)
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
        .list(Some(invalid_value), None, None, None, None, None, None)
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
            assert!(
                valid_values.contains("open"),
                "Error should mention valid status 'open'"
            );
            assert!(
                valid_values.contains("closed"),
                "Error should mention valid status 'closed'"
            );
            assert!(
                valid_values.contains("in_progress"),
                "Error should mention valid status 'in_progress'"
            );
            assert!(
                valid_values.contains("blocked"),
                "Error should mention valid status 'blocked'"
            );
        }
        e => panic!("Expected InvalidArgument error, got: {e:?}"),
    }
}

/// Test invalid `issue_type` values return appropriate errors.
#[rstest]
#[case::invalid_type("invalid", "issue_type")]
#[case::story_type("story", "issue_type")]
#[case::spike_type("spike", "issue_type")]
#[case::enhancement_type("enhancement", "issue_type")]
#[case::defect_type("defect", "issue_type")]
#[tokio::test]
async fn test_invalid_issue_type_values(#[case] invalid_value: &str, #[case] expected_field: &str) {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;

    // Test in list filter
    let list_result = tools
        .list(None, None, Some(invalid_value), None, None, None, None)
        .await;

    assert!(
        list_result.is_err(),
        "Expected error for invalid issue_type in list: {invalid_value}"
    );
    match list_result.unwrap_err() {
        Error::InvalidArgument {
            field,
            value,
            valid_values,
        } => {
            assert_eq!(field, expected_field);
            assert_eq!(value, invalid_value);
            assert!(
                valid_values.contains("bug"),
                "Error should mention valid type 'bug'"
            );
            assert!(
                valid_values.contains("feature"),
                "Error should mention valid type 'feature'"
            );
            assert!(
                valid_values.contains("task"),
                "Error should mention valid type 'task'"
            );
            assert!(
                valid_values.contains("epic"),
                "Error should mention valid type 'epic'"
            );
            assert!(
                valid_values.contains("chore"),
                "Error should mention valid type 'chore'"
            );
        }
        e => panic!("Expected InvalidArgument error, got: {e:?}"),
    }

    // Test in create
    let create_result = tools
        .create(
            "Test".to_string(),
            None,
            None,
            Some(invalid_value),
            None,
            None,
            None,
            None,
            None,
        )
        .await;

    assert!(
        create_result.is_err(),
        "Expected error for invalid issue_type in create: {invalid_value}"
    );

    // Test in ready filter
    let ready_result = tools
        .ready(None, None, Some(invalid_value), None, None, None)
        .await;

    assert!(
        ready_result.is_err(),
        "Expected error for invalid issue_type in ready: {invalid_value}"
    );
}

/// Test invalid `dep_type` values return appropriate errors.
#[rstest]
#[case::invalid_dep("invalid", "dep_type")]
#[case::requires_dep("requires", "dep_type")]
#[case::depends_dep("depends", "dep_type")]
#[case::blocked_by_dep("blocked-by", "dep_type")]
#[case::subtask_dep("subtask", "dep_type")]
#[tokio::test]
async fn test_invalid_dep_type_values(#[case] invalid_value: &str, #[case] expected_field: &str) {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;

    // Create two issues to link
    let issue1 = create_issue(&tools, "Issue 1").await;
    let issue2 = create_issue(&tools, "Issue 2").await;

    let result = tools
        .dep(&issue1.id, &issue2.id, Some(invalid_value), None)
        .await;

    assert!(
        result.is_err(),
        "Expected error for invalid dep_type: {invalid_value}"
    );
    match result.unwrap_err() {
        Error::InvalidArgument {
            field,
            value,
            valid_values,
        } => {
            assert_eq!(field, expected_field);
            assert_eq!(value, invalid_value);
            assert!(
                valid_values.contains("blocks"),
                "Error should mention valid dep_type 'blocks'"
            );
            assert!(
                valid_values.contains("related"),
                "Error should mention valid dep_type 'related'"
            );
            assert!(
                valid_values.contains("parent-child"),
                "Error should mention valid dep_type 'parent-child'"
            );
            assert!(
                valid_values.contains("discovered-from"),
                "Error should mention valid dep_type 'discovered-from'"
            );
        }
        e => panic!("Expected InvalidArgument error, got: {e:?}"),
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
        .update(
            &issue.id,
            None,
            None,
            Some(invalid_value),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
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
        .list(Some("bogus_status"), None, None, None, None, None, None)
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
async fn test_complete_issue_lifecycle_all_states() {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;

    // Create issue (starts as open)
    let created = tools
        .create(
            "Lifecycle Issue".to_string(),
            Some("Testing full lifecycle".to_string()),
            Some(2),
            Some("feature"),
            Some("developer".to_string()),
            Some(vec!["lifecycle-test".to_string()]),
            Some("Design notes here".to_string()),
            Some("- [ ] Criteria 1".to_string()),
            None,
        )
        .await
        .expect("create should succeed");

    assert_eq!(created.status, "open");
    assert!(created.closed_at.is_none());

    // Transition to in_progress
    let in_progress = tools
        .update(
            &created.id,
            None,
            None,
            Some("in_progress"),
            None,
            None,
            None,
            None,
            Some("Started working on this".to_string()),
            None,
            None,
        )
        .await
        .expect("update to in_progress should succeed");

    assert_eq!(in_progress.status, "in_progress");
    assert_eq!(
        in_progress.notes,
        Some("Started working on this".to_string())
    );

    // Transition to blocked
    let blocked = tools
        .update(
            &created.id,
            None,
            None,
            Some("blocked"),
            None,
            None,
            None,
            None,
            Some("Waiting for dependency".to_string()),
            None,
            None,
        )
        .await
        .expect("update to blocked should succeed");

    assert_eq!(blocked.status, "blocked");

    // Verify it appears in blocked list
    let _blocked_issues = tools.blocked(None).await.expect("blocked should succeed");
    // Note: Issue is blocked by status, not by dependency, so it may or may not appear
    // in blocked_issues depending on implementation

    // Back to in_progress
    let resumed = tools
        .update(
            &created.id,
            None,
            None,
            Some("in_progress"),
            None,
            None,
            None,
            None,
            Some("Resumed work".to_string()),
            None,
            None,
        )
        .await
        .expect("update back to in_progress should succeed");

    assert_eq!(resumed.status, "in_progress");

    // Close the issue
    let closed = tools
        .close(
            &created.id,
            Some("Completed successfully".to_string()),
            None,
        )
        .await
        .expect("close should succeed");

    assert_eq!(closed.status, "closed");
    assert!(closed.closed_at.is_some());

    // Verify final state via show
    let final_state = tools
        .show(&created.id, None)
        .await
        .expect("show should succeed");

    assert_eq!(final_state.status, "closed");
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
        .create(
            "Original Title".to_string(),
            Some("Original Description".to_string()),
            Some(1),
            Some("bug"),
            Some("alice".to_string()),
            Some(vec!["label1".to_string(), "label2".to_string()]),
            Some("Original Design".to_string()),
            Some("Original Criteria".to_string()),
            None,
        )
        .await
        .expect("create should succeed");

    // Update only the title
    let updated = tools
        .update(
            &created.id,
            Some("New Title".to_string()),
            None, // Don't update description
            None, // Don't update status
            None, // Don't update priority
            None, // Don't update assignee
            None, // Don't update design
            None, // Don't update acceptance
            None, // Don't update notes
            None, // Don't update external_ref
            None,
        )
        .await
        .expect("update should succeed");

    // Verify title changed
    assert_eq!(updated.title, "New Title");

    // Verify all other fields preserved
    assert_eq!(updated.description, "Original Description");
    assert_eq!(updated.status, "open");
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
                .list(None, None, None, None, None, None, None)
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
        .list(None, None, None, None, None, None, None)
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
        .list(Some("bogus"), None, None, None, None, None, None)
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
        .create(
            "Integration Test Issue".to_string(),
            Some("Full integration test".to_string()),
            Some(1),
            Some("task"),
            Some("tester".to_string()),
            Some(vec!["integration".to_string()]),
            Some("Design".to_string()),
            Some("Criteria".to_string()),
            None,
        )
        .await
        .expect("create should succeed");
    assert!(!created.id.is_empty());

    // 4. show
    let shown = tools
        .show(&created.id, None)
        .await
        .expect("show should succeed");
    assert_eq!(shown.id, created.id);
    assert_eq!(shown.title, "Integration Test Issue");

    // 5. list
    let listed = tools
        .list(None, None, None, None, None, None, None)
        .await
        .expect("list should succeed");
    assert!(!listed.is_empty());

    // 6. ready
    let ready = tools
        .ready(None, None, None, None, None, None)
        .await
        .expect("ready should succeed");
    assert!(!ready.is_empty());

    // 7. update
    let updated = tools
        .update(
            &created.id,
            Some("Updated Title".to_string()),
            None,
            Some("in_progress"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .expect("update should succeed");
    assert_eq!(updated.title, "Updated Title");
    assert_eq!(updated.status, "in_progress");

    // 8. Create another issue for dependency testing
    let blocker = tools
        .create(
            "Blocker Issue".to_string(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .expect("create blocker should succeed");

    // 9. dep
    let dep_result = tools
        .dep(&created.id, &blocker.id, Some("blocks"), None)
        .await
        .expect("dep should succeed");
    assert!(dep_result.contains("Added dependency"));

    // 10. blocked
    let blocked_issues = tools.blocked(None).await.expect("blocked should succeed");
    assert!(!blocked_issues.is_empty());
    assert!(blocked_issues.iter().any(|b| b.issue.id == created.id));

    // 11. close
    let closed = tools
        .close(&created.id, Some("Test completed".to_string()), None)
        .await
        .expect("close should succeed");
    assert_eq!(closed.status, "closed");
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
        .dep(&issue_b.id, &issue_c.id, Some("blocks"), None)
        .await
        .expect("B->C dep should succeed");

    // A depends on B
    tools
        .dep(&issue_a.id, &issue_b.id, Some("blocks"), None)
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
        .ready(None, None, None, None, None, None)
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
async fn test_closing_blocker_unblocks_dependent() {
    let workspace = create_temp_workspace();
    let tools = create_tools();
    set_context(&tools, workspace.path()).await;

    // Create blocker and dependent
    let blocker = create_issue(&tools, "Blocker").await;
    let dependent = create_issue(&tools, "Dependent").await;

    // Add dependency
    tools
        .dep(&dependent.id, &blocker.id, Some("blocks"), None)
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
        .close(&blocker.id, Some("Done".to_string()), None)
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
        .ready(None, None, None, None, None, None)
        .await
        .expect("ready should succeed");
    assert!(
        ready.iter().any(|i| i.id == dependent.id),
        "Dependent should be ready after blocker is closed"
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
    let issue2 = create_issue(&tools, "In Progress Issue").await;
    let issue3 = create_issue(&tools, "To Close").await;

    tools
        .update(
            &issue2.id,
            None,
            None,
            Some("in_progress"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();

    tools
        .close(&issue3.id, Some("Done".to_string()), None)
        .await
        .unwrap();

    // Verify counts
    let all = tools
        .list(None, None, None, None, None, None, None)
        .await
        .unwrap();
    assert_eq!(all.len(), 3, "Should have 3 total issues");

    let open = tools
        .list(Some("open"), None, None, None, None, None, None)
        .await
        .unwrap();
    assert_eq!(open.len(), 1, "Should have 1 open issue");
    assert_eq!(open[0].id, issue1.id);

    let in_progress = tools
        .list(Some("in_progress"), None, None, None, None, None, None)
        .await
        .unwrap();
    assert_eq!(in_progress.len(), 1, "Should have 1 in_progress issue");
    assert_eq!(in_progress[0].id, issue2.id);

    let closed = tools
        .list(Some("closed"), None, None, None, None, None, None)
        .await
        .unwrap();
    assert_eq!(closed.len(), 1, "Should have 1 closed issue");
    assert_eq!(closed[0].id, issue3.id);
}
