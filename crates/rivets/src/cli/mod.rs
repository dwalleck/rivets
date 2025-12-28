//! CLI argument parsing and command dispatch.
//!
//! This module provides the command-line interface for rivets using clap's derive API.
//! Each command has its own argument struct with validation and helpful error messages.
//!
//! # Commands
//!
//! - `init`: Initialize a new rivets repository
//! - `create`: Create a new issue
//! - `list`: List issues with optional filters
//! - `show`: Show issue details
//! - `update`: Update an existing issue
//! - `close`: Close an issue
//! - `delete`: Delete an issue
//! - `ready`: Show ready-to-work issues
//!
//! # Global Flags
//!
//! - `--json`: Output in JSON format (applies to all commands)
//!
//! # Example
//!
//! ```bash
//! rivets create --title "Fix bug" --priority 1 --type bug
//! rivets list --status open --priority 1
//! rivets update proj-abc --status in_progress
//! rivets close proj-abc --reason "Fixed in PR #123"
//! ```

mod args;
mod execute;
mod types;
mod validators;

use anyhow::Result;
use clap::{Parser, Subcommand};

// Re-export argument structs
pub use args::{
    BlockedArgs, CloseArgs, CreateArgs, DeleteArgs, DepAction, DepArgs, InfoArgs, InitArgs,
    LabelAction, LabelArgs, ListArgs, ReadyArgs, ReopenArgs, ShowArgs, StaleArgs, StatsArgs,
    UpdateArgs,
};

// Re-export types
pub use types::{
    BatchError, BatchResult, DependencyTypeArg, IssueStatusArg, IssueTypeArg, SortOrderArg,
    SortPolicyArg,
};

// Re-export validators for external use
pub use validators::{validate_description, validate_issue_id, validate_prefix, validate_title};

/// Rivets - A Rust-based issue tracking system
///
/// Track issues, dependencies, and project progress using JSONL storage.
/// Issues are stored in `.rivets/issues.jsonl` for easy version control integration.
#[derive(Parser, Debug)]
#[command(name = "rivets")]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
pub struct Cli {
    /// Output in JSON format for programmatic use
    #[arg(long, global = true)]
    pub json: bool,

    /// Subcommand to execute
    #[command(subcommand)]
    pub command: Option<Commands>,
}

/// Available commands
#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    /// Initialize a new rivets repository
    ///
    /// Creates the `.rivets/` directory with configuration and empty issue database.
    /// Run this once in your project root to start tracking issues.
    Init(InitArgs),

    /// Show repository information
    ///
    /// Displays database path, issue prefix, and summary statistics.
    Info(InfoArgs),

    /// Create a new issue
    ///
    /// Creates a new issue with the given properties. If title is not provided,
    /// an interactive prompt will be shown.
    Create(CreateArgs),

    /// List issues with optional filters
    ///
    /// Shows all issues matching the filter criteria. By default, shows all
    /// non-closed issues sorted by priority and creation date.
    List(ListArgs),

    /// Show detailed information about an issue
    ///
    /// Displays all fields of an issue including dependencies, design notes,
    /// and acceptance criteria.
    Show(ShowArgs),

    /// Update an existing issue
    ///
    /// Modifies one or more fields of an existing issue. Only provided fields
    /// are updated; other fields remain unchanged.
    Update(UpdateArgs),

    /// Close an issue
    ///
    /// Marks an issue as completed. Optionally provide a reason for closing.
    Close(CloseArgs),

    /// Reopen a closed issue
    ///
    /// Changes a closed issue's status back to open. Optionally provide a reason.
    Reopen(ReopenArgs),

    /// Delete an issue permanently
    ///
    /// Removes an issue from the database. This cannot be undone.
    /// Use `--force` to skip confirmation.
    Delete(DeleteArgs),

    /// Show issues ready to work on
    ///
    /// Lists issues that are not blocked by dependencies. Issues are sorted
    /// by priority (hybrid by default) to help you pick what to work on next.
    Ready(ReadyArgs),

    /// Add a dependency between issues
    ///
    /// Creates a dependency relationship where one issue depends on another.
    Dep(DepArgs),

    /// Manage issue labels
    ///
    /// Add, remove, or list labels on issues.
    Label(LabelArgs),

    /// Find stale issues
    ///
    /// Lists issues that haven't been updated in a specified number of days.
    Stale(StaleArgs),

    /// Show blocked issues
    ///
    /// Lists issues that are blocked by dependencies, along with their blockers.
    Blocked(BlockedArgs),

    /// Show project statistics
    ///
    /// Displays summary statistics about issues, completion rates, and trends.
    Stats(StatsArgs),
}

impl Cli {
    /// Parse CLI arguments from command line
    pub fn parse_args() -> Self {
        <Self as Parser>::parse()
    }

    /// Parse CLI arguments from an iterator (for testing)
    pub fn try_parse_from<I, T>(iter: I) -> std::result::Result<Self, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<std::ffi::OsString> + Clone,
    {
        <Self as Parser>::try_parse_from(iter)
    }

    /// Execute the CLI command
    pub async fn execute(&self) -> Result<()> {
        use crate::app::App;
        use crate::output::OutputMode;

        let output_mode = if self.json {
            OutputMode::Json
        } else {
            OutputMode::Text
        };

        match &self.command {
            Some(Commands::Init(args)) => execute::execute_init(args).await,
            Some(Commands::Info(args)) => {
                let app = App::from_directory(&std::env::current_dir()?).await?;
                execute::execute_info(&app, args, output_mode).await
            }
            Some(Commands::Create(args)) => {
                let mut app = App::from_directory(&std::env::current_dir()?).await?;
                execute::execute_create(&mut app, args, output_mode).await
            }
            Some(Commands::List(args)) => {
                let app = App::from_directory(&std::env::current_dir()?).await?;
                execute::execute_list(&app, args, output_mode).await
            }
            Some(Commands::Show(args)) => {
                let app = App::from_directory(&std::env::current_dir()?).await?;
                execute::execute_show(&app, args, output_mode).await
            }
            Some(Commands::Update(args)) => {
                let mut app = App::from_directory(&std::env::current_dir()?).await?;
                execute::execute_update(&mut app, args, output_mode).await
            }
            Some(Commands::Close(args)) => {
                let mut app = App::from_directory(&std::env::current_dir()?).await?;
                execute::execute_close(&mut app, args, output_mode).await
            }
            Some(Commands::Reopen(args)) => {
                let mut app = App::from_directory(&std::env::current_dir()?).await?;
                execute::execute_reopen(&mut app, args, output_mode).await
            }
            Some(Commands::Delete(args)) => {
                let mut app = App::from_directory(&std::env::current_dir()?).await?;
                execute::execute_delete(&mut app, args, output_mode).await
            }
            Some(Commands::Ready(args)) => {
                let app = App::from_directory(&std::env::current_dir()?).await?;
                execute::execute_ready(&app, args, output_mode).await
            }
            Some(Commands::Dep(args)) => {
                let mut app = App::from_directory(&std::env::current_dir()?).await?;
                execute::execute_dep(&mut app, args, output_mode).await
            }
            Some(Commands::Label(args)) => {
                let mut app = App::from_directory(&std::env::current_dir()?).await?;
                execute::execute_label(&mut app, args, output_mode).await
            }
            Some(Commands::Stale(args)) => {
                let app = App::from_directory(&std::env::current_dir()?).await?;
                execute::execute_stale(&app, args, output_mode).await
            }
            Some(Commands::Blocked(args)) => {
                let app = App::from_directory(&std::env::current_dir()?).await?;
                execute::execute_blocked(&app, args, output_mode).await
            }
            Some(Commands::Stats(args)) => {
                let app = App::from_directory(&std::env::current_dir()?).await?;
                execute::execute_stats(&app, args, output_mode).await
            }
            None => {
                println!("Rivets issue tracking system");
                println!("Use --help for more information");
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========== CLI Parsing Tests ==========

    #[test]
    fn test_parse_no_command() {
        let cli = Cli::try_parse_from(["rivets"]).unwrap();
        assert!(cli.command.is_none());
        assert!(!cli.json);
    }

    #[test]
    fn test_parse_global_json_flag() {
        let cli = Cli::try_parse_from(["rivets", "--json", "list"]).unwrap();
        assert!(cli.json);
        assert!(matches!(cli.command, Some(Commands::List(_))));
    }

    #[test]
    fn test_parse_init_default() {
        let cli = Cli::try_parse_from(["rivets", "init"]).unwrap();
        match cli.command {
            Some(Commands::Init(args)) => {
                assert!(args.prefix.is_none());
                assert!(!args.quiet);
            }
            _ => panic!("Expected Init command"),
        }
    }

    #[test]
    fn test_parse_init_with_prefix() {
        let cli = Cli::try_parse_from(["rivets", "init", "--prefix", "myproj"]).unwrap();
        match cli.command {
            Some(Commands::Init(args)) => {
                assert_eq!(args.prefix, Some("myproj".to_string()));
            }
            _ => panic!("Expected Init command"),
        }
    }

    #[test]
    fn test_parse_init_quiet() {
        let cli = Cli::try_parse_from(["rivets", "init", "-q"]).unwrap();
        match cli.command {
            Some(Commands::Init(args)) => {
                assert!(args.quiet);
            }
            _ => panic!("Expected Init command"),
        }
    }

    #[test]
    fn test_parse_info() {
        let cli = Cli::try_parse_from(["rivets", "info"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::Info(_))));
    }

    #[test]
    fn test_parse_info_with_json() {
        let cli = Cli::try_parse_from(["rivets", "--json", "info"]).unwrap();
        assert!(cli.json);
        assert!(matches!(cli.command, Some(Commands::Info(_))));
    }

    #[test]
    fn test_parse_create_minimal() {
        let cli = Cli::try_parse_from(["rivets", "create"]).unwrap();
        match cli.command {
            Some(Commands::Create(args)) => {
                assert!(args.title.is_none());
                assert_eq!(args.priority, 2); // default
                assert_eq!(args.issue_type, IssueTypeArg::Task); // default
            }
            _ => panic!("Expected Create command"),
        }
    }

    #[test]
    fn test_parse_create_full() {
        let cli = Cli::try_parse_from([
            "rivets",
            "create",
            "--title",
            "Fix bug",
            "--description",
            "Detailed desc",
            "--priority",
            "1",
            "--type",
            "bug",
            "--assignee",
            "alice",
            "--labels",
            "urgent,backend",
        ])
        .unwrap();

        match cli.command {
            Some(Commands::Create(args)) => {
                assert_eq!(args.title, Some("Fix bug".to_string()));
                assert_eq!(args.description, Some("Detailed desc".to_string()));
                assert_eq!(args.priority, 1);
                assert_eq!(args.issue_type, IssueTypeArg::Bug);
                assert_eq!(args.assignee, Some("alice".to_string()));
                assert_eq!(args.labels, vec!["urgent", "backend"]);
            }
            _ => panic!("Expected Create command"),
        }
    }

    #[test]
    fn test_parse_create_invalid_priority() {
        let result = Cli::try_parse_from(["rivets", "create", "--priority", "5"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_list_default() {
        let cli = Cli::try_parse_from(["rivets", "list"]).unwrap();
        match cli.command {
            Some(Commands::List(args)) => {
                assert!(args.status.is_none());
                assert!(args.priority.is_none());
                assert_eq!(args.limit, 50); // default
                assert_eq!(args.sort, SortOrderArg::Priority); // default
            }
            _ => panic!("Expected List command"),
        }
    }

    #[test]
    fn test_parse_list_with_filters() {
        let cli = Cli::try_parse_from([
            "rivets",
            "list",
            "--status",
            "open",
            "--priority",
            "1",
            "--type",
            "bug",
            "--assignee",
            "bob",
            "--limit",
            "10",
        ])
        .unwrap();

        match cli.command {
            Some(Commands::List(args)) => {
                assert_eq!(args.status, Some(IssueStatusArg::Open));
                assert_eq!(args.priority, Some(1));
                assert_eq!(args.issue_type, Some(IssueTypeArg::Bug));
                assert_eq!(args.assignee, Some("bob".to_string()));
                assert_eq!(args.limit, 10);
            }
            _ => panic!("Expected List command"),
        }
    }

    #[test]
    fn test_parse_list_status_in_progress() {
        let cli = Cli::try_parse_from(["rivets", "list", "--status", "in_progress"]).unwrap();
        match cli.command {
            Some(Commands::List(args)) => {
                assert_eq!(args.status, Some(IssueStatusArg::InProgress));
            }
            _ => panic!("Expected List command"),
        }
    }

    #[test]
    fn test_parse_list_status_in_progress_alias() {
        let cli = Cli::try_parse_from(["rivets", "list", "--status", "in-progress"]).unwrap();
        match cli.command {
            Some(Commands::List(args)) => {
                assert_eq!(args.status, Some(IssueStatusArg::InProgress));
            }
            _ => panic!("Expected List command"),
        }
    }

    #[test]
    fn test_parse_show() {
        let cli = Cli::try_parse_from(["rivets", "show", "proj-abc"]).unwrap();
        match cli.command {
            Some(Commands::Show(args)) => {
                assert_eq!(args.issue_ids, vec!["proj-abc"]);
            }
            _ => panic!("Expected Show command"),
        }
    }

    #[test]
    fn test_parse_show_multiple_ids() {
        let cli =
            Cli::try_parse_from(["rivets", "show", "proj-abc", "proj-def", "proj-ghi"]).unwrap();
        match cli.command {
            Some(Commands::Show(args)) => {
                assert_eq!(args.issue_ids, vec!["proj-abc", "proj-def", "proj-ghi"]);
            }
            _ => panic!("Expected Show command"),
        }
    }

    #[test]
    fn test_parse_show_invalid_id() {
        let result = Cli::try_parse_from(["rivets", "show", "invalid"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_update() {
        let cli = Cli::try_parse_from([
            "rivets",
            "update",
            "proj-abc",
            "--title",
            "New title",
            "--status",
            "in_progress",
            "--priority",
            "0",
        ])
        .unwrap();

        match cli.command {
            Some(Commands::Update(args)) => {
                assert_eq!(args.issue_ids, vec!["proj-abc"]);
                assert_eq!(args.title, Some("New title".to_string()));
                assert_eq!(args.status, Some(IssueStatusArg::InProgress));
                assert_eq!(args.priority, Some(0));
            }
            _ => panic!("Expected Update command"),
        }
    }

    #[test]
    fn test_parse_update_multiple_ids() {
        let cli = Cli::try_parse_from([
            "rivets",
            "update",
            "proj-abc",
            "proj-def",
            "--status",
            "in_progress",
        ])
        .unwrap();

        match cli.command {
            Some(Commands::Update(args)) => {
                assert_eq!(args.issue_ids, vec!["proj-abc", "proj-def"]);
                assert_eq!(args.status, Some(IssueStatusArg::InProgress));
            }
            _ => panic!("Expected Update command"),
        }
    }

    #[test]
    fn test_parse_close() {
        let cli = Cli::try_parse_from(["rivets", "close", "proj-abc"]).unwrap();
        match cli.command {
            Some(Commands::Close(args)) => {
                assert_eq!(args.issue_ids, vec!["proj-abc"]);
                assert_eq!(args.reason, "Completed"); // default
            }
            _ => panic!("Expected Close command"),
        }
    }

    #[test]
    fn test_parse_close_multiple_ids() {
        let cli = Cli::try_parse_from([
            "rivets",
            "close",
            "proj-abc",
            "proj-def",
            "--reason",
            "Batch done",
        ])
        .unwrap();
        match cli.command {
            Some(Commands::Close(args)) => {
                assert_eq!(args.issue_ids, vec!["proj-abc", "proj-def"]);
                assert_eq!(args.reason, "Batch done");
            }
            _ => panic!("Expected Close command"),
        }
    }

    #[test]
    fn test_parse_close_with_reason() {
        let cli =
            Cli::try_parse_from(["rivets", "close", "proj-abc", "--reason", "Fixed in PR #42"])
                .unwrap();
        match cli.command {
            Some(Commands::Close(args)) => {
                assert_eq!(args.reason, "Fixed in PR #42");
            }
            _ => panic!("Expected Close command"),
        }
    }

    #[test]
    fn test_parse_reopen() {
        let cli = Cli::try_parse_from(["rivets", "reopen", "proj-abc"]).unwrap();
        match cli.command {
            Some(Commands::Reopen(args)) => {
                assert_eq!(args.issue_ids, vec!["proj-abc"]);
                assert!(args.reason.is_none());
            }
            _ => panic!("Expected Reopen command"),
        }
    }

    #[test]
    fn test_parse_reopen_multiple_ids() {
        let cli = Cli::try_parse_from(["rivets", "reopen", "proj-abc", "proj-def"]).unwrap();
        match cli.command {
            Some(Commands::Reopen(args)) => {
                assert_eq!(args.issue_ids, vec!["proj-abc", "proj-def"]);
            }
            _ => panic!("Expected Reopen command"),
        }
    }

    #[test]
    fn test_parse_reopen_with_reason() {
        let cli = Cli::try_parse_from([
            "rivets",
            "reopen",
            "proj-abc",
            "--reason",
            "Needs more work",
        ])
        .unwrap();
        match cli.command {
            Some(Commands::Reopen(args)) => {
                assert_eq!(args.issue_ids, vec!["proj-abc"]);
                assert_eq!(args.reason, Some("Needs more work".to_string()));
            }
            _ => panic!("Expected Reopen command"),
        }
    }

    #[test]
    fn test_parse_delete() {
        let cli = Cli::try_parse_from(["rivets", "delete", "proj-abc"]).unwrap();
        match cli.command {
            Some(Commands::Delete(args)) => {
                assert_eq!(args.issue_id, "proj-abc");
                assert!(!args.force);
            }
            _ => panic!("Expected Delete command"),
        }
    }

    #[test]
    fn test_parse_delete_force() {
        let cli = Cli::try_parse_from(["rivets", "delete", "proj-abc", "--force"]).unwrap();
        match cli.command {
            Some(Commands::Delete(args)) => {
                assert!(args.force);
            }
            _ => panic!("Expected Delete command"),
        }
    }

    #[test]
    fn test_parse_ready_default() {
        let cli = Cli::try_parse_from(["rivets", "ready"]).unwrap();
        match cli.command {
            Some(Commands::Ready(args)) => {
                assert!(args.assignee.is_none());
                assert_eq!(args.limit, 10); // default
                assert_eq!(args.sort, SortPolicyArg::Hybrid); // default
            }
            _ => panic!("Expected Ready command"),
        }
    }

    #[test]
    fn test_parse_ready_with_options() {
        let cli = Cli::try_parse_from([
            "rivets",
            "ready",
            "--assignee",
            "alice",
            "--limit",
            "5",
            "--sort",
            "priority",
        ])
        .unwrap();

        match cli.command {
            Some(Commands::Ready(args)) => {
                assert_eq!(args.assignee, Some("alice".to_string()));
                assert_eq!(args.limit, 5);
                assert_eq!(args.sort, SortPolicyArg::Priority);
            }
            _ => panic!("Expected Ready command"),
        }
    }

    #[test]
    fn test_parse_dep_add() {
        let cli = Cli::try_parse_from([
            "rivets", "dep", "add", "proj-abc", "proj-xyz", "-t", "blocks",
        ])
        .unwrap();

        match cli.command {
            Some(Commands::Dep(args)) => match args.action {
                DepAction::Add { from, to, dep_type } => {
                    assert_eq!(from, "proj-abc");
                    assert_eq!(to, "proj-xyz");
                    assert_eq!(dep_type, DependencyTypeArg::Blocks);
                }
                _ => panic!("Expected Add action"),
            },
            _ => panic!("Expected Dep command"),
        }
    }

    #[test]
    fn test_parse_dep_remove() {
        let cli = Cli::try_parse_from(["rivets", "dep", "remove", "proj-abc", "proj-xyz"]).unwrap();

        match cli.command {
            Some(Commands::Dep(args)) => match args.action {
                DepAction::Remove { from, to } => {
                    assert_eq!(from, "proj-abc");
                    assert_eq!(to, "proj-xyz");
                }
                _ => panic!("Expected Remove action"),
            },
            _ => panic!("Expected Dep command"),
        }
    }

    #[test]
    fn test_parse_dep_list() {
        let cli = Cli::try_parse_from(["rivets", "dep", "list", "proj-abc", "--reverse"]).unwrap();

        match cli.command {
            Some(Commands::Dep(args)) => match args.action {
                DepAction::List { issue_id, reverse } => {
                    assert_eq!(issue_id, "proj-abc");
                    assert!(reverse);
                }
                _ => panic!("Expected List action"),
            },
            _ => panic!("Expected Dep command"),
        }
    }

    #[test]
    fn test_parse_blocked() {
        let cli = Cli::try_parse_from(["rivets", "blocked"]).unwrap();
        assert!(matches!(cli.command, Some(Commands::Blocked(_))));
    }

    #[test]
    fn test_parse_stats() {
        let cli = Cli::try_parse_from(["rivets", "stats", "--detailed"]).unwrap();
        match cli.command {
            Some(Commands::Stats(args)) => {
                assert!(args.detailed);
            }
            _ => panic!("Expected Stats command"),
        }
    }
}
