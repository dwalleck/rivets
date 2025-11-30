//! Domain types for issue tracking.
//!
//! This module contains the core domain types for the rivets issue tracker.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Unique identifier for an issue
///
/// Wraps a string ID in a newtype for type safety. The inner field is private
/// to enforce encapsulation and allow future changes to the ID format.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct IssueId(String);

impl IssueId {
    /// Create a new issue ID
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Get the ID as a string slice
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for IssueId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for IssueId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for IssueId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Represents an issue in the tracking system
///
/// Note: Dependencies are managed by the storage backend and accessed via
/// `IssueStorage::get_dependencies()` rather than being stored on the Issue
/// itself. This prevents data duplication and ensures a single source of truth.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
    /// Unique identifier for the issue
    pub id: IssueId,

    /// Issue title
    pub title: String,

    /// Issue description
    pub description: String,

    /// Current status
    pub status: IssueStatus,

    /// Priority level (0 = highest, 4 = lowest)
    pub priority: u8,

    /// Issue type
    pub issue_type: IssueType,

    /// Assignee (optional)
    pub assignee: Option<String>,

    /// Labels
    pub labels: Vec<String>,

    /// Design notes (optional)
    pub design: Option<String>,

    /// Acceptance criteria (optional)
    pub acceptance_criteria: Option<String>,

    /// Additional notes
    pub notes: Option<String>,

    /// External reference (e.g., GitHub issue number)
    pub external_ref: Option<String>,

    /// Dependencies (issues this issue depends on)
    ///
    /// **Note**: This field is maintained for JSONL serialization. The dependency
    /// graph in storage (petgraph) is the source of truth for internal operations.
    /// This field should be kept in sync with the graph.
    ///
    /// **Ordering**: Dependencies are sorted lexicographically by `depends_on_id` and then
    /// by `dep_type` before serialization to ensure deterministic JSONL output. This prevents
    /// spurious diffs in version control when dependencies are added/removed in different orders.
    pub dependencies: Vec<Dependency>,

    /// Creation timestamp
    pub created_at: DateTime<Utc>,

    /// Last update timestamp
    pub updated_at: DateTime<Utc>,

    /// Closed timestamp (optional)
    pub closed_at: Option<DateTime<Utc>>,
}

impl Issue {
    /// Validate issue data integrity
    ///
    /// Checks:
    /// - Title is not empty and within MAX_TITLE_LENGTH
    /// - Priority is within valid range (0-4)
    ///
    /// Returns Ok(()) if valid, Err with description if invalid.
    pub fn validate(&self) -> Result<(), String> {
        let trimmed_title = self.title.trim();

        if trimmed_title.is_empty() {
            return Err("Title cannot be empty".to_string());
        }

        if self.title.len() > MAX_TITLE_LENGTH {
            return Err(format!(
                "Title length ({}) exceeds maximum of {}",
                self.title.len(),
                MAX_TITLE_LENGTH
            ));
        }

        if self.priority > 4 {
            return Err(format!("Priority {} exceeds maximum of 4", self.priority));
        }

        Ok(())
    }
}

/// Status of an issue
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueStatus {
    /// Issue is open and ready to work on
    Open,

    /// Issue is currently being worked on
    #[serde(rename = "in_progress")]
    InProgress,

    /// Issue is blocked by dependencies
    Blocked,

    /// Issue has been completed
    Closed,
}

impl fmt::Display for IssueStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open => write!(f, "open"),
            Self::InProgress => write!(f, "in_progress"),
            Self::Blocked => write!(f, "blocked"),
            Self::Closed => write!(f, "closed"),
        }
    }
}

/// Type of issue
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IssueType {
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

impl fmt::Display for IssueType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bug => write!(f, "bug"),
            Self::Feature => write!(f, "feature"),
            Self::Task => write!(f, "task"),
            Self::Epic => write!(f, "epic"),
            Self::Chore => write!(f, "chore"),
        }
    }
}

/// Dependency between issues
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Dependency {
    /// ID of the issue this depends on
    pub depends_on_id: IssueId,

    /// Type of dependency
    pub dep_type: DependencyType,
}

/// Type of dependency relationship
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DependencyType {
    /// Hard blocker - prevents work
    Blocks,

    /// Soft link - informational
    Related,

    /// Hierarchical - epic to task
    ParentChild,

    /// Found during work
    DiscoveredFrom,
}

impl fmt::Display for DependencyType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Blocks => write!(f, "blocks"),
            Self::Related => write!(f, "related"),
            Self::ParentChild => write!(f, "parent-child"),
            Self::DiscoveredFrom => write!(f, "discovered-from"),
        }
    }
}

/// Sort policy for ready work queries.
///
/// Controls how ready-to-work issues are ordered in the results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortPolicy {
    /// Hybrid sorting (default): Recent issues (< 48h) by priority, older by age.
    ///
    /// This balances urgency with preventing starvation of older issues:
    /// - Issues created within the last 48 hours are sorted by priority (P0 first)
    /// - Older issues are sorted by creation date (oldest first)
    /// - Recent issues come before older issues at the same priority level
    #[default]
    Hybrid,

    /// Strict priority sorting: P0 -> P1 -> P2 -> P3 -> P4.
    ///
    /// Issues are sorted purely by priority, with ties broken by creation date
    /// (oldest first within the same priority).
    Priority,

    /// Age-based sorting: oldest issues first.
    ///
    /// Issues are sorted by creation date ascending, ignoring priority.
    /// Use this to prevent starvation of older, lower-priority issues.
    Oldest,
}

/// Maximum length for issue titles
pub const MAX_TITLE_LENGTH: usize = 200;

/// Minimum priority level (0 = critical)
pub const MIN_PRIORITY: u8 = 0;

/// Maximum priority level (4 = backlog)
pub const MAX_PRIORITY: u8 = 4;

/// Data for creating a new issue
#[derive(Debug, Clone)]
pub struct NewIssue {
    /// Issue title
    pub title: String,

    /// Issue description
    pub description: String,

    /// Priority level (0-4)
    pub priority: u8,

    /// Issue type
    pub issue_type: IssueType,

    /// Assignee (optional)
    pub assignee: Option<String>,

    /// Labels
    pub labels: Vec<String>,

    /// Design notes (optional)
    pub design: Option<String>,

    /// Acceptance criteria (optional)
    pub acceptance_criteria: Option<String>,

    /// Additional notes
    pub notes: Option<String>,

    /// External reference
    pub external_ref: Option<String>,

    /// Dependencies
    pub dependencies: Vec<(IssueId, DependencyType)>,
}

impl NewIssue {
    /// Validate the new issue data
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Title is empty or exceeds MAX_TITLE_LENGTH
    /// - Priority is not in range 0-4
    pub fn validate(&self) -> Result<(), String> {
        let trimmed_title = self.title.trim();

        if trimmed_title.is_empty() {
            return Err("Title cannot be empty".to_string());
        }

        if trimmed_title.len() > MAX_TITLE_LENGTH {
            return Err(format!(
                "Title cannot exceed {} characters (got {})",
                MAX_TITLE_LENGTH,
                trimmed_title.len()
            ));
        }

        if self.priority > 4 {
            return Err(format!(
                "Priority must be in range 0-4 (got {})",
                self.priority
            ));
        }

        Ok(())
    }
}

impl Default for NewIssue {
    /// Create a NewIssue with sensible defaults for testing.
    ///
    /// Default values:
    /// - title: "Untitled Issue"
    /// - description: ""
    /// - priority: 2 (medium)
    /// - issue_type: Task
    /// - All optional fields: None or empty
    fn default() -> Self {
        Self {
            title: "Untitled Issue".to_string(),
            description: String::new(),
            priority: 2,
            issue_type: IssueType::Task,
            assignee: None,
            labels: vec![],
            design: None,
            acceptance_criteria: None,
            notes: None,
            external_ref: None,
            dependencies: vec![],
        }
    }
}

/// Data for updating an existing issue
#[derive(Debug, Clone, Default)]
pub struct IssueUpdate {
    /// New title (if updating)
    pub title: Option<String>,

    /// New description (if updating)
    pub description: Option<String>,

    /// New status (if updating)
    pub status: Option<IssueStatus>,

    /// New priority (if updating)
    pub priority: Option<u8>,

    /// New assignee (if updating)
    ///
    /// This uses the double-Option pattern to represent three distinct states:
    /// - `None`: Don't modify the assignee (leave unchanged)
    /// - `Some(None)`: Clear the assignee (set to unassigned)
    /// - `Some(Some(name))`: Set assignee to the given name
    pub assignee: Option<Option<String>>,

    /// New design notes (if updating)
    pub design: Option<String>,

    /// New acceptance criteria (if updating)
    pub acceptance_criteria: Option<String>,

    /// New notes (if updating)
    pub notes: Option<String>,

    /// New external reference (if updating)
    pub external_ref: Option<String>,
}

/// Filter for querying issues
#[derive(Debug, Clone, Default)]
pub struct IssueFilter {
    /// Filter by status
    pub status: Option<IssueStatus>,

    /// Filter by priority
    pub priority: Option<u8>,

    /// Filter by issue type
    pub issue_type: Option<IssueType>,

    /// Filter by assignee
    pub assignee: Option<String>,

    /// Filter by label
    pub label: Option<String>,

    /// Limit number of results
    pub limit: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===== IssueId Tests =====

    #[test]
    fn test_issue_id_display() {
        let id = IssueId::new("test-123");
        assert_eq!(format!("{}", id), "test-123");
    }

    #[test]
    fn test_issue_id_from_string() {
        let id = IssueId::from("test-456".to_string());
        assert_eq!(id.as_str(), "test-456");
    }

    #[test]
    fn test_issue_id_from_str() {
        let id = IssueId::from("test-789");
        assert_eq!(id.as_str(), "test-789");
    }

    #[test]
    fn test_issue_id_as_str() {
        let id = IssueId::new("proj-abc");
        assert_eq!(id.as_str(), "proj-abc");
    }

    #[test]
    fn test_issue_id_equality() {
        let id1 = IssueId::new("same-id");
        let id2 = IssueId::new("same-id");
        let id3 = IssueId::new("different-id");

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
    }

    // ===== NewIssue::validate() Tests =====

    #[test]
    fn test_validate_valid_issue() {
        let issue = NewIssue {
            title: "Valid Title".to_string(),
            priority: 2,
            ..Default::default()
        };
        assert!(issue.validate().is_ok());
    }

    #[test]
    fn test_validate_empty_title() {
        let issue = NewIssue {
            title: "".to_string(),
            ..Default::default()
        };
        let result = issue.validate();
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Title cannot be empty");
    }

    #[test]
    fn test_validate_whitespace_only_title() {
        let issue = NewIssue {
            title: "   \t\n  ".to_string(),
            ..Default::default()
        };
        let result = issue.validate();
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Title cannot be empty");
    }

    #[test]
    fn test_validate_title_too_long() {
        let long_title = "x".repeat(MAX_TITLE_LENGTH + 1);
        let issue = NewIssue {
            title: long_title.clone(),
            ..Default::default()
        };
        let result = issue.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains(&format!("cannot exceed {}", MAX_TITLE_LENGTH)));
    }

    #[test]
    fn test_validate_title_exactly_max_length() {
        let max_title = "x".repeat(MAX_TITLE_LENGTH);
        let issue = NewIssue {
            title: max_title,
            ..Default::default()
        };
        assert!(issue.validate().is_ok());
    }

    #[test]
    fn test_validate_title_with_whitespace() {
        let issue = NewIssue {
            title: "  Valid Title  ".to_string(),
            ..Default::default()
        };
        assert!(issue.validate().is_ok());
    }

    #[test]
    fn test_validate_invalid_priority_low() {
        let issue = NewIssue {
            title: "Valid Title".to_string(),
            priority: 5,
            ..Default::default()
        };
        let result = issue.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("Priority must be in range 0-4"));
    }

    #[test]
    fn test_validate_invalid_priority_high() {
        let issue = NewIssue {
            title: "Valid Title".to_string(),
            priority: 255,
            ..Default::default()
        };
        let result = issue.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("Priority must be in range 0-4"));
    }

    #[test]
    fn test_validate_priority_boundaries() {
        for priority in 0..=4 {
            let issue = NewIssue {
                title: "Valid Title".to_string(),
                priority,
                ..Default::default()
            };
            assert!(
                issue.validate().is_ok(),
                "Priority {} should be valid",
                priority
            );
        }
    }

    // ===== NewIssue::default() Tests =====

    #[test]
    fn test_new_issue_default() {
        let issue = NewIssue::default();
        assert_eq!(issue.title, "Untitled Issue");
        assert_eq!(issue.description, "");
        assert_eq!(issue.priority, 2);
        assert_eq!(issue.issue_type, IssueType::Task);
        assert!(issue.assignee.is_none());
        assert!(issue.labels.is_empty());
        assert!(issue.dependencies.is_empty());
    }

    #[test]
    fn test_new_issue_default_validates() {
        let issue = NewIssue::default();
        assert!(issue.validate().is_ok());
    }

    // ===== Display Implementation Tests =====

    #[test]
    fn test_issue_status_display() {
        assert_eq!(format!("{}", IssueStatus::Open), "open");
        assert_eq!(format!("{}", IssueStatus::InProgress), "in_progress");
        assert_eq!(format!("{}", IssueStatus::Blocked), "blocked");
        assert_eq!(format!("{}", IssueStatus::Closed), "closed");
    }

    #[test]
    fn test_issue_type_display() {
        assert_eq!(format!("{}", IssueType::Bug), "bug");
        assert_eq!(format!("{}", IssueType::Feature), "feature");
        assert_eq!(format!("{}", IssueType::Task), "task");
        assert_eq!(format!("{}", IssueType::Epic), "epic");
        assert_eq!(format!("{}", IssueType::Chore), "chore");
    }

    #[test]
    fn test_dependency_type_display() {
        assert_eq!(format!("{}", DependencyType::Blocks), "blocks");
        assert_eq!(format!("{}", DependencyType::Related), "related");
        assert_eq!(format!("{}", DependencyType::ParentChild), "parent-child");
        assert_eq!(
            format!("{}", DependencyType::DiscoveredFrom),
            "discovered-from"
        );
    }
}
