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

        /// Use LSP (rust-analyzer) for enhanced reference resolution
        #[arg(long)]
        lsp: bool,

        /// Timeout in seconds for LSP solution loading (default: 60, env: `TETHYS_LSP_TIMEOUT`)
        #[arg(long)]
        lsp_timeout: Option<u64>,
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

        /// Use LSP (rust-analyzer) for enhanced reference resolution
        #[arg(long)]
        lsp: bool,
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

        /// Use LSP (rust-analyzer) for enhanced reference resolution
        #[arg(long)]
        lsp: bool,
    },

    /// Detect circular dependencies
    Cycles,

    /// Show index statistics
    Stats,

    /// Analyze symbol reachability (forward/backward data flow)
    Reachable {
        /// Qualified symbol name (e.g., "`auth::validate`")
        symbol: String,

        /// Direction: 'forward' (what can this reach) or 'backward' (who can reach this)
        #[arg(short, long, default_value = "forward")]
        direction: String,

        /// Maximum depth for traversal
        #[arg(short = 'n', long, default_value = "10")]
        max_depth: usize,
    },

    /// Find tests affected by changes to specified files
    AffectedTests {
        /// Files that have changed (relative or absolute paths)
        files: Vec<String>,

        /// Output only test names (one per line, for CI integration)
        #[arg(long)]
        names_only: bool,
    },

    /// Find potential panic points (`.unwrap()` and `.expect()` calls)
    PanicPoints {
        /// Include panic points in test code
        #[arg(long)]
        include_tests: bool,

        /// Output as JSON
        #[arg(long)]
        json: bool,

        /// Filter to specific file (path relative to workspace root)
        #[arg(long)]
        file: Option<String>,
    },
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
        Commands::Index {
            rebuild,
            lsp,
            lsp_timeout,
        } => cli::index::run(&workspace, rebuild, lsp, lsp_timeout),
        Commands::Search { query, kind, limit } => {
            cli::search::run(&workspace, &query, kind.as_deref(), limit)
        }
        Commands::Callers {
            symbol,
            transitive,
            lsp,
        } => cli::callers::run(&workspace, &symbol, transitive, lsp),
        Commands::Impact {
            target,
            symbol,
            depth,
            lsp,
        } => cli::impact::run(&workspace, &target, symbol, depth, lsp),
        Commands::Cycles => cli::cycles::run(&workspace),
        Commands::Stats => cli::stats::run(&workspace),
        Commands::Reachable {
            symbol,
            direction,
            max_depth,
        } => cli::reachable::run(&workspace, &symbol, &direction, Some(max_depth)),
        Commands::AffectedTests { files, names_only } => {
            cli::affected_tests::run(&workspace, &files, names_only)
        }
        Commands::PanicPoints {
            include_tests,
            json,
            file,
        } => cli::panic_points::run(&workspace, include_tests, json, file.as_deref()),
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
