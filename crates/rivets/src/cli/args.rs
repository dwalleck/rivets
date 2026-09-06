//! CLI argument structs for all commands.
//!
//! Each command has its own argument struct with clap derive attributes
//! for parsing and validation.

use clap::{Parser, Subcommand};

use std::num::NonZeroUsize;

use super::types::SortPolicyArg;
use super::validators::{
    validate_assignee, validate_description, validate_issue_id, validate_prefix, validate_title,
};
use crate::domain::{IssueKind, IssueStatus, Label, MAX_PRIORITY, MIN_PRIORITY, ResourceRole};

fn parse_canonical_status(value: &str) -> Result<IssueStatus, String> {
    value
        .parse::<IssueStatus>()
        .map_err(|error| error.to_string())
}

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
    /// Maximum length defined by `MAX_TITLE_LENGTH` (currently 200 characters).
    /// Note: `allow_hyphen_values` is intentionally omitted here — titles are
    /// short identifiers, not markdown. Catching accidental flag-like input
    /// (e.g., `--title --description`) is more useful than allowing `- ...`.
    #[arg(long, value_parser = validate_title)]
    pub title: Option<String>,

    /// Detailed description
    #[arg(short = 'D', long, allow_hyphen_values = true, value_parser = validate_description)]
    pub description: Option<String>,

    /// Priority level (0=critical, 1=high, 2=medium, 3=low, 4=backlog)
    #[arg(short, long, value_parser = clap::value_parser!(u8).range(MIN_PRIORITY as i64..=MAX_PRIORITY as i64), default_value = "2")]
    pub priority: u8,

    /// Issue kind
    #[arg(short = 'k', long = "kind", value_enum, default_value = "task")]
    pub issue_kind: IssueKind,

    /// Assignee username
    #[arg(short, long)]
    pub assignee: Option<String>,

    /// Labels (comma-separated)
    #[arg(short, long, value_delimiter = ',')]
    pub labels: Vec<Label>,

    /// Blocking prerequisite Issue IDs. Repeat for multiple prerequisites.
    #[arg(long = "prerequisite", value_parser = validate_issue_id)]
    pub prerequisites: Vec<String>,

    /// Design notes
    #[arg(long, allow_hyphen_values = true)]
    pub design: Option<String>,

    /// Acceptance criteria
    #[arg(long, allow_hyphen_values = true)]
    pub acceptance: Option<String>,

    /// Initial Note
    #[arg(long, allow_hyphen_values = true)]
    pub notes: Option<String>,
}

/// Arguments for the `list` command
#[derive(Parser, Debug, Clone)]
pub struct ListArgs {
    /// Filter by status
    #[arg(short, long, value_parser = parse_canonical_status)]
    pub status: Option<IssueStatus>,

    /// Filter by priority
    #[arg(short, long, value_parser = clap::value_parser!(u8).range(MIN_PRIORITY as i64..=MAX_PRIORITY as i64))]
    pub priority: Option<u8>,

    /// Filter by issue kind
    #[arg(short = 'k', long = "kind", value_enum)]
    pub issue_kind: Option<IssueKind>,

    /// Filter by assignee
    #[arg(short, long)]
    pub assignee: Option<String>,

    /// Filter by label
    #[arg(short, long)]
    pub label: Option<Label>,

    /// Maximum number of issues to display
    #[arg(short = 'n', long)]
    pub limit: NonZeroUsize,
}

/// Arguments for the `show` command
#[derive(Parser, Debug, Clone)]
pub struct ShowArgs {
    /// Issue ID(s) to display, space-separated (e.g., rivets-abc rivets-def)
    #[arg(required = true, value_parser = validate_issue_id)]
    pub issue_ids: Vec<String>,
}

/// Arguments for the `update` command.
///
/// General Update only changes descriptive Issue fields. Workflow state,
/// Assignment, labels, and Notes each have dedicated commands.
#[derive(Parser, Debug, Clone)]
pub struct UpdateArgs {
    /// Issue ID(s) to update, space-separated (e.g., rivets-abc rivets-def)
    #[arg(required = true, value_parser = validate_issue_id)]
    pub issue_ids: Vec<String>,

    /// New title (maximum length: `MAX_TITLE_LENGTH`)
    /// Note: `allow_hyphen_values` intentionally omitted (see `CreateArgs::title`).
    #[arg(long, value_parser = validate_title)]
    pub title: Option<String>,

    /// New description
    #[arg(short = 'D', long, allow_hyphen_values = true, value_parser = validate_description)]
    pub description: Option<String>,

    /// New priority
    #[arg(short, long, value_parser = clap::value_parser!(u8).range(MIN_PRIORITY as i64..=MAX_PRIORITY as i64))]
    pub priority: Option<u8>,

    /// New issue kind
    #[arg(short = 'k', long = "kind", value_enum)]
    pub issue_kind: Option<IssueKind>,

    /// New design notes
    #[arg(long, allow_hyphen_values = true)]
    pub design: Option<String>,

    /// New acceptance criteria
    #[arg(long, allow_hyphen_values = true)]
    pub acceptance: Option<String>,
}
/// Arguments for the `start` command.
#[derive(Parser, Debug, Clone)]
pub struct StartArgs {
    /// Issue ID(s) to start, space-separated (e.g., rivets-abc rivets-def)
    #[arg(required = true, value_parser = validate_issue_id)]
    pub issue_ids: Vec<String>,
}

/// Arguments for the `return-to-open` command.
#[derive(Parser, Debug, Clone)]
pub struct ReturnToOpenArgs {
    /// Issue ID(s) to return to Open, space-separated (e.g., rivets-abc rivets-def)
    #[arg(required = true, value_parser = validate_issue_id)]
    pub issue_ids: Vec<String>,
}

/// Arguments for an Assignment Claim or Release.
#[derive(Parser, Debug, Clone)]
pub struct AssignmentArgs {
    /// Issue ID whose Assignment changes.
    #[arg(value_parser = validate_issue_id)]
    pub issue_id: String,

    /// Exact Assignee identity to claim as or release.
    #[arg(short, long, value_parser = validate_assignee)]
    pub assignee: String,
}

/// Arguments for the `note` command.
#[derive(Parser, Debug, Clone)]
pub struct NoteArgs {
    #[command(subcommand)]
    pub action: NoteAction,
}

/// Note management actions.
#[derive(Subcommand, Debug, Clone)]
pub enum NoteAction {
    /// Append a Note to one or more Issues.
    Append {
        /// Issue ID(s) to receive the Note, space-separated.
        #[arg(required = true, value_parser = validate_issue_id)]
        issue_ids: Vec<String>,

        /// Note content. Multi-line text is allowed.
        #[arg(long, allow_hyphen_values = true)]
        content: String,
    },
}

/// Arguments for the `close` command
#[derive(Parser, Debug, Clone)]
pub struct CloseArgs {
    /// Issue ID(s) to close, space-separated (e.g., rivets-abc rivets-def)
    #[arg(required = true, value_parser = validate_issue_id)]
    pub issue_ids: Vec<String>,

    /// Reason for closing (only added to notes if provided)
    #[arg(short, long)]
    pub reason: Option<String>,
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
    /// Include only Issues assigned to this exact assignee
    #[arg(short, long, conflicts_with = "all_assignees")]
    pub assignee: Option<String>,

    /// Include Issues regardless of Assignment
    #[arg(long, conflicts_with = "assignee")]
    pub all_assignees: bool,

    /// Filter by priority
    #[arg(short, long, value_parser = clap::value_parser!(u8).range(MIN_PRIORITY as i64..=MAX_PRIORITY as i64))]
    pub priority: Option<u8>,

    /// Filter by issue kind
    #[arg(short = 'k', long = "kind", value_enum)]
    pub issue_kind: Option<IssueKind>,

    /// Filter by label
    #[arg(short, long)]
    pub label: Option<Label>,

    /// Maximum number of issues to display
    #[arg(short = 'n', long, default_value = "10")]
    pub limit: usize,

    /// Sort policy
    #[arg(long, value_enum, default_value = "hybrid")]
    pub sort: SortPolicyArg,
}

/// Arguments for canonical Blocking Dependency operations.
#[derive(Parser, Debug, Clone)]
pub struct BlockingDependencyArgs {
    /// Blocking Dependency subcommand.
    #[command(subcommand)]
    pub action: BlockingDependencyAction,
}

/// Canonical Blocking Dependency actions.
#[derive(Subcommand, Debug, Clone)]
pub enum BlockingDependencyAction {
    /// Add a dependent-to-prerequisite Blocking Dependency.
    Add {
        /// Issue that depends on the prerequisite.
        #[arg(long, value_parser = validate_issue_id)]
        dependent: String,
        /// Issue that must be completed first.
        #[arg(long, value_parser = validate_issue_id)]
        prerequisite: String,
    },
    /// Remove one dependent-to-prerequisite Blocking Dependency.
    Remove {
        /// Issue that depends on the prerequisite.
        #[arg(long, value_parser = validate_issue_id)]
        dependent: String,
        /// Issue that must be completed first.
        #[arg(long, value_parser = validate_issue_id)]
        prerequisite: String,
    },
    /// List prerequisites of a dependent or dependents of a prerequisite.
    List(BlockingDependencyListArgs),
    /// Display the transitive prerequisite tree for a dependent.
    Tree {
        /// Root dependent Issue.
        #[arg(long, value_parser = validate_issue_id)]
        dependent: String,
        /// Maximum depth; zero means unlimited.
        #[arg(long, default_value = "5")]
        depth: usize,
    },
}

impl BlockingDependencyAction {
    pub(crate) const fn mutates_workspace(&self) -> bool {
        match self {
            Self::Add { .. } | Self::Remove { .. } => true,
            Self::List(_) | Self::Tree { .. } => false,
        }
    }
}
/// Arguments for canonical Parentage operations.
#[derive(Parser, Debug, Clone)]
pub struct ParentArgs {
    /// Parentage subcommand.
    #[command(subcommand)]
    pub action: ParentAction,
}

/// Canonical Parentage actions.
#[derive(Subcommand, Debug, Clone)]
pub enum ParentAction {
    /// Attach an unparented child to an Epic.
    Set {
        /// Issue owned by the Epic.
        #[arg(long, value_parser = validate_issue_id)]
        child: String,
        /// Epic that owns the child.
        #[arg(long, value_parser = validate_issue_id)]
        parent: String,
    },
    /// Remove one child's Parentage.
    Clear {
        /// Child whose Parentage is removed.
        #[arg(long, value_parser = validate_issue_id)]
        child: String,
    },
    /// Replace one child's existing Epic parent.
    Move {
        /// Child whose Parentage is replaced.
        #[arg(long, value_parser = validate_issue_id)]
        child: String,
        /// New Epic parent.
        #[arg(long, value_parser = validate_issue_id)]
        parent: String,
    },
    /// Show one child's current Epic parent.
    Show {
        /// Child whose Parentage is shown.
        #[arg(long, value_parser = validate_issue_id)]
        child: String,
    },
}

/// Select exactly one Blocking Dependency endpoint perspective.
#[derive(Parser, Debug, Clone)]
#[command(group(
    clap::ArgGroup::new("endpoint")
        .required(true)
        .multiple(false)
        .args(["dependent", "prerequisite"])
))]
pub struct BlockingDependencyListArgs {
    /// List prerequisites required by this dependent.
    #[arg(long, value_parser = validate_issue_id)]
    pub dependent: Option<String>,
    /// List Issues that depend on this prerequisite.
    #[arg(long, value_parser = validate_issue_id)]
    pub prerequisite: Option<String>,
}

/// Arguments for Related Association operations.
#[derive(Parser, Debug, Clone)]
pub struct RelatedArgs {
    /// Related Association subcommand.
    #[command(subcommand)]
    pub action: RelatedAction,
}

/// Related Association actions.
#[derive(Subcommand, Debug, Clone)]
pub enum RelatedAction {
    /// Add a symmetric Related Association.
    Add {
        /// One endpoint of the association.
        #[arg(long, value_parser = validate_issue_id)]
        issue: String,
        /// The other endpoint of the association.
        #[arg(long, value_parser = validate_issue_id)]
        related: String,
    },
    /// Remove a symmetric Related Association.
    Remove {
        /// One endpoint of the association.
        #[arg(long, value_parser = validate_issue_id)]
        issue: String,
        /// The other endpoint of the association.
        #[arg(long, value_parser = validate_issue_id)]
        related: String,
    },
    /// List every Related Association containing an Issue.
    List {
        /// Issue whose Related Associations to list.
        #[arg(long, value_parser = validate_issue_id)]
        issue: String,
    },
}

impl RelatedAction {
    pub(crate) const fn mutates_workspace(&self) -> bool {
        match self {
            Self::Add { .. } | Self::Remove { .. } => true,
            Self::List { .. } => false,
        }
    }
}

/// Arguments for Discovery Origin operations.
#[derive(Parser, Debug, Clone)]
pub struct DiscoveryArgs {
    /// Discovery Origin subcommand.
    #[command(subcommand)]
    pub action: DiscoveryAction,
}

/// Discovery Origin actions.
#[derive(Subcommand, Debug, Clone)]
pub enum DiscoveryAction {
    /// Add a directed Discovery Origin.
    Add {
        /// Issue discovered while working on the source.
        #[arg(long, value_parser = validate_issue_id)]
        discovered: String,
        /// Issue whose work surfaced the discovered Issue.
        #[arg(long, value_parser = validate_issue_id)]
        source: String,
    },
    /// Remove a directed Discovery Origin.
    Remove {
        /// Issue discovered while working on the source.
        #[arg(long, value_parser = validate_issue_id)]
        discovered: String,
        /// Issue whose work surfaced the discovered Issue.
        #[arg(long, value_parser = validate_issue_id)]
        source: String,
    },
    /// List every Discovery Origin for one discovered Issue.
    List {
        /// Discovered Issue whose sources to list.
        #[arg(long, value_parser = validate_issue_id)]
        discovered: String,
    },
}

impl DiscoveryAction {
    pub(crate) const fn mutates_workspace(&self) -> bool {
        match self {
            Self::Add { .. } | Self::Remove { .. } => true,
            Self::List { .. } => false,
        }
    }
}
/// Arguments for the `blocked` command
#[derive(Parser, Debug, Clone, Default)]
pub struct BlockedArgs {
    /// Filter by assignee
    #[arg(short, long)]
    pub assignee: Option<String>,
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
    #[arg(short, long, value_parser = parse_canonical_status)]
    pub status: Option<IssueStatus>,

    /// Maximum number of issues to display
    #[arg(short = 'n', long)]
    pub limit: NonZeroUsize,
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
        label: Label,

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
        label: Label,

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

impl LabelAction {
    pub(crate) const fn mutates_workspace(&self) -> bool {
        match self {
            Self::Add { .. } | Self::Remove { .. } => true,
            Self::List { .. } | Self::ListAll => false,
        }
    }
}

/// Arguments for the `resource` command.
#[derive(Parser, Debug, Clone)]
pub struct ResourceArgs {
    /// Associated Resource subcommand.
    #[command(subcommand)]
    pub action: ResourceAction,
}

/// Associated Resource management actions.
#[derive(Subcommand, Debug, Clone)]
pub enum ResourceAction {
    /// Associate a target with an Issue.
    Add {
        /// Issue ID.
        #[arg(value_parser = validate_issue_id)]
        issue_id: String,

        /// Absolute HTTP or HTTPS URL (conflicts with --path).
        #[arg(long, conflicts_with = "path", required_unless_present = "path")]
        url: Option<String>,

        /// Path relative to the workspace root (conflicts with --url).
        #[arg(long)]
        path: Option<String>,

        /// Why this resource matters to the Issue.
        #[arg(long, value_enum)]
        role: ResourceRole,

        /// Optional human-readable label.
        #[arg(long)]
        label: Option<String>,
    },

    /// Update an existing Associated Resource by its stable identifier.
    ///
    /// Only the provided fields change; the resource keeps its identifier and
    /// position. At least one field is required.
    #[command(group = clap::ArgGroup::new("resource_update_field")
        .args(["url", "path", "role", "label", "no_label"])
        .required(true)
        .multiple(true))]
    Update {
        /// Issue ID.
        #[arg(value_parser = validate_issue_id)]
        issue_id: String,

        /// Stable resource identifier (e.g. r3).
        #[arg(long, value_name = "RESOURCE_ID")]
        resource: String,

        /// New absolute HTTP or HTTPS URL (conflicts with --path).
        #[arg(long, conflicts_with = "path")]
        url: Option<String>,

        /// New path relative to the workspace root (conflicts with --url).
        #[arg(long)]
        path: Option<String>,

        /// New role.
        #[arg(long, value_enum)]
        role: Option<ResourceRole>,

        /// New human-readable label (conflicts with --no-label).
        #[arg(long, conflicts_with = "no_label")]
        label: Option<String>,

        /// Clear the resource's label.
        #[arg(long)]
        no_label: bool,
    },

    /// Remove an Associated Resource by its stable identifier.
    ///
    /// The remaining resources keep their identifiers and positions.
    Remove {
        /// Issue ID.
        #[arg(value_parser = validate_issue_id)]
        issue_id: String,

        /// Stable resource identifier (e.g. r3).
        #[arg(long, value_name = "RESOURCE_ID")]
        resource: String,
    },

    /// List an Issue's Associated Resources in insertion order.
    List {
        /// Issue ID.
        #[arg(value_parser = validate_issue_id)]
        issue_id: String,
    },
}

impl ResourceAction {
    pub(crate) const fn mutates_workspace(&self) -> bool {
        match self {
            Self::Add { .. } | Self::Update { .. } | Self::Remove { .. } => true,
            Self::List { .. } => false,
        }
    }
}
