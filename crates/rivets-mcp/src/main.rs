//! Rivets MCP server binary.
//!
//! This binary runs the MCP server using stdio transport.

use rivets_mcp::RivetsMcpServer;
use rmcp::ServiceExt;
use tokio::io::{stdin, stdout};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing to stderr (stdout is used for MCP protocol)
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    tracing::info!("Starting rivets-mcp server");

    // Create the server
    let server = RivetsMcpServer::new();

    // Serve over stdio transport
    let service = server.serve((stdin(), stdout())).await?;

    tracing::info!("Rivets MCP server ready");

    // Wait for the service to complete (e.g., client disconnect or shutdown)
    service.waiting().await?;

    tracing::info!("Rivets MCP server stopped");
    Ok(())
}
