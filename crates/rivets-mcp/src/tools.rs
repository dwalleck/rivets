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
    BlockedIssueResponse, BlockingDependencyListQuery, BlockingDependencyTreeEntry,
    BlockingDependencyTreeResponse, CreateParams, ListParams, ReadyParams, ResourceUpdateParams,
    SetContextResponse, UpdateParams, WhereAmIResponse,
};
use rivets::domain::{
    AssignmentError, AssociatedResource, BlockingDependency, DiscoveryOrigin, Issue, IssueFilter,
    IssueId, IssueKind, IssueStatus, IssueUpdate, Label, NewIssue, NewResource, NoteContent,
    Parentage, ReadyAssignmentFilter, ReadyFilter, RelatedAssociation, ResourceId, ResourceLabel,
    ResourceRole, ResourceTarget, ResourceUpdate, WebUrl, WorkspacePath,
};
use rivets::storage::IssueStorage;
use rivets::workspace_lock::WorkspaceMutationLock;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::{OwnedRwLockWriteGuard, RwLock};
use tracing::{debug, instrument};

/// Default limit for list/ready queries when none is specified.
///
/// Prevents potential OOM errors with large issue databases by ensuring
/// queries always have a reasonable upper bound.
const DEFAULT_QUERY_LIMIT: usize = 100;

#[derive(Clone, Copy)]
enum AssignmentOperation {
    Claim,
    Release,
}

/// Parse an untrusted Issue ID before any storage lookup or mutation.
fn parse_issue_id(input: &str) -> Result<IssueId> {
    Ok(input.parse()?)
}

/// Parse an untrusted Issue Label before any query or mutation.
fn parse_label(input: &str) -> Result<Label> {
    Ok(input.parse()?)
}

/// Parse a collection of untrusted Issue Labels without partial success.
fn parse_labels(inputs: Vec<String>) -> Result<Vec<Label>> {
    inputs
        .into_iter()
        .map(|input| Label::try_from(input).map_err(Error::from))
        .collect()
}

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

async fn save_or_reload(storage: &mut dyn IssueStorage) -> Result<()> {
    if let Err(error) = storage.save().await {
        if let Err(reload_error) = storage.reload().await {
            tracing::error!(error = %reload_error, "Failed to reload after save error");
        }
        return Err(error.into());
    }
    Ok(())
}

struct MutationStorage {
    storage: OwnedRwLockWriteGuard<Box<dyn IssueStorage>>,
    _workspace_lock: Option<WorkspaceMutationLock>,
}

impl MutationStorage {
    fn as_mut(&mut self) -> &mut dyn IssueStorage {
        self.storage.as_mut()
    }
}

impl std::ops::Deref for MutationStorage {
    type Target = dyn IssueStorage;

    fn deref(&self) -> &Self::Target {
        self.storage.as_ref()
    }
}

impl std::ops::DerefMut for MutationStorage {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.storage.as_mut()
    }
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
            match context.storage_for_async(workspace_path).await {
                Ok(storage) => return Ok(storage),
                Err(Error::WorkspaceNotInitialized(_)) => {}
                Err(error) => return Err(error),
            }
        }

        let mut context = self.context.write().await;
        context.storage_for_or_init(workspace_path).await
    }

    /// Resolve storage, serialize in-process mutations, then acquire the durable lock.
    async fn mutation_storage_for(&self, workspace_root: Option<&str>) -> Result<MutationStorage> {
        let workspace_path = workspace_root.map(Path::new);
        let storage = self.storage_for(workspace_root).await?;
        let lock_root = {
            let context = self.context.read().await;
            context.mutation_lock_root_async(workspace_path).await?
        };
        let storage = storage.write_owned().await;
        let workspace_lock = match lock_root {
            Some(root) => Some(WorkspaceMutationLock::try_acquire_async(root).await?),
            None => None,
        };
        Ok(MutationStorage {
            storage,
            _workspace_lock: workspace_lock,
        })
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
        let workspace = context.current_workspace().cloned();
        let db_path = context.current_database_path().cloned();
        drop(context);

        match workspace {
            Some(workspace) => {
                // Try to load the config to get the issue prefix. Use async
                // metadata so a slow filesystem cannot block the MCP runtime.
                let config_path = workspace.join(".rivets").join("config.yaml");
                let issue_prefix = match tokio::fs::metadata(&config_path).await {
                    Ok(_) => match rivets::commands::init::RivetsConfig::load(&config_path).await {
                        Ok(config) => Some(config.issue_prefix),
                        Err(error) => {
                            debug!(error = %error, "Failed to load config for issue_prefix");
                            None
                        }
                    },
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                    Err(error) => {
                        debug!(error = %error, "Failed to inspect config for issue_prefix");
                        None
                    }
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
    /// Returns an error if no context is set, both Assignment selectors are
    /// provided, or storage operations fail.
    #[instrument(skip(self, params), fields(limit = params.limit, priority = params.priority))]
    pub async fn ready(&self, params: ReadyParams) -> Result<Vec<Issue>> {
        debug!("Finding ready issues");
        let issue_kind = params.kind.resolve("ready");
        let assignment = match (params.assignee, params.all_assignees) {
            (Some(_), true) => {
                return Err(Error::InvalidArgument {
                    field: "assignment selector",
                    value: "assignee and all_assignees".to_string(),
                    valid_values: "unassigned, assignee, all_assignees",
                });
            }
            (Some(assignee), false) => ReadyAssignmentFilter::Assignee(assignee),
            (None, true) => ReadyAssignmentFilter::All,
            (None, false) => ReadyAssignmentFilter::Unassigned,
        };
        let label = params.label.map(Label::try_from).transpose()?;

        // Release context lock before acquiring storage lock to prevent deadlocks
        let storage = self.storage_for(params.workspace_root.as_deref()).await?;
        let storage = storage.read().await;
        let filter = ReadyFilter {
            priority: params.priority,
            issue_kind,
            assignment,
            label,
            limit: Some(params.limit.unwrap_or(DEFAULT_QUERY_LIMIT)),
        };

        let issues = storage.ready_to_work(&filter, None).await?;
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
        let label = params.label.map(Label::try_from).transpose()?;

        let storage = self.storage_for(params.workspace_root.as_deref()).await?;
        let storage = storage.read().await;

        let filter = IssueFilter {
            status,
            priority: params.priority,
            issue_kind,
            assignee: params.assignee,
            label,
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
        let id = parse_issue_id(issue_id)?;
        let storage = self.storage_for(workspace_root).await?;
        let storage = storage.read().await;

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
        let labels = parse_labels(params.labels.unwrap_or_default())?;

        let mut storage = self
            .mutation_storage_for(params.workspace_root.as_deref())
            .await?;

        let new_issue = NewIssue {
            title: params.title,
            description: params.description.unwrap_or_default(),
            priority: params.priority.unwrap_or(2),
            issue_kind,
            assignee: params.assignee,
            labels,
            design: params.design,
            acceptance_criteria: params.acceptance,
            initial_note,
            prerequisites: vec![],
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
        if params.contains_legacy_assignee() {
            return Err(Error::InvalidArgument {
                field: "assignee",
                value: "legacy assignee field".to_string(),
                valid_values: "use claim or release",
            });
        }
        if !params.has_updates() {
            return Err(Error::InvalidArgument {
                field: "updates",
                value: "no update fields provided".to_string(),
                valid_values: "at least one update field",
            });
        }

        let status = params.status.as_deref().map(validate_status).transpose()?;
        let issue_kind = params.kind.resolve("update");

        let labels = params.labels.map(parse_labels).transpose()?;
        let id = parse_issue_id(&params.issue_id)?;
        let mut storage = self
            .mutation_storage_for(params.workspace_root.as_deref())
            .await?;

        let updates = IssueUpdate {
            title: params.title,
            description: params.description,
            status,
            priority: params.priority,
            issue_kind,

            design: params.design,
            acceptance_criteria: params.acceptance_criteria,
            note: None,
            labels,
        };

        let issue = storage.update(&id, updates).await?;
        save_or_reload(storage.as_mut()).await?;
        debug!("Updated issue");
        Ok(issue)
    }

    async fn mutate_assignment(
        &self,
        issue_id: &str,
        assignee: &str,
        workspace_root: Option<&str>,
        operation: AssignmentOperation,
    ) -> Result<Issue> {
        let issue_id = parse_issue_id(issue_id)?;
        if assignee.trim().is_empty() {
            return Err(Error::Assignment(AssignmentError::BlankAssignee {
                issue_id,
            }));
        }

        let mut storage = self.mutation_storage_for(workspace_root).await?;
        let issue = match operation {
            AssignmentOperation::Claim => storage.claim(&issue_id, assignee).await?,
            AssignmentOperation::Release => storage.release(&issue_id, assignee).await?,
        };
        save_or_reload(storage.as_mut()).await?;
        Ok(issue)
    }

    /// Atomically claim one Open, unblocked Issue.
    ///
    /// # Errors
    ///
    /// Returns a typed Assignment error if the Issue is not claimable, or a
    /// Workspace error if context, locking, loading, or persistence fails.
    #[instrument(skip(self), fields(%issue_id, %assignee))]
    pub async fn claim(
        &self,
        issue_id: &str,
        assignee: &str,
        workspace_root: Option<&str>,
    ) -> Result<Issue> {
        self.mutate_assignment(
            issue_id,
            assignee,
            workspace_root,
            AssignmentOperation::Claim,
        )
        .await
    }

    /// Atomically release one Open Issue from its exact Assignee.
    ///
    /// # Errors
    ///
    /// Returns a typed Assignment error if the expected Assignee does not own
    /// the Issue, or a Workspace error if context, locking, loading, or
    /// persistence fails.
    #[instrument(skip(self), fields(%issue_id, %assignee))]
    pub async fn release(
        &self,
        issue_id: &str,
        assignee: &str,
        workspace_root: Option<&str>,
    ) -> Result<Issue> {
        self.mutate_assignment(
            issue_id,
            assignee,
            workspace_root,
            AssignmentOperation::Release,
        )
        .await
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
        let issue_id = parse_issue_id(issue_id)?;
        let note = NoteContent::new(content)?;
        let mut storage = self.mutation_storage_for(workspace_root).await?;

        let issue = storage
            .update(
                &issue_id,
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
        let issue_id = parse_issue_id(issue_id)?;
        let target = parse_resource_target(url, path)?;
        let resource = NewResource {
            target,
            role: validate_resource_role(role)?,
            label: label.map(ResourceLabel::new).transpose()?,
        };
        let mut storage = self.mutation_storage_for(workspace_root).await?;

        let issue = storage.add_resource(&issue_id, resource).await?;
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
        let issue_id = parse_issue_id(&issue_id)?;
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
        let mut storage = self.mutation_storage_for(workspace_root.as_deref()).await?;

        let issue = storage
            .update_resource(&issue_id, &ResourceId::new(resource_id)?, update)
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
        let issue_id = parse_issue_id(issue_id)?;
        let mut storage = self.mutation_storage_for(workspace_root).await?;

        let issue = storage
            .remove_resource(&issue_id, &ResourceId::new(resource_id)?)
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
        let issue_id = parse_issue_id(issue_id)?;
        let storage = self.storage_for(workspace_root).await?;
        let storage = storage.read().await;
        let issue = storage
            .get(&issue_id)
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
        let id = parse_issue_id(issue_id)?;
        let note = reason.map(NoteContent::closing_reason).transpose()?;
        let mut storage = self.mutation_storage_for(workspace_root).await?;

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

    /// Add one role-safe Blocking Dependency.
    ///
    /// # Errors
    ///
    /// Returns an error for self-reference, missing endpoints, duplicates,
    /// Blocking cycles, missing context, or persistence failure.
    #[instrument(skip(self), fields(%dependent_id, %prerequisite_id))]
    pub async fn blocking_dependency_add(
        &self,
        dependent_id: &str,
        prerequisite_id: &str,
        workspace_root: Option<&str>,
    ) -> Result<BlockingDependency> {
        let dependency = BlockingDependency::new(
            parse_issue_id(dependent_id)?,
            parse_issue_id(prerequisite_id)?,
        )?;
        let mut storage = self.mutation_storage_for(workspace_root).await?;
        storage.add_blocking_dependency(dependency.clone()).await?;
        save_or_reload(storage.as_mut()).await?;
        Ok(dependency)
    }

    /// Remove one role-safe Blocking Dependency.
    ///
    /// # Errors
    ///
    /// Returns an error for self-reference, missing endpoints, an absent
    /// Blocking Dependency, missing context, or persistence failure.
    #[instrument(skip(self), fields(%dependent_id, %prerequisite_id))]
    pub async fn blocking_dependency_remove(
        &self,
        dependent_id: &str,
        prerequisite_id: &str,
        workspace_root: Option<&str>,
    ) -> Result<BlockingDependency> {
        let dependency = BlockingDependency::new(
            parse_issue_id(dependent_id)?,
            parse_issue_id(prerequisite_id)?,
        )?;
        let mut storage = self.mutation_storage_for(workspace_root).await?;
        storage.remove_blocking_dependency(&dependency).await?;
        save_or_reload(storage.as_mut()).await?;
        Ok(dependency)
    }

    /// List Blocking Dependencies from one explicit endpoint perspective.
    ///
    /// # Errors
    ///
    /// Returns an error when the requested endpoint or Workspace is missing,
    /// or storage cannot be queried.
    pub async fn blocking_dependency_list(
        &self,
        query: &BlockingDependencyListQuery,
        workspace_root: Option<&str>,
    ) -> Result<Vec<BlockingDependency>> {
        let endpoint_id = match query {
            BlockingDependencyListQuery::PrerequisitesOf { dependent_id } => dependent_id,
            BlockingDependencyListQuery::DependentsOf { prerequisite_id } => prerequisite_id,
        };
        let endpoint_id = parse_issue_id(endpoint_id)?;
        let storage = self.storage_for(workspace_root).await?;
        let storage = storage.read().await;
        match query {
            BlockingDependencyListQuery::PrerequisitesOf { .. } => {
                Ok(storage.blocking_prerequisites(&endpoint_id).await?)
            }
            BlockingDependencyListQuery::DependentsOf { .. } => {
                Ok(storage.blocking_dependents(&endpoint_id).await?)
            }
        }
    }

    /// Return the role-named Blocking prerequisite tree.
    ///
    /// # Errors
    ///
    /// Returns an error when the root dependent or Workspace is missing, or
    /// storage cannot be queried.
    pub async fn blocking_dependency_tree(
        &self,
        dependent_id: &str,
        depth: Option<usize>,
        workspace_root: Option<&str>,
    ) -> Result<BlockingDependencyTreeResponse> {
        let dependent_id = parse_issue_id(dependent_id)?;
        let storage = self.storage_for(workspace_root).await?;
        let storage = storage.read().await;
        let max_depth = depth.filter(|depth| *depth != 0);
        let prerequisites = storage
            .blocking_dependency_tree(&dependent_id, max_depth)
            .await?
            .into_iter()
            .map(|(dependency, depth)| BlockingDependencyTreeEntry {
                dependent_id: dependency.dependent_id().to_string(),
                prerequisite_id: dependency.prerequisite_id().to_string(),
                depth,
            })
            .collect();
        Ok(BlockingDependencyTreeResponse {
            root_dependent_id: dependent_id.to_string(),
            prerequisites,
        })
    }

    /// Add one symmetric Related Association.
    ///
    /// # Errors
    ///
    /// Returns an error for self-reference, missing endpoints, missing context,
    /// or persistence failure.
    #[instrument(skip(self), fields(%issue_id, %related_issue_id))]
    pub async fn related_add(
        &self,
        issue_id: &str,
        related_issue_id: &str,
        workspace_root: Option<&str>,
    ) -> Result<RelatedAssociation> {
        let association =
            RelatedAssociation::new(parse_issue_id(issue_id)?, parse_issue_id(related_issue_id)?)?;
        let mut storage = self.mutation_storage_for(workspace_root).await?;
        storage.add_related_association(association.clone()).await?;
        save_or_reload(storage.as_mut()).await?;
        Ok(association)
    }

    /// Remove one symmetric Related Association.
    ///
    /// # Errors
    ///
    /// Returns an error for self-reference, missing endpoints, an absent
    /// association, missing context, or persistence failure.
    #[instrument(skip(self), fields(%issue_id, %related_issue_id))]
    pub async fn related_remove(
        &self,
        issue_id: &str,
        related_issue_id: &str,
        workspace_root: Option<&str>,
    ) -> Result<RelatedAssociation> {
        let association =
            RelatedAssociation::new(parse_issue_id(issue_id)?, parse_issue_id(related_issue_id)?)?;
        let mut storage = self.mutation_storage_for(workspace_root).await?;
        storage.remove_related_association(&association).await?;
        save_or_reload(storage.as_mut()).await?;
        Ok(association)
    }

    /// List every Related Association containing one Issue.
    ///
    /// # Errors
    ///
    /// Returns an error when the Issue or Workspace is missing, or storage
    /// cannot be queried.
    #[instrument(skip(self), fields(%issue_id))]
    pub async fn related_list(
        &self,
        issue_id: &str,
        workspace_root: Option<&str>,
    ) -> Result<Vec<RelatedAssociation>> {
        let issue_id = parse_issue_id(issue_id)?;
        let storage = self.storage_for(workspace_root).await?;
        let storage = storage.read().await;
        Ok(storage.related_associations(&issue_id).await?)
    }

    /// Add one directed Discovery Origin.
    ///
    /// # Errors
    ///
    /// Returns an error for self-reference, missing endpoints, a duplicate,
    /// a Discovery cycle, missing context, or persistence failure.
    #[instrument(skip(self), fields(%discovered_issue_id, %source_issue_id))]
    pub async fn discovery_add(
        &self,
        discovered_issue_id: &str,
        source_issue_id: &str,
        workspace_root: Option<&str>,
    ) -> Result<DiscoveryOrigin> {
        let origin = DiscoveryOrigin::new(
            parse_issue_id(discovered_issue_id)?,
            parse_issue_id(source_issue_id)?,
        )?;
        let mut storage = self.mutation_storage_for(workspace_root).await?;
        storage.add_discovery_origin(origin.clone()).await?;
        save_or_reload(storage.as_mut()).await?;
        Ok(origin)
    }

    /// Remove one directed Discovery Origin.
    ///
    /// # Errors
    ///
    /// Returns an error for self-reference, missing endpoints, an absent
    /// origin, missing context, or persistence failure.
    #[instrument(skip(self), fields(%discovered_issue_id, %source_issue_id))]
    pub async fn discovery_remove(
        &self,
        discovered_issue_id: &str,
        source_issue_id: &str,
        workspace_root: Option<&str>,
    ) -> Result<DiscoveryOrigin> {
        let origin = DiscoveryOrigin::new(
            parse_issue_id(discovered_issue_id)?,
            parse_issue_id(source_issue_id)?,
        )?;
        let mut storage = self.mutation_storage_for(workspace_root).await?;
        storage.remove_discovery_origin(&origin).await?;
        save_or_reload(storage.as_mut()).await?;
        Ok(origin)
    }

    /// List every Discovery Origin for one discovered Issue.
    ///
    /// # Errors
    ///
    /// Returns an error when the Issue or Workspace is missing, or storage
    /// cannot be queried.
    #[instrument(skip(self), fields(%discovered_issue_id))]
    pub async fn discovery_list(
        &self,
        discovered_issue_id: &str,
        workspace_root: Option<&str>,
    ) -> Result<Vec<DiscoveryOrigin>> {
        let discovered_issue_id = parse_issue_id(discovered_issue_id)?;
        let storage = self.storage_for(workspace_root).await?;
        let storage = storage.read().await;
        Ok(storage.discovery_origins(&discovered_issue_id).await?)
    }
    /// Attach one unparented child to an Epic.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid Parentage, missing context, contention, or
    /// persistence failure.
    #[instrument(skip(self), fields(%child_id, %parent_id))]
    pub async fn parent_set(
        &self,
        child_id: &str,
        parent_id: &str,
        workspace_root: Option<&str>,
    ) -> Result<Parentage> {
        let parentage = Parentage::new(parse_issue_id(child_id)?, parse_issue_id(parent_id)?)?;
        let mut storage = self.mutation_storage_for(workspace_root).await?;
        let parentage = storage.set_parent(parentage).await?;
        save_or_reload(storage.as_mut()).await?;
        Ok(parentage)
    }

    /// Remove one child's current Parentage.
    ///
    /// # Errors
    ///
    /// Returns an error for a missing child or Parentage, missing context,
    /// contention, or persistence failure.
    #[instrument(skip(self), fields(%child_id))]
    pub async fn parent_clear(
        &self,
        child_id: &str,
        workspace_root: Option<&str>,
    ) -> Result<Parentage> {
        let child_id = parse_issue_id(child_id)?;
        let mut storage = self.mutation_storage_for(workspace_root).await?;
        let parentage = storage.clear_parent(&child_id).await?;
        save_or_reload(storage.as_mut()).await?;
        Ok(parentage)
    }

    /// Replace one child's existing Epic parent.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid Parentage, missing context, contention, or
    /// persistence failure. Validation completes before the old edge changes.
    #[instrument(skip(self), fields(%child_id, %parent_id))]
    pub async fn parent_move(
        &self,
        child_id: &str,
        parent_id: &str,
        workspace_root: Option<&str>,
    ) -> Result<Parentage> {
        let parentage = Parentage::new(parse_issue_id(child_id)?, parse_issue_id(parent_id)?)?;
        let mut storage = self.mutation_storage_for(workspace_root).await?;
        storage.move_parent(parentage.clone()).await?;
        save_or_reload(storage.as_mut()).await?;
        Ok(parentage)
    }

    /// Show one child's current Parentage.
    ///
    /// # Errors
    ///
    /// Returns an error for a missing child or Workspace, or a storage failure.
    pub async fn parent_show(
        &self,
        child_id: &str,
        workspace_root: Option<&str>,
    ) -> Result<Option<Parentage>> {
        let child_id = parse_issue_id(child_id)?;
        let storage = self.storage_for(workspace_root).await?;
        let storage = storage.read().await;
        Ok(storage.parent_of(&child_id).await?)
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
        let id = parse_issue_id(issue_id)?;
        let note = reason.map(NoteContent::reopening_reason).transpose()?;
        let mut storage = self.mutation_storage_for(workspace_root).await?;

        let current = storage
            .get(&id)
            .await?
            .ok_or_else(|| Error::IssueNotFound(issue_id.to_string()))?;
        current.status.validate_reopen()?;
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
        let id = parse_issue_id(issue_id)?;
        let label = parse_label(label)?;
        let mut storage = self.mutation_storage_for(workspace_root).await?;

        let issue = storage.add_label(&id, &label).await?;
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
        let id = parse_issue_id(issue_id)?;
        let label = parse_label(label)?;
        let mut storage = self.mutation_storage_for(workspace_root).await?;

        let issue = storage.remove_label(&id, &label).await?;
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
        let id = parse_issue_id(issue_id)?;
        let storage = self.storage_for(workspace_root).await?;
        let storage = storage.read().await;

        let issue = storage
            .get(&id)
            .await?
            .ok_or_else(|| Error::IssueNotFound(issue_id.to_string()))?;

        debug!(count = issue.labels.len(), "Found labels");
        Ok(issue.labels.into_iter().map(Label::into_string).collect())
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
        let mut labels: Vec<Label> = issues.into_iter().flat_map(|issue| issue.labels).collect();
        labels.sort();
        labels.dedup();

        debug!(count = labels.len(), "Found unique labels");
        Ok(labels.into_iter().map(Label::into_string).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivets::storage::in_memory::new_in_memory_storage;
    use rstest::{fixture, rstest};
    use std::collections::BTreeSet;
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
    #[case::closed("closed", IssueStatus::Closed)]
    fn validate_status_accepts_canonical(#[case] input: &str, #[case] expected: IssueStatus) {
        assert_eq!(validate_status(input).expect("canonical status"), expected);
    }

    #[rstest]
    #[case::uppercase("OPEN")]
    #[case::cli_alias("in-progress")]
    #[case::blocked("blocked")]
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
                assert_eq!(valid_values, "open, in_progress, closed");
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
            legacy_assignee: crate::models::LegacyAssigneePresence::default(),
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
        let shown = tools
            .show(issue.id.as_str(), None)
            .await
            .expect("show should find the created issue");
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

        let issue = tools
            .create(create_params(
                "Original Title".to_string(),
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
            ))
            .await
            .expect("update should succeed");

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
            .expect("close should succeed");

        assert_eq!(closed.status, IssueStatus::Closed);
    }

    #[rstest]
    #[tokio::test]
    async fn ready_assignment_selectors(#[future] tools: Tools) {
        let tools = tools.await;
        let unassigned = create_issue(&tools, "Unassigned").await;
        let alice = tools
            .create(create_params(
                "Alice".to_string(),
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
        assert_eq!(
            ready_ids(
                tools
                    .ready(ReadyParams {
                        all_assignees: true,
                        ..ready_params(None, None, None, None, None, None)
                    })
                    .await
                    .expect("all-assignees Ready query should succeed")
            ),
            BTreeSet::from([unassigned.id, alice.id])
        );

        let error = tools
            .ready(ReadyParams {
                assignee: Some("alice".to_string()),
                all_assignees: true,
                ..ready_params(None, None, None, None, None, None)
            })
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            Error::InvalidArgument {
                field: "assignment selector",
                value,
                valid_values: "unassigned, assignee, all_assignees",
            } if value == "assignee and all_assignees"
        ));
    }

    #[rstest]
    #[tokio::test]
    async fn test_add_blocking_dependency(#[future] tools: Tools) {
        let tools = tools.await;

        let issue1 = create_issue(&tools, "Issue 1").await;
        let issue2 = create_issue(&tools, "Issue 2").await;

        // Add dependency
        let result = tools
            .blocking_dependency_add(issue1.id.as_str(), issue2.id.as_str(), None)
            .await
            .expect("dep should succeed");

        assert_eq!(result.dependent_id(), &issue1.id);
        assert_eq!(result.prerequisite_id(), &issue2.id);
    }

    #[rstest]
    #[tokio::test]
    async fn test_blocked_issues(#[future] tools: Tools) {
        let tools = tools.await;

        // Create two issues: one blocks the other
        let blocking_issue = create_issue(&tools, "Blocking Issue").await;
        let dependent_issue = create_issue(&tools, "Dependent Issue").await;

        tools
            .blocking_dependency_add(
                dependent_issue.id.as_str(),
                blocking_issue.id.as_str(),
                None,
            )
            .await
            .expect("dep should succeed");

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
            .expect("close should succeed");
        assert_eq!(closed.status, IssueStatus::Closed);

        // Reopen the issue
        let reopened = tools
            .reopen(issue.id.as_str(), Some("Work not done".to_string()), None)
            .await
            .expect("reopen should succeed");
        assert_eq!(reopened.status, IssueStatus::Open);
    }

    #[rstest]
    #[tokio::test]
    async fn test_reopen_rejects_open_and_in_progress_without_mutation(#[future] tools: Tools) {
        let tools = tools.await;

        let open = create_issue(&tools, "Still open").await;
        let open_updated_at = open.updated_at;
        assert!(matches!(
            tools.reopen(open.id.as_str(), None, None).await,
            Err(Error::InvalidStatusTransition(_))
        ));
        let open_after = tools
            .show(open.id.as_str(), None)
            .await
            .expect("open issue should remain readable");
        assert_eq!(open_after.status, IssueStatus::Open);
        assert_eq!(open_after.updated_at, open_updated_at);

        let in_progress = tools
            .create(create_params(
                "Already in progress".to_string(),
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
            .expect("assigned issue should be created");
        let in_progress = tools
            .update(update_params(
                in_progress.id.as_str(),
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
            .expect("assigned issue should enter progress");
        let in_progress_updated_at = in_progress.updated_at;
        assert!(matches!(
            tools.reopen(in_progress.id.as_str(), None, None).await,
            Err(Error::InvalidStatusTransition(_))
        ));
        let in_progress_after = tools
            .show(in_progress.id.as_str(), None)
            .await
            .expect("in-progress issue should remain readable");
        assert_eq!(in_progress_after.status, IssueStatus::InProgress);
        assert_eq!(in_progress_after.updated_at, in_progress_updated_at);
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
            .expect("label_add should succeed");
        assert!(
            updated
                .labels
                .iter()
                .any(|label| label.as_str() == "feature")
        );

        // List labels
        let labels = tools
            .label_list(issue.id.as_str(), None)
            .await
            .expect("label_list should succeed");
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
        assert!(issue.labels.iter().any(|label| label.as_str() == "bug"));

        // Remove the label
        let updated = tools
            .label_remove(issue.id.as_str(), "bug", None)
            .await
            .expect("label_remove should succeed");
        assert!(!updated.labels.iter().any(|label| label.as_str() == "bug"));
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
            .expect("close should succeed");

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
