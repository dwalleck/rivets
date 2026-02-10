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
    /// Maximum length defined by `MAX_TITLE_LENGTH` (currently 200 characters).
    #[arg(long, value_parser = validate_title)]
    pub title: Option<String>,

    /// Detailed description
    #[arg(short = 'D', long, allow_hyphen_values = true, value_parser = validate_description)]
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
    #[arg(long, allow_hyphen_values = true)]
    pub design: Option<String>,

    /// Acceptance criteria
    #[arg(long, allow_hyphen_values = true)]
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

    /// New title (maximum length: `MAX_TITLE_LENGTH`)
    #[arg(long, value_parser = validate_title)]
    pub title: Option<String>,

    /// New description
    #[arg(short = 'D', long, allow_hyphen_values = true, value_parser = validate_description)]
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
    #[arg(long, allow_hyphen_values = true)]
    pub design: Option<String>,

    /// New acceptance criteria
    #[arg(long, allow_hyphen_values = true)]
    pub acceptance: Option<String>,

    /// New notes
    #[arg(long, allow_hyphen_values = true)]
    pub notes: Option<String>,

    /// New external reference
    #[arg(long)]
    pub external_ref: Option<String>,
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
            || self.assignee.is_some()
            || self.no_assignee
            || self.design.is_some()
            || self.acceptance.is_some()
            || self.notes.is_some()
            || self.external_ref.is_some()
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
                assignee: None,
                no_assignee: false,
                design: None,
                acceptance: None,
                notes: None,
                external_ref: None,
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
            args.status = Some(IssueStatusArg::InProgress);
            assert!(args.has_updates());
        }

        #[test]
        fn test_has_updates_priority() {
            let mut args = create_empty_update_args();
            args.priority = Some(1);
            assert!(args.has_updates());
        }

        #[test]
        fn test_has_updates_assignee() {
            let mut args = create_empty_update_args();
            args.assignee = Some("user@example.com".to_string());
            assert!(args.has_updates());
        }

        #[test]
        fn test_has_updates_no_assignee_flag() {
            let mut args = create_empty_update_args();
            args.no_assignee = true;
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
        fn test_has_updates_external_ref() {
            let mut args = create_empty_update_args();
            args.external_ref = Some("https://example.com/issue".to_string());
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
                "--assignee",
                "--no-assignee",
                "--design",
                "--acceptance",
                "--notes",
                "--external-ref",
            ];

            for flag in expected_flags {
                assert!(
                    help.contains(flag),
                    "Expected flag '{}' not found in help: {}",
                    flag,
                    help
                );
            }
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
            assert!(
                help.contains("(-a)"),
                "Expected short flag -a for assignee, got: {}",
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
