//! Tethys CLI - Code intelligence from the command line.
//!
//! Tethys indexes source files with tree-sitter and provides fast queries
//! for symbols, references, and dependency analysis.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use colored::Colorize;
use tracing_subscriber::EnvFilter;

mod cli;

/// Tethys: Code intelligence cache and query interface.
#[derive(Parser)]
#[command(name = "tethys")]
#[command(version, about, long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    /// Workspace root directory (defaults to current directory)
    #[arg(short, long, global = true)]
    workspace: Option<PathBuf>,

    /// Verbose output (can be repeated: -v, -vv, -vvv)
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Index source files in the workspace
    Index {
        /// Rebuild index from scratch (clears existing data)
        #[arg(long)]
        rebuild: bool,
    },

    /// Search for symbols by name
    Search {
        /// Search query (supports partial matching)
        query: String,

        /// Filter by symbol kind (function, method, struct, class, enum, trait, interface)
        #[arg(short, long)]
        kind: Option<String>,

        /// Maximum number of results
        #[arg(short, long, default_value = "20")]
        limit: usize,
    },

    /// Show callers of a symbol
    Callers {
        /// Qualified symbol name (e.g., "`AuthService::authenticate`")
        symbol: String,

        /// Include transitive callers (callers of callers)
        #[arg(short, long)]
        transitive: bool,
    },

    /// Analyze impact of changes to a file or symbol
    Impact {
        /// File path or symbol name (with --symbol flag)
        target: String,

        /// Analyze impact of a symbol instead of a file
        #[arg(short, long)]
        symbol: bool,

        /// Maximum depth for transitive analysis (not yet implemented)
        #[arg(short, long)]
        depth: Option<u32>,
    },

    /// Detect circular dependencies
    Cycles,

    /// Show index statistics
    Stats,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    // Set up logging based on verbosity
    let filter = match cli.verbose {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(filter)),
        )
        .with_target(false)
        .init();

    // Determine workspace root
    let workspace = match cli.workspace {
        Some(w) => w,
        None => match std::env::current_dir() {
            Ok(dir) => dir,
            Err(e) => {
                eprintln!(
                    "{}: failed to get current directory: {e}",
                    "error".red().bold()
                );
                return ExitCode::FAILURE;
            }
        },
    };

    // Run the appropriate command
    let result = match cli.command {
        Commands::Index { rebuild } => cli::index::run(&workspace, rebuild),
        Commands::Search { query, kind, limit } => {
            cli::search::run(&workspace, &query, kind.as_deref(), limit)
        }
        Commands::Callers { symbol, transitive } => {
            cli::callers::run(&workspace, &symbol, transitive)
        }
        Commands::Impact {
            target,
            symbol,
            depth,
        } => cli::impact::run(&workspace, &target, symbol, depth),
        Commands::Cycles => cli::cycles::run(&workspace),
        Commands::Stats => cli::stats::run(&workspace),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{}: {e}", "error".red().bold());
            // Show cause chain for nested errors
            let mut source = std::error::Error::source(&e);
            while let Some(cause) = source {
                eprintln!("  {}: {cause}", "caused by".dimmed());
                source = std::error::Error::source(cause);
            }
            ExitCode::FAILURE
        }
    }
}
