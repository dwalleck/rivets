//! CLI argument structs for all commands.
//!
//! Each command has its own argument struct with clap derive attributes
//! for parsing and validation.

use clap::{Parser, Subcommand};

use super::types::{DependencyTypeArg, IssueStatusArg, IssueTypeArg, SortOrderArg, SortPolicyArg};
use super::validators::{validate_description, validate_issue_id, validate_prefix, validate_title};
use crate::domain::{MAX_PRIORITY, MIN_PRIORITY};

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
    /// Maximum 200 characters.
    #[arg(long, value_parser = validate_title)]
    pub title: Option<String>,

    /// Detailed description
    #[arg(short = 'D', long, value_parser = validate_description)]
    pub description: Option<String>,

    /// Priority level (0=critical, 1=high, 2=medium, 3=low, 4=backlog)
    #[arg(short, long, value_parser = clap::value_parser!(u8).range(MIN_PRIORITY as i64..=MAX_PRIORITY as i64), default_value = "2")]
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
    #[arg(short, long, value_parser = clap::value_parser!(u8).range(MIN_PRIORITY as i64..=MAX_PRIORITY as i64))]
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

    /// New title (maximum 200 characters)
    #[arg(long, value_parser = validate_title)]
    pub title: Option<String>,

    /// New description
    #[arg(short = 'D', long, value_parser = validate_description)]
    pub description: Option<String>,

    /// New status
    #[arg(short, long, value_enum)]
    pub status: Option<IssueStatusArg>,

    /// New priority
    #[arg(short, long, value_parser = clap::value_parser!(u8).range(MIN_PRIORITY as i64..=MAX_PRIORITY as i64))]
    pub priority: Option<u8>,

    /// New assignee
    ///
    /// Note: To unassign, use `--no-assignee` flag instead. Clap does not
    /// support empty strings ("") as argument values by default.
    ///
    /// TODO: Implement --no-assignee flag for explicit unassignment
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
    #[arg(short, long, value_parser = clap::value_parser!(u8).range(MIN_PRIORITY as i64..=MAX_PRIORITY as i64))]
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
