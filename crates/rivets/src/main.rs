//! Rivets - A Rust-based issue tracking system.

#![forbid(unsafe_code)]

mod cli;
mod commands;
mod config;
mod domain;
mod error;
mod storage;

use anyhow::Result;
use tracing_subscriber::EnvFilter;

fn main() -> Result<()> {
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

    let cli = cli::Cli::parse();
    cli.execute()?;

    tracing::debug!("Rivets CLI completed successfully");
    Ok(())
}
