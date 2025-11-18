//! Domain types for issue tracking.
//!
//! This module contains the core domain types for the rivets issue tracker.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Unique identifier for an issue
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IssueId(pub String);

impl IssueId {
    /// Create a new issue ID
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
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

    /// Dependencies on other issues
    pub dependencies: Vec<Dependency>,

    /// Design notes (optional)
    pub design: Option<String>,

    /// Acceptance criteria (optional)
    pub acceptance_criteria: Option<String>,

    /// Additional notes
    pub notes: Option<String>,

    /// External reference (e.g., GitHub issue number)
    pub external_ref: Option<String>,

    /// Creation timestamp (ISO 8601)
    pub created_at: String,

    /// Last update timestamp (ISO 8601)
    pub updated_at: String,

    /// Closed timestamp (ISO 8601, optional)
    pub closed_at: Option<String>,
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

/// Dependency between issues
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dependency {
    /// ID of the issue this depends on
    pub depends_on_id: IssueId,

    /// Type of dependency
    pub dep_type: DependencyType,
}

/// Type of dependency relationship
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

    /// New assignee (if updating, None to clear)
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
