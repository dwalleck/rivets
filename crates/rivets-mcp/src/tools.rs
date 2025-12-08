//! MCP tool implementations.
//!
//! This module contains the implementations for all MCP tools exposed by the server.
//!
//! # Workspace Parameter Design
//!
//! Most tool methods accept an optional `workspace_root` parameter. This enables:
//!
//! - **Multi-workspace support**: AI assistants can work with multiple projects
//!   in a single session without repeatedly calling `set_context`
//! - **MCP protocol compatibility**: Each tool call can specify its target workspace,
//!   matching how MCP tools receive parameters from the client
//! - **Fallback behavior**: If `workspace_root` is `None`, the current context
//!   (set via `set_context`) is used
//!
//! This design mirrors the beads MCP server's approach for compatibility.

use crate::context::Context;
use crate::error::{Error, Result};
use crate::models::{
    dep_type_to_str, parse_dep_type, parse_issue_type, parse_status, BlockedIssueResponse,
    McpIssue, SetContextResponse, WhereAmIResponse,
};
use rivets::domain::{
    DependencyType, IssueFilter, IssueId, IssueStatus, IssueType, IssueUpdate, NewIssue,
};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, instrument};

/// Default limit for list/ready queries when none is specified.
///
/// Prevents potential OOM errors with large issue databases by ensuring
/// queries always have a reasonable upper bound.
const DEFAULT_QUERY_LIMIT: usize = 100;

/// Parse and validate a status string.
fn validate_status(status: &str) -> Result<IssueStatus> {
    parse_status(status).ok_or_else(|| Error::InvalidArgument {
        field: "status",
        value: status.to_string(),
        valid_values: "open, in_progress, blocked, closed",
    })
}

/// Parse and validate an issue type string.
fn validate_issue_type(issue_type: &str) -> Result<IssueType> {
    parse_issue_type(issue_type).ok_or_else(|| Error::InvalidArgument {
        field: "issue_type",
        value: issue_type.to_string(),
        valid_values: "bug, feature, task, epic, chore",
    })
}

/// Parse and validate a dependency type string.
fn validate_dep_type(dep_type: &str) -> Result<DependencyType> {
    parse_dep_type(dep_type).ok_or_else(|| Error::InvalidArgument {
        field: "dep_type",
        value: dep_type.to_string(),
        valid_values: "blocks, related, parent-child, discovered-from",
    })
}

/// Tool implementations for the rivets MCP server.
pub struct Tools {
    context: Arc<RwLock<Context>>,
}

impl Tools {
    /// Create a new Tools instance with the given context.
    #[must_use]
    pub fn new(context: Arc<RwLock<Context>>) -> Self {
        Self { context }
    }

    /// Set the workspace context.
    ///
    /// # Errors
    ///
    /// Returns an error if the workspace path is invalid or has no `.rivets/` directory.
    #[instrument(skip(self), fields(workspace = %workspace_root))]
    pub async fn set_context(&self, workspace_root: &str) -> Result<SetContextResponse> {
        debug!("Setting workspace context");
        let path = Path::new(workspace_root);
        let mut context = self.context.write().await;
        let info = context.set_workspace(path).await?;

        debug!(db_path = %info.database_path.display(), "Context set successfully");
        Ok(SetContextResponse {
            workspace_root: info.workspace_root.display().to_string(),
            database_path: info.database_path.display().to_string(),
            message: "Context set successfully".to_string(),
        })
    }

    /// Get current workspace information.
    ///
    /// # Errors
    ///
    /// This function does not currently return errors but returns `Result` for API consistency.
    pub async fn where_am_i(&self) -> Result<WhereAmIResponse> {
        let context = self.context.read().await;

        match context.current_workspace() {
            Some(workspace) => {
                let db_path = context.current_database_path();

                Ok(WhereAmIResponse {
                    workspace_root: Some(workspace.display().to_string()),
                    database_path: db_path.map(|p| p.display().to_string()),
                    context_set: true,
                })
            }
            None => Ok(WhereAmIResponse {
                workspace_root: None,
                database_path: None,
                context_set: false,
            }),
        }
    }

    /// Get issues ready to work on.
    ///
    /// If no limit is specified, defaults to [`DEFAULT_QUERY_LIMIT`] (100) to prevent
    /// potential OOM errors with large issue databases.
    ///
    /// # Errors
    ///
    /// Returns an error if no context is set or storage operations fail.
    #[instrument(skip(self, assignee, label), fields(limit, priority))]
    pub async fn ready(
        &self,
        limit: Option<usize>,
        priority: Option<u8>,
        issue_type: Option<&str>,
        assignee: Option<String>,
        label: Option<String>,
        workspace_root: Option<&str>,
    ) -> Result<Vec<McpIssue>> {
        debug!("Finding ready issues");
        // Validate enum values before acquiring locks
        let issue_type = issue_type.map(validate_issue_type).transpose()?;

        // Release context lock before acquiring storage lock to prevent deadlocks
        let storage = {
            let context = self.context.read().await;
            context.storage_for(workspace_root.map(Path::new))?
        };
        let storage = storage.read().await;

        let filter = IssueFilter {
            priority,
            issue_type,
            assignee,
            label,
            limit: Some(limit.unwrap_or(DEFAULT_QUERY_LIMIT)),
            ..Default::default()
        };

        let issues = storage.ready_to_work(Some(&filter), None).await?;
        debug!(count = issues.len(), "Found ready issues");
        Ok(issues.into_iter().map(Into::into).collect())
    }

    /// List issues with optional filters.
    ///
    /// If no limit is specified, defaults to [`DEFAULT_QUERY_LIMIT`] (100) to prevent
    /// potential OOM errors with large issue databases.
    ///
    /// # Errors
    ///
    /// Returns an error if no context is set, invalid filter values, or storage operations fail.
    #[allow(clippy::too_many_arguments)]
    #[instrument(skip(self, assignee, label), fields(limit, priority))]
    pub async fn list(
        &self,
        status: Option<&str>,
        priority: Option<u8>,
        issue_type: Option<&str>,
        assignee: Option<String>,
        label: Option<String>,
        limit: Option<usize>,
        workspace_root: Option<&str>,
    ) -> Result<Vec<McpIssue>> {
        debug!("Listing issues");
        // Validate enum values before acquiring locks
        let status = status.map(validate_status).transpose()?;
        let issue_type = issue_type.map(validate_issue_type).transpose()?;

        let storage = {
            let context = self.context.read().await;
            context.storage_for(workspace_root.map(Path::new))?
        };
        let storage = storage.read().await;

        let filter = IssueFilter {
            status,
            priority,
            issue_type,
            assignee,
            label,
            limit: Some(limit.unwrap_or(DEFAULT_QUERY_LIMIT)),
        };

        let issues = storage.list(&filter).await?;
        debug!(count = issues.len(), "Listed issues");
        Ok(issues.into_iter().map(Into::into).collect())
    }

    /// Show details for a specific issue.
    ///
    /// # Errors
    ///
    /// Returns an error if no context is set, issue not found, or storage operations fail.
    pub async fn show(&self, issue_id: &str, workspace_root: Option<&str>) -> Result<McpIssue> {
        let storage = {
            let context = self.context.read().await;
            context.storage_for(workspace_root.map(Path::new))?
        };
        let storage = storage.read().await;

        let id = IssueId::new(issue_id);
        let issue = storage
            .get(&id)
            .await?
            .ok_or_else(|| Error::IssueNotFound(issue_id.to_string()))?;
        Ok(issue.into())
    }

    /// Get blocked issues.
    ///
    /// # Errors
    ///
    /// Returns an error if no context is set or storage operations fail.
    pub async fn blocked(&self, workspace_root: Option<&str>) -> Result<Vec<BlockedIssueResponse>> {
        let storage = {
            let context = self.context.read().await;
            context.storage_for(workspace_root.map(Path::new))?
        };
        let storage = storage.read().await;

        let blocked = storage.blocked_issues().await?;
        Ok(blocked
            .into_iter()
            .map(|(issue, blockers)| BlockedIssueResponse {
                issue: issue.into(),
                blockers: blockers.into_iter().map(Into::into).collect(),
            })
            .collect())
    }

    /// Create a new issue.
    ///
    /// # Errors
    ///
    /// Returns an error if no context is set, invalid `issue_type`, or storage operations fail.
    #[allow(clippy::too_many_arguments)]
    #[instrument(skip(self, description, labels, design, acceptance_criteria), fields(%title))]
    pub async fn create(
        &self,
        title: String,
        description: Option<String>,
        priority: Option<u8>,
        issue_type: Option<&str>,
        assignee: Option<String>,
        labels: Option<Vec<String>>,
        design: Option<String>,
        acceptance_criteria: Option<String>,
        workspace_root: Option<&str>,
    ) -> Result<McpIssue> {
        debug!("Creating issue");
        // Validate issue_type before acquiring locks
        let issue_type = issue_type
            .map(validate_issue_type)
            .transpose()?
            .unwrap_or(IssueType::Task);

        let storage = {
            let context = self.context.read().await;
            context.storage_for(workspace_root.map(Path::new))?
        };
        let mut storage = storage.write().await;

        let new_issue = NewIssue {
            title,
            description: description.unwrap_or_default(),
            priority: priority.unwrap_or(2),
            issue_type,
            assignee,
            labels: labels.unwrap_or_default(),
            design,
            acceptance_criteria,
            notes: None,
            external_ref: None,
            dependencies: vec![],
        };

        let issue = storage.create(new_issue).await?;
        storage.save().await?;
        debug!(issue_id = %issue.id, "Created issue");
        Ok(issue.into())
    }

    /// Update an existing issue.
    ///
    /// # Errors
    ///
    /// Returns an error if no context is set, invalid status, issue not found, or storage fails.
    #[allow(clippy::too_many_arguments)]
    #[instrument(skip(self, title, description, design, acceptance_criteria, notes, external_ref), fields(%issue_id))]
    pub async fn update(
        &self,
        issue_id: &str,
        title: Option<String>,
        description: Option<String>,
        status: Option<&str>,
        priority: Option<u8>,
        assignee: Option<Option<String>>,
        design: Option<String>,
        acceptance_criteria: Option<String>,
        notes: Option<String>,
        external_ref: Option<String>,
        workspace_root: Option<&str>,
    ) -> Result<McpIssue> {
        debug!("Updating issue");
        // Validate status before acquiring locks
        let status = status.map(validate_status).transpose()?;

        let storage = {
            let context = self.context.read().await;
            context.storage_for(workspace_root.map(Path::new))?
        };
        let mut storage = storage.write().await;

        let id = IssueId::new(issue_id);
        let updates = IssueUpdate {
            title,
            description,
            status,
            priority,
            assignee,
            design,
            acceptance_criteria,
            notes,
            external_ref,
        };

        let issue = storage.update(&id, updates).await?;
        storage.save().await?;
        debug!("Updated issue");
        Ok(issue.into())
    }

    /// Close an issue.
    ///
    /// # Errors
    ///
    /// Returns an error if no context is set, issue not found, or storage operations fail.
    #[instrument(skip(self, reason), fields(%issue_id))]
    pub async fn close(
        &self,
        issue_id: &str,
        reason: Option<String>,
        workspace_root: Option<&str>,
    ) -> Result<McpIssue> {
        debug!("Closing issue");
        let storage = {
            let context = self.context.read().await;
            context.storage_for(workspace_root.map(Path::new))?
        };
        let mut storage = storage.write().await;

        let id = IssueId::new(issue_id);
        let updates = IssueUpdate {
            status: Some(rivets::domain::IssueStatus::Closed),
            notes: reason,
            ..Default::default()
        };

        let issue = storage.update(&id, updates).await?;
        storage.save().await?;
        debug!("Closed issue");
        Ok(issue.into())
    }

    /// Add a dependency between issues.
    ///
    /// # Errors
    ///
    /// Returns an error if no context is set, invalid `dep_type`, issues not found, cycle detected,
    /// or storage fails.
    #[instrument(skip(self), fields(%issue_id, %depends_on_id))]
    pub async fn dep(
        &self,
        issue_id: &str,
        depends_on_id: &str,
        dep_type: Option<&str>,
        workspace_root: Option<&str>,
    ) -> Result<String> {
        debug!("Adding dependency");
        // Validate dep_type before acquiring locks
        let dep_type = dep_type
            .map(validate_dep_type)
            .transpose()?
            .unwrap_or(DependencyType::Blocks);

        let storage = {
            let context = self.context.read().await;
            context.storage_for(workspace_root.map(Path::new))?
        };
        let mut storage = storage.write().await;

        let from = IssueId::new(issue_id);
        let to = IssueId::new(depends_on_id);

        storage.add_dependency(&from, &to, dep_type).await?;
        storage.save().await?;

        debug!(dep_type = %dep_type_to_str(dep_type), "Added dependency");
        Ok(format!(
            "Added dependency: {issue_id} depends on {depends_on_id} ({})",
            dep_type_to_str(dep_type)
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivets::storage::in_memory::new_in_memory_storage;
    use rstest::{fixture, rstest};
    use std::path::PathBuf;

    /// Async fixture that creates Tools with in-memory storage.
    #[fixture]
    async fn tools() -> Tools {
        let context = Arc::new(RwLock::new(Context::new()));
        let tools = Tools::new(context);

        // Set up test workspace with in-memory storage
        let storage = new_in_memory_storage("test".to_string());
        let mut ctx = tools.context.write().await;
        ctx.set_test_workspace(PathBuf::from("/test/workspace"), storage);
        drop(ctx);

        tools
    }

    /// Helper to create a simple issue with just a title.
    async fn create_issue(tools: &Tools, title: &str) -> McpIssue {
        tools
            .create(
                title.to_string(),
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
            .unwrap()
    }

    #[rstest]
    #[tokio::test]
    async fn test_create_and_show_issue(#[future] tools: Tools) {
        let tools = tools.await;

        let issue = tools
            .create(
                "Test Issue".to_string(),
                Some("Test description".to_string()),
                Some(1),
                Some("task"),
                Some("alice".to_string()),
                Some(vec!["label1".to_string()]),
                None,
                None,
                None,
            )
            .await
            .unwrap();

        assert_eq!(issue.title, "Test Issue");
        assert_eq!(issue.description, "Test description");
        assert_eq!(issue.priority, 1);
        assert_eq!(issue.issue_type, "task");
        assert_eq!(issue.assignee, Some("alice".to_string()));

        // Show the issue
        let shown = tools.show(&issue.id, None).await.unwrap();
        assert_eq!(shown.title, "Test Issue");
    }

    #[rstest]
    #[tokio::test]
    async fn test_list_issues(#[future] tools: Tools) {
        let tools = tools.await;

        create_issue(&tools, "Issue 1").await;
        create_issue(&tools, "Issue 2").await;

        let issues = tools
            .list(None, None, None, None, None, None, None)
            .await
            .unwrap();
        assert_eq!(issues.len(), 2);
    }

    #[rstest]
    #[tokio::test]
    async fn test_update_issue(#[future] tools: Tools) {
        let tools = tools.await;

        let issue = create_issue(&tools, "Original Title").await;

        let updated = tools
            .update(
                &issue.id,
                Some("Updated Title".to_string()),
                None,
                Some("in_progress"),
                Some(0),
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();

        assert_eq!(updated.title, "Updated Title");
        assert_eq!(updated.status, "in_progress");
        assert_eq!(updated.priority, 0);
    }

    #[rstest]
    #[tokio::test]
    async fn test_close_issue(#[future] tools: Tools) {
        let tools = tools.await;

        let issue = create_issue(&tools, "To Close").await;

        let closed = tools
            .close(&issue.id, Some("Completed".to_string()), None)
            .await
            .unwrap();

        assert_eq!(closed.status, "closed");
    }

    #[rstest]
    #[tokio::test]
    async fn test_ready_to_work(#[future] tools: Tools) {
        let tools = tools.await;

        create_issue(&tools, "Ready Issue").await;

        let ready = tools
            .ready(None, None, None, None, None, None)
            .await
            .unwrap();
        assert!(!ready.is_empty());
    }

    #[rstest]
    #[tokio::test]
    async fn test_add_dependency(#[future] tools: Tools) {
        let tools = tools.await;

        let issue1 = create_issue(&tools, "Issue 1").await;
        let issue2 = create_issue(&tools, "Issue 2").await;

        // Add dependency
        let result = tools
            .dep(&issue1.id, &issue2.id, Some("blocks"), None)
            .await
            .unwrap();

        assert!(result.contains("Added dependency"));
        assert!(result.contains("blocks"));
    }

    #[rstest]
    #[tokio::test]
    async fn test_blocked_issues(#[future] tools: Tools) {
        let tools = tools.await;

        // Create two issues: one blocks the other
        let blocking_issue = create_issue(&tools, "Blocking Issue").await;
        let dependent_issue = create_issue(&tools, "Dependent Issue").await;

        tools
            .dep(
                &dependent_issue.id,
                &blocking_issue.id,
                Some("blocks"),
                None,
            )
            .await
            .unwrap();

        // Get blocked issues
        let result = tools.blocked(None).await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].issue.id, dependent_issue.id);
    }

    #[rstest]
    #[tokio::test]
    async fn test_where_am_i_with_context(#[future] tools: Tools) {
        let tools = tools.await;

        let info = tools.where_am_i().await.unwrap();
        assert!(info.context_set);
        assert_eq!(info.workspace_root, Some("/test/workspace".to_string()));
    }

    #[tokio::test]
    async fn test_where_am_i_without_context() {
        let context = Arc::new(RwLock::new(Context::new()));
        let tools = Tools::new(context);

        let info = tools.where_am_i().await.unwrap();
        assert!(!info.context_set);
        assert!(info.workspace_root.is_none());
    }

    #[tokio::test]
    async fn test_no_context_error() {
        let context = Arc::new(RwLock::new(Context::new()));
        let tools = Tools::new(context);

        let result = tools.list(None, None, None, None, None, None, None).await;
        assert!(result.is_err());
    }

    /// Test that explicit limits are respected by list and ready.
    #[rstest]
    #[tokio::test]
    async fn test_explicit_limit_is_respected(#[future] tools: Tools) {
        let tools = tools.await;

        // Create 5 issues
        for i in 0..5 {
            create_issue(&tools, &format!("Issue {i}")).await;
        }

        // List with limit of 2
        let issues = tools
            .list(None, None, None, None, None, Some(2), None)
            .await
            .unwrap();
        assert_eq!(issues.len(), 2, "list should respect explicit limit");

        // Ready with limit of 3
        let ready = tools
            .ready(Some(3), None, None, None, None, None)
            .await
            .unwrap();
        assert_eq!(ready.len(), 3, "ready should respect explicit limit");
    }

    /// Test concurrent access to Tools methods.
    ///
    /// This test exercises the lock ordering fix - if context lock was held
    /// while acquiring storage lock, concurrent operations could deadlock.
    /// The timeout ensures the test fails rather than hanging forever.
    #[rstest]
    #[tokio::test]
    async fn test_concurrent_access(#[future] tools: Tools) {
        use std::time::Duration;

        let tools = Arc::new(tools.await);

        // Spawn multiple concurrent operations
        let mut handles = vec![];

        // Readers
        for _ in 0..5 {
            let tools = Arc::clone(&tools);
            handles.push(tokio::spawn(async move {
                for _ in 0..10 {
                    let _ = tools.list(None, None, None, None, None, None, None).await;
                    let _ = tools.ready(None, None, None, None, None, None).await;
                }
            }));
        }

        // Writers
        for i in 0..3 {
            let tools = Arc::clone(&tools);
            handles.push(tokio::spawn(async move {
                for j in 0..5 {
                    let _ = tools
                        .create(
                            format!("Concurrent Issue {i}-{j}"),
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                        )
                        .await;
                }
            }));
        }

        // Wait with timeout - if deadlock, this will fail
        let result = tokio::time::timeout(Duration::from_secs(5), async {
            for handle in handles {
                handle.await.unwrap();
            }
        })
        .await;

        assert!(
            result.is_ok(),
            "Concurrent operations timed out - possible deadlock"
        );
    }
}
