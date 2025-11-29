//! MCP models.
//!
//! This module contains types for MCP tool inputs and outputs.
//! They wrap or transform rivets domain types for MCP compatibility.

use rivets::domain::{Dependency, DependencyType, Issue, IssueStatus, IssueType};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ============================================================================
// Tool Input Parameters
// ============================================================================

/// Parameters for the `set_context` tool.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SetContextParams {
    /// The workspace root directory path.
    pub workspace_root: String,
}

/// Parameters for the `ready` tool.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct ReadyParams {
    /// Maximum number of issues to return.
    pub limit: Option<usize>,

    /// Filter by priority level.
    pub priority: Option<u8>,

    /// Filter by assignee.
    pub assignee: Option<String>,

    /// Optional workspace root (uses current context if not specified).
    pub workspace_root: Option<String>,
}

/// Parameters for the `list` tool.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct ListParams {
    /// Filter by status.
    pub status: Option<String>,

    /// Filter by priority level.
    pub priority: Option<u8>,

    /// Filter by issue type.
    pub issue_type: Option<String>,

    /// Filter by assignee.
    pub assignee: Option<String>,

    /// Maximum number of issues to return.
    pub limit: Option<usize>,

    /// Optional workspace root (uses current context if not specified).
    pub workspace_root: Option<String>,
}

/// Parameters for the `show` tool.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ShowParams {
    /// The issue ID to show.
    pub issue_id: String,

    /// Optional workspace root (uses current context if not specified).
    pub workspace_root: Option<String>,
}

/// Parameters for the `blocked` tool.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct BlockedParams {
    /// Optional workspace root (uses current context if not specified).
    pub workspace_root: Option<String>,
}

/// Parameters for the `create` tool.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CreateParams {
    /// Issue title.
    pub title: String,

    /// Issue description.
    pub description: Option<String>,

    /// Priority level (1-5, default 2).
    pub priority: Option<u8>,

    /// Issue type (bug, feature, task, epic, chore).
    pub issue_type: Option<String>,

    /// Assignee.
    pub assignee: Option<String>,

    /// Labels.
    pub labels: Option<Vec<String>>,

    /// Design notes.
    pub design: Option<String>,

    /// Acceptance criteria.
    pub acceptance: Option<String>,

    /// Optional workspace root (uses current context if not specified).
    pub workspace_root: Option<String>,
}

/// Parameters for the `update` tool.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UpdateParams {
    /// The issue ID to update.
    pub issue_id: String,

    /// New status.
    pub status: Option<String>,

    /// New priority.
    pub priority: Option<u8>,

    /// New assignee.
    pub assignee: Option<String>,

    /// New title.
    pub title: Option<String>,

    /// New description.
    pub description: Option<String>,

    /// New design notes.
    pub design: Option<String>,

    /// New acceptance criteria.
    pub acceptance_criteria: Option<String>,

    /// New notes.
    pub notes: Option<String>,

    /// New external reference.
    pub external_ref: Option<String>,

    /// Optional workspace root (uses current context if not specified).
    pub workspace_root: Option<String>,
}

/// Parameters for the `close` tool.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CloseParams {
    /// The issue ID to close.
    pub issue_id: String,

    /// Reason for closing.
    pub reason: Option<String>,

    /// Optional workspace root (uses current context if not specified).
    pub workspace_root: Option<String>,
}

/// Parameters for the `dep` tool.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DepParams {
    /// The issue that has the dependency.
    pub issue_id: String,

    /// The issue that is depended on.
    pub depends_on_id: String,

    /// Dependency type (blocks, related, parent-child, discovered-from).
    pub dep_type: Option<String>,

    /// Optional workspace root (uses current context if not specified).
    pub workspace_root: Option<String>,
}

// ============================================================================
// Tool Output Responses
// ============================================================================

/// Response from the `set_context` tool.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SetContextResponse {
    /// The workspace root that was set.
    pub workspace_root: String,

    /// The path to the database file.
    pub database_path: String,

    /// Status message.
    pub message: String,
}

/// Response from the `where_am_i` tool.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WhereAmIResponse {
    /// The current workspace root, if set.
    pub workspace_root: Option<String>,

    /// The current database path, if set.
    pub database_path: Option<String>,

    /// Whether a context is currently set.
    pub context_set: bool,
}

/// Issue representation for MCP responses.
///
/// This is a simplified view of an issue optimized for MCP transport.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct McpIssue {
    /// Unique identifier.
    pub id: String,

    /// Issue title.
    pub title: String,

    /// Issue description.
    pub description: String,

    /// Current status.
    pub status: String,

    /// Priority level (0-4).
    pub priority: u8,

    /// Issue type.
    pub issue_type: String,

    /// Assignee, if any.
    pub assignee: Option<String>,

    /// Labels.
    pub labels: Vec<String>,

    /// Design notes.
    pub design: Option<String>,

    /// Acceptance criteria.
    pub acceptance_criteria: Option<String>,

    /// Additional notes.
    pub notes: Option<String>,

    /// External reference.
    pub external_ref: Option<String>,

    /// Dependencies.
    pub dependencies: Vec<McpDependency>,

    /// Creation timestamp (ISO 8601).
    pub created_at: String,

    /// Last update timestamp (ISO 8601).
    pub updated_at: String,

    /// Closed timestamp (ISO 8601), if closed.
    pub closed_at: Option<String>,
}

impl From<Issue> for McpIssue {
    fn from(issue: Issue) -> Self {
        Self {
            id: issue.id.to_string(),
            title: issue.title,
            description: issue.description,
            status: status_to_str(issue.status).to_string(),
            priority: issue.priority,
            issue_type: issue_type_to_str(issue.issue_type).to_string(),
            assignee: issue.assignee,
            labels: issue.labels,
            design: issue.design,
            acceptance_criteria: issue.acceptance_criteria,
            notes: issue.notes,
            external_ref: issue.external_ref,
            dependencies: issue.dependencies.into_iter().map(Into::into).collect(),
            created_at: issue.created_at.to_rfc3339(),
            updated_at: issue.updated_at.to_rfc3339(),
            closed_at: issue.closed_at.map(|t| t.to_rfc3339()),
        }
    }
}

/// Dependency representation for MCP responses.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct McpDependency {
    /// ID of the issue this depends on.
    pub depends_on_id: String,

    /// Type of dependency.
    pub dep_type: String,
}

impl From<Dependency> for McpDependency {
    fn from(dep: Dependency) -> Self {
        Self {
            depends_on_id: dep.depends_on_id.to_string(),
            dep_type: dep_type_to_str(dep.dep_type).to_string(),
        }
    }
}

/// Blocked issue response.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BlockedIssueResponse {
    /// The blocked issue.
    pub issue: McpIssue,

    /// Issues blocking this one.
    pub blockers: Vec<McpIssue>,
}

/// Statistics response.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StatsResponse {
    /// Total number of issues.
    pub total: usize,

    /// Number of open issues.
    pub open: usize,

    /// Number of in-progress issues.
    pub in_progress: usize,

    /// Number of blocked issues.
    pub blocked: usize,

    /// Number of closed issues.
    pub closed: usize,

    /// Number of ready-to-work issues.
    pub ready: usize,
}

// Helper functions for converting enums to strings
// These return &'static str to avoid unnecessary allocations

fn status_to_str(status: IssueStatus) -> &'static str {
    match status {
        IssueStatus::Open => "open",
        IssueStatus::InProgress => "in_progress",
        IssueStatus::Blocked => "blocked",
        IssueStatus::Closed => "closed",
    }
}

fn issue_type_to_str(issue_type: IssueType) -> &'static str {
    match issue_type {
        IssueType::Bug => "bug",
        IssueType::Feature => "feature",
        IssueType::Task => "task",
        IssueType::Epic => "epic",
        IssueType::Chore => "chore",
    }
}

/// Convert a `DependencyType` to its string representation.
#[must_use]
pub fn dep_type_to_str(dep_type: DependencyType) -> &'static str {
    match dep_type {
        DependencyType::Blocks => "blocks",
        DependencyType::Related => "related",
        DependencyType::ParentChild => "parent-child",
        DependencyType::DiscoveredFrom => "discovered-from",
    }
}

/// Parse a status string into an `IssueStatus`.
#[must_use]
pub fn parse_status(s: &str) -> Option<IssueStatus> {
    match s.to_lowercase().as_str() {
        "open" => Some(IssueStatus::Open),
        "in_progress" | "in-progress" => Some(IssueStatus::InProgress),
        "blocked" => Some(IssueStatus::Blocked),
        "closed" => Some(IssueStatus::Closed),
        _ => None,
    }
}

/// Parse an issue type string into an `IssueType`.
#[must_use]
pub fn parse_issue_type(s: &str) -> Option<IssueType> {
    match s.to_lowercase().as_str() {
        "bug" => Some(IssueType::Bug),
        "feature" => Some(IssueType::Feature),
        "task" => Some(IssueType::Task),
        "epic" => Some(IssueType::Epic),
        "chore" => Some(IssueType::Chore),
        _ => None,
    }
}

/// Parse a dependency type string into a `DependencyType`.
#[must_use]
pub fn parse_dep_type(s: &str) -> Option<DependencyType> {
    match s.to_lowercase().as_str() {
        "blocks" => Some(DependencyType::Blocks),
        "related" => Some(DependencyType::Related),
        "parent-child" | "parent_child" => Some(DependencyType::ParentChild),
        "discovered-from" | "discovered_from" => Some(DependencyType::DiscoveredFrom),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case::open("open", Some(IssueStatus::Open))]
    #[case::open_uppercase("OPEN", Some(IssueStatus::Open))]
    #[case::in_progress_underscore("in_progress", Some(IssueStatus::InProgress))]
    #[case::in_progress_hyphen("in-progress", Some(IssueStatus::InProgress))]
    #[case::blocked("blocked", Some(IssueStatus::Blocked))]
    #[case::closed("closed", Some(IssueStatus::Closed))]
    #[case::invalid("invalid", None)]
    #[case::empty("", None)]
    fn test_parse_status(#[case] input: &str, #[case] expected: Option<IssueStatus>) {
        assert_eq!(parse_status(input), expected);
    }

    #[rstest]
    #[case::bug("bug", Some(IssueType::Bug))]
    #[case::feature("feature", Some(IssueType::Feature))]
    #[case::task("task", Some(IssueType::Task))]
    #[case::epic("epic", Some(IssueType::Epic))]
    #[case::chore("chore", Some(IssueType::Chore))]
    #[case::uppercase("BUG", Some(IssueType::Bug))]
    #[case::invalid("invalid", None)]
    fn test_parse_issue_type(#[case] input: &str, #[case] expected: Option<IssueType>) {
        assert_eq!(parse_issue_type(input), expected);
    }

    #[rstest]
    #[case::blocks("blocks", Some(DependencyType::Blocks))]
    #[case::related("related", Some(DependencyType::Related))]
    #[case::parent_child_hyphen("parent-child", Some(DependencyType::ParentChild))]
    #[case::parent_child_underscore("parent_child", Some(DependencyType::ParentChild))]
    #[case::discovered_from_hyphen("discovered-from", Some(DependencyType::DiscoveredFrom))]
    #[case::discovered_from_underscore("discovered_from", Some(DependencyType::DiscoveredFrom))]
    #[case::uppercase("BLOCKS", Some(DependencyType::Blocks))]
    #[case::invalid("invalid", None)]
    fn test_parse_dep_type(#[case] input: &str, #[case] expected: Option<DependencyType>) {
        assert_eq!(parse_dep_type(input), expected);
    }
}
