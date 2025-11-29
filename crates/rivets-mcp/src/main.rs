//! Rivets MCP server binary.
//!
//! This binary runs the MCP server using stdio transport.

use rivets_mcp::RivetsMcpServer;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    tracing::info!("Starting rivets-mcp server");

    // Create and run the server
    let server = RivetsMcpServer::new();
    server.run().await?;

    Ok(())
}
