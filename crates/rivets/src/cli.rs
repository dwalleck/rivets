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

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};

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

    /// Show blocked issues
    ///
    /// Lists issues that are blocked by dependencies, along with their blockers.
    Blocked(BlockedArgs),

    /// Show project statistics
    ///
    /// Displays summary statistics about issues, completion rates, and trends.
    Stats(StatsArgs),
}

// ============================================================================
// Argument Structs
// ============================================================================

/// Arguments for the `init` command
#[derive(Parser, Debug, Clone)]
pub struct InitArgs {
    /// Issue ID prefix (e.g., "proj" for "proj-abc")
    ///
    /// Must be 2-20 alphanumeric characters. This prefix is used for all
    /// issue IDs in this repository.
    #[arg(short, long, value_parser = validate_prefix)]
    pub prefix: Option<String>,

    /// Suppress output messages
    #[arg(short, long)]
    pub quiet: bool,
}

/// Arguments for the `create` command
#[derive(Parser, Debug, Clone)]
pub struct CreateArgs {
    /// Issue title (required, or prompted interactively)
    ///
    /// Short description of the issue. Will be prompted if not provided.
    #[arg(long)]
    pub title: Option<String>,

    /// Detailed description
    #[arg(short = 'D', long)]
    pub description: Option<String>,

    /// Priority level (0=critical, 1=high, 2=medium, 3=low, 4=backlog)
    #[arg(short, long, value_parser = clap::value_parser!(u8).range(0..=4), default_value = "2")]
    pub priority: u8,

    /// Issue type
    #[arg(short = 't', long = "type", value_enum, default_value = "task")]
    pub issue_type: IssueTypeArg,

    /// Assignee username
    #[arg(short, long)]
    pub assignee: Option<String>,

    /// Labels (comma-separated)
    #[arg(short, long, value_delimiter = ',')]
    pub labels: Vec<String>,

    /// Dependencies (comma-separated issue IDs)
    ///
    /// Format: "issue-id" or "type:issue-id" where type is blocks, related,
    /// parent-child, or discovered-from.
    #[arg(long, value_delimiter = ',')]
    pub deps: Vec<String>,

    /// Design notes
    #[arg(long)]
    pub design: Option<String>,

    /// Acceptance criteria
    #[arg(long)]
    pub acceptance: Option<String>,

    /// External reference (e.g., GitHub issue URL)
    #[arg(long)]
    pub external_ref: Option<String>,
}

/// Arguments for the `list` command
#[derive(Parser, Debug, Clone)]
pub struct ListArgs {
    /// Filter by status
    #[arg(short, long, value_enum)]
    pub status: Option<IssueStatusArg>,

    /// Filter by priority
    #[arg(short, long, value_parser = clap::value_parser!(u8).range(0..=4))]
    pub priority: Option<u8>,

    /// Filter by issue type
    #[arg(short = 't', long = "type", value_enum)]
    pub issue_type: Option<IssueTypeArg>,

    /// Filter by assignee
    #[arg(short, long)]
    pub assignee: Option<String>,

    /// Filter by label
    #[arg(short, long)]
    pub label: Option<String>,

    /// Maximum number of issues to display
    #[arg(short = 'n', long, default_value = "50")]
    pub limit: usize,

    /// Sort order
    #[arg(long, value_enum, default_value = "priority")]
    pub sort: SortOrderArg,
}

/// Arguments for the `show` command
#[derive(Parser, Debug, Clone)]
pub struct ShowArgs {
    /// Issue ID to display
    #[arg(value_parser = validate_issue_id)]
    pub issue_id: String,
}

/// Arguments for the `update` command
#[derive(Parser, Debug, Clone)]
pub struct UpdateArgs {
    /// Issue ID to update
    #[arg(value_parser = validate_issue_id)]
    pub issue_id: String,

    /// New title
    #[arg(long)]
    pub title: Option<String>,

    /// New description
    #[arg(short = 'D', long)]
    pub description: Option<String>,

    /// New status
    #[arg(short, long, value_enum)]
    pub status: Option<IssueStatusArg>,

    /// New priority
    #[arg(short, long, value_parser = clap::value_parser!(u8).range(0..=4))]
    pub priority: Option<u8>,

    /// New assignee (use "" to unassign)
    #[arg(short, long)]
    pub assignee: Option<String>,

    /// New design notes
    #[arg(long)]
    pub design: Option<String>,

    /// New acceptance criteria
    #[arg(long)]
    pub acceptance: Option<String>,

    /// New notes
    #[arg(long)]
    pub notes: Option<String>,

    /// New external reference
    #[arg(long)]
    pub external_ref: Option<String>,
}

/// Arguments for the `close` command
#[derive(Parser, Debug, Clone)]
pub struct CloseArgs {
    /// Issue ID to close
    #[arg(value_parser = validate_issue_id)]
    pub issue_id: String,

    /// Reason for closing
    #[arg(short, long, default_value = "Completed")]
    pub reason: String,
}

/// Arguments for the `delete` command
#[derive(Parser, Debug, Clone)]
pub struct DeleteArgs {
    /// Issue ID to delete
    #[arg(value_parser = validate_issue_id)]
    pub issue_id: String,

    /// Skip confirmation prompt
    #[arg(short, long)]
    pub force: bool,
}

/// Arguments for the `ready` command
#[derive(Parser, Debug, Clone)]
pub struct ReadyArgs {
    /// Filter by assignee
    #[arg(short, long)]
    pub assignee: Option<String>,

    /// Filter by priority
    #[arg(short, long, value_parser = clap::value_parser!(u8).range(0..=4))]
    pub priority: Option<u8>,

    /// Maximum number of issues to display
    #[arg(short = 'n', long, default_value = "10")]
    pub limit: usize,

    /// Sort policy
    #[arg(long, value_enum, default_value = "hybrid")]
    pub sort: SortPolicyArg,
}

/// Arguments for the `dep` command
#[derive(Parser, Debug, Clone)]
pub struct DepArgs {
    /// Dependency subcommand
    #[command(subcommand)]
    pub action: DepAction,
}

/// Dependency management actions
#[derive(Subcommand, Debug, Clone)]
pub enum DepAction {
    /// Add a dependency
    Add {
        /// Issue that depends on another
        #[arg(value_parser = validate_issue_id)]
        from: String,

        /// Issue being depended on
        #[arg(value_parser = validate_issue_id)]
        to: String,

        /// Dependency type
        #[arg(short = 't', long = "type", value_enum, default_value = "blocks")]
        dep_type: DependencyTypeArg,
    },

    /// Remove a dependency
    Remove {
        /// Issue that depends on another
        #[arg(value_parser = validate_issue_id)]
        from: String,

        /// Issue being depended on
        #[arg(value_parser = validate_issue_id)]
        to: String,
    },

    /// List dependencies for an issue
    List {
        /// Issue ID
        #[arg(value_parser = validate_issue_id)]
        issue_id: String,

        /// Show reverse dependencies (issues that depend on this one)
        #[arg(short, long)]
        reverse: bool,
    },
}

/// Arguments for the `blocked` command
#[derive(Parser, Debug, Clone, Default)]
pub struct BlockedArgs {
    /// Filter by assignee
    #[arg(short, long)]
    pub assignee: Option<String>,
}

/// Arguments for the `stats` command
#[derive(Parser, Debug, Clone, Default)]
pub struct StatsArgs {
    /// Show detailed breakdown
    #[arg(short, long)]
    pub detailed: bool,
}

// ============================================================================
// Value Enums
// ============================================================================

/// Issue type for CLI arguments
#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueTypeArg {
    /// Bug fix
    Bug,
    /// New feature
    Feature,
    /// General task
    Task,
    /// Epic (parent issue)
    Epic,
    /// Maintenance/chore
    Chore,
}

impl std::fmt::Display for IssueTypeArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bug => write!(f, "bug"),
            Self::Feature => write!(f, "feature"),
            Self::Task => write!(f, "task"),
            Self::Epic => write!(f, "epic"),
            Self::Chore => write!(f, "chore"),
        }
    }
}

/// Issue status for CLI arguments
#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueStatusArg {
    /// Open and ready to work on
    Open,
    /// Currently being worked on
    #[value(name = "in_progress", alias = "in-progress")]
    InProgress,
    /// Blocked by dependencies
    Blocked,
    /// Completed
    Closed,
}

impl std::fmt::Display for IssueStatusArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Open => write!(f, "open"),
            Self::InProgress => write!(f, "in_progress"),
            Self::Blocked => write!(f, "blocked"),
            Self::Closed => write!(f, "closed"),
        }
    }
}

/// Dependency type for CLI arguments
#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyTypeArg {
    /// Hard blocker - prevents work
    Blocks,
    /// Soft link - informational
    Related,
    /// Hierarchical - epic to task
    #[value(name = "parent-child")]
    ParentChild,
    /// Found during work
    #[value(name = "discovered-from")]
    DiscoveredFrom,
}

impl std::fmt::Display for DependencyTypeArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Blocks => write!(f, "blocks"),
            Self::Related => write!(f, "related"),
            Self::ParentChild => write!(f, "parent-child"),
            Self::DiscoveredFrom => write!(f, "discovered-from"),
        }
    }
}

/// Sort order for list command
#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortOrderArg {
    /// Sort by priority (highest first)
    #[default]
    Priority,
    /// Sort by creation date (newest first)
    Newest,
    /// Sort by creation date (oldest first)
    Oldest,
    /// Sort by last update (most recent first)
    Updated,
}

impl std::fmt::Display for SortOrderArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Priority => write!(f, "priority"),
            Self::Newest => write!(f, "newest"),
            Self::Oldest => write!(f, "oldest"),
            Self::Updated => write!(f, "updated"),
        }
    }
}

/// Sort policy for ready command
#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortPolicyArg {
    /// Recent issues (48h) by priority, older by age
    #[default]
    Hybrid,
    /// Strict priority ordering (P0 -> P1 -> P2 -> P3 -> P4)
    Priority,
    /// Oldest issues first
    Oldest,
}

impl std::fmt::Display for SortPolicyArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Hybrid => write!(f, "hybrid"),
            Self::Priority => write!(f, "priority"),
            Self::Oldest => write!(f, "oldest"),
        }
    }
}

// ============================================================================
// Validators
// ============================================================================

/// Validate issue ID prefix format.
///
/// Requirements:
/// - 2-20 characters
/// - Alphanumeric only (letters and digits)
/// - No special characters or spaces
fn validate_prefix(s: &str) -> std::result::Result<String, String> {
    let s = s.trim();

    if s.len() < 2 {
        return Err("Prefix must be at least 2 characters".to_string());
    }

    if s.len() > 20 {
        return Err("Prefix cannot exceed 20 characters".to_string());
    }

    if !s.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err("Prefix must contain only alphanumeric characters".to_string());
    }

    Ok(s.to_string())
}

/// Validate issue ID format.
///
/// Expected format: `prefix-suffix` where:
/// - prefix: 2-20 alphanumeric characters
/// - suffix: 1+ alphanumeric characters
///
/// Examples: `proj-abc`, `rivets-12x`, `test-1`
fn validate_issue_id(s: &str) -> std::result::Result<String, String> {
    let s = s.trim();

    if s.is_empty() {
        return Err("Issue ID cannot be empty".to_string());
    }

    // Check for the prefix-suffix format
    let parts: Vec<&str> = s.splitn(2, '-').collect();
    if parts.len() != 2 {
        return Err(format!(
            "Invalid issue ID format: '{}'. Expected format: prefix-suffix (e.g., proj-abc)",
            s
        ));
    }

    let prefix = parts[0];
    let suffix = parts[1];

    // Validate prefix
    if prefix.len() < 2 || prefix.len() > 20 {
        return Err(format!(
            "Issue ID prefix must be 2-20 characters, got {} characters",
            prefix.len()
        ));
    }

    if !prefix.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err("Issue ID prefix must be alphanumeric".to_string());
    }

    // Validate suffix
    if suffix.is_empty() {
        return Err("Issue ID suffix cannot be empty".to_string());
    }

    if !suffix.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err("Issue ID suffix must be alphanumeric".to_string());
    }

    Ok(s.to_string())
}

// ============================================================================
// CLI Implementation
// ============================================================================

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
    ///
    /// This is a placeholder that will be implemented when storage integration
    /// is complete (rivets-cgl).
    pub async fn execute(&self) -> Result<()> {
        match &self.command {
            Some(Commands::Init(args)) => {
                if !args.quiet {
                    println!(
                        "Initializing rivets repository{}...",
                        args.prefix
                            .as_ref()
                            .map(|p| format!(" with prefix '{}'", p))
                            .unwrap_or_default()
                    );
                }
                Ok(())
            }
            Some(Commands::Create(args)) => {
                println!(
                    "Creating issue: {}",
                    args.title.as_deref().unwrap_or("[interactive]")
                );
                Ok(())
            }
            Some(Commands::List(args)) => {
                println!(
                    "Listing issues (limit: {}, sort: {})",
                    args.limit, args.sort
                );
                Ok(())
            }
            Some(Commands::Show(args)) => {
                println!("Showing issue: {}", args.issue_id);
                Ok(())
            }
            Some(Commands::Update(args)) => {
                println!("Updating issue: {}", args.issue_id);
                Ok(())
            }
            Some(Commands::Close(args)) => {
                println!("Closing issue: {} (reason: {})", args.issue_id, args.reason);
                Ok(())
            }
            Some(Commands::Delete(args)) => {
                if args.force {
                    println!("Deleting issue: {} (forced)", args.issue_id);
                } else {
                    println!(
                        "Deleting issue: {} (would prompt for confirmation)",
                        args.issue_id
                    );
                }
                Ok(())
            }
            Some(Commands::Ready(args)) => {
                println!(
                    "Finding ready issues (limit: {}, sort: {})",
                    args.limit, args.sort
                );
                Ok(())
            }
            Some(Commands::Dep(args)) => {
                match &args.action {
                    DepAction::Add { from, to, dep_type } => {
                        println!("Adding dependency: {} --[{}]--> {}", from, dep_type, to);
                    }
                    DepAction::Remove { from, to } => {
                        println!("Removing dependency: {} --> {}", from, to);
                    }
                    DepAction::List { issue_id, reverse } => {
                        if *reverse {
                            println!("Listing dependents of: {}", issue_id);
                        } else {
                            println!("Listing dependencies of: {}", issue_id);
                        }
                    }
                }
                Ok(())
            }
            Some(Commands::Blocked(_args)) => {
                println!("Showing blocked issues");
                Ok(())
            }
            Some(Commands::Stats(args)) => {
                if args.detailed {
                    println!("Showing detailed statistics");
                } else {
                    println!("Showing statistics");
                }
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

// ============================================================================
// Conversion Implementations
// ============================================================================

use crate::domain::{DependencyType, IssueStatus, IssueType};

impl From<IssueTypeArg> for IssueType {
    fn from(arg: IssueTypeArg) -> Self {
        match arg {
            IssueTypeArg::Bug => IssueType::Bug,
            IssueTypeArg::Feature => IssueType::Feature,
            IssueTypeArg::Task => IssueType::Task,
            IssueTypeArg::Epic => IssueType::Epic,
            IssueTypeArg::Chore => IssueType::Chore,
        }
    }
}

impl From<IssueType> for IssueTypeArg {
    fn from(t: IssueType) -> Self {
        match t {
            IssueType::Bug => IssueTypeArg::Bug,
            IssueType::Feature => IssueTypeArg::Feature,
            IssueType::Task => IssueTypeArg::Task,
            IssueType::Epic => IssueTypeArg::Epic,
            IssueType::Chore => IssueTypeArg::Chore,
        }
    }
}

impl From<IssueStatusArg> for IssueStatus {
    fn from(arg: IssueStatusArg) -> Self {
        match arg {
            IssueStatusArg::Open => IssueStatus::Open,
            IssueStatusArg::InProgress => IssueStatus::InProgress,
            IssueStatusArg::Blocked => IssueStatus::Blocked,
            IssueStatusArg::Closed => IssueStatus::Closed,
        }
    }
}

impl From<IssueStatus> for IssueStatusArg {
    fn from(s: IssueStatus) -> Self {
        match s {
            IssueStatus::Open => IssueStatusArg::Open,
            IssueStatus::InProgress => IssueStatusArg::InProgress,
            IssueStatus::Blocked => IssueStatusArg::Blocked,
            IssueStatus::Closed => IssueStatusArg::Closed,
        }
    }
}

impl From<DependencyTypeArg> for DependencyType {
    fn from(arg: DependencyTypeArg) -> Self {
        match arg {
            DependencyTypeArg::Blocks => DependencyType::Blocks,
            DependencyTypeArg::Related => DependencyType::Related,
            DependencyTypeArg::ParentChild => DependencyType::ParentChild,
            DependencyTypeArg::DiscoveredFrom => DependencyType::DiscoveredFrom,
        }
    }
}

impl From<DependencyType> for DependencyTypeArg {
    fn from(d: DependencyType) -> Self {
        match d {
            DependencyType::Blocks => DependencyTypeArg::Blocks,
            DependencyType::Related => DependencyTypeArg::Related,
            DependencyType::ParentChild => DependencyTypeArg::ParentChild,
            DependencyType::DiscoveredFrom => DependencyTypeArg::DiscoveredFrom,
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ========== Validator Tests ==========

    #[test]
    fn test_validate_prefix_valid() {
        assert!(validate_prefix("proj").is_ok());
        assert!(validate_prefix("rivets").is_ok());
        assert!(validate_prefix("AB").is_ok());
        assert!(validate_prefix("test123").is_ok());
        assert!(validate_prefix("a1b2c3d4e5f6g7h8i9j0").is_ok()); // 20 chars
    }

    #[test]
    fn test_validate_prefix_too_short() {
        let result = validate_prefix("a");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("at least 2 characters"));
    }

    #[test]
    fn test_validate_prefix_too_long() {
        let result = validate_prefix("a".repeat(21).as_str());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot exceed 20"));
    }

    #[test]
    fn test_validate_prefix_invalid_chars() {
        assert!(validate_prefix("proj-test").is_err()); // hyphen
        assert!(validate_prefix("proj_test").is_err()); // underscore
        assert!(validate_prefix("proj test").is_err()); // space
        assert!(validate_prefix("proj.test").is_err()); // dot
    }

    #[test]
    fn test_validate_prefix_trims_whitespace() {
        assert_eq!(validate_prefix("  proj  ").unwrap(), "proj");
    }

    #[test]
    fn test_validate_issue_id_valid() {
        assert!(validate_issue_id("proj-abc").is_ok());
        assert!(validate_issue_id("rivets-123").is_ok());
        assert!(validate_issue_id("ab-1").is_ok());
        assert!(validate_issue_id("TEST-xyz").is_ok());
    }

    #[test]
    fn test_validate_issue_id_empty() {
        let result = validate_issue_id("");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot be empty"));
    }

    #[test]
    fn test_validate_issue_id_no_hyphen() {
        let result = validate_issue_id("projabc");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Expected format"));
    }

    #[test]
    fn test_validate_issue_id_empty_suffix() {
        let result = validate_issue_id("proj-");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("suffix cannot be empty"));
    }

    #[test]
    fn test_validate_issue_id_prefix_too_short() {
        let result = validate_issue_id("a-123");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("prefix must be 2-20"));
    }

    #[test]
    fn test_validate_issue_id_invalid_chars() {
        assert!(validate_issue_id("proj-abc_123").is_err()); // underscore in suffix
        assert!(validate_issue_id("proj_test-abc").is_err()); // underscore in prefix
    }

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
                assert_eq!(args.issue_id, "proj-abc");
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
                assert_eq!(args.issue_id, "proj-abc");
                assert_eq!(args.title, Some("New title".to_string()));
                assert_eq!(args.status, Some(IssueStatusArg::InProgress));
                assert_eq!(args.priority, Some(0));
            }
            _ => panic!("Expected Update command"),
        }
    }

    #[test]
    fn test_parse_close() {
        let cli = Cli::try_parse_from(["rivets", "close", "proj-abc"]).unwrap();
        match cli.command {
            Some(Commands::Close(args)) => {
                assert_eq!(args.issue_id, "proj-abc");
                assert_eq!(args.reason, "Completed"); // default
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

    // ========== Conversion Tests ==========

    #[test]
    fn test_issue_type_conversion() {
        assert_eq!(IssueType::from(IssueTypeArg::Bug), IssueType::Bug);
        assert_eq!(IssueType::from(IssueTypeArg::Feature), IssueType::Feature);
        assert_eq!(IssueType::from(IssueTypeArg::Task), IssueType::Task);
        assert_eq!(IssueType::from(IssueTypeArg::Epic), IssueType::Epic);
        assert_eq!(IssueType::from(IssueTypeArg::Chore), IssueType::Chore);

        // Reverse conversion
        assert_eq!(IssueTypeArg::from(IssueType::Bug), IssueTypeArg::Bug);
        assert_eq!(
            IssueTypeArg::from(IssueType::Feature),
            IssueTypeArg::Feature
        );
    }

    #[test]
    fn test_issue_status_conversion() {
        assert_eq!(IssueStatus::from(IssueStatusArg::Open), IssueStatus::Open);
        assert_eq!(
            IssueStatus::from(IssueStatusArg::InProgress),
            IssueStatus::InProgress
        );
        assert_eq!(
            IssueStatus::from(IssueStatusArg::Blocked),
            IssueStatus::Blocked
        );
        assert_eq!(
            IssueStatus::from(IssueStatusArg::Closed),
            IssueStatus::Closed
        );

        // Reverse conversion
        assert_eq!(
            IssueStatusArg::from(IssueStatus::Open),
            IssueStatusArg::Open
        );
    }

    #[test]
    fn test_dependency_type_conversion() {
        assert_eq!(
            DependencyType::from(DependencyTypeArg::Blocks),
            DependencyType::Blocks
        );
        assert_eq!(
            DependencyType::from(DependencyTypeArg::Related),
            DependencyType::Related
        );
        assert_eq!(
            DependencyType::from(DependencyTypeArg::ParentChild),
            DependencyType::ParentChild
        );
        assert_eq!(
            DependencyType::from(DependencyTypeArg::DiscoveredFrom),
            DependencyType::DiscoveredFrom
        );
    }

    // ========== Display Tests ==========

    #[test]
    fn test_display_implementations() {
        assert_eq!(format!("{}", IssueTypeArg::Bug), "bug");
        assert_eq!(format!("{}", IssueStatusArg::InProgress), "in_progress");
        assert_eq!(
            format!("{}", DependencyTypeArg::ParentChild),
            "parent-child"
        );
        assert_eq!(format!("{}", SortOrderArg::Priority), "priority");
        assert_eq!(format!("{}", SortPolicyArg::Hybrid), "hybrid");
    }
}
