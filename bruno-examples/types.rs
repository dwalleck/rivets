//! Domain types for the .rivet issue format.

use chrono::{DateTime, Utc};

/// A complete issue document parsed from a .rivet file.
#[derive(Debug, Clone, PartialEq)]
pub struct RivetDocument {
    pub meta: IssueMeta,
    pub title: String,
    pub description: String,
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
    pub dependencies: Vec<Dependency>,
    pub notes: Option<String>,
    pub design: Option<String>,
}

impl Default for RivetDocument {
    fn default() -> Self {
        Self {
            meta: IssueMeta::default(),
            title: String::new(),
            description: String::new(),
            labels: Vec::new(),
            assignees: Vec::new(),
            dependencies: Vec::new(),
            notes: None,
            design: None,
        }
    }
}

/// Metadata fields for an issue.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueMeta {
    pub id: String,
    pub status: IssueStatus,
    pub priority: u8,
    pub created: DateTime<Utc>,
    pub updated: Option<DateTime<Utc>>,
    pub closed: Option<DateTime<Utc>>,
}

impl Default for IssueMeta {
    fn default() -> Self {
        Self {
            id: String::new(),
            status: IssueStatus::Open,
            priority: 2,
            created: Utc::now(),
            updated: None,
            closed: None,
        }
    }
}

/// The status of an issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IssueStatus {
    #[default]
    Open,
    InProgress,
    Blocked,
    Closed,
}

impl IssueStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            IssueStatus::Open => "open",
            IssueStatus::InProgress => "in-progress",
            IssueStatus::Blocked => "blocked",
            IssueStatus::Closed => "closed",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "open" => Some(IssueStatus::Open),
            "in-progress" | "in_progress" | "inprogress" => Some(IssueStatus::InProgress),
            "blocked" => Some(IssueStatus::Blocked),
            "closed" => Some(IssueStatus::Closed),
            _ => None,
        }
    }
}

/// A dependency relationship to another issue.
#[derive(Debug, Clone, PartialEq)]
pub struct Dependency {
    pub issue_id: String,
    pub dep_type: DependencyType,
}

/// The type of dependency relationship.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DependencyType {
    /// This issue blocks the referenced issue
    Blocks,
    /// This issue is blocked by the referenced issue
    BlockedBy,
    /// This issue is related to the referenced issue
    #[default]
    Related,
}

impl DependencyType {
    pub fn as_str(&self) -> &'static str {
        match self {
            DependencyType::Blocks => "blocks",
            DependencyType::BlockedBy => "blocked-by",
            DependencyType::Related => "related",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "blocks" => Some(DependencyType::Blocks),
            "blocked-by" | "blocked_by" | "blockedby" => Some(DependencyType::BlockedBy),
            "related" => Some(DependencyType::Related),
            _ => None,
        }
    }
}
