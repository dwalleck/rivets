//! CLI argument structs for all commands.
//!
//! Each command has its own argument struct with clap derive attributes
//! for parsing and validation.

use clap::{Parser, Subcommand};

use super::types::{SortOrderArg, SortPolicyArg};
use super::validators::{
    validate_description, validate_issue_id, validate_label, validate_prefix, validate_title,
};
use crate::domain::{IssueKind, IssueStatus, MAX_PRIORITY, MIN_PRIORITY, ResourceRole};

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
    pub labels: Vec<String>,

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
    #[arg(short, long, value_enum)]
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

    /// New title (maximum length: `MAX_TITLE_LENGTH`)
    /// Note: `allow_hyphen_values` intentionally omitted (see `CreateArgs::title`).
    #[arg(long, value_parser = validate_title)]
    pub title: Option<String>,

    /// New description
    #[arg(short = 'D', long, allow_hyphen_values = true, value_parser = validate_description)]
    pub description: Option<String>,

    /// New status
    #[arg(short, long, value_enum)]
    pub status: Option<IssueStatus>,

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

    /// Note to append
    #[arg(long, allow_hyphen_values = true)]
    pub notes: Option<String>,
}

/// Arguments for an Assignment Claim or Release.
#[derive(Parser, Debug, Clone)]
pub struct AssignmentArgs {
    /// Issue ID whose Assignment changes.
    #[arg(value_parser = validate_issue_id)]
    pub issue_id: String,

    /// Exact Assignee identity to claim as or release.
    #[arg(short, long)]
    pub assignee: String,
}

impl UpdateArgs {
    /// Returns a formatted string of available flags for error messages.
    ///
    /// This dynamically generates the list from clap's argument definitions,
    /// ensuring it stays in sync with the actual struct fields.
    #[must_use]
    pub fn available_flags_help() -> String {
        use clap::CommandFactory;

        let cmd = Self::command();
        cmd.get_arguments()
            .filter(|arg| {
                // Filter out positional arguments (issue_ids) and help/version
                let id = arg.get_id().as_str();
                arg.get_long().is_some() && id != "help" && id != "version"
            })
            .map(|arg| {
                let long = format!("--{}", arg.get_long().unwrap());
                match arg.get_short() {
                    Some(short) => format!("{} (-{})", long, short),
                    None => long,
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Returns true if any update field is specified.
    #[must_use]
    pub fn has_updates(&self) -> bool {
        self.title.is_some()
            || self.description.is_some()
            || self.status.is_some()
            || self.priority.is_some()
            || self.issue_kind.is_some()
            || self.design.is_some()
            || self.acceptance.is_some()
            || self.notes.is_some()
    }
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
    pub label: Option<String>,

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
    pub status: Option<IssueStatus>,

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

#[cfg(test)]
mod tests {
    use super::*;

    mod update_args_has_updates_tests {
        use super::*;

        fn create_empty_update_args() -> UpdateArgs {
            UpdateArgs {
                issue_ids: vec!["test-abc".to_string()],
                title: None,
                description: None,
                status: None,
                priority: None,
                issue_kind: None,
                design: None,
                acceptance: None,
                notes: None,
            }
        }

        #[test]
        fn test_has_updates_returns_false_when_all_fields_none() {
            let args = create_empty_update_args();
            assert!(!args.has_updates());
        }

        #[test]
        fn test_has_updates_title() {
            let mut args = create_empty_update_args();
            args.title = Some("New title".to_string());
            assert!(args.has_updates());
        }

        #[test]
        fn test_has_updates_description() {
            let mut args = create_empty_update_args();
            args.description = Some("New description".to_string());
            assert!(args.has_updates());
        }

        #[test]
        fn test_has_updates_status() {
            let mut args = create_empty_update_args();
            args.status = Some(IssueStatus::InProgress);
            assert!(args.has_updates());
        }

        #[test]
        fn test_has_updates_priority() {
            let mut args = create_empty_update_args();
            args.priority = Some(1);
            assert!(args.has_updates());
        }

        #[test]
        fn test_has_updates_issue_kind() {
            let mut args = create_empty_update_args();
            args.issue_kind = Some(IssueKind::Bug);
            assert!(args.has_updates());
        }

        #[test]
        fn test_has_updates_design() {
            let mut args = create_empty_update_args();
            args.design = Some("Design notes".to_string());
            assert!(args.has_updates());
        }

        #[test]
        fn test_has_updates_acceptance() {
            let mut args = create_empty_update_args();
            args.acceptance = Some("Acceptance criteria".to_string());
            assert!(args.has_updates());
        }

        #[test]
        fn test_has_updates_notes() {
            let mut args = create_empty_update_args();
            args.notes = Some("Notes".to_string());
            assert!(args.has_updates());
        }

        #[test]
        fn test_has_updates_multiple_fields() {
            let mut args = create_empty_update_args();
            args.title = Some("New title".to_string());
            args.priority = Some(1);
            args.notes = Some("Notes".to_string());
            assert!(args.has_updates());
        }
    }

    mod available_flags_help_tests {
        use super::*;

        #[test]
        fn test_contains_expected_flags() {
            let help = UpdateArgs::available_flags_help();

            // Verify all expected flags are present
            let expected_flags = [
                "--title",
                "--description",
                "--status",
                "--priority",
                "--kind",
                "--design",
                "--acceptance",
                "--notes",
            ];

            for flag in expected_flags {
                assert!(
                    help.contains(flag),
                    "Expected flag '{}' not found in help: {}",
                    flag,
                    help
                );
            }
            assert!(!help.contains("assignee"));
        }

        #[test]
        fn test_contains_short_flags_where_defined() {
            let help = UpdateArgs::available_flags_help();

            // These flags have short versions defined in the struct
            assert!(
                help.contains("(-D)"),
                "Expected short flag -D for description, got: {}",
                help
            );
            assert!(
                help.contains("(-s)"),
                "Expected short flag -s for status, got: {}",
                help
            );
            assert!(
                help.contains("(-p)"),
                "Expected short flag -p for priority, got: {}",
                help
            );
        }

        #[test]
        fn test_excludes_positional_and_meta_args() {
            let help = UpdateArgs::available_flags_help();

            // Should not contain help/version or positional args
            assert!(
                !help.contains("--help"),
                "Should not contain --help: {}",
                help
            );
            assert!(
                !help.contains("--version"),
                "Should not contain --version: {}",
                help
            );
            // issue_ids is positional, should not appear
            assert!(
                !help.contains("issue_ids"),
                "Should not contain positional arg: {}",
                help
            );
        }

        #[test]
        fn test_format_is_comma_separated() {
            let help = UpdateArgs::available_flags_help();

            // Should be comma-separated
            assert!(
                help.contains(", "),
                "Expected comma-separated format: {}",
                help
            );

            // Count commas to verify multiple flags
            let comma_count = help.matches(", ").count();
            assert!(
                comma_count >= 5,
                "Expected at least 5 commas (6+ flags), got {}: {}",
                comma_count,
                help
            );
        }
    }
}
