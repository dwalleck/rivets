//! MCP server implementation.
//!
//! This module contains the main server setup using rmcp.

use crate::context::Context;
use crate::error::Error;
use crate::models::{
    AddNoteParams, BlockedParams, BlockingDependencyListParams, BlockingDependencyPairParams,
    BlockingDependencyTreeParams, CloseParams, CreateParams, LabelAddParams, LabelListAllParams,
    LabelListParams, LabelRemoveParams, ListParams, ReadyParams, ReopenParams, ResourceAddParams,
    ResourceListParams, ResourceRemoveParams, ResourceUpdateParams, SetContextParams, ShowParams,
    StaleParams, UpdateParams,
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

/// Maps error types to appropriate MCP error codes:
/// - `NoContext`, `InvalidArgument`, `InvalidNote`, `InvalidResource`,
///   `InvalidStatusTransition` -> `invalid_params` (user needs to fix their request)
/// - `IssueNotFound` -> `invalid_params` (requested resource doesn't exist)
/// - Other errors -> `internal_error`
fn to_mcp_error(e: &Error) -> McpError {
    match e {
        Error::NoContext
        | Error::InvalidArgument { .. }
        | Error::InvalidNote(_)
        | Error::InvalidResource(_)
        | Error::InvalidBlockingDependency(_)
        | Error::InvalidStatusTransition(_)
        | Error::IssueNotFound(_) => McpError::invalid_params(e.to_string(), None),
        _ => McpError::internal_error(e.to_string(), None),
    }
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

    /// Find issues ready to work on.
    #[tool(
        description = "Find tasks that have no blockers and are ready to be worked on. Returns up to 100 results by default if no limit specified. Uses workspace_root if provided, otherwise uses current context."
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
        description = "Update an existing issue's status, priority, kind, assignee, labels, description, design notes, or acceptance criteria. Use empty string for assignee to clear it. Labels replace existing labels when provided. Uses workspace_root if provided, otherwise uses current context."
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
    use rmcp::handler::server::ServerHandler;

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
    fn test_tool_router_has_all_tools() {
        let server = RivetsMcpServer::new();
        // Access the tool_router directly to list tools
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
        assert!(input_properties("blocking_dependency_list").contains_key("query"));
        let tree = input_properties("blocking_dependency_tree");
        assert!(tree.contains_key("dependent_id"));
        assert!(tree.contains_key("depth"));
        let list_tool = tools
            .iter()
            .find(|tool| tool.name == "blocking_dependency_list")
            .expect("Blocking list tool should be registered");
        let list_schema = serde_json::to_string(&list_tool.input_schema).unwrap();
        assert!(list_schema.contains("prerequisites_of"));
        assert!(list_schema.contains("dependents_of"));
        assert_eq!(tools.len(), 24);
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
