//! CLI argument structs for all commands.
//!
//! Each command has its own argument struct with clap derive attributes
//! for parsing and validation.

use clap::{Parser, Subcommand};

use super::types::{DependencyTypeArg, IssueStatusArg, IssueTypeArg, SortOrderArg, SortPolicyArg};
use super::validators::{
    validate_description, validate_issue_id, validate_label, validate_prefix, validate_title,
};
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
    /// Issue ID(s) to display, space-separated (e.g., rivets-abc rivets-def)
    #[arg(required = true, value_parser = validate_issue_id)]
    pub issue_ids: Vec<String>,
}

/// Arguments for the `update` command
///
/// # Labels
///
/// Labels are intentionally not modifiable via `update`. Use the dedicated
/// `label add` and `label remove` commands instead. This avoids ambiguity
/// about replace-vs-add semantics - the dedicated commands make the intent
/// explicit.
#[derive(Parser, Debug, Clone)]
pub struct UpdateArgs {
    /// Issue ID(s) to update, space-separated (e.g., rivets-abc rivets-def)
    #[arg(required = true, value_parser = validate_issue_id)]
    pub issue_ids: Vec<String>,

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
    #[arg(short, long, conflicts_with = "no_assignee")]
    pub assignee: Option<String>,

    /// Remove the current assignee (unassign the issue)
    #[arg(long, conflicts_with = "assignee")]
    pub no_assignee: bool,

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
    /// Issue ID(s) to close, space-separated (e.g., rivets-abc rivets-def)
    #[arg(required = true, value_parser = validate_issue_id)]
    pub issue_ids: Vec<String>,

    /// Reason for closing
    #[arg(short, long, default_value = "Completed")]
    pub reason: String,
}

/// Arguments for the `reopen` command
#[derive(Parser, Debug, Clone)]
pub struct ReopenArgs {
    /// Issue ID(s) to reopen, space-separated (e.g., rivets-abc rivets-def)
    #[arg(required = true, value_parser = validate_issue_id)]
    pub issue_ids: Vec<String>,

    /// Reason for reopening
    #[arg(short, long)]
    pub reason: Option<String>,
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

    /// Display dependency tree for an issue
    Tree {
        /// Issue ID
        #[arg(value_parser = validate_issue_id)]
        issue_id: String,

        /// Maximum depth to traverse (use 0 for unlimited)
        #[arg(short, long, default_value = "5")]
        depth: usize,
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

/// Arguments for the `info` command
#[derive(Parser, Debug, Clone, Default)]
pub struct InfoArgs {
    // No arguments for now, just --json global flag
}

/// Arguments for the `stale` command
#[derive(Parser, Debug, Clone)]
pub struct StaleArgs {
    /// Number of days since last update to consider stale
    #[arg(short, long, default_value = "30")]
    pub days: u32,

    /// Filter by status
    #[arg(short, long, value_enum)]
    pub status: Option<IssueStatusArg>,

    /// Maximum number of issues to display
    #[arg(short = 'n', long, default_value = "50")]
    pub limit: usize,
}

/// Arguments for the `label` command
#[derive(Parser, Debug, Clone)]
pub struct LabelArgs {
    /// Label subcommand
    #[command(subcommand)]
    pub action: LabelAction,
}

/// Label management actions
#[derive(Subcommand, Debug, Clone)]
pub enum LabelAction {
    /// Add a label to one or more issues
    Add {
        /// Label to add (lowercase, alphanumeric with hyphens/underscores)
        #[arg(value_parser = validate_label)]
        label: String,

        /// Issue ID (for single issue)
        #[arg(value_parser = validate_issue_id)]
        issue_id: Option<String>,

        /// Issue ID(s), space-separated (for multiple issues)
        #[arg(long = "ids", num_args = 1.., value_parser = validate_issue_id)]
        ids: Vec<String>,
    },

    /// Remove a label from one or more issues
    Remove {
        /// Label to remove (lowercase, alphanumeric with hyphens/underscores)
        #[arg(value_parser = validate_label)]
        label: String,

        /// Issue ID (for single issue)
        #[arg(value_parser = validate_issue_id)]
        issue_id: Option<String>,

        /// Issue ID(s), space-separated (for multiple issues)
        #[arg(long = "ids", num_args = 1.., value_parser = validate_issue_id)]
        ids: Vec<String>,
    },

    /// List labels for a specific issue
    List {
        /// Issue ID
        #[arg(value_parser = validate_issue_id)]
        issue_id: String,
    },

    /// List all labels used across all issues
    ListAll,
}
