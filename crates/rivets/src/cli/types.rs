//! CLI value enums and domain type conversions.
//!
//! This module contains the value enums used for CLI argument parsing
//! and their conversions to/from domain types.

use clap::ValueEnum;

use crate::domain::{DependencyType, IssueStatus, IssueType};

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
// Domain Type Conversions
// ============================================================================

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

#[cfg(test)]
mod tests {
    use super::*;

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
