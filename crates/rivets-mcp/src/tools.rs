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
    BlockedIssueResponse, CreateParams, ListParams, ReadyParams, ResourceUpdateParams,
    SetContextResponse, UpdateParams, WhereAmIResponse,
};
use rivets::domain::{
    AssociatedResource, DependencyType, Issue, IssueFilter, IssueId, IssueKind, IssueStatus,
    IssueUpdate, NewIssue, NewResource, NoteContent, ResourceId, ResourceLabel, ResourceRole,
    ResourceTarget, ResourceUpdate, WebUrl, WorkspacePath,
};
use rivets::storage::IssueStorage;
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
    status.parse().map_err(|_| Error::InvalidArgument {
        field: "status",
        value: status.to_string(),
        valid_values: IssueStatus::valid_values(),
    })
}

/// Parse and validate a Resource Role string.
fn validate_resource_role(role: &str) -> Result<ResourceRole> {
    role.parse().map_err(|_| Error::InvalidArgument {
        field: "role",
        value: role.to_string(),
        valid_values: ResourceRole::valid_values(),
    })
}

/// Parse at most one Resource Target argument into the domain type.
///
/// The four-arm match is the single canonical url/path classification for
/// this crate; `parse_resource_target` layers the "exactly one" requirement
/// on top of it.
fn parse_optional_resource_target(
    url: Option<String>,
    path: Option<String>,
) -> Result<Option<ResourceTarget>> {
    match (url, path) {
        (Some(url), None) => Ok(Some(ResourceTarget::web(WebUrl::new(url)?))),
        (None, Some(path)) => Ok(Some(ResourceTarget::path(WorkspacePath::new(path)?))),
        (None, None) => Ok(None),
        (Some(_), Some(_)) => Err(Error::InvalidArgument {
            field: "target",
            value: "url and path both provided".to_string(),
            valid_values: "at most one of url or path",
        }),
    }
}

/// Parse exactly one Resource Target argument into the domain type.
fn parse_resource_target(url: Option<String>, path: Option<String>) -> Result<ResourceTarget> {
    parse_optional_resource_target(url, path)?.ok_or(Error::InvalidArgument {
        field: "target",
        value: "neither url nor path provided".to_string(),
        valid_values: "exactly one of url or path",
    })
}

/// Parse and validate a dependency type string.
fn validate_dep_type(dep_type: &str) -> Result<DependencyType> {
    dep_type.parse().map_err(|_| Error::InvalidArgument {
        field: "dep_type",
        value: dep_type.to_string(),
        valid_values: DependencyType::valid_values(),
    })
}

async fn save_or_reload(storage: &mut dyn IssueStorage) -> Result<()> {
    if let Err(error) = storage.save().await {
        if let Err(reload_error) = storage.reload().await {
            tracing::error!(error = %reload_error, "Failed to reload after save error");
        }
        return Err(error.into());
    }
    Ok(())
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

    /// Resolve cached storage under a shared lock, escalating only for first use.
    async fn storage_for(
        &self,
        workspace_root: Option<&str>,
    ) -> Result<Arc<RwLock<Box<dyn IssueStorage>>>> {
        let workspace_path = workspace_root.map(Path::new);
        {
            let context = self.context.read().await;
            match context.storage_for(workspace_path) {
                Ok(storage) => return Ok(storage),
                Err(Error::WorkspaceNotInitialized(_)) => {}
                Err(error) => return Err(error),
            }
        }

        let mut context = self.context.write().await;
        context.storage_for_or_init(workspace_path).await
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

                // Try to load the config to get the issue prefix
                let config_path = workspace.join(".rivets").join("config.yaml");
                let issue_prefix = if config_path.exists() {
                    match rivets::commands::init::RivetsConfig::load(&config_path).await {
                        Ok(config) => Some(config.issue_prefix),
                        Err(e) => {
                            debug!("Failed to load config for issue_prefix: {}", e);
                            None
                        }
                    }
                } else {
                    None
                };

                Ok(WhereAmIResponse {
                    workspace_root: Some(workspace.display().to_string()),
                    database_path: db_path.map(|p| p.display().to_string()),
                    context_set: true,
                    issue_prefix,
                })
            }
            None => Ok(WhereAmIResponse {
                workspace_root: None,
                database_path: None,
                context_set: false,
                issue_prefix: None,
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
    #[instrument(skip(self, params), fields(limit = params.limit, priority = params.priority))]
    pub async fn ready(&self, params: ReadyParams) -> Result<Vec<Issue>> {
        debug!("Finding ready issues");
        let issue_kind = params.kind.resolve("ready");

        // Release context lock before acquiring storage lock to prevent deadlocks
        let storage = self.storage_for(params.workspace_root.as_deref()).await?;
        let storage = storage.read().await;

        let filter = IssueFilter {
            priority: params.priority,
            issue_kind,
            assignee: params.assignee,
            label: params.label,
            limit: Some(params.limit.unwrap_or(DEFAULT_QUERY_LIMIT)),
            ..Default::default()
        };

        let issues = storage.ready_to_work(Some(&filter), None).await?;
        debug!(count = issues.len(), "Found ready issues");
        Ok(issues)
    }

    /// List issues with optional filters.
    ///
    /// If no limit is specified, defaults to [`DEFAULT_QUERY_LIMIT`] (100) to prevent
    /// potential OOM errors with large issue databases.
    ///
    /// # Errors
    ///
    /// Returns an error if no context is set, status is invalid, or storage operations fail.
    #[instrument(skip(self, params), fields(limit = params.limit, priority = params.priority))]
    pub async fn list(&self, params: ListParams) -> Result<Vec<Issue>> {
        debug!("Listing issues");
        let status = params.status.as_deref().map(validate_status).transpose()?;
        let issue_kind = params.kind.resolve("list");

        let storage = self.storage_for(params.workspace_root.as_deref()).await?;
        let storage = storage.read().await;

        let filter = IssueFilter {
            status,
            priority: params.priority,
            issue_kind,
            assignee: params.assignee,
            label: params.label,
            limit: Some(params.limit.unwrap_or(DEFAULT_QUERY_LIMIT)),
        };

        let issues = storage.list(&filter).await?;
        debug!(count = issues.len(), "Listed issues");
        Ok(issues)
    }

    /// Show details for a specific issue.
    ///
    /// # Errors
    ///
    /// Returns an error if no context is set, issue not found, or storage operations fail.
    #[instrument(skip(self), fields(%issue_id))]
    pub async fn show(&self, issue_id: &str, workspace_root: Option<&str>) -> Result<Issue> {
        let storage = self.storage_for(workspace_root).await?;
        let storage = storage.read().await;

        let id = IssueId::new(issue_id);
        let issue = storage
            .get(&id)
            .await?
            .ok_or_else(|| Error::IssueNotFound(issue_id.to_string()))?;
        Ok(issue)
    }

    /// Get blocked issues.
    ///
    /// # Errors
    ///
    /// Returns an error if no context is set or storage operations fail.
    #[instrument(skip(self))]
    pub async fn blocked(&self, workspace_root: Option<&str>) -> Result<Vec<BlockedIssueResponse>> {
        let storage = self.storage_for(workspace_root).await?;
        let storage = storage.read().await;

        let blocked = storage.blocked_issues().await?;
        Ok(blocked
            .into_iter()
            .map(|(issue, blockers)| BlockedIssueResponse { issue, blockers })
            .collect())
    }

    /// Create a new issue.
    ///
    /// # Errors
    ///
    /// Returns an error if no context is set or storage operations fail.
    #[instrument(skip(self, params), fields(title = %params.title))]
    pub async fn create(&self, params: CreateParams) -> Result<Issue> {
        debug!("Creating issue");
        let issue_kind = params.kind.resolve("create").unwrap_or(IssueKind::Task);
        let initial_note = params.initial_note.map(NoteContent::new).transpose()?;

        let storage = self.storage_for(params.workspace_root.as_deref()).await?;
        let mut storage = storage.write().await;

        let new_issue = NewIssue {
            title: params.title,
            description: params.description.unwrap_or_default(),
            priority: params.priority.unwrap_or(2),
            issue_kind,
            assignee: params.assignee,
            labels: params.labels.unwrap_or_default(),
            design: params.design,
            acceptance_criteria: params.acceptance,
            initial_note,
            dependencies: vec![],
        };

        let issue = storage.create(new_issue).await?;
        save_or_reload(storage.as_mut()).await?;
        debug!(issue_id = %issue.id, "Created issue");
        Ok(issue)
    }

    /// Update an existing issue.
    /// # Errors
    ///
    /// Returns an error if no context is set, status is invalid, the issue is missing, or storage fails.
    #[instrument(skip(self, params), fields(issue_id = %params.issue_id))]
    pub async fn update(&self, params: UpdateParams) -> Result<Issue> {
        debug!("Updating issue");
        let status = params.status.as_deref().map(validate_status).transpose()?;
        let issue_kind = params.kind.resolve("update");
        let assignee = params
            .assignee
            .map(|value| if value.is_empty() { None } else { Some(value) });

        let storage = self.storage_for(params.workspace_root.as_deref()).await?;
        let mut storage = storage.write().await;

        let id = IssueId::new(&params.issue_id);
        let updates = IssueUpdate {
            title: params.title,
            description: params.description,
            status,
            priority: params.priority,
            issue_kind,
            assignee,
            design: params.design,
            acceptance_criteria: params.acceptance_criteria,
            note: None,
            labels: params.labels,
        };

        let issue = storage.update(&id, updates).await?;
        save_or_reload(storage.as_mut()).await?;
        debug!("Updated issue");
        Ok(issue)
    }

    /// Append an immutable Note to an Issue.
    ///
    /// # Errors
    ///
    /// Returns an error if the Note is empty, no context is set, the Issue is
    /// not found, or persistence fails.
    #[instrument(skip(self, content), fields(%issue_id))]
    pub async fn add_note(
        &self,
        issue_id: &str,
        content: String,
        workspace_root: Option<&str>,
    ) -> Result<Issue> {
        let note = NoteContent::new(content)?;
        let storage = self.storage_for(workspace_root).await?;
        let mut storage = storage.write().await;

        let issue = storage
            .update(
                &IssueId::new(issue_id),
                IssueUpdate {
                    note: Some(note),
                    ..Default::default()
                },
            )
            .await?;
        save_or_reload(storage.as_mut()).await?;
        Ok(issue)
    }

    /// Associate a Web URL or Workspace Path target with an Issue.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid URL, path, role, or label, a missing
    /// Issue, a context failure, a duplicate target-and-role association, or
    /// a persistence failure. Exactly one of `url`/`path` is required.
    #[instrument(skip(self, url, path, label), fields(%issue_id, %role))]
    pub async fn resource_add(
        &self,
        issue_id: &str,
        url: Option<String>,
        path: Option<String>,
        role: &str,
        label: Option<String>,
        workspace_root: Option<&str>,
    ) -> Result<Issue> {
        let target = parse_resource_target(url, path)?;
        let resource = NewResource {
            target,
            role: validate_resource_role(role)?,
            label: label.map(ResourceLabel::new).transpose()?,
        };
        let storage = self.storage_for(workspace_root).await?;
        let mut storage = storage.write().await;

        let issue = storage
            .add_resource(&IssueId::new(issue_id), resource)
            .await?;
        save_or_reload(storage.as_mut()).await?;
        Ok(issue)
    }

    /// Update an Issue's Associated Resource by its stable identifier.
    ///
    /// Only the provided fields change; the resource keeps its identifier and
    /// position.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid URL, path, role, or label, a missing
    /// Issue or resource identifier, a duplicate post-update target-and-role
    /// association, an update with no fields, or a persistence failure.
    #[instrument(skip(self, params), fields(%params.issue_id, %params.resource_id))]
    pub async fn resource_update(&self, params: ResourceUpdateParams) -> Result<Issue> {
        let ResourceUpdateParams {
            issue_id,
            resource_id,
            url,
            path,
            role,
            label,
            clear_label,
            workspace_root,
        } = params;
        let target = parse_optional_resource_target(url, path)?;
        let label = match (label, clear_label) {
            (Some(label), false) => Some(Some(ResourceLabel::new(label)?)),
            (None, true) => Some(None),
            (None, false) => None,
            (Some(_), true) => {
                return Err(Error::InvalidArgument {
                    field: "label",
                    value: "label and clear_label both provided".to_string(),
                    valid_values: "at most one of label or clear_label",
                });
            }
        };
        let update = ResourceUpdate {
            target,
            role: role.as_deref().map(validate_resource_role).transpose()?,
            label,
        };
        let storage = self.storage_for(workspace_root.as_deref()).await?;
        let mut storage = storage.write().await;

        let issue = storage
            .update_resource(
                &IssueId::new(issue_id),
                &ResourceId::new(resource_id)?,
                update,
            )
            .await?;
        save_or_reload(storage.as_mut()).await?;
        Ok(issue)
    }

    /// Remove an Issue's Associated Resource by its stable identifier.
    ///
    /// The remaining resources keep their identifiers and positions.
    ///
    /// # Errors
    ///
    /// Returns an error when the context, Issue, or resource identifier is
    /// missing, or storage fails.
    #[instrument(skip(self), fields(%issue_id, %resource_id))]
    pub async fn resource_remove(
        &self,
        issue_id: &str,
        resource_id: &str,
        workspace_root: Option<&str>,
    ) -> Result<Issue> {
        let storage = self.storage_for(workspace_root).await?;
        let mut storage = storage.write().await;

        let issue = storage
            .remove_resource(&IssueId::new(issue_id), &ResourceId::new(resource_id)?)
            .await?;
        save_or_reload(storage.as_mut()).await?;
        Ok(issue)
    }

    /// List an Issue's Associated Resources in insertion order.
    ///
    /// # Errors
    ///
    /// Returns an error when the context or Issue is missing, or storage fails.
    #[instrument(skip(self), fields(%issue_id))]
    pub async fn resource_list(
        &self,
        issue_id: &str,
        workspace_root: Option<&str>,
    ) -> Result<Vec<AssociatedResource>> {
        let storage = self.storage_for(workspace_root).await?;
        let storage = storage.read().await;
        let issue = storage
            .get(&IssueId::new(issue_id))
            .await?
            .ok_or_else(|| Error::IssueNotFound(issue_id.to_string()))?;
        Ok(issue.resources().to_vec())
    }

    /// Close an issue.
    ///
    /// # Errors
    ///
    /// Returns an error if the reason is invalid, no context is set, the Issue
    /// is not found, or storage operations fail.
    #[instrument(skip(self, reason), fields(%issue_id))]
    pub async fn close(
        &self,
        issue_id: &str,
        reason: Option<String>,
        workspace_root: Option<&str>,
    ) -> Result<Issue> {
        debug!("Closing issue");
        let note = reason.map(NoteContent::closing_reason).transpose()?;
        let storage = self.storage_for(workspace_root).await?;
        let mut storage = storage.write().await;

        let id = IssueId::new(issue_id);
        let updates = IssueUpdate {
            status: Some(rivets::domain::IssueStatus::Closed),
            note,
            ..Default::default()
        };

        let issue = storage.update(&id, updates).await?;
        save_or_reload(storage.as_mut()).await?;
        debug!("Closed issue");
        Ok(issue)
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

        let storage = self.storage_for(workspace_root).await?;
        let mut storage = storage.write().await;

        let from = IssueId::new(issue_id);
        let to = IssueId::new(depends_on_id);

        storage.add_dependency(&from, &to, dep_type).await?;
        save_or_reload(storage.as_mut()).await?;

        let dep_type_str = dep_type.to_string();
        debug!(dep_type = %dep_type_str, "Added dependency");
        Ok(format!(
            "Added dependency: {issue_id} depends on {depends_on_id} ({dep_type_str})"
        ))
    }

    /// Reopen a closed issue.
    ///
    /// # Errors
    ///
    /// Returns an error if the reason is invalid, no context is set, the Issue
    /// is not found, or storage operations fail.
    #[instrument(skip(self, reason), fields(%issue_id))]
    pub async fn reopen(
        &self,
        issue_id: &str,
        reason: Option<String>,
        workspace_root: Option<&str>,
    ) -> Result<Issue> {
        debug!("Reopening issue");
        let note = reason.map(NoteContent::reopening_reason).transpose()?;
        let storage = self.storage_for(workspace_root).await?;
        let mut storage = storage.write().await;

        let id = IssueId::new(issue_id);
        let updates = IssueUpdate {
            status: Some(IssueStatus::Open),
            note,
            ..Default::default()
        };

        let issue = storage.update(&id, updates).await?;
        save_or_reload(storage.as_mut()).await?;
        debug!("Reopened issue");
        Ok(issue)
    }

    /// Find stale issues that haven't been updated recently.
    ///
    /// # Performance Note
    ///
    /// This method loads all issues matching the optional status filter into memory,
    /// then filters by `updated_at` timestamp. For very large issue databases (10,000+),
    /// consider adding a storage-level query method that filters at the database layer.
    ///
    /// # Errors
    ///
    /// Returns an error if no context is set or storage operations fail.
    #[instrument(skip(self), fields(days, ?status, limit))]
    pub async fn stale(
        &self,
        days: Option<u32>,
        status: Option<&str>,
        limit: Option<usize>,
        workspace_root: Option<&str>,
    ) -> Result<Vec<Issue>> {
        debug!("Finding stale issues");
        let status = status.map(validate_status).transpose()?;
        let days = days.unwrap_or(30);
        let limit = limit.unwrap_or(DEFAULT_QUERY_LIMIT);

        let storage = self.storage_for(workspace_root).await?;
        let storage = storage.read().await;

        // Get all issues with optional status filter
        let filter = IssueFilter {
            status,
            ..Default::default()
        };

        let cutoff = chrono::Utc::now() - chrono::Duration::days(i64::from(days));
        let issues = storage.list(&filter).await?;

        // Filter by updated_at timestamp and apply limit
        let stale_issues: Vec<Issue> = issues
            .into_iter()
            .filter(|issue| issue.updated_at < cutoff)
            .take(limit)
            .collect();

        debug!(count = stale_issues.len(), "Found stale issues");
        Ok(stale_issues)
    }

    /// Add a label to an issue.
    ///
    /// # Errors
    ///
    /// Returns an error if no context is set, issue not found, or storage operations fail.
    #[instrument(skip(self), fields(%issue_id, %label))]
    pub async fn label_add(
        &self,
        issue_id: &str,
        label: &str,
        workspace_root: Option<&str>,
    ) -> Result<Issue> {
        debug!("Adding label to issue");
        let storage = self.storage_for(workspace_root).await?;
        let mut storage = storage.write().await;

        let id = IssueId::new(issue_id);
        let issue = storage.add_label(&id, label).await?;
        save_or_reload(storage.as_mut()).await?;
        debug!("Added label");
        Ok(issue)
    }

    /// Remove a label from an issue.
    ///
    /// # Errors
    ///
    /// Returns an error if no context is set, issue not found, or storage operations fail.
    #[instrument(skip(self), fields(%issue_id, %label))]
    pub async fn label_remove(
        &self,
        issue_id: &str,
        label: &str,
        workspace_root: Option<&str>,
    ) -> Result<Issue> {
        debug!("Removing label from issue");
        let storage = self.storage_for(workspace_root).await?;
        let mut storage = storage.write().await;

        let id = IssueId::new(issue_id);
        let issue = storage.remove_label(&id, label).await?;
        save_or_reload(storage.as_mut()).await?;
        debug!("Removed label");
        Ok(issue)
    }

    /// List labels for a specific issue.
    ///
    /// # Errors
    ///
    /// Returns an error if no context is set, issue not found, or storage operations fail.
    #[instrument(skip(self), fields(%issue_id))]
    pub async fn label_list(
        &self,
        issue_id: &str,
        workspace_root: Option<&str>,
    ) -> Result<Vec<String>> {
        debug!("Listing labels for issue");
        let storage = self.storage_for(workspace_root).await?;
        let storage = storage.read().await;

        let id = IssueId::new(issue_id);
        let issue = storage
            .get(&id)
            .await?
            .ok_or_else(|| Error::IssueNotFound(issue_id.to_string()))?;

        debug!(count = issue.labels.len(), "Found labels");
        Ok(issue.labels)
    }

    /// List all unique labels across all issues.
    ///
    /// # Errors
    ///
    /// Returns an error if no context is set or storage operations fail.
    #[instrument(skip(self))]
    pub async fn label_list_all(&self, workspace_root: Option<&str>) -> Result<Vec<String>> {
        debug!("Listing all labels");
        let storage = self.storage_for(workspace_root).await?;
        let storage = storage.read().await;

        let issues = storage.list(&IssueFilter::default()).await?;

        // Collect unique labels
        let mut labels: Vec<String> = issues.into_iter().flat_map(|issue| issue.labels).collect();
        labels.sort();
        labels.dedup();

        debug!(count = labels.len(), "Found unique labels");
        Ok(labels)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivets::storage::in_memory::new_in_memory_storage;
    use rstest::{fixture, rstest};
    use std::path::PathBuf;

    fn kind_input(value: Option<&str>) -> crate::models::IssueKindInput {
        // Parse through the domain FromStr so tests exercise the real
        // vocabulary instead of a parallel arm table.
        let issue_kind = value.map(|value| value.parse().expect("valid test Issue Kind"));
        crate::models::IssueKindInput::canonical(issue_kind)
    }

    #[rstest]
    #[case::open("open", IssueStatus::Open)]
    #[case::in_progress("in_progress", IssueStatus::InProgress)]
    #[case::blocked("blocked", IssueStatus::Blocked)]
    #[case::closed("closed", IssueStatus::Closed)]
    fn validate_status_accepts_canonical(#[case] input: &str, #[case] expected: IssueStatus) {
        assert_eq!(validate_status(input).expect("canonical status"), expected);
    }

    #[rstest]
    #[case::uppercase("OPEN")]
    #[case::cli_alias("in-progress")]
    #[case::unknown("bogus")]
    #[case::empty("")]
    fn validate_status_rejects_lenient(#[case] lenient: &str) {
        // The former lenient spellings (case-folded, in-progress alias) are
        // rejected with the existing InvalidArgument shape.
        let error = validate_status(lenient).expect_err("lenient status rejected");
        match error {
            Error::InvalidArgument {
                field,
                value,
                valid_values,
            } => {
                assert_eq!(field, "status");
                assert_eq!(value, lenient);
                assert_eq!(valid_values, "open, in_progress, blocked, closed");
            }
            other => panic!("expected InvalidArgument, got: {other:?}"),
        }
    }

    #[rstest]
    #[case::blocks("blocks", DependencyType::Blocks)]
    #[case::related("related", DependencyType::Related)]
    #[case::parent_child("parent-child", DependencyType::ParentChild)]
    #[case::discovered_from("discovered-from", DependencyType::DiscoveredFrom)]
    fn validate_dep_type_accepts_canonical(#[case] input: &str, #[case] expected: DependencyType) {
        assert_eq!(
            validate_dep_type(input).expect("canonical dep type"),
            expected
        );
    }

    #[rstest]
    #[case::uppercase("BLOCKS")]
    #[case::underscore_parent("parent_child")]
    #[case::underscore_discovered("discovered_from")]
    #[case::unknown("bogus")]
    #[case::empty("")]
    fn validate_dep_type_rejects_lenient(#[case] lenient: &str) {
        // The former lenient spellings (case-folded, underscore forms) are
        // rejected with the existing InvalidArgument shape.
        let error = validate_dep_type(lenient).expect_err("lenient dep type rejected");
        match error {
            Error::InvalidArgument {
                field,
                value,
                valid_values,
            } => {
                assert_eq!(field, "dep_type");
                assert_eq!(value, lenient);
                assert_eq!(
                    valid_values,
                    "blocks, related, parent-child, discovered-from"
                );
            }
            other => panic!("expected InvalidArgument, got: {other:?}"),
        }
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

    #[allow(clippy::too_many_arguments)]
    fn update_params(
        issue_id: &str,
        title: Option<String>,
        description: Option<String>,
        status: Option<&str>,
        priority: Option<u8>,
        issue_kind: Option<&str>,
        assignee: Option<String>,
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
            assignee,
            title,
            description,
            design,
            acceptance_criteria,
            labels,
            workspace_root: workspace_root.map(str::to_string),
        }
    }

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
    async fn create_issue(tools: &Tools, title: &str) -> Issue {
        tools
            .create(create_params(
                title.to_string(),
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
            .unwrap()
    }

    #[rstest]
    #[tokio::test]
    async fn cached_storage_lookup_allows_concurrent_context_readers(#[future] tools: Tools) {
        let tools = tools.await;
        let context_reader = tools.context.read().await;

        let result = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            tools.list(list_params(None, None, None, None, None, None, None)),
        )
        .await;

        drop(context_reader);
        let issues = result
            .expect("cached lookup should not wait for other context readers")
            .expect("list should use cached storage");
        assert!(issues.is_empty());
    }

    #[rstest]
    #[tokio::test]
    async fn test_create_and_show_issue(#[future] tools: Tools) {
        let tools = tools.await;

        let issue = tools
            .create(create_params(
                "Test Issue".to_string(),
                Some("Test description".to_string()),
                Some(1),
                Some("task"),
                Some("alice".to_string()),
                Some(vec!["label1".to_string()]),
                None,
                None,
                None,
            ))
            .await
            .unwrap();

        assert_eq!(issue.title, "Test Issue");
        assert_eq!(issue.description, "Test description");
        assert_eq!(issue.priority, 1);
        assert_eq!(issue.issue_kind, IssueKind::Task);
        assert_eq!(issue.assignee, Some("alice".to_string()));

        // Show the issue
        let shown = tools.show(issue.id.as_str(), None).await.unwrap();
        assert_eq!(shown.title, "Test Issue");
    }

    #[rstest]
    #[tokio::test]
    async fn test_list_issues(#[future] tools: Tools) {
        let tools = tools.await;

        create_issue(&tools, "Issue 1").await;
        create_issue(&tools, "Issue 2").await;

        let issues = tools
            .list(list_params(None, None, None, None, None, None, None))
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
            .update(update_params(
                issue.id.as_str(),
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
            ))
            .await
            .unwrap();

        assert_eq!(updated.title, "Updated Title");
        assert_eq!(updated.status, IssueStatus::InProgress);
        assert_eq!(updated.priority, 0);
    }

    #[rstest]
    #[tokio::test]
    async fn test_close_issue(#[future] tools: Tools) {
        let tools = tools.await;

        let issue = create_issue(&tools, "To Close").await;

        let closed = tools
            .close(issue.id.as_str(), Some("Completed".to_string()), None)
            .await
            .unwrap();

        assert_eq!(closed.status, IssueStatus::Closed);
    }

    #[rstest]
    #[tokio::test]
    async fn test_ready_to_work(#[future] tools: Tools) {
        let tools = tools.await;

        create_issue(&tools, "Ready Issue").await;

        let ready = tools
            .ready(ready_params(None, None, None, None, None, None))
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
            .dep(issue1.id.as_str(), issue2.id.as_str(), Some("blocks"), None)
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
                dependent_issue.id.as_str(),
                blocking_issue.id.as_str(),
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
        // issue_prefix is None for test workspaces without config file
        assert!(info.issue_prefix.is_none());
    }

    #[tokio::test]
    async fn test_where_am_i_without_context() {
        let context = Arc::new(RwLock::new(Context::new()));
        let tools = Tools::new(context);

        let info = tools.where_am_i().await.unwrap();
        assert!(!info.context_set);
        assert!(info.workspace_root.is_none());
        assert!(info.issue_prefix.is_none());
    }

    #[tokio::test]
    async fn test_no_context_error() {
        let context = Arc::new(RwLock::new(Context::new()));
        let tools = Tools::new(context);

        let result = tools
            .list(list_params(None, None, None, None, None, None, None))
            .await;
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
            .list(list_params(None, None, None, None, None, Some(2), None))
            .await
            .unwrap();
        assert_eq!(issues.len(), 2, "list should respect explicit limit");

        // Ready with limit of 3
        let ready = tools
            .ready(ready_params(Some(3), None, None, None, None, None))
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
                    let _ = tools
                        .list(list_params(None, None, None, None, None, None, None))
                        .await;
                    let _ = tools
                        .ready(ready_params(None, None, None, None, None, None))
                        .await;
                }
            }));
        }

        // Writers
        for i in 0..3 {
            let tools = Arc::clone(&tools);
            handles.push(tokio::spawn(async move {
                for j in 0..5 {
                    let _ = tools
                        .create(create_params(
                            format!("Concurrent Issue {i}-{j}"),
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                        ))
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

    #[rstest]
    #[tokio::test]
    async fn test_reopen_issue(#[future] tools: Tools) {
        let tools = tools.await;

        // Create and close an issue
        let issue = create_issue(&tools, "To Reopen").await;
        let closed = tools
            .close(issue.id.as_str(), Some("Completed".to_string()), None)
            .await
            .unwrap();
        assert_eq!(closed.status, IssueStatus::Closed);

        // Reopen the issue
        let reopened = tools
            .reopen(issue.id.as_str(), Some("Work not done".to_string()), None)
            .await
            .unwrap();
        assert_eq!(reopened.status, IssueStatus::Open);
    }

    #[rstest]
    #[tokio::test]
    async fn test_label_add_and_list(#[future] tools: Tools) {
        let tools = tools.await;

        let issue = create_issue(&tools, "Labeled Issue").await;

        // Add a label
        let updated = tools
            .label_add(issue.id.as_str(), "feature", None)
            .await
            .unwrap();
        assert!(updated.labels.contains(&"feature".to_string()));

        // List labels
        let labels = tools.label_list(issue.id.as_str(), None).await.unwrap();
        assert!(labels.contains(&"feature".to_string()));
    }

    #[rstest]
    #[tokio::test]
    async fn test_label_remove(#[future] tools: Tools) {
        let tools = tools.await;

        // Create issue with label
        let issue = tools
            .create(create_params(
                "Issue with Label".to_string(),
                None,
                None,
                None,
                None,
                Some(vec!["bug".to_string()]),
                None,
                None,
                None,
            ))
            .await
            .unwrap();
        assert!(issue.labels.contains(&"bug".to_string()));

        // Remove the label
        let updated = tools
            .label_remove(issue.id.as_str(), "bug", None)
            .await
            .unwrap();
        assert!(!updated.labels.contains(&"bug".to_string()));
    }

    #[rstest]
    #[tokio::test]
    async fn test_label_list_all(#[future] tools: Tools) {
        let tools = tools.await;

        // Create issues with different labels
        tools
            .create(create_params(
                "Issue 1".to_string(),
                None,
                None,
                None,
                None,
                Some(vec!["feature".to_string(), "backend".to_string()]),
                None,
                None,
                None,
            ))
            .await
            .unwrap();

        tools
            .create(create_params(
                "Issue 2".to_string(),
                None,
                None,
                None,
                None,
                Some(vec!["feature".to_string(), "frontend".to_string()]),
                None,
                None,
                None,
            ))
            .await
            .unwrap();

        // List all labels (should be deduplicated and sorted)
        let labels = tools.label_list_all(None).await.unwrap();
        assert!(labels.contains(&"feature".to_string()));
        assert!(labels.contains(&"backend".to_string()));
        assert!(labels.contains(&"frontend".to_string()));
        // Feature should only appear once (deduplicated)
        assert_eq!(labels.iter().filter(|&l| l == "feature").count(), 1);
        // Verify labels are sorted alphabetically
        let mut sorted = labels.clone();
        sorted.sort();
        assert_eq!(labels, sorted, "Labels should be sorted alphabetically");
    }

    #[rstest]
    #[tokio::test]
    async fn test_stale_issues(#[future] tools: Tools) {
        let tools = tools.await;

        // Create an issue (will have current timestamp)
        create_issue(&tools, "Fresh Issue").await;

        // Finding stale issues from the last 30 days should return empty
        // (issue was just created, so it's not stale)
        let stale = tools.stale(Some(30), None, None, None).await.unwrap();
        assert_eq!(stale.len(), 0, "Newly created issue should not be stale");

        // Finding stale issues from 0 days should return the issue
        // (0 days means anything older than right now)
        let stale = tools.stale(Some(0), None, None, None).await.unwrap();
        assert_eq!(
            stale.len(),
            1,
            "Issue should be stale with 0 days threshold"
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_stale_with_status_filter(#[future] tools: Tools) {
        let tools = tools.await;

        // Create an open issue
        let open_issue = create_issue(&tools, "Open Issue").await;

        // Create and close another issue
        let closed_issue = create_issue(&tools, "Closed Issue").await;
        tools
            .close(closed_issue.id.as_str(), Some("Done".to_string()), None)
            .await
            .unwrap();

        // Find stale open issues with 0-day threshold
        let stale_open = tools
            .stale(Some(0), Some("open"), None, None)
            .await
            .unwrap();
        assert_eq!(stale_open.len(), 1);
        assert_eq!(stale_open[0].id, open_issue.id);

        // Find stale closed issues with 0-day threshold
        let stale_closed = tools
            .stale(Some(0), Some("closed"), None, None)
            .await
            .unwrap();
        assert_eq!(stale_closed.len(), 1);
        assert_eq!(stale_closed[0].id, closed_issue.id);
    }
}
