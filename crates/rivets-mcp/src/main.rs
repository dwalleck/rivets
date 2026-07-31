//! Rivets MCP server binary.
//!
//! This binary runs the MCP server using stdio transport.

use rivets_mcp::RivetsMcpServer;
use rmcp::ServiceExt;
use tokio::io::{stdin, stdout};
use tracing_subscriber::EnvFilter;

const DEFAULT_LOG_FILTER: &str = "error,rivets_mcp=info,rivets=info";

fn env_filter_from(rust_log: Option<&str>) -> EnvFilter {
    match rust_log {
        Some(value) if !value.trim().is_empty() => {
            if let Ok(filter) = EnvFilter::try_new(value) {
                filter
            } else {
                eprintln!(
                    "warning: ignoring malformed RUST_LOG value {value:?}; using default filter"
                );
                EnvFilter::new(DEFAULT_LOG_FILTER)
            }
        }
        Some(_) | None => EnvFilter::new(DEFAULT_LOG_FILTER),
    }
}

fn env_filter() -> EnvFilter {
    let rust_log = std::env::var(EnvFilter::DEFAULT_ENV).ok();
    env_filter_from(rust_log.as_deref())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing to stderr (stdout is used for MCP protocol)
    tracing_subscriber::fmt()
        .with_env_filter(env_filter())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_default_directives(filter: &EnvFilter) {
        let rendered = filter.to_string();
        for expected in ["error", "rivets_mcp=info", "rivets=info"] {
            assert!(
                rendered.split(',').any(|directive| directive == expected),
                "expected {expected} in filter {rendered}"
            );
        }
    }

    #[test]
    fn empty_rust_log_uses_default_filter() {
        assert_default_directives(&env_filter_from(Some("")));
    }

    #[test]
    fn default_filter_preserves_dependency_errors() {
        assert_default_directives(&env_filter_from(None));
        assert_default_directives(&env_filter_from(Some("invalid[")));
    }
}
