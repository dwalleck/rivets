//! CLI argument parsing and command dispatch.

use anyhow::Result;
use clap::{Parser, Subcommand};

/// Rivets - A Rust-based issue tracking system
#[derive(Parser, Debug)]
#[command(name = "rivets")]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Subcommand to execute
    #[command(subcommand)]
    command: Option<Commands>,
}

/// Available commands
#[derive(Subcommand, Debug)]
enum Commands {
    /// Initialize a new rivets repository
    Init,

    /// Create a new issue
    Create,

    /// List issues
    List,

    /// Show issue details
    Show,

    /// Update an issue
    Update,
}

impl Cli {
    /// Parse CLI arguments
    pub fn parse() -> Self {
        <Self as Parser>::parse()
    }

    /// Execute the CLI command
    pub async fn execute(&self) -> Result<()> {
        match &self.command {
            Some(Commands::Init) => {
                println!("Initializing rivets repository...");
                Ok(())
            }
            Some(Commands::Create) => {
                println!("Creating issue...");
                Ok(())
            }
            Some(Commands::List) => {
                println!("Listing issues...");
                Ok(())
            }
            Some(Commands::Show) => {
                println!("Showing issue...");
                Ok(())
            }
            Some(Commands::Update) => {
                println!("Updating issue...");
                Ok(())
            }
            None => {
                println!("Rivets issue tracking system");
                println!("Use --help for more information");
                Ok(())
            }
        }
    }
}
