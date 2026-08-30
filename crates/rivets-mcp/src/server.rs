//! MCP server implementation.
//!
//! This module contains the main server setup using rmcp.

use crate::context::Context;
use crate::error::Error;
use crate::models::{
    AddNoteParams, AssignmentParams, BlockedParams, BlockingDependencyListParams,
    BlockingDependencyPairParams, BlockingDependencyTreeParams, CloseParams, CreateParams,
    DiscoveryListParams, DiscoveryPairParams, LabelAddParams, LabelListAllParams, LabelListParams,
    LabelRemoveParams, ListParams, ParentChildParams, ParentPairParams, ReadyParams,
    RelatedListParams, RelatedPairParams, ReopenParams, ResourceAddParams, ResourceListParams,
    ResourceRemoveParams, ResourceUpdateParams, SetContextParams, ShowParams, StaleParams,
    UpdateParams,
};
use crate::tools::Tools;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, Content, Implementation, ProtocolVersion, ServerCapabilities, ServerInfo,
};
use rmcp::{
    ErrorData as McpError, handler::server::ServerHandler, tool, tool_handler, tool_router,
};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Maps typed errors to MCP protocol errors:
/// - Invalid user inputs and missing Issues -> `invalid_params`
/// - Workspace contention -> retryable `internal_error` data
/// - Other failures -> `internal_error`
fn to_mcp_error(error: &Error) -> McpError {
    error.to_mcp_error()
}

/// The rivets MCP server.
///
/// Provides MCP protocol handling over stdio transport.
#[derive(Clone)]
pub struct RivetsMcpServer {
    /// Shared context for workspace management.
    context: Arc<RwLock<Context>>,
    /// Tool implementations.
    tools: Arc<Tools>,
    /// Tool router for MCP dispatch.
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl RivetsMcpServer {
    /// Set the workspace context for subsequent operations.
    #[tool(
        description = "Set the default workspace root for calls that omit workspace_root. Calls with workspace_root initialize that workspace directly."
    )]
    async fn set_context(
        &self,
        Parameters(params): Parameters<SetContextParams>,
    ) -> Result<CallToolResult, McpError> {
        match self.tools.set_context(&params.workspace_root).await {
            Ok(response) => Ok(CallToolResult::success(vec![Content::json(response)?])),
            Err(e) => Err(to_mcp_error(&e)),
        }
    }

    /// Get current workspace context information.
    #[tool(description = "Show current workspace context and database path. Useful for debugging.")]
    async fn where_am_i(&self) -> Result<CallToolResult, McpError> {
        match self.tools.where_am_i().await {
            Ok(response) => Ok(CallToolResult::success(vec![Content::json(response)?])),
            Err(e) => Err(to_mcp_error(&e)),
        }
    }

    /// Find Ready Issues.
    #[tool(
        description = "Find Open Issues without unresolved direct Blocking Dependencies. Omitting assignee and all_assignees returns unassigned Issues; assignee selects one exact assignee, and all_assignees includes every Assignment. Returns up to 100 results by default if no limit is specified. Uses workspace_root if provided, otherwise uses current context."
    )]
    async fn ready(
        &self,
        Parameters(params): Parameters<ReadyParams>,
    ) -> Result<CallToolResult, McpError> {
        match self.tools.ready(params).await {
            Ok(issues) => Ok(CallToolResult::success(vec![Content::json(issues)?])),
            Err(e) => Err(to_mcp_error(&e)),
        }
    }

    /// List issues with optional filters.
    #[tool(
        description = "List all issues with optional filters (status, priority, kind, assignee, label). Returns up to 100 results by default if no limit specified. Uses workspace_root if provided, otherwise uses current context."
    )]
    async fn list(
        &self,
        Parameters(params): Parameters<ListParams>,
    ) -> Result<CallToolResult, McpError> {
        match self.tools.list(params).await {
            Ok(issues) => Ok(CallToolResult::success(vec![Content::json(issues)?])),
            Err(e) => Err(to_mcp_error(&e)),
        }
    }

    /// Show detailed information about a specific issue.
    #[tool(
        description = "Show detailed information about a specific issue including dependencies and dependents. Uses workspace_root if provided, otherwise uses current context."
    )]
    async fn show(
        &self,
        Parameters(params): Parameters<ShowParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .tools
            .show(&params.issue_id, params.workspace_root.as_deref())
            .await
        {
            Ok(issue) => Ok(CallToolResult::success(vec![Content::json(issue)?])),
            Err(e) => Err(to_mcp_error(&e)),
        }
    }

    /// Get blocked issues and their blockers.
    #[tool(
        description = "Get blocked issues showing what dependencies are blocking them from being worked on. Uses workspace_root if provided, otherwise uses current context."
    )]
    async fn blocked(
        &self,
        Parameters(params): Parameters<BlockedParams>,
    ) -> Result<CallToolResult, McpError> {
        match self.tools.blocked(params.workspace_root.as_deref()).await {
            Ok(blocked) => Ok(CallToolResult::success(vec![Content::json(blocked)?])),
            Err(e) => Err(to_mcp_error(&e)),
        }
    }

    /// Create a new issue.
    #[tool(
        description = "Create a new issue (bug, feature, task, epic, or chore) with an optional initial Note, design, acceptance criteria, and dependencies. Uses workspace_root if provided, otherwise uses current context."
    )]
    async fn create(
        &self,
        Parameters(params): Parameters<CreateParams>,
    ) -> Result<CallToolResult, McpError> {
        match self.tools.create(params).await {
            Ok(issue) => Ok(CallToolResult::success(vec![Content::json(issue)?])),
            Err(e) => Err(to_mcp_error(&e)),
        }
    }

    /// Update an existing issue.
    #[tool(
        description = "Update an existing issue's status, priority, kind, labels, description, design notes, or acceptance criteria. Assignment changes use claim or release. Labels replace existing labels when provided. Uses workspace_root if provided, otherwise uses current context."
    )]
    async fn update(
        &self,
        Parameters(params): Parameters<UpdateParams>,
    ) -> Result<CallToolResult, McpError> {
        match self.tools.update(params).await {
            Ok(issue) => Ok(CallToolResult::success(vec![Content::json(issue)?])),
            Err(e) => Err(to_mcp_error(&e)),
        }
    }

    /// Atomically claim an Open, unblocked Issue.
    #[tool(
        description = "Atomically assign one Open Issue without unresolved direct Blocking Dependencies. A same-Assignee retry is idempotent only while the Issue remains Open; a different Assignee receives Already Claimed. Uses workspace_root if provided, otherwise uses current context."
    )]
    async fn claim(
        &self,
        Parameters(params): Parameters<AssignmentParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .tools
            .claim(
                &params.issue_id,
                &params.assignee,
                params.workspace_root.as_deref(),
            )
            .await
        {
            Ok(issue) => Ok(CallToolResult::success(vec![Content::json(issue)?])),
            Err(error) => Err(to_mcp_error(&error)),
        }
    }

    /// Atomically release an Open Issue from its exact Assignee.
    #[tool(
        description = "Atomically clear Assignment from one Open Issue when assignee exactly matches its current owner. Release remains valid while the Open Issue is blocked. Uses workspace_root if provided, otherwise uses current context."
    )]
    async fn release(
        &self,
        Parameters(params): Parameters<AssignmentParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .tools
            .release(
                &params.issue_id,
                &params.assignee,
                params.workspace_root.as_deref(),
            )
            .await
        {
            Ok(issue) => Ok(CallToolResult::success(vec![Content::json(issue)?])),
            Err(error) => Err(to_mcp_error(&error)),
        }
    }

    /// Append an immutable Note to an Issue.
    #[tool(
        description = "Append one immutable, timestamped Note to an issue. Existing Note history is preserved. Uses workspace_root if provided, otherwise uses current context."
    )]
    async fn add_note(
        &self,
        Parameters(params): Parameters<AddNoteParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .tools
            .add_note(
                &params.issue_id,
                params.content,
                params.workspace_root.as_deref(),
            )
            .await
        {
            Ok(issue) => Ok(CallToolResult::success(vec![Content::json(issue)?])),
            Err(error) => Err(to_mcp_error(&error)),
        }
    }

    /// Associate a Web URL or Workspace Path target with an Issue.
    #[tool(
        description = "Associate an absolute HTTP or HTTPS Web URL or a workspace-relative Path with an issue using a canonical role and optional label. Exactly one of url/path is required. Uses workspace_root if provided, otherwise uses current context."
    )]
    async fn resource_add(
        &self,
        Parameters(params): Parameters<ResourceAddParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .tools
            .resource_add(
                &params.issue_id,
                params.url,
                params.path,
                &params.role,
                params.label,
                params.workspace_root.as_deref(),
            )
            .await
        {
            Ok(issue) => Ok(CallToolResult::success(vec![Content::json(issue)?])),
            Err(error) => Err(to_mcp_error(&error)),
        }
    }

    /// Update an Issue's Associated Resource by its stable identifier.
    #[tool(
        description = "Update an issue's Associated Resource by its stable resource identifier. Only the provided fields change; the resource keeps its identifier and position. Uses workspace_root if provided, otherwise uses current context."
    )]
    async fn resource_update(
        &self,
        Parameters(params): Parameters<ResourceUpdateParams>,
    ) -> Result<CallToolResult, McpError> {
        match self.tools.resource_update(params).await {
            Ok(issue) => Ok(CallToolResult::success(vec![Content::json(issue)?])),
            Err(error) => Err(to_mcp_error(&error)),
        }
    }

    /// Remove an Issue's Associated Resource by its stable identifier.
    #[tool(
        description = "Remove an issue's Associated Resource by its stable resource identifier. The remaining resources keep their identifiers and positions. Uses workspace_root if provided, otherwise uses current context."
    )]
    async fn resource_remove(
        &self,
        Parameters(params): Parameters<ResourceRemoveParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .tools
            .resource_remove(
                &params.issue_id,
                &params.resource_id,
                params.workspace_root.as_deref(),
            )
            .await
        {
            Ok(issue) => Ok(CallToolResult::success(vec![Content::json(issue)?])),
            Err(error) => Err(to_mcp_error(&error)),
        }
    }

    /// List an Issue's Associated Resources.
    #[tool(
        description = "List an issue's Associated Resources in insertion order. Uses workspace_root if provided, otherwise uses current context."
    )]
    async fn resource_list(
        &self,
        Parameters(params): Parameters<ResourceListParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .tools
            .resource_list(&params.issue_id, params.workspace_root.as_deref())
            .await
        {
            Ok(resources) => Ok(CallToolResult::success(vec![Content::json(resources)?])),
            Err(error) => Err(to_mcp_error(&error)),
        }
    }

    /// Close an issue.
    #[tool(
        description = "Close (complete) an issue. Mark work as done when you've finished implementing/fixing it. Uses workspace_root if provided, otherwise uses current context."
    )]
    async fn close(
        &self,
        Parameters(params): Parameters<CloseParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .tools
            .close(
                &params.issue_id,
                params.reason,
                params.workspace_root.as_deref(),
            )
            .await
        {
            Ok(issue) => Ok(CallToolResult::success(vec![Content::json(issue)?])),
            Err(e) => Err(to_mcp_error(&e)),
        }
    }

    /// Add a directed Blocking Dependency from dependent to prerequisite.
    #[tool(
        description = "Add a directed Blocking Dependency. dependent_id is blocked by and depends on prerequisite_id. Uses workspace_root if provided, otherwise uses current context."
    )]
    async fn blocking_dependency_add(
        &self,
        Parameters(params): Parameters<BlockingDependencyPairParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .tools
            .blocking_dependency_add(
                &params.dependent_id,
                &params.prerequisite_id,
                params.workspace_root.as_deref(),
            )
            .await
        {
            Ok(dependency) => Ok(CallToolResult::success(vec![Content::json(dependency)?])),
            Err(error) => Err(to_mcp_error(&error)),
        }
    }

    /// Remove one directed Blocking Dependency.
    #[tool(
        description = "Remove one directed Blocking Dependency without changing either Issue's Workflow State. Uses workspace_root if provided, otherwise uses current context."
    )]
    async fn blocking_dependency_remove(
        &self,
        Parameters(params): Parameters<BlockingDependencyPairParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .tools
            .blocking_dependency_remove(
                &params.dependent_id,
                &params.prerequisite_id,
                params.workspace_root.as_deref(),
            )
            .await
        {
            Ok(dependency) => Ok(CallToolResult::success(vec![Content::json(dependency)?])),
            Err(error) => Err(to_mcp_error(&error)),
        }
    }

    /// List Blocking Dependencies from one explicit endpoint perspective.
    #[tool(
        description = "List either prerequisites of a dependent or dependents of a prerequisite using the tagged query. Uses workspace_root if provided, otherwise uses current context."
    )]
    async fn blocking_dependency_list(
        &self,
        Parameters(params): Parameters<BlockingDependencyListParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .tools
            .blocking_dependency_list(&params.query, params.workspace_root.as_deref())
            .await
        {
            Ok(dependencies) => Ok(CallToolResult::success(vec![Content::json(dependencies)?])),
            Err(error) => Err(to_mcp_error(&error)),
        }
    }

    /// Traverse only Blocking prerequisites from one dependent.
    #[tool(
        description = "Show a dependent's transitive Blocking prerequisite tree. depth zero means unlimited. Uses workspace_root if provided, otherwise uses current context."
    )]
    async fn blocking_dependency_tree(
        &self,
        Parameters(params): Parameters<BlockingDependencyTreeParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .tools
            .blocking_dependency_tree(
                &params.dependent_id,
                params.depth,
                params.workspace_root.as_deref(),
            )
            .await
        {
            Ok(tree) => Ok(CallToolResult::success(vec![Content::json(tree)?])),
            Err(error) => Err(to_mcp_error(&error)),
        }
    }

    /// Add one symmetric Related Association.
    #[tool(
        description = "Add a symmetric, non-blocking Related Association between issue_id and related_issue_id. Reversed endpoint order identifies the same association. Uses workspace_root if provided, otherwise uses current context."
    )]
    async fn related_add(
        &self,
        Parameters(params): Parameters<RelatedPairParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .tools
            .related_add(
                &params.issue_id,
                &params.related_issue_id,
                params.workspace_root.as_deref(),
            )
            .await
        {
            Ok(association) => Ok(CallToolResult::success(vec![Content::json(association)?])),
            Err(error) => Err(to_mcp_error(&error)),
        }
    }

    /// Remove one symmetric Related Association.
    #[tool(
        description = "Remove a symmetric, non-blocking Related Association between issue_id and related_issue_id. Endpoint order does not matter. Uses workspace_root if provided, otherwise uses current context."
    )]
    async fn related_remove(
        &self,
        Parameters(params): Parameters<RelatedPairParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .tools
            .related_remove(
                &params.issue_id,
                &params.related_issue_id,
                params.workspace_root.as_deref(),
            )
            .await
        {
            Ok(association) => Ok(CallToolResult::success(vec![Content::json(association)?])),
            Err(error) => Err(to_mcp_error(&error)),
        }
    }

    /// List every Related Association containing one Issue.
    #[tool(
        description = "List symmetric, non-blocking Related Associations containing issue_id. Uses workspace_root if provided, otherwise uses current context."
    )]
    async fn related_list(
        &self,
        Parameters(params): Parameters<RelatedListParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .tools
            .related_list(&params.issue_id, params.workspace_root.as_deref())
            .await
        {
            Ok(associations) => Ok(CallToolResult::success(vec![Content::json(associations)?])),
            Err(error) => Err(to_mcp_error(&error)),
        }
    }

    /// Add one directed Discovery Origin.
    #[tool(
        description = "Add a directed, non-blocking Discovery Origin from discovered_issue_id to source_issue_id. Uses workspace_root if provided, otherwise uses current context."
    )]
    async fn discovery_add(
        &self,
        Parameters(params): Parameters<DiscoveryPairParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .tools
            .discovery_add(
                &params.discovered_issue_id,
                &params.source_issue_id,
                params.workspace_root.as_deref(),
            )
            .await
        {
            Ok(origin) => Ok(CallToolResult::success(vec![Content::json(origin)?])),
            Err(error) => Err(to_mcp_error(&error)),
        }
    }

    /// Remove one directed Discovery Origin.
    #[tool(
        description = "Remove a directed, non-blocking Discovery Origin from discovered_issue_id to source_issue_id. Uses workspace_root if provided, otherwise uses current context."
    )]
    async fn discovery_remove(
        &self,
        Parameters(params): Parameters<DiscoveryPairParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .tools
            .discovery_remove(
                &params.discovered_issue_id,
                &params.source_issue_id,
                params.workspace_root.as_deref(),
            )
            .await
        {
            Ok(origin) => Ok(CallToolResult::success(vec![Content::json(origin)?])),
            Err(error) => Err(to_mcp_error(&error)),
        }
    }

    /// List every Discovery Origin for one discovered Issue.
    #[tool(
        description = "List directed, non-blocking Discovery Origins for discovered_issue_id. Uses workspace_root if provided, otherwise uses current context."
    )]
    async fn discovery_list(
        &self,
        Parameters(params): Parameters<DiscoveryListParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .tools
            .discovery_list(
                &params.discovered_issue_id,
                params.workspace_root.as_deref(),
            )
            .await
        {
            Ok(origins) => Ok(CallToolResult::success(vec![Content::json(origins)?])),
}
    /// Attach an unparented child to an Epic.
    #[tool(
        description = "Set single-Epic Parentage using explicit child_id and parent_id roles. Uses workspace_root if provided, otherwise uses current context."
    )]
    async fn parent_set(
        &self,
        Parameters(params): Parameters<ParentPairParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .tools
            .parent_set(
                &params.child_id,
                &params.parent_id,
                params.workspace_root.as_deref(),
            )
            .await
        {
            Ok(parentage) => Ok(CallToolResult::success(vec![Content::json(parentage)?])),
            Err(error) => Err(to_mcp_error(&error)),
        }
    }

    /// Remove one child's Parentage.
    #[tool(
        description = "Clear one child's Parentage without changing Workflow State or Blocking Dependencies. Uses workspace_root if provided, otherwise uses current context."
    )]
    async fn parent_clear(
        &self,
        Parameters(params): Parameters<ParentChildParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .tools
            .parent_clear(&params.child_id, params.workspace_root.as_deref())
            .await
        {
            Ok(parentage) => Ok(CallToolResult::success(vec![Content::json(parentage)?])),
            Err(error) => Err(to_mcp_error(&error)),
        }
    }

    /// Replace one child's existing Epic parent.
    #[tool(
        description = "Move one child from its existing Epic parent to parent_id. The candidate is validated before replacement. Uses workspace_root if provided, otherwise uses current context."
    )]
    async fn parent_move(
        &self,
        Parameters(params): Parameters<ParentPairParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .tools
            .parent_move(
                &params.child_id,
                &params.parent_id,
                params.workspace_root.as_deref(),
            )
            .await
        {
            Ok(parentage) => Ok(CallToolResult::success(vec![Content::json(parentage)?])),
            Err(error) => Err(to_mcp_error(&error)),
        }
    }

    /// Show one child's current Parentage.
    #[tool(
        description = "Show one child's current Epic parent, or null when the child is unparented. Uses workspace_root if provided, otherwise uses current context."
    )]
    async fn parent_show(
        &self,
        Parameters(params): Parameters<ParentChildParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .tools
            .parent_show(&params.child_id, params.workspace_root.as_deref())
            .await
        {
            Ok(parentage) => Ok(CallToolResult::success(vec![Content::json(parentage)?])),
            Err(error) => Err(to_mcp_error(&error)),
        }
    }

    /// Reopen a closed issue.
    #[tool(
        description = "Reopen a previously closed issue. Use when work needs to continue or was not actually complete. Uses workspace_root if provided, otherwise uses current context."
    )]
    async fn reopen(
        &self,
        Parameters(params): Parameters<ReopenParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .tools
            .reopen(
                &params.issue_id,
                params.reason,
                params.workspace_root.as_deref(),
            )
            .await
        {
            Ok(issue) => Ok(CallToolResult::success(vec![Content::json(issue)?])),
            Err(e) => Err(to_mcp_error(&e)),
        }
    }

    /// Find stale issues.
    #[tool(
        description = "Find issues that haven't been updated recently. Default is 30 days. Useful for identifying forgotten work or issues needing attention. Uses workspace_root if provided, otherwise uses current context."
    )]
    async fn stale(
        &self,
        Parameters(params): Parameters<StaleParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .tools
            .stale(
                params.days,
                params.status.as_deref(),
                params.limit,
                params.workspace_root.as_deref(),
            )
            .await
        {
            Ok(issues) => Ok(CallToolResult::success(vec![Content::json(issues)?])),
            Err(e) => Err(to_mcp_error(&e)),
        }
    }

    /// Add a label to an issue.
    #[tool(
        description = "Add a label to an issue for categorization. Labels should be lowercase, alphanumeric with hyphens/underscores. Uses workspace_root if provided, otherwise uses current context."
    )]
    async fn label_add(
        &self,
        Parameters(params): Parameters<LabelAddParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .tools
            .label_add(
                &params.issue_id,
                &params.label,
                params.workspace_root.as_deref(),
            )
            .await
        {
            Ok(issue) => Ok(CallToolResult::success(vec![Content::json(issue)?])),
            Err(e) => Err(to_mcp_error(&e)),
        }
    }

    /// Remove a label from an issue.
    #[tool(
        description = "Remove a label from an issue. Uses workspace_root if provided, otherwise uses current context."
    )]
    async fn label_remove(
        &self,
        Parameters(params): Parameters<LabelRemoveParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .tools
            .label_remove(
                &params.issue_id,
                &params.label,
                params.workspace_root.as_deref(),
            )
            .await
        {
            Ok(issue) => Ok(CallToolResult::success(vec![Content::json(issue)?])),
            Err(e) => Err(to_mcp_error(&e)),
        }
    }

    /// List labels for a specific issue.
    #[tool(
        description = "List all labels assigned to a specific issue. Uses workspace_root if provided, otherwise uses current context."
    )]
    async fn label_list(
        &self,
        Parameters(params): Parameters<LabelListParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .tools
            .label_list(&params.issue_id, params.workspace_root.as_deref())
            .await
        {
            Ok(labels) => Ok(CallToolResult::success(vec![Content::json(labels)?])),
            Err(e) => Err(to_mcp_error(&e)),
        }
    }

    /// List all unique labels across all issues.
    #[tool(
        description = "List all unique labels used across all issues in the workspace. Useful for understanding available categorizations. Uses workspace_root if provided, otherwise uses current context."
    )]
    async fn label_list_all(
        &self,
        Parameters(params): Parameters<LabelListAllParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .tools
            .label_list_all(params.workspace_root.as_deref())
            .await
        {
            Ok(labels) => Ok(CallToolResult::success(vec![Content::json(labels)?])),
            Err(e) => Err(to_mcp_error(&e)),
        }
    }
}

impl RivetsMcpServer {
    /// Create a new rivets MCP server.
    #[must_use]
    pub fn new() -> Self {
        let context = Arc::new(RwLock::new(Context::new()));
        let tools = Arc::new(Tools::new(Arc::clone(&context)));

        Self {
            context,
            tools,
            tool_router: Self::tool_router(),
        }
    }

    /// Get a reference to the context.
    #[must_use]
    pub fn context(&self) -> &Arc<RwLock<Context>> {
        &self.context
    }
}

impl Default for RivetsMcpServer {
    fn default() -> Self {
        Self::new()
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for RivetsMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_protocol_version(ProtocolVersion::V_2024_11_05)
            .with_server_info(Implementation::new(
                "rivets-mcp",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(
                "Rivets MCP server for issue tracking. Pass workspace_root to a tool call, or use set_context to set the default workspace.",
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ListParams, ReadyParams, ShowParams};
    use clap::{Command, CommandFactory};
    use rmcp::handler::server::ServerHandler;
    use serde::Deserialize;
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::Path;

    #[derive(Debug, Deserialize)]
    struct ParityRegistry {
        schema_version: u32,
        decision: ParityDecision,
        current_parity_values: BTreeMap<String, String>,
        target_status_values: BTreeMap<String, String>,
        operations: Vec<ParityOperation>,
        delivery_groups: Vec<ParityDeliveryGroup>,
    }

    #[derive(Debug, Deserialize)]
    struct ParityDecision {
        id: String,
        path: String,
    }

    #[derive(Debug, Deserialize)]
    struct ParityOperation {
        id: String,
        requirement: String,
        current_parity: String,
        target_status: String,
        risk: String,
        required_resolution: String,
        cli: ParityAdapter,
        mcp: ParityAdapter,
    }

    #[derive(Debug, Deserialize)]
    struct ParityAdapter {
        surfaces: Vec<String>,
        #[serde(default)]
        forms: Vec<ParityForm>,
    }

    #[derive(Debug, Deserialize)]
    struct ParityForm {
        surface: String,
        argument: String,
    }

    #[derive(Debug, Deserialize)]
    struct ParityDeliveryGroup {
        id: String,
        title: String,
        tracking_issue: String,
        intents: Vec<String>,
        blocked_by: Vec<String>,
    }

    fn collect_cli_leaf_paths(command: &Command, prefix: &str, leaves: &mut BTreeSet<String>) {
        let mut has_subcommands = false;
        for subcommand in command
            .get_subcommands()
            .filter(|subcommand| subcommand.get_name() != "help")
        {
            has_subcommands = true;
            let path = if prefix.is_empty() {
                subcommand.get_name().to_string()
            } else {
                format!("{prefix} {}", subcommand.get_name())
            };
            collect_cli_leaf_paths(subcommand, &path, leaves);
        }

        if !has_subcommands && !prefix.is_empty() {
            leaves.insert(prefix.to_string());
        }
    }

    fn cli_command_for_path<'a>(command: &'a Command, path: &str) -> Option<&'a Command> {
        path.split_whitespace().try_fold(command, |parent, name| {
            parent
                .get_subcommands()
                .find(|subcommand| subcommand.get_name() == name)
        })
    }

    fn parity_registry() -> ParityRegistry {
        serde_json::from_str(include_str!("../../../docs/cli-mcp-parity.json"))
            .expect("CLI/MCP parity registry should be valid")
    }

    struct ClassifiedSurfaces {
        cli: BTreeSet<String>,
        mcp: BTreeSet<String>,
        cli_forms: BTreeSet<(String, String)>,
    }

    const REQUIRED_FUTURE_INTENTS: &[&str] = &[
        "claim_assignment",
        "release_assignment",
        "add_blocking_dependency",
        "remove_blocking_dependency",
        "list_blocking_dependencies",
        "show_blocking_dependency_tree",
        "set_parentage",
        "clear_parentage",
        "move_parentage",
        "show_parentage",
        "add_related_association",
        "remove_related_association",
        "list_related_associations",
        "add_discovery_origin",
        "remove_discovery_origin",
        "list_discovery_origins",
    ];

    fn validate_registry_header(registry: &ParityRegistry) {
        assert_eq!(
            registry.schema_version, 1,
            "unsupported parity registry schema"
        );
        assert!(
            !registry.decision.id.trim().is_empty(),
            "parity registry decision id must not be empty"
        );
        let decision_number = registry
            .decision
            .id
            .strip_prefix("ADR-")
            .expect("parity registry decision id must use ADR-NNNN");
        assert!(
            decision_number.len() == 4 && decision_number.bytes().all(|byte| byte.is_ascii_digit()),
            "parity registry decision id must use ADR-NNNN: {}",
            registry.decision.id
        );
        let decision_file_name = Path::new(&registry.decision.path)
            .file_name()
            .and_then(|name| name.to_str())
            .expect("parity registry decision path must name a UTF-8 file");
        assert!(
            decision_file_name.starts_with(&format!("{decision_number}-")),
            "parity registry decision id and path disagree: {} != {}",
            registry.decision.id,
            registry.decision.path
        );
        let decision_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs")
            .join(&registry.decision.path);
        assert!(
            decision_path.is_file(),
            "parity registry decision does not exist: {}",
            decision_path.display()
        );
    }

    fn classify_registry_operations(registry: &ParityRegistry) -> ClassifiedSurfaces {
        let mut operation_ids = BTreeSet::new();
        let mut classified = ClassifiedSurfaces {
            cli: BTreeSet::new(),
            mcp: BTreeSet::new(),
            cli_forms: BTreeSet::new(),
        };

        for operation in &registry.operations {
            assert!(
                operation_ids.insert(operation.id.clone()),
                "duplicate parity operation id: {}",
                operation.id
            );
            assert!(
                ["shared", "future_shared", "cli_only", "mcp_only", "legacy"]
                    .contains(&operation.requirement.as_str()),
                "unknown parity requirement for {}: {}",
                operation.id,
                operation.requirement
            );
            assert!(
                registry
                    .current_parity_values
                    .contains_key(&operation.current_parity),
                "unknown current parity classification for {}: {}",
                operation.id,
                operation.current_parity
            );
            assert!(
                registry
                    .target_status_values
                    .contains_key(&operation.target_status),
                "unknown target status classification for {}: {}",
                operation.id,
                operation.target_status
            );
            assert!(
                !operation.risk.trim().is_empty(),
                "parity operation must state its risk: {}",
                operation.id
            );
            assert!(
                !operation.required_resolution.trim().is_empty(),
                "parity operation must state its required resolution: {}",
                operation.id
            );
            for surface in &operation.cli.surfaces {
                assert!(
                    classified.cli.insert(surface.clone()),
                    "CLI leaf is classified more than once: {surface}"
                );
            }
            for form in &operation.cli.forms {
                assert!(
                    classified
                        .cli_forms
                        .insert((form.surface.clone(), form.argument.clone())),
                    "CLI form is classified more than once: {} --{}",
                    form.surface,
                    form.argument
                );
            }
            for surface in &operation.mcp.surfaces {
                assert!(
                    classified.mcp.insert(surface.clone()),
                    "MCP tool is classified more than once: {surface}"
                );
            }
        }
        classified
    }

    fn assert_classified_cli_forms(cli: &Command, forms: &BTreeSet<(String, String)>) {
        for (surface, argument) in forms {
            let command = cli_command_for_path(cli, surface)
                .unwrap_or_else(|| panic!("classified CLI form has no command: {surface}"));
            assert!(
                command
                    .get_arguments()
                    .any(|candidate| candidate.get_long() == Some(argument.as_str())),
                "classified CLI form has no --{argument} argument: {surface}"
            );
        }
    }

    fn assert_required_future_intents(registry: &ParityRegistry) {
        for required_id in REQUIRED_FUTURE_INTENTS {
            let operation = registry
                .operations
                .iter()
                .find(|operation| operation.id == *required_id)
                .unwrap_or_else(|| panic!("required future intent is unclassified: {required_id}"));
            assert_eq!(
                operation.requirement, "future_shared",
                "required future intent has wrong requirement: {required_id}"
            );
        }
    }

    fn validate_delivery_groups(registry: &ParityRegistry) {
        let operation_ids = registry
            .operations
            .iter()
            .map(|operation| operation.id.as_str())
            .collect::<BTreeSet<_>>();
        let required_tracking = registry
            .operations
            .iter()
            .filter(|operation| {
                operation.target_status.starts_with("gap_")
                    || operation.target_status == "legacy_cutover"
            })
            .map(|operation| operation.id.as_str())
            .collect::<BTreeSet<_>>();
        let tracker_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.rivets/issues.jsonl");
        let tracker = std::fs::read_to_string(&tracker_path).unwrap_or_else(|error| {
            panic!(
                "parity registry tracker cannot be read at {}: {error}",
                tracker_path.display()
            )
        });
        let tracker_issues = tracker
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                let issue: serde_json::Value =
                    serde_json::from_str(line).expect("tracker line should be valid JSON");
                let issue_id = issue["id"]
                    .as_str()
                    .expect("tracker Issue should have a string id")
                    .to_string();
                let blocking_dependencies = issue["dependencies"]
                    .as_array()
                    .expect("tracker Issue should have dependencies")
                    .iter()
                    .filter(|dependency| dependency["dep_type"] == "blocks")
                    .map(|dependency| {
                        dependency["depends_on_id"]
                            .as_str()
                            .expect("Blocking Dependency should have depends_on_id")
                            .to_string()
                    })
                    .collect::<BTreeSet<_>>();
                (issue_id, blocking_dependencies)
            })
            .collect::<BTreeMap<_, _>>();

        let mut group_ids = BTreeSet::new();
        let mut tracked_intents = BTreeSet::new();
        for group in &registry.delivery_groups {
            assert!(
                group_ids.insert(group.id.as_str()),
                "duplicate parity delivery group: {}",
                group.id
            );
            assert!(
                !group.title.trim().is_empty(),
                "parity delivery group must have a title: {}",
                group.id
            );
            assert!(
                tracker_issues.contains_key(&group.tracking_issue),
                "parity delivery group references an unknown Issue: {}",
                group.tracking_issue
            );
            assert!(
                !group.intents.is_empty(),
                "parity delivery group must own at least one intent: {}",
                group.id
            );
            for intent in &group.intents {
                assert!(
                    operation_ids.contains(intent.as_str()),
                    "parity delivery group {} references an unknown intent: {intent}",
                    group.id
                );
                tracked_intents.insert(intent.as_str());
            }
            for blocker in &group.blocked_by {
                assert!(
                    tracker_issues.contains_key(blocker),
                    "parity delivery group {} references an unknown blocker: {blocker}",
                    group.id
                );
            }
            let registered_blockers = group.blocked_by.iter().cloned().collect::<BTreeSet<_>>();
            assert_eq!(
                tracker_issues[&group.tracking_issue], registered_blockers,
                "parity delivery group blockers drifted from tracker Issue {}",
                group.tracking_issue
            );
        }

        assert_eq!(
            required_tracking
                .difference(&tracked_intents)
                .copied()
                .collect::<Vec<_>>(),
            Vec::<&str>::new(),
            "every parity gap must map to a tracked delivery group"
        );
    }

    #[test]
    fn test_server_creation() {
        let server = RivetsMcpServer::new();
        assert!(server.context().try_read().is_ok());
    }

    #[test]
    fn test_server_default() {
        let server = RivetsMcpServer::default();
        assert!(server.context().try_read().is_ok());
    }

    #[test]
    fn test_server_info() {
        let server = RivetsMcpServer::new();
        let info = server.get_info();
        assert_eq!(info.server_info.name, "rivets-mcp");
        assert!(!info.server_info.version.is_empty());
        assert!(info.instructions.is_some());
    }

    #[test]
    fn parity_registry_classifies_every_cli_leaf_and_mcp_tool() {
        let registry = parity_registry();
        validate_registry_header(&registry);
        let classified = classify_registry_operations(&registry);

        assert_required_future_intents(&registry);
        validate_delivery_groups(&registry);
        let cli = rivets::cli::Cli::command();
        let mut current_cli = BTreeSet::new();
        collect_cli_leaf_paths(&cli, "", &mut current_cli);
        assert_classified_cli_forms(&cli, &classified.cli_forms);

        let current_mcp: BTreeSet<String> = RivetsMcpServer::new()
            .tool_router
            .list_all()
            .into_iter()
            .map(|tool| tool.name.to_string())
            .collect();

        assert_eq!(
            classified.cli, current_cli,
            "every current CLI leaf must have exactly one parity classification"
        );
        assert_eq!(
            classified.mcp, current_mcp,
            "every current MCP tool must have exactly one parity classification"
        );
    }

    #[test]
    fn test_tool_schemas_publish_expected_fields() {
        let server = RivetsMcpServer::new();
        let tools = server.tool_router.list_all();

        // Verify all expected tools are registered
        let tool_names: Vec<&str> = tools.iter().map(|t| &*t.name).collect();

        assert!(tool_names.contains(&"set_context"));
        assert!(tool_names.contains(&"where_am_i"));
        assert!(tool_names.contains(&"ready"));
        assert!(tool_names.contains(&"list"));
        assert!(tool_names.contains(&"show"));
        assert!(tool_names.contains(&"blocked"));
        assert!(tool_names.contains(&"create"));
        assert!(tool_names.contains(&"update"));
        assert!(tool_names.contains(&"claim"));
        assert!(tool_names.contains(&"release"));
        assert!(tool_names.contains(&"add_note"));
        assert!(tool_names.contains(&"close"));
        assert!(tool_names.contains(&"reopen"));
        assert!(tool_names.contains(&"stale"));
        assert!(tool_names.contains(&"label_add"));
        assert!(tool_names.contains(&"label_remove"));
        assert!(tool_names.contains(&"label_list"));
        assert!(tool_names.contains(&"label_list_all"));
        assert!(tool_names.contains(&"resource_add"));
        assert!(tool_names.contains(&"resource_list"));
        assert!(tool_names.contains(&"resource_update"));
        assert!(tool_names.contains(&"resource_remove"));
        assert!(tool_names.contains(&"blocking_dependency_add"));
        assert!(tool_names.contains(&"blocking_dependency_remove"));
        assert!(tool_names.contains(&"blocking_dependency_list"));
        assert!(tool_names.contains(&"blocking_dependency_tree"));
        assert!(tool_names.contains(&"related_add"));
        assert!(tool_names.contains(&"related_remove"));
        assert!(tool_names.contains(&"related_list"));
        assert!(tool_names.contains(&"discovery_add"));
        assert!(tool_names.contains(&"discovery_remove"));
        assert!(tool_names.contains(&"discovery_list"));
        assert!(tool_names.contains(&"parent_set"));
        assert!(tool_names.contains(&"parent_clear"));
        assert!(tool_names.contains(&"parent_move"));
        assert!(tool_names.contains(&"parent_show"));
        let input_properties = |name: &str| {
            tools
                .iter()
                .find(|tool| tool.name == name)
                .and_then(|tool| tool.input_schema.get("properties"))
                .and_then(serde_json::Value::as_object)
                .expect("tool input schema should expose properties")
        };
        assert!(input_properties("create").contains_key("initial_note"));
        assert!(!input_properties("update").contains_key("notes"));
        assert!(!input_properties("update").contains_key("assignee"));
        for tool_name in ["claim", "release"] {
            let properties = input_properties(tool_name);
            assert!(properties.contains_key("issue_id"));
            assert!(properties.contains_key("assignee"));
            assert!(properties.contains_key("workspace_root"));
        }
        assert!(input_properties("add_note").contains_key("content"));
        assert!(input_properties("resource_add").contains_key("url"));
        assert!(input_properties("resource_add").contains_key("path"));
        assert!(input_properties("resource_add").contains_key("role"));
        assert!(input_properties("resource_update").contains_key("resource_id"));
        assert!(input_properties("resource_remove").contains_key("resource_id"));
        for tool_name in ["blocking_dependency_add", "blocking_dependency_remove"] {
            let properties = input_properties(tool_name);
            assert!(properties.contains_key("dependent_id"));
            assert!(properties.contains_key("prerequisite_id"));
            assert!(!properties.contains_key("issue_id"));
            assert!(!properties.contains_key("depends_on_id"));
        }
        for tool_name in ["parent_set", "parent_move"] {
            let properties = input_properties(tool_name);
            assert!(properties.contains_key("child_id"));
            assert!(properties.contains_key("parent_id"));
            assert!(!properties.contains_key("issue_id"));
        }
        for tool_name in ["parent_clear", "parent_show"] {
            let properties = input_properties(tool_name);
            assert!(properties.contains_key("child_id"));
            assert!(!properties.contains_key("parent_id"));
            assert!(!properties.contains_key("issue_id"));
        }
        assert!(input_properties("blocking_dependency_list").contains_key("query"));
        let tree = input_properties("blocking_dependency_tree");
        assert!(tree.contains_key("dependent_id"));
        assert!(tree.contains_key("depth"));
        let list_tool = tools
            .iter()
            .find(|tool| tool.name == "blocking_dependency_list")
            .expect("Blocking list tool should be registered");
        let list_schema = serde_json::to_string(&list_tool.input_schema)
            .expect("Blocking Dependency list schema should serialize");
        assert!(list_schema.contains("prerequisites_of"));
        assert!(list_schema.contains("dependents_of"));
        for tool_name in ["related_add", "related_remove"] {
            let properties = input_properties(tool_name);
            assert!(properties.contains_key("issue_id"));
            assert!(properties.contains_key("related_issue_id"));
            assert!(!properties.contains_key("discovered_issue_id"));
            assert!(!properties.contains_key("source_issue_id"));
        }
        let related_list = input_properties("related_list");
        assert!(related_list.contains_key("issue_id"));
        assert!(!related_list.contains_key("related_issue_id"));
        for tool_name in ["discovery_add", "discovery_remove"] {
            let properties = input_properties(tool_name);
            assert!(properties.contains_key("discovered_issue_id"));
            assert!(properties.contains_key("source_issue_id"));
            assert!(!properties.contains_key("issue_id"));
            assert!(!properties.contains_key("related_issue_id"));
        }
        let discovery_list = input_properties("discovery_list");
        assert!(discovery_list.contains_key("discovered_issue_id"));
        assert!(!discovery_list.contains_key("source_issue_id"));
        assert_eq!(tools.len(), 32);
}
        assert_eq!(tools.len(), 28);
    }

    #[test]
    fn parentage_tool_router_and_schemas() {
        let tools = RivetsMcpServer::new().tool_router.list_all();
        for (tool_name, has_parent_id) in [
            ("parent_set", true),
            ("parent_clear", false),
            ("parent_move", true),
            ("parent_show", false),
        ] {
            let tool = tools
                .iter()
                .find(|tool| tool.name == tool_name)
                .expect("Parentage tool should be registered");
            let properties = tool
                .input_schema
                .get("properties")
                .and_then(serde_json::Value::as_object)
                .expect("Parentage schema should expose properties");
            assert!(properties.contains_key("child_id"));
            assert_eq!(properties.contains_key("parent_id"), has_parent_id);
            assert!(properties.contains_key("workspace_root"));
            assert!(!properties.contains_key("issue_id"));
            assert!(!properties.contains_key("depends_on_id"));
        }
    }

    #[test]
    fn test_kind_tool_schemas_publish_only_canonical_field() {
        let server = RivetsMcpServer::new();
        let tools = server.tool_router.list_all();

        for tool_name in ["ready", "list", "create", "update"] {
            let tool = tools
                .iter()
                .find(|tool| tool.name == tool_name)
                .expect("Kind-aware tool should be registered");
            let schema = serde_json::to_string(&tool.input_schema)
                .expect("tool input schema should serialize");

            assert!(
                schema.contains("\"issue_kind\""),
                "{tool_name} schema should publish issue_kind: {schema}"
            );
            assert!(
                !schema.contains("\"issue_type\""),
                "{tool_name} schema should hide migration-only issue_type: {schema}"
            );
        }
    }

    #[test]
    fn generic_dependency_mcp_tool_is_absent() {
        let tool_names = RivetsMcpServer::new()
            .tool_router
            .list_all()
            .into_iter()
            .map(|tool| tool.name.to_string())
            .collect::<std::collections::BTreeSet<_>>();
        assert!(!tool_names.contains("dep"));
        for canonical in [
            "blocking_dependency_add",
            "blocking_dependency_remove",
            "blocking_dependency_list",
            "blocking_dependency_tree",
        ] {
            assert!(tool_names.contains(canonical));
        }
    }

    // =========================================================================
    // Tool dispatch integration tests
    // =========================================================================

    #[tokio::test]
    async fn test_list_without_context_returns_invalid_params() {
        let server = RivetsMcpServer::new();
        let result = server.list(Parameters(ListParams::default())).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        // NoContext should map to invalid_params
        assert!(
            err.message.contains("No workspace context set"),
            "Expected NoContext error, got: {}",
            err.message
        );
    }

    #[tokio::test]
    async fn test_ready_without_context_returns_invalid_params() {
        let server = RivetsMcpServer::new();
        let result = server.ready(Parameters(ReadyParams::default())).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message.contains("No workspace context set"),
            "Expected NoContext error, got: {}",
            err.message
        );
    }

    #[tokio::test]
    async fn test_show_without_context_returns_invalid_params() {
        let server = RivetsMcpServer::new();
        let result = server
            .show(Parameters(ShowParams {
                issue_id: "test-123".to_string(),
                workspace_root: None,
            }))
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message.contains("No workspace context set"),
            "Expected NoContext error, got: {}",
            err.message
        );
    }

    #[tokio::test]
    async fn test_list_with_invalid_status_returns_invalid_params() {
        let server = RivetsMcpServer::new();

        // First set a context to get past NoContext error
        let temp = std::env::temp_dir().join("rivets-test-invalid-status");
        std::fs::create_dir_all(temp.join(".rivets")).ok();
        std::fs::write(temp.join(".rivets/rivets.jsonl"), "").ok();

        let _ = server
            .set_context(Parameters(SetContextParams {
                workspace_root: temp.display().to_string(),
            }))
            .await;

        // Now test with invalid status
        let result = server
            .list(Parameters(ListParams {
                status: Some("invalid_status".to_string()),
                ..Default::default()
            }))
            .await;

        // Cleanup
        std::fs::remove_dir_all(&temp).ok();

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.message.contains("Invalid status"),
            "Expected InvalidArgument error for status, got: {}",
            err.message
        );
        assert!(
            err.message.contains("invalid_status"),
            "Error should contain the invalid value"
        );
    }

    #[test]
    fn test_list_params_reject_invalid_issue_kind() {
        let error = serde_json::from_value::<ListParams>(serde_json::json!({
            "issue_kind": "invalid_kind"
        }))
        .expect_err("invalid Issue Kind should fail at the MCP parameter boundary");

        assert!(error.to_string().contains("invalid_kind"));
    }

    #[tokio::test]
    async fn test_to_mcp_error_maps_correctly() {
        use rmcp::model::ErrorCode;

        // Test NoContext -> invalid_params
        let err = to_mcp_error(&Error::NoContext);
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
        assert!(err.message.contains("No workspace context set"));

        // Test InvalidArgument -> invalid_params
        let err = to_mcp_error(&Error::InvalidArgument {
            field: "status",
            value: "bad".to_string(),
            valid_values: "open, closed",
        });
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
        assert!(err.message.contains("Invalid status"));

        // Test IssueNotFound -> invalid_params
        let err = to_mcp_error(&Error::IssueNotFound("test-123".to_string()));
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
        assert!(err.message.contains("Issue not found: test-123"));
    }

    #[test]
    fn relationship_rejections_are_invalid_params() {
        use rivets::domain::{DiscoveryOriginError, IssueId, RelatedAssociationError};
        use rmcp::model::ErrorCode;

        let errors = [
            Error::InvalidRelatedAssociation(RelatedAssociationError::SelfReference {
                issue_id: IssueId::new("test-a"),
            }),
            Error::InvalidDiscoveryOrigin(DiscoveryOriginError::SelfReference {
                issue_id: IssueId::new("test-a"),
            }),
            Error::RelatedAssociationNotFound {
                left_issue_id: "test-a".to_string(),
                right_issue_id: "test-b".to_string(),
            },
            Error::DuplicateDiscoveryOrigin {
                discovered_issue_id: "test-a".to_string(),
                source_issue_id: "test-b".to_string(),
            },
            Error::DiscoveryOriginNotFound {
                discovered_issue_id: "test-a".to_string(),
                source_issue_id: "test-b".to_string(),
            },
            Error::CircularDiscoveryOrigin {
                discovered_issue_id: "test-a".to_string(),
                source_issue_id: "test-b".to_string(),
            },
        ];

        for error in errors {
            assert_eq!(to_mcp_error(&error).code, ErrorCode::INVALID_PARAMS);
        }
    }

    #[test]
    fn test_to_mcp_error_classifies_rejected_transition_as_invalid_params() {
        use rivets::domain::{IssueStatus, StatusTransitionError};
        use rmcp::model::ErrorCode;

        // A rejected transition is a client-fixable request, not a server
        // fault: the JSON-RPC boundary must say invalid_params (-32602).
        let err = to_mcp_error(&Error::InvalidStatusTransition(
            StatusTransitionError::AlreadyClosed {
                current: IssueStatus::Closed,
            },
        ));
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
        assert!(err.message.contains("Issue is already closed"));

        let err = to_mcp_error(&Error::InvalidStatusTransition(
            StatusTransitionError::NotClosed {
                current: IssueStatus::Open,
            },
        ));
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
        assert!(err.message.contains("Issue is not closed"));

        // Counter-case: non-domain failures stay internal_error.
        let err = to_mcp_error(&Error::WorkspaceNotInitialized("/tmp/x".to_string()));
        assert_eq!(err.code, ErrorCode::INTERNAL_ERROR);
    }
}
