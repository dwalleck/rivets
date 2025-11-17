//! Rivets - A Rust-based issue tracking system.

#![forbid(unsafe_code)]

mod cli;
mod commands;
mod config;
mod domain;
mod error;
mod storage;

use anyhow::Result;

fn main() -> Result<()> {
    let cli = cli::Cli::parse();
    cli.execute()?;
    Ok(())
}
