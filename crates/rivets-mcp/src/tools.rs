//! MCP tool implementations.
//!
//! This module contains the implementations for all MCP tools exposed by the server.

use crate::context::Context;
use crate::error::Result;
use crate::models::{
    parse_dep_type, parse_issue_type, parse_status, BlockedIssueResponse, McpIssue,
    SetContextResponse, WhereAmIResponse,
};
use rivets::domain::{IssueFilter, IssueId, IssueType, IssueUpdate, NewIssue};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Tool implementations for the rivets MCP server.
pub struct Tools {
    context: Arc<RwLock<Context>>,
}

impl Tools {
    /// Create a new Tools instance with the given context.
    pub fn new(context: Arc<RwLock<Context>>) -> Self {
        Self { context }
    }

    /// Set the workspace context.
    ///
    /// # Errors
    ///
    /// Returns an error if the workspace path is invalid or has no `.rivets/` directory.
    pub async fn set_context(&self, workspace_root: &str) -> Result<SetContextResponse> {
        let path = Path::new(workspace_root);
        let mut context = self.context.write().await;
        let info = context.set_workspace(path).await?;

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
    /// # Errors
    ///
    /// Returns an error if no context is set or storage operations fail.
    pub async fn ready(
        &self,
        limit: Option<usize>,
        priority: Option<u8>,
        assignee: Option<String>,
        workspace_root: Option<&str>,
    ) -> Result<Vec<McpIssue>> {
        let context = self.context.read().await;
        let storage = context.storage_for(workspace_root.map(Path::new))?;
        let storage = storage.read().await;

        let filter = IssueFilter {
            priority,
            assignee,
            limit,
            ..Default::default()
        };

        let issues = storage.ready_to_work(Some(&filter), None).await?;
        Ok(issues.into_iter().map(Into::into).collect())
    }

    /// List issues with optional filters.
    ///
    /// # Errors
    ///
    /// Returns an error if no context is set or storage operations fail.
    pub async fn list(
        &self,
        status: Option<&str>,
        priority: Option<u8>,
        issue_type: Option<&str>,
        assignee: Option<String>,
        limit: Option<usize>,
        workspace_root: Option<&str>,
    ) -> Result<Vec<McpIssue>> {
        let context = self.context.read().await;
        let storage = context.storage_for(workspace_root.map(Path::new))?;
        let storage = storage.read().await;

        let filter = IssueFilter {
            status: status.and_then(parse_status),
            priority,
            issue_type: issue_type.and_then(parse_issue_type),
            assignee,
            limit,
            ..Default::default()
        };

        let issues = storage.list(&filter).await?;
        Ok(issues.into_iter().map(Into::into).collect())
    }

    /// Show details for a specific issue.
    ///
    /// # Errors
    ///
    /// Returns an error if no context is set or storage operations fail.
    pub async fn show(
        &self,
        issue_id: &str,
        workspace_root: Option<&str>,
    ) -> Result<Option<McpIssue>> {
        let context = self.context.read().await;
        let storage = context.storage_for(workspace_root.map(Path::new))?;
        let storage = storage.read().await;

        let id = IssueId::new(issue_id);
        let issue = storage.get(&id).await?;
        Ok(issue.map(Into::into))
    }

    /// Get blocked issues.
    ///
    /// # Errors
    ///
    /// Returns an error if no context is set or storage operations fail.
    pub async fn blocked(&self, workspace_root: Option<&str>) -> Result<Vec<BlockedIssueResponse>> {
        let context = self.context.read().await;
        let storage = context.storage_for(workspace_root.map(Path::new))?;
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
    /// Returns an error if no context is set, validation fails, or storage operations fail.
    #[allow(clippy::too_many_arguments)]
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
        let context = self.context.read().await;
        let storage = context.storage_for(workspace_root.map(Path::new))?;
        let mut storage = storage.write().await;

        let new_issue = NewIssue {
            title,
            description: description.unwrap_or_default(),
            priority: priority.unwrap_or(2),
            issue_type: issue_type
                .and_then(parse_issue_type)
                .unwrap_or(IssueType::Task),
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
        Ok(issue.into())
    }

    /// Update an existing issue.
    ///
    /// # Errors
    ///
    /// Returns an error if no context is set, issue not found, or storage operations fail.
    #[allow(clippy::too_many_arguments)]
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
        workspace_root: Option<&str>,
    ) -> Result<McpIssue> {
        let context = self.context.read().await;
        let storage = context.storage_for(workspace_root.map(Path::new))?;
        let mut storage = storage.write().await;

        let id = IssueId::new(issue_id);
        let updates = IssueUpdate {
            title,
            description,
            status: status.and_then(parse_status),
            priority,
            assignee,
            design,
            acceptance_criteria,
            notes,
            external_ref: None,
        };

        let issue = storage.update(&id, updates).await?;
        storage.save().await?;
        Ok(issue.into())
    }

    /// Close an issue.
    ///
    /// # Errors
    ///
    /// Returns an error if no context is set, issue not found, or storage operations fail.
    pub async fn close(
        &self,
        issue_id: &str,
        reason: Option<String>,
        workspace_root: Option<&str>,
    ) -> Result<McpIssue> {
        let context = self.context.read().await;
        let storage = context.storage_for(workspace_root.map(Path::new))?;
        let mut storage = storage.write().await;

        let id = IssueId::new(issue_id);
        let updates = IssueUpdate {
            status: Some(rivets::domain::IssueStatus::Closed),
            notes: reason,
            ..Default::default()
        };

        let issue = storage.update(&id, updates).await?;
        storage.save().await?;
        Ok(issue.into())
    }

    /// Add a dependency between issues.
    ///
    /// # Errors
    ///
    /// Returns an error if no context is set, issues not found, cycle detected, or storage fails.
    pub async fn dep(
        &self,
        issue_id: &str,
        depends_on_id: &str,
        dep_type: Option<&str>,
        workspace_root: Option<&str>,
    ) -> Result<String> {
        let context = self.context.read().await;
        let storage = context.storage_for(workspace_root.map(Path::new))?;
        let mut storage = storage.write().await;

        let from = IssueId::new(issue_id);
        let to = IssueId::new(depends_on_id);
        let dep_type = dep_type
            .and_then(parse_dep_type)
            .unwrap_or(rivets::domain::DependencyType::Blocks);

        storage.add_dependency(&from, &to, dep_type).await?;
        storage.save().await?;

        Ok(format!(
            "Added dependency: {issue_id} depends on {depends_on_id} ({dep_type:?})"
        ))
    }
}
