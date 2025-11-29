//! MCP server implementation.
//!
//! This module contains the main server setup using rmcp.

use crate::context::Context;
use crate::error::Result;
use crate::tools::Tools;
use std::sync::Arc;
use tokio::sync::RwLock;

/// The rivets MCP server.
///
/// Provides MCP protocol handling over stdio transport.
pub struct RivetsMcpServer {
    context: Arc<RwLock<Context>>,
    #[allow(dead_code)]
    tools: Tools,
}

impl RivetsMcpServer {
    /// Create a new rivets MCP server.
    #[must_use]
    pub fn new() -> Self {
        let context = Arc::new(RwLock::new(Context::new()));
        let tools = Tools::new(Arc::clone(&context));

        Self { context, tools }
    }

    /// Run the MCP server.
    ///
    /// This starts the server and listens for MCP protocol messages on stdio.
    ///
    /// # Errors
    ///
    /// Returns an error if the server fails to start or encounters a fatal error.
    pub async fn run(&self) -> Result<()> {
        tracing::info!("Rivets MCP server starting...");

        // TODO: Implement MCP server using rmcp
        // This is a placeholder - the actual implementation will:
        // 1. Set up stdio transport
        // 2. Register all tools with the MCP server
        // 3. Handle incoming requests
        // 4. Route to appropriate tool implementations

        tracing::info!("Rivets MCP server ready");

        // For now, just wait indefinitely
        // In the actual implementation, this will be replaced with the rmcp event loop
        tokio::signal::ctrl_c()
            .await
            .map_err(|e| crate::error::Error::Io(std::io::Error::other(e)))?;

        tracing::info!("Rivets MCP server shutting down");
        Ok(())
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
