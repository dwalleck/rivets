//! MCP server implementation.
//!
//! This module contains the main server setup using rmcp.

use crate::context::Context;
use crate::models::{
    BlockedParams, CloseParams, CreateParams, DepParams, ListParams, ReadyParams, SetContextParams,
    ShowParams, UpdateParams,
};
use crate::tools::Tools;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, Content, Implementation, ProtocolVersion, ServerCapabilities, ServerInfo,
};
use rmcp::{
    handler::server::ServerHandler, tool, tool_handler, tool_router, ErrorData as McpError,
};
use std::sync::Arc;
use tokio::sync::RwLock;

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
        description = "Set the workspace root directory for all subsequent operations. Call this first before using other tools."
    )]
    async fn set_context(
        &self,
        Parameters(params): Parameters<SetContextParams>,
    ) -> Result<CallToolResult, McpError> {
        match self.tools.set_context(&params.workspace_root).await {
            Ok(response) => Ok(CallToolResult::success(vec![Content::json(response)?])),
            Err(e) => Err(McpError::internal_error(e.to_string(), None)),
        }
    }

    /// Get current workspace context information.
    #[tool(description = "Show current workspace context and database path. Useful for debugging.")]
    async fn where_am_i(&self) -> Result<CallToolResult, McpError> {
        match self.tools.where_am_i().await {
            Ok(response) => Ok(CallToolResult::success(vec![Content::json(response)?])),
            Err(e) => Err(McpError::internal_error(e.to_string(), None)),
        }
    }

    /// Find issues ready to work on.
    #[tool(description = "Find tasks that have no blockers and are ready to be worked on.")]
    async fn ready(
        &self,
        Parameters(params): Parameters<ReadyParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .tools
            .ready(
                params.limit,
                params.priority,
                params.assignee,
                params.workspace_root.as_deref(),
            )
            .await
        {
            Ok(issues) => Ok(CallToolResult::success(vec![Content::json(issues)?])),
            Err(e) => Err(McpError::internal_error(e.to_string(), None)),
        }
    }

    /// List issues with optional filters.
    #[tool(
        description = "List all issues with optional filters (status, priority, type, assignee)."
    )]
    async fn list(
        &self,
        Parameters(params): Parameters<ListParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .tools
            .list(
                params.status.as_deref(),
                params.priority,
                params.issue_type.as_deref(),
                params.assignee,
                params.limit,
                params.workspace_root.as_deref(),
            )
            .await
        {
            Ok(issues) => Ok(CallToolResult::success(vec![Content::json(issues)?])),
            Err(e) => Err(McpError::internal_error(e.to_string(), None)),
        }
    }

    /// Show detailed information about a specific issue.
    #[tool(
        description = "Show detailed information about a specific issue including dependencies and dependents."
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
            Err(e) => Err(McpError::internal_error(e.to_string(), None)),
        }
    }

    /// Get blocked issues and their blockers.
    #[tool(
        description = "Get blocked issues showing what dependencies are blocking them from being worked on."
    )]
    async fn blocked(
        &self,
        Parameters(params): Parameters<BlockedParams>,
    ) -> Result<CallToolResult, McpError> {
        match self.tools.blocked(params.workspace_root.as_deref()).await {
            Ok(blocked) => Ok(CallToolResult::success(vec![Content::json(blocked)?])),
            Err(e) => Err(McpError::internal_error(e.to_string(), None)),
        }
    }

    /// Create a new issue.
    #[tool(
        description = "Create a new issue (bug, feature, task, epic, or chore) with optional design, acceptance criteria, and dependencies."
    )]
    async fn create(
        &self,
        Parameters(params): Parameters<CreateParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .tools
            .create(
                params.title,
                params.description,
                params.priority,
                params.issue_type.as_deref(),
                params.assignee,
                params.labels,
                params.design,
                params.acceptance,
                params.workspace_root.as_deref(),
            )
            .await
        {
            Ok(issue) => Ok(CallToolResult::success(vec![Content::json(issue)?])),
            Err(e) => Err(McpError::internal_error(e.to_string(), None)),
        }
    }

    /// Update an existing issue.
    #[tool(
        description = "Update an existing issue's status, priority, assignee, description, design notes, or acceptance criteria."
    )]
    async fn update(
        &self,
        Parameters(params): Parameters<UpdateParams>,
    ) -> Result<CallToolResult, McpError> {
        // Convert assignee to Option<Option<String>> for clearing support
        let assignee = params.assignee.map(Some);

        match self
            .tools
            .update(
                &params.issue_id,
                params.title,
                params.description,
                params.status.as_deref(),
                params.priority,
                assignee,
                params.design,
                params.acceptance_criteria,
                params.notes,
                params.workspace_root.as_deref(),
            )
            .await
        {
            Ok(issue) => Ok(CallToolResult::success(vec![Content::json(issue)?])),
            Err(e) => Err(McpError::internal_error(e.to_string(), None)),
        }
    }

    /// Close an issue.
    #[tool(
        description = "Close (complete) an issue. Mark work as done when you've finished implementing/fixing it."
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
            Err(e) => Err(McpError::internal_error(e.to_string(), None)),
        }
    }

    /// Add a dependency between issues.
    #[tool(
        description = "Add a dependency between issues. Types: blocks (hard blocker), related (soft link), parent-child (epic/subtask), discovered-from (found during work)."
    )]
    async fn dep(
        &self,
        Parameters(params): Parameters<DepParams>,
    ) -> Result<CallToolResult, McpError> {
        match self
            .tools
            .dep(
                &params.issue_id,
                &params.depends_on_id,
                params.dep_type.as_deref(),
                params.workspace_root.as_deref(),
            )
            .await
        {
            Ok(message) => Ok(CallToolResult::success(vec![Content::text(message)])),
            Err(e) => Err(McpError::internal_error(e.to_string(), None)),
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

#[tool_handler]
impl ServerHandler for RivetsMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "rivets-mcp".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                title: None,
                icons: None,
                website_url: None,
            },
            instructions: Some(
                "Rivets MCP server for issue tracking. Call set_context first to set the workspace."
                    .into(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        assert!(tool_names.contains(&"close"));
        assert!(tool_names.contains(&"dep"));
        assert_eq!(tools.len(), 10);
    }
}
