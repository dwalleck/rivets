//! Rivets CLI binary.

use anyhow::Result;
use rivets::cli::Cli;
use tracing_subscriber::EnvFilter;

/// Main entry point for the rivets CLI.
///
/// Uses tokio's current_thread runtime for simplicity and lower overhead.
/// This is appropriate for CLI applications with sequential I/O-bound operations.
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    // Initialize tracing subscriber
    // Can be controlled via RUST_LOG environment variable
    // Example: RUST_LOG=rivets=debug,rivets_jsonl=trace cargo run
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("rivets=info,rivets_jsonl=info")),
        )
        .with_target(false)
        .init();

    tracing::debug!("Starting rivets CLI");

    let cli = Cli::parse_args();
    cli.execute().await?;

    tracing::debug!("Rivets CLI completed successfully");
    Ok(())
}
