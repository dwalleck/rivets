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
//! - `--yes` / `-y`: Skip confirmation prompts (for scripting)
//!
//! # Example
//!
//! ```bash
//! rivets create --title "Fix bug" --priority 1 --kind bug
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

use crate::app::App;

// Re-export argument structs
pub use args::{
    AssignmentArgs, BlockedArgs, BlockingDependencyAction, BlockingDependencyArgs,
    BlockingDependencyListArgs, CloseArgs, CreateArgs, DeleteArgs, DiscoveryAction, DiscoveryArgs,
    InfoArgs, InitArgs, LabelAction, LabelArgs, ListArgs, ParentAction, ParentArgs, ReadyArgs,
    RelatedAction, RelatedArgs, ReopenArgs, ResourceAction, ResourceArgs, ShowArgs, StaleArgs,
    StatsArgs, UpdateArgs,
};

// Re-export types
pub use types::{BatchError, BatchResult, SortOrderArg, SortPolicyArg};

// Re-export validators for external use
pub use validators::{
    validate_assignee, validate_description, validate_issue_id, validate_prefix, validate_title,
};

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

    /// Skip confirmation prompts (for scripting)
    #[arg(short = 'y', long, global = true)]
    pub yes: bool,

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
    /// Claim an Open, unblocked Issue for one Assignee.
    Claim(AssignmentArgs),

    /// Release an Open Issue owned by the exact Assignee.
    Release(AssignmentArgs),

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

    /// Show Open, unblocked Issues matching Assignment visibility
    ///
    /// Defaults to unassigned Issues. Use `--assignee` for one assignee or
    /// `--all-assignees` to include every Assignment.
    Ready(ReadyArgs),

    /// Manage directed Blocking Dependencies with explicit endpoint roles.
    BlockingDependency(BlockingDependencyArgs),

    /// Manage symmetric, non-blocking Related Associations.
    Related(RelatedArgs),

    /// Manage directed, non-blocking Discovery Origins.
    Discovery(DiscoveryArgs),

    /// Manage single-Epic Parentage with explicit child and parent roles.
    Parent(ParentArgs),

    /// Manage issue labels
    ///
    /// Add, remove, or list labels on issues.
    Label(LabelArgs),

    /// Manage Associated Resources.
    ///
    /// Add and list typed references to relevant information and artifacts.
    Resource(ResourceArgs),

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

impl Commands {
    const fn mutates_workspace(&self) -> bool {
        match self {
            Self::Create(_)
            | Self::Update(_)
            | Self::Claim(_)
            | Self::Release(_)
            | Self::Close(_)
            | Self::Reopen(_)
            | Self::Delete(_) => true,
            Self::BlockingDependency(args) => args.action.mutates_workspace(),
            Self::Related(args) => args.action.mutates_workspace(),
            Self::Discovery(args) => args.action.mutates_workspace(),
            Self::Parent(args) => matches!(
                args.action,
                ParentAction::Set { .. } | ParentAction::Clear { .. } | ParentAction::Move { .. }
            ),
            Self::Label(args) => args.action.mutates_workspace(),
            Self::Resource(args) => args.action.mutates_workspace(),
            Self::Init(_)
            | Self::Info(_)
            | Self::List(_)
            | Self::Show(_)
            | Self::Ready(_)
            | Self::Stale(_)
            | Self::Blocked(_)
            | Self::Stats(_) => false,
        }
    }
}

/// Load the App from the current working directory.
///
/// This helper centralizes the common pattern of initializing the App
/// from `std::env::current_dir()`, reducing duplication in command handlers.
async fn load_app_from_cwd(for_mutation: bool) -> Result<App> {
    let current_dir = std::env::current_dir()?;
    let app = if for_mutation {
        App::from_directory_for_mutation(&current_dir).await?
    } else {
        App::from_directory(&current_dir).await?
    };
    Ok(app)
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
        use crate::output::OutputMode;

        let output_mode = if self.json {
            OutputMode::Json
        } else {
            OutputMode::Text
        };

        let mutates_workspace = self
            .command
            .as_ref()
            .is_some_and(Commands::mutates_workspace);

        match &self.command {
            Some(Commands::Init(args)) => execute::execute_init(args).await,
            Some(Commands::Info(args)) => {
                let app = load_app_from_cwd(mutates_workspace).await?;
                execute::execute_info(&app, args, output_mode).await
            }
            Some(Commands::Create(args)) => {
                let title = execute::resolve_create_title(args)?;
                let mut app = load_app_from_cwd(mutates_workspace).await?;
                execute::execute_create(&mut app, args, title, output_mode).await
            }
            Some(Commands::List(args)) => {
                let app = load_app_from_cwd(mutates_workspace).await?;
                execute::execute_list(&app, args, output_mode).await
            }
            Some(Commands::Show(args)) => {
                let app = load_app_from_cwd(mutates_workspace).await?;
                execute::execute_show(&app, args, output_mode).await
            }
            Some(Commands::Update(args)) => {
                let mut app = load_app_from_cwd(mutates_workspace).await?;
                execute::execute_update(&mut app, args, output_mode).await
            }
            Some(Commands::Claim(args)) => {
                let mut app = load_app_from_cwd(mutates_workspace).await?;
                execute::execute_claim(&mut app, args, output_mode).await
            }
            Some(Commands::Release(args)) => {
                let mut app = load_app_from_cwd(mutates_workspace).await?;
                execute::execute_release(&mut app, args, output_mode).await
            }
            Some(Commands::Close(args)) => {
                if !execute::confirm_batch("Close", args.issue_ids.len(), self.yes)? {
                    return Ok(());
                }
                let mut app = load_app_from_cwd(mutates_workspace).await?;
                execute::execute_close(&mut app, args, output_mode).await
            }
            Some(Commands::Reopen(args)) => {
                if !execute::confirm_batch("Reopen", args.issue_ids.len(), self.yes)? {
                    return Ok(());
                }
                let mut app = load_app_from_cwd(mutates_workspace).await?;
                execute::execute_reopen(&mut app, args, output_mode).await
            }
            Some(Commands::Delete(args)) => {
                if !args.force && !self.yes {
                    let read_app = load_app_from_cwd(false).await?;
                    if !execute::confirm_delete(&read_app, args, false).await? {
                        return Ok(());
                    }
                }
                let mut app = load_app_from_cwd(mutates_workspace).await?;
                execute::execute_delete(&mut app, args, output_mode).await
            }
            Some(Commands::Ready(args)) => {
                let app = load_app_from_cwd(mutates_workspace).await?;
                execute::execute_ready(&app, args, output_mode).await
            }
            Some(Commands::BlockingDependency(args)) => {
                let mut app = load_app_from_cwd(mutates_workspace).await?;
                execute::execute_blocking_dependency(&mut app, args, output_mode).await
            }
            Some(Commands::Related(args)) => {
                let mut app = load_app_from_cwd(mutates_workspace).await?;
                execute::execute_related(&mut app, args, output_mode).await
            }
            Some(Commands::Discovery(args)) => {
                let mut app = load_app_from_cwd(mutates_workspace).await?;
                execute::execute_discovery(&mut app, args, output_mode).await
            }
            Some(Commands::Parent(args)) => {
                let mut app = load_app_from_cwd(mutates_workspace).await?;
                execute::execute_parent(&mut app, args, output_mode).await
            }
            Some(Commands::Label(args)) => {
                let mut app = load_app_from_cwd(mutates_workspace).await?;
                execute::execute_label(&mut app, args, output_mode).await
            }
            Some(Commands::Resource(args)) => {
                let mut app = load_app_from_cwd(mutates_workspace).await?;
                execute::execute_resource(&mut app, args, output_mode).await
            }
            Some(Commands::Stale(args)) => {
                let app = load_app_from_cwd(mutates_workspace).await?;
                execute::execute_stale(&app, args, output_mode).await
            }
            Some(Commands::Blocked(args)) => {
                let app = load_app_from_cwd(mutates_workspace).await?;
                execute::execute_blocked(&app, args, output_mode).await
            }
            Some(Commands::Stats(args)) => {
                let app = load_app_from_cwd(mutates_workspace).await?;
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
    use crate::domain::{IssueKind, IssueStatus};

    fn parses_as_mutation(args: &[&str]) -> bool {
        let argv: Vec<_> = std::iter::once("rivets")
            .chain(args.iter().copied())
            .collect();
        Cli::try_parse_from(argv)
            .expect("classification fixture should parse")
            .command
            .as_ref()
            .is_some_and(Commands::mutates_workspace)
    }

    #[test]
    fn workspace_mutation_lock_classification_is_exhaustive() {
        for args in [
            &["create", "--title", "Issue"][..],
            &["update", "test-abc", "--title", "Updated"],
            &["close", "test-abc"],
            &["reopen", "test-abc"],
            &["delete", "test-abc", "--force"],
            &["claim", "test-abc", "--assignee", "alice"],
            &["release", "test-abc", "--assignee", "alice"],
            &[
                "blocking-dependency",
                "add",
                "--dependent",
                "test-abc",
                "--prerequisite",
                "test-def",
            ],
            &[
                "blocking-dependency",
                "remove",
                "--dependent",
                "test-abc",
                "--prerequisite",
                "test-def",
            ],
            &[
                "related",
                "add",
                "--issue",
                "test-abc",
                "--related",
                "test-def",
            ],
            &[
                "related",
                "remove",
                "--issue",
                "test-abc",
                "--related",
                "test-def",
            ],
            &[
                "discovery",
                "add",
                "--discovered",
                "test-abc",
                "--source",
                "test-def",
            ],
            &[
                "discovery",
                "remove",
                "--discovered",
                "test-abc",
                "--source",
                "test-def",
            ],
            &[
                "parent", "set", "--child", "test-abc", "--parent", "test-def",
            ],
            &["parent", "clear", "--child", "test-abc"],
            &[
                "parent", "move", "--child", "test-abc", "--parent", "test-def",
            ],
            &["label", "add", "urgent", "test-abc"],
            &["label", "remove", "urgent", "test-abc"],
            &[
                "resource",
                "add",
                "test-abc",
                "--url",
                "https://example.com",
                "--role",
                "reference",
            ],
            &[
                "resource",
                "update",
                "test-abc",
                "--resource",
                "r1",
                "--label",
                "Updated",
            ],
            &["resource", "remove", "test-abc", "--resource", "r1"],
        ] {
            assert!(parses_as_mutation(args), "should lock mutation: {args:?}");
        }

        for args in [
            &["init"][..],
            &["info"],
            &["list"],
            &["show", "test-abc"],
            &["ready"],
            &["blocking-dependency", "list", "--dependent", "test-abc"],
            &["blocking-dependency", "tree", "--dependent", "test-abc"],
            &["related", "list", "--issue", "test-abc"],
            &["discovery", "list", "--discovered", "test-abc"],
            &["parent", "show", "--child", "test-abc"],
            &["label", "list", "test-abc"],
            &["label", "list-all"],
            &["resource", "list", "test-abc"],
            &["stale"],
            &["blocked"],
            &["stats"],
        ] {
            assert!(!parses_as_mutation(args), "should not lock read: {args:?}");
        }
    }

    #[test]
    fn parent_leaves_parse_explicit_endpoint_roles() {
        let set = Cli::try_parse_from([
            "rivets", "parent", "set", "--child", "test-abc", "--parent", "test-def",
        ])
        .unwrap();
        assert!(matches!(
            set.command,
            Some(Commands::Parent(ParentArgs {
                action: ParentAction::Set { child, parent }
            })) if child == "test-abc" && parent == "test-def"
        ));

        let clear =
            Cli::try_parse_from(["rivets", "parent", "clear", "--child", "test-abc"]).unwrap();
        assert!(matches!(
            clear.command,
            Some(Commands::Parent(ParentArgs {
                action: ParentAction::Clear { child }
            })) if child == "test-abc"
        ));

        let moved = Cli::try_parse_from([
            "rivets", "parent", "move", "--child", "test-abc", "--parent", "test-def",
        ])
        .unwrap();
        assert!(matches!(
            moved.command,
            Some(Commands::Parent(ParentArgs {
                action: ParentAction::Move { child, parent }
            })) if child == "test-abc" && parent == "test-def"
        ));

        let show =
            Cli::try_parse_from(["rivets", "parent", "show", "--child", "test-abc"]).unwrap();
        assert!(matches!(
            show.command,
            Some(Commands::Parent(ParentArgs {
                action: ParentAction::Show { child }
            })) if child == "test-abc"
        ));
    }

    #[test]
    fn parent_leaves_reject_missing_or_positional_roles() {
        assert!(Cli::try_parse_from(["rivets", "parent", "set", "--child", "test-abc"]).is_err());
        assert!(Cli::try_parse_from(["rivets", "parent", "show", "test-abc"]).is_err());
        assert!(
            Cli::try_parse_from([
                "rivets", "parent", "clear", "--child", "test-abc", "--parent", "test-def"
            ])
            .is_err()
        );
    }

    // ========== CLI Parsing Tests ==========

    #[test]
    fn test_parse_no_command() {
        let cli = Cli::try_parse_from(["rivets"]).unwrap();
        assert!(cli.command.is_none());
        assert!(!cli.json);
        assert!(!cli.yes);
    }

    #[test]
    fn test_parse_global_json_flag() {
        let cli = Cli::try_parse_from(["rivets", "--json", "list"]).unwrap();
        assert!(cli.json);
        assert!(matches!(cli.command, Some(Commands::List(_))));
    }

    #[test]
    fn test_parse_global_yes_flag_long() {
        let cli = Cli::try_parse_from(["rivets", "--yes", "close", "proj-abc"]).unwrap();
        assert!(cli.yes);
        assert!(matches!(cli.command, Some(Commands::Close(_))));
    }

    #[test]
    fn test_parse_global_yes_flag_short() {
        let cli = Cli::try_parse_from(["rivets", "-y", "delete", "proj-abc"]).unwrap();
        assert!(cli.yes);
        assert!(matches!(cli.command, Some(Commands::Delete(_))));
    }

    #[test]
    fn test_parse_yes_and_json_combined() {
        let cli = Cli::try_parse_from(["rivets", "--yes", "--json", "close", "proj-abc"]).unwrap();
        assert!(cli.yes);
        assert!(cli.json);
        assert!(matches!(cli.command, Some(Commands::Close(_))));
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
                assert_eq!(args.issue_kind, IssueKind::Task); // default
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
            "--kind",
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
                assert_eq!(args.issue_kind, IssueKind::Bug);
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
            "--kind",
            "bug",
            "--assignee",
            "bob",
            "--limit",
            "10",
        ])
        .unwrap();

        match cli.command {
            Some(Commands::List(args)) => {
                assert_eq!(args.status, Some(IssueStatus::Open));
                assert_eq!(args.priority, Some(1));
                assert_eq!(args.issue_kind, Some(IssueKind::Bug));
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
                assert_eq!(args.status, Some(IssueStatus::InProgress));
            }
            _ => panic!("Expected List command"),
        }
    }

    #[test]
    fn test_parse_list_status_in_progress_alias() {
        let cli = Cli::try_parse_from(["rivets", "list", "--status", "in-progress"]).unwrap();
        match cli.command {
            Some(Commands::List(args)) => {
                assert_eq!(args.status, Some(IssueStatus::InProgress));
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
    fn all_issue_id_inputs_use_domain_parser() {
        let mut cases = vec![
            vec![
                "rivets",
                "create",
                "--title",
                "Title",
                "--prerequisite",
                "invalid",
            ],
            vec!["rivets", "show", "invalid"],
            vec!["rivets", "update", "invalid", "--title", "Title"],
            vec!["rivets", "close", "invalid"],
            vec!["rivets", "reopen", "invalid"],
            vec!["rivets", "delete", "invalid"],
            vec![
                "rivets",
                "blocking-dependency",
                "add",
                "--dependent",
                "invalid",
                "--prerequisite",
                "ab-1",
            ],
            vec![
                "rivets",
                "blocking-dependency",
                "add",
                "--dependent",
                "ab-1",
                "--prerequisite",
                "invalid",
            ],
            vec![
                "rivets",
                "blocking-dependency",
                "list",
                "--dependent",
                "invalid",
            ],
            vec![
                "rivets",
                "blocking-dependency",
                "list",
                "--prerequisite",
                "invalid",
            ],
            vec![
                "rivets",
                "blocking-dependency",
                "tree",
                "--dependent",
                "invalid",
            ],
            vec!["rivets", "label", "add", "urgent", "invalid"],
            vec!["rivets", "label", "remove", "urgent", "invalid"],
            vec!["rivets", "label", "list", "invalid"],
            vec![
                "rivets",
                "resource",
                "add",
                "invalid",
                "--url",
                "https://example.com",
                "--role",
                "reference",
            ],
            vec![
                "rivets",
                "resource",
                "update",
                "invalid",
                "--resource",
                "r1",
                "--role",
                "evidence",
            ],
            vec![
                "rivets",
                "resource",
                "remove",
                "invalid",
                "--resource",
                "r1",
            ],
            vec!["rivets", "resource", "list", "invalid"],
        ];

        for action in ["claim", "release"] {
            cases.push(vec!["rivets", action, "invalid", "--assignee", "agent"]);
        }
        for (command, action, first_role, second_role) in [
            (
                "blocking-dependency",
                "remove",
                "--dependent",
                "--prerequisite",
            ),
            ("parent", "set", "--child", "--parent"),
            ("parent", "move", "--child", "--parent"),
            ("related", "add", "--issue", "--related"),
            ("related", "remove", "--issue", "--related"),
            ("discovery", "add", "--discovered", "--source"),
            ("discovery", "remove", "--discovered", "--source"),
        ] {
            for (first, second) in [("invalid", "ab-1"), ("ab-1", "invalid")] {
                cases.push(vec![
                    "rivets",
                    command,
                    action,
                    first_role,
                    first,
                    second_role,
                    second,
                ]);
            }
        }
        for (command, action, role) in [
            ("parent", "clear", "--child"),
            ("parent", "show", "--child"),
            ("related", "list", "--issue"),
            ("discovery", "list", "--discovered"),
        ] {
            cases.push(vec!["rivets", command, action, role, "invalid"]);
        }

        for invalid_args in cases {
            let error = Cli::try_parse_from(&invalid_args)
                .expect_err("malformed Issue ID should fail at the CLI boundary");
            assert_eq!(
                error.kind(),
                clap::error::ErrorKind::ValueValidation,
                "unexpected error for {invalid_args:?}: {error}"
            );

            let valid_args = invalid_args
                .into_iter()
                .map(|argument| {
                    if argument == "invalid" {
                        "abcdefghijklmnopqrst-feature-123"
                    } else {
                        argument
                    }
                })
                .collect::<Vec<_>>();
            Cli::try_parse_from(&valid_args)
                .unwrap_or_else(|error| panic!("valid control failed for {valid_args:?}: {error}"));
        }
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
                assert_eq!(args.status, Some(IssueStatus::InProgress));
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
                assert_eq!(args.status, Some(IssueStatus::InProgress));
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
                assert!(args.reason.is_none()); // no default
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
                assert_eq!(args.reason, Some("Batch done".to_string()));
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
                assert_eq!(args.reason, Some("Fixed in PR #42".to_string()));
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
                assert!(!args.all_assignees);
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
                assert!(!args.all_assignees);
                assert_eq!(args.limit, 5);
                assert_eq!(args.sort, SortPolicyArg::Priority);
            }
            _ => panic!("Expected Ready command"),
        }
    }

    #[test]
    fn ready_assignment_selectors_are_mutually_exclusive() {
        let cli = Cli::try_parse_from(["rivets", "ready", "--all-assignees"])
            .expect("all-assignees Ready syntax should parse");
        assert!(matches!(
            cli.command,
            Some(Commands::Ready(ReadyArgs {
                all_assignees: true,
                assignee: None,
                ..
            }))
        ));

        assert!(
            Cli::try_parse_from(["rivets", "ready", "--assignee", "alice", "--all-assignees"])
                .is_err()
        );
    }

    #[test]
    fn generic_dependency_cli_is_absent() {
        assert!(Cli::try_parse_from(["rivets", "dep", "add", "proj-abc", "proj-xyz"]).is_err());
        let parsed = Cli::try_parse_from([
            "rivets",
            "blocking-dependency",
            "add",
            "--dependent",
            "proj-abc",
            "--prerequisite",
            "proj-xyz",
        ])
        .unwrap();
        assert!(matches!(
            parsed.command,
            Some(Commands::BlockingDependency(BlockingDependencyArgs {
                action: BlockingDependencyAction::Add { .. }
            }))
        ));
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

    #[test]
    fn general_update_rejects_assignment_flags() {
        for flag in ["--assignee", "--no-assignee"] {
            let mut argv = vec!["rivets", "update", "proj-abc", flag];
            if flag == "--assignee" {
                argv.push("alice");
            }
            assert!(
                Cli::try_parse_from(argv).is_err(),
                "general update must reject {flag}"
            );
        }
    }

    #[test]
    fn claim_and_release_require_one_issue_and_explicit_assignee() {
        for command in ["claim", "release"] {
            let parsed =
                Cli::try_parse_from(["rivets", command, "proj-abc", "--assignee", "alice"])
                    .expect("intent should parse");
            let args = match parsed.command {
                Some(Commands::Claim(args) | Commands::Release(args)) => args,
                _ => panic!("Expected Assignment command"),
            };
            assert_eq!(args.issue_id, "proj-abc");
            assert_eq!(args.assignee, "alice");

            assert!(Cli::try_parse_from(["rivets", command, "proj-abc"]).is_err());
            for assignee in ["", " \t "] {
                let error =
                    Cli::try_parse_from(["rivets", command, "proj-abc", "--assignee", assignee])
                        .expect_err("blank Assignee must reject at the CLI seam");
                assert!(error.to_string().contains("Assignee cannot be blank"));
            }
            assert!(
                Cli::try_parse_from([
                    "rivets",
                    command,
                    "proj-abc",
                    "proj-def",
                    "--assignee",
                    "alice",
                ])
                .is_err()
            );
        }
    }
}
