//! Domain types for issue tracking.
//!
//! This module contains the core domain types for the rivets issue tracker.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

mod resource;

pub use resource::{
    AssociatedResource, NewResource, ResourceError, ResourceId, ResourceLabel, ResourceRole,
    ResourceTarget, WebUrl,
};

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

/// Validated content for a Note that has not yet been timestamped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteContent(String);

impl NoteContent {
    /// Parse Note content without changing its bytes.
    ///
    /// # Errors
    ///
    /// Returns an error when the content is empty after trimming or contains
    /// a control character that is unsafe for multiline terminal output.
    pub fn new(content: impl Into<String>) -> Result<Self, NoteError> {
        let content = content.into();
        if content.trim().is_empty() {
            return Err(NoteError::EmptyContent);
        }
        if let Some(position) = find_control_char_multiline(&content) {
            return Err(NoteError::InvalidControlCharacter { position });
        }
        Ok(Self(content))
    }

    /// Construct canonical Note content for a close reason.
    ///
    /// The reason is validated before the lifecycle prefix is added so empty
    /// input is rejected and control-character positions refer to user input.
    pub fn closing_reason(reason: impl Into<String>) -> Result<Self, NoteError> {
        Self::lifecycle_reason("Closed", reason)
    }

    /// Construct canonical Note content for a reopen reason.
    ///
    /// The reason is validated before the lifecycle prefix is added so empty
    /// input is rejected and control-character positions refer to user input.
    pub fn reopening_reason(reason: impl Into<String>) -> Result<Self, NoteError> {
        Self::lifecycle_reason("Reopened", reason)
    }

    fn lifecycle_reason(prefix: &str, reason: impl Into<String>) -> Result<Self, NoteError> {
        let reason = Self::new(reason)?;
        Ok(Self(format!("{prefix}: {}", reason.0)))
    }

    pub(crate) fn into_string(self) -> String {
        self.0
    }
}

/// A failure to construct valid Note content.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NoteError {
    /// Note content was empty or whitespace-only.
    #[error("Note content cannot be empty")]
    EmptyContent,
    /// Note content included an unsafe control character.
    #[error("Note content contains invalid control character at position {position}")]
    InvalidControlCharacter {
        /// Character offset of the invalid value.
        position: usize,
    },
}

/// An immutable, timestamped entry in an Issue's chronological history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Note {
    content: String,
    created_at: DateTime<Utc>,
}

impl Note {
    pub(crate) fn from_parts(content: NoteContent, created_at: DateTime<Utc>) -> Self {
        Self {
            content: content.into_string(),
            created_at,
        }
    }

    /// Return the Note content exactly as recorded.
    pub fn content(&self) -> &str {
        &self.content
    }

    /// Return the creation timestamp assigned when this Note was constructed
    /// (system time for appends, the legacy `updated_at` for migrated records).
    pub fn created_at(&self) -> &DateTime<Utc> {
        &self.created_at
    }

    pub(crate) fn into_parts(self) -> (String, DateTime<Utc>) {
        (self.content, self.created_at)
    }
}

/// Represents an issue in the tracking system
///
/// Note: Dependencies are managed by the storage backend and accessed via
/// `IssueStorage::get_dependencies()` rather than being stored on the Issue
/// itself. This prevents data duplication and ensures a single source of truth.
#[derive(Debug, Clone, Serialize)]
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

    /// Issue kind
    pub issue_kind: IssueKind,

    /// Assignee (optional)
    pub assignee: Option<String>,

    /// Labels
    pub labels: Vec<String>,

    /// Design notes (optional)
    pub design: Option<String>,

    /// Acceptance criteria (optional)
    pub acceptance_criteria: Option<String>,

    /// Ordered, append-only Note history
    pub(crate) notes: Vec<Note>,

    /// Ordered, curated Associated Resource index
    pub(crate) resources: Vec<AssociatedResource>,

    /// Next Associated Resource identifier number to assign.
    ///
    /// Monotonic per Issue so identifiers are never reused, even after a
    /// resource is removed. Skipped in domain JSON output; the persistence
    /// boundary owns its serialized form.
    #[serde(skip)]
    pub(crate) next_resource_id: u64,

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
    /// Return Notes in chronological insertion order.
    pub fn notes(&self) -> &[Note] {
        &self.notes
    }

    pub(crate) fn append_note(&mut self, content: NoteContent, created_at: DateTime<Utc>) {
        self.notes.push(Note::from_parts(content, created_at));
    }

    /// Return Associated Resources in insertion order.
    pub fn resources(&self) -> &[AssociatedResource] {
        &self.resources
    }

    /// Rehydrate the persisted resource index while restoring its invariants.
    pub(crate) fn rehydrate_resources(
        &mut self,
        resources: Vec<AssociatedResource>,
        next_resource_id: u64,
    ) -> Result<(), ResourceError> {
        for (index, resource) in resources.iter().enumerate() {
            let prior = &resources[..index];
            if prior
                .iter()
                .any(|candidate| candidate.id() == resource.id())
            {
                return Err(ResourceError::DuplicateResourceId {
                    id: resource.id().clone(),
                });
            }
            if prior.iter().any(|candidate| {
                candidate.target() == resource.target() && candidate.role() == resource.role()
            }) {
                return Err(ResourceError::DuplicateTargetRole {
                    target: resource.target().clone(),
                    role: resource.role(),
                });
            }
        }

        let min_unused = match resources
            .iter()
            .filter_map(|resource| resource.id().as_str().strip_prefix('r'))
            .filter_map(|suffix| suffix.parse::<u64>().ok())
            .max()
        {
            Some(maximum) => maximum
                .checked_add(1)
                .ok_or(ResourceError::IdSequenceExhausted)?,
            None => 1,
        };
        self.resources = resources;
        self.next_resource_id = next_resource_id.max(min_unused).max(1);
        Ok(())
    }

    /// Associate a new resource, assigning its stable identifier.
    ///
    /// Identifiers come from a monotonic per-Issue sequence, so they are
    /// never reused even after a resource is removed.
    ///
    /// # Errors
    ///
    /// Returns [`ResourceError::DuplicateTargetRole`] when an association with
    /// the same target and role already exists.
    pub fn add_resource(&mut self, new: NewResource) -> Result<ResourceId, ResourceError> {
        if self
            .resources
            .iter()
            .any(|r| *r.target() == new.target && r.role() == new.role)
        {
            return Err(ResourceError::DuplicateTargetRole {
                target: new.target.clone(),
                role: new.role,
            });
        }
        let id = ResourceId::new(format!("r{}", self.next_resource_id))?;
        self.next_resource_id = self
            .next_resource_id
            .checked_add(1)
            .ok_or(ResourceError::IdSequenceExhausted)?;
        self.resources.push(AssociatedResource::from_parts(
            id.clone(),
            new.target,
            new.role,
            new.label,
        ));
        Ok(id)
    }

    /// Validate issue data integrity
    ///
    /// Checks:
    /// - Title is not empty and within MAX_TITLE_LENGTH
    /// - Priority is within valid range (0-MAX_PRIORITY)
    /// - No text fields contain control characters
    ///
    /// Returns Ok(()) if valid, Err with description if invalid.
    pub fn validate(&self) -> Result<(), String> {
        validate_title_and_priority(&self.title, self.priority)?;
        validate_text_fields(
            &self.description,
            self.assignee.as_deref(),
            &self.labels,
            self.design.as_deref(),
            self.acceptance_criteria.as_deref(),
        )
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

/// Current classification of an issue
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IssueKind {
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

impl fmt::Display for IssueKind {
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

/// Check a single-line field for control characters (0x00-0x1F except tab, and 0x7F-0x9F).
///
/// Returns the position of the first offending character, if any.
pub(crate) fn find_control_char(s: &str) -> Option<usize> {
    s.chars().position(|c| {
        let code = c as u32;
        (code < 0x20 && code != 0x09) || (0x7F..=0x9F).contains(&code)
    })
}

/// Check a multi-line field for control characters, allowing tab, LF, and CR.
///
/// Returns the position of the first offending character, if any.
fn find_control_char_multiline(s: &str) -> Option<usize> {
    s.chars().position(|c| {
        let code = c as u32;
        (code < 0x20 && code != 0x09 && code != 0x0A && code != 0x0D)
            || (0x7F..=0x9F).contains(&code)
    })
}

/// Validate title and priority fields.
///
/// Shared validation logic used by both `Issue::validate()` and `NewIssue::validate()`.
///
/// # Errors
///
/// Returns an error if:
/// - Title (after trimming) is empty
/// - Title (after trimming) exceeds MAX_TITLE_LENGTH
/// - Title contains control characters
/// - Priority exceeds MAX_PRIORITY
fn validate_title_and_priority(title: &str, priority: u8) -> Result<(), String> {
    let trimmed = title.trim();

    if trimmed.is_empty() {
        return Err("Title cannot be empty".to_string());
    }

    if trimmed.len() > MAX_TITLE_LENGTH {
        return Err(format!(
            "Title cannot exceed {} characters (got {})",
            MAX_TITLE_LENGTH,
            trimmed.len()
        ));
    }

    if let Some(pos) = find_control_char(trimmed) {
        return Err(format!(
            "Title contains invalid control character at position {pos}"
        ));
    }

    if priority > MAX_PRIORITY {
        return Err(format!(
            "Priority must be in range {}-{} (got {})",
            MIN_PRIORITY, MAX_PRIORITY, priority
        ));
    }

    Ok(())
}

/// Validate all text fields on an issue for control characters.
///
/// Defense-in-depth: rejects terminal-injection characters (ANSI escape sequences,
/// etc.) even if the CLI layer already validated them. Protects against data
/// entering through non-CLI paths (import, API, corrupted JSONL).
fn validate_text_fields(
    description: &str,
    assignee: Option<&str>,
    labels: &[String],
    design: Option<&str>,
    acceptance_criteria: Option<&str>,
) -> Result<(), String> {
    if let Some(pos) = find_control_char_multiline(description) {
        return Err(format!(
            "Description contains invalid control character at position {pos}"
        ));
    }
    if let Some(val) = assignee
        && let Some(pos) = find_control_char(val)
    {
        return Err(format!(
            "Assignee contains invalid control character at position {pos}"
        ));
    }
    for (i, label) in labels.iter().enumerate() {
        if let Some(pos) = find_control_char(label) {
            return Err(format!(
                "Label {i} contains invalid control character at position {pos}"
            ));
        }
    }
    if let Some(val) = design
        && let Some(pos) = find_control_char_multiline(val)
    {
        return Err(format!(
            "Design contains invalid control character at position {pos}"
        ));
    }
    if let Some(val) = acceptance_criteria
        && let Some(pos) = find_control_char_multiline(val)
    {
        return Err(format!(
            "Acceptance criteria contains invalid control character at position {pos}"
        ));
    }
    Ok(())
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

    /// Issue kind
    pub issue_kind: IssueKind,

    /// Assignee (optional)
    pub assignee: Option<String>,

    /// Labels
    pub labels: Vec<String>,

    /// Design notes (optional)
    pub design: Option<String>,

    /// Acceptance criteria (optional)
    pub acceptance_criteria: Option<String>,

    /// Initial Note recorded with the Issue creation timestamp
    pub initial_note: Option<NoteContent>,

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
    /// - Priority is not in range 0-MAX_PRIORITY
    /// - Any text field contains control characters
    pub fn validate(&self) -> Result<(), String> {
        validate_title_and_priority(&self.title, self.priority)?;
        validate_text_fields(
            &self.description,
            self.assignee.as_deref(),
            &self.labels,
            self.design.as_deref(),
            self.acceptance_criteria.as_deref(),
        )
    }
}

impl Default for NewIssue {
    /// Create a NewIssue with sensible defaults for testing.
    ///
    /// Default values:
    /// - title: "Untitled Issue"
    /// - description: ""
    /// - priority: 2 (medium)
    /// - issue_kind: Task
    /// - All optional fields: None or empty
    fn default() -> Self {
        Self {
            title: "Untitled Issue".to_string(),
            description: String::new(),
            priority: 2,
            issue_kind: IssueKind::Task,
            assignee: None,
            labels: vec![],
            design: None,
            acceptance_criteria: None,
            initial_note: None,
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

    /// New issue kind (if reclassifying)
    pub issue_kind: Option<IssueKind>,

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

    /// Note to append with this mutation's timestamp
    pub note: Option<NoteContent>,

    /// New labels (if updating) - replaces existing labels
    pub labels: Option<Vec<String>>,
}

/// Filter for querying issues
#[derive(Debug, Clone, Default)]
pub struct IssueFilter {
    /// Filter by status
    pub status: Option<IssueStatus>,

    /// Filter by priority
    pub priority: Option<u8>,

    /// Filter by issue kind
    pub issue_kind: Option<IssueKind>,

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
        assert!(
            result
                .unwrap_err()
                .contains(&format!("cannot exceed {}", MAX_TITLE_LENGTH))
        );
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
        assert!(
            result
                .unwrap_err()
                .contains("Priority must be in range 0-4")
        );
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
        assert!(
            result
                .unwrap_err()
                .contains("Priority must be in range 0-4")
        );
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

    // ===== validate_title_and_priority() Tests =====

    mod validate_title_and_priority_tests {
        use super::super::{
            MAX_PRIORITY, MAX_TITLE_LENGTH, MIN_PRIORITY, validate_title_and_priority,
        };
        use rstest::rstest;

        #[rstest]
        #[case::valid_title_and_priority("Valid Title", 2, true)]
        #[case::empty_title("", 2, false)]
        #[case::whitespace_only_title("   ", 2, false)]
        #[case::priority_zero("Valid", 0, true)]
        #[case::priority_max("Valid", MAX_PRIORITY, true)]
        #[case::priority_too_high("Valid", MAX_PRIORITY + 1, false)]
        fn test_validate_title_and_priority(
            #[case] title: &str,
            #[case] priority: u8,
            #[case] should_pass: bool,
        ) {
            let result = validate_title_and_priority(title, priority);
            assert_eq!(result.is_ok(), should_pass);
        }

        #[test]
        fn test_title_exactly_max_length() {
            let title = "x".repeat(MAX_TITLE_LENGTH);
            assert!(validate_title_and_priority(&title, 2).is_ok());
        }

        #[test]
        fn test_title_exceeds_max_length() {
            let title = "x".repeat(MAX_TITLE_LENGTH + 1);
            let result = validate_title_and_priority(&title, 2);
            assert!(result.is_err());
            assert!(result.unwrap_err().contains("cannot exceed"));
        }

        #[test]
        fn test_priority_error_message_includes_range() {
            let result = validate_title_and_priority("Valid", MAX_PRIORITY + 1);
            let err = result.unwrap_err();
            assert!(err.contains(&format!("{}-{}", MIN_PRIORITY, MAX_PRIORITY)));
        }
    }

    // ===== NewIssue::default() Tests =====

    #[test]
    fn test_new_issue_default() {
        let issue = NewIssue::default();
        assert_eq!(issue.title, "Untitled Issue");
        assert_eq!(issue.description, "");
        assert_eq!(issue.priority, 2);
        assert_eq!(issue.issue_kind, IssueKind::Task);
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
    fn test_issue_kind_display() {
        assert_eq!(format!("{}", IssueKind::Bug), "bug");
        assert_eq!(format!("{}", IssueKind::Feature), "feature");
        assert_eq!(format!("{}", IssueKind::Task), "task");
        assert_eq!(format!("{}", IssueKind::Epic), "epic");
        assert_eq!(format!("{}", IssueKind::Chore), "chore");
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

    // ===== Control Character Validation Tests =====

    mod control_char_tests {
        use super::super::{find_control_char, find_control_char_multiline, validate_text_fields};
        use super::*;
        use rstest::rstest;

        #[rstest]
        #[case::null_byte("hello\x00world", Some(5))]
        #[case::escape("before\x1bafter", Some(6))]
        #[case::bell("ding\x07dong", Some(4))]
        #[case::del("test\x7fval", Some(4))]
        #[case::c1_control("test\u{0090}val", Some(4))]
        #[case::tab_allowed("hello\tworld", None)]
        #[case::clean_text("hello world 123!@#", None)]
        fn test_find_control_char(#[case] input: &str, #[case] expected: Option<usize>) {
            assert_eq!(find_control_char(input), expected);
        }

        #[rstest]
        #[case::newline_allowed("line1\nline2", None)]
        #[case::cr_allowed("line1\rline2", None)]
        #[case::crlf_allowed("line1\r\nline2", None)]
        #[case::tab_allowed("col1\tcol2", None)]
        #[case::escape_rejected("before\x1b[31mred\x1b[0m", Some(6))]
        #[case::null_rejected("has\x00null", Some(3))]
        fn test_find_control_char_multiline(#[case] input: &str, #[case] expected: Option<usize>) {
            assert_eq!(find_control_char_multiline(input), expected);
        }

        #[test]
        fn title_with_escape_sequence_rejected() {
            let issue = NewIssue {
                title: "Normal \x1b[31mRED\x1b[0m title".to_string(),
                ..Default::default()
            };
            let result = issue.validate();
            assert!(result.is_err());
            assert!(
                result.unwrap_err().contains("Title"),
                "Error should mention 'Title'"
            );
        }

        #[test]
        fn description_with_escape_sequence_rejected() {
            let issue = NewIssue {
                title: "Clean title".to_string(),
                description: "Has \x1b[1mbold\x1b[0m text".to_string(),
                ..Default::default()
            };
            let result = issue.validate();
            assert!(result.is_err());
            assert!(
                result.unwrap_err().contains("Description"),
                "Error should mention 'Description'"
            );
        }

        #[test]
        fn description_with_newlines_accepted() {
            let issue = NewIssue {
                title: "Clean title".to_string(),
                description: "Line 1\nLine 2\nLine 3".to_string(),
                ..Default::default()
            };
            assert!(issue.validate().is_ok());
        }

        #[test]
        fn assignee_with_control_char_rejected() {
            let issue = NewIssue {
                title: "Clean title".to_string(),
                assignee: Some("user\x00name".to_string()),
                ..Default::default()
            };
            let result = issue.validate();
            assert!(result.is_err());
            assert!(result.unwrap_err().contains("Assignee"));
        }

        #[test]
        fn label_with_control_char_rejected() {
            let issue = NewIssue {
                title: "Clean title".to_string(),
                labels: vec!["good".to_string(), "bad\x1btag".to_string()],
                ..Default::default()
            };
            let result = issue.validate();
            assert!(result.is_err());
            let err = result.unwrap_err();
            assert!(err.contains("Label 1"), "Error should identify label index");
        }

        #[test]
        fn notes_with_escape_rejected() {
            let result = NoteContent::new("See \x1b[4munderlined\x1b[0m note");
            assert!(matches!(
                result,
                Err(NoteError::InvalidControlCharacter { .. })
            ));
        }

        #[test]
        fn lifecycle_reason_is_validated_before_prefixing() {
            assert_eq!(
                NoteContent::closing_reason("   "),
                Err(NoteError::EmptyContent)
            );
            assert_eq!(
                NoteContent::reopening_reason("bad\x1breason"),
                Err(NoteError::InvalidControlCharacter { position: 3 })
            );
        }

        #[test]
        fn all_clean_fields_accepted() {
            assert!(
                validate_text_fields(
                    "A normal description\nwith newlines",
                    Some("alice"),
                    &["bug".to_string(), "urgent".to_string()],
                    Some("Use approach A"),
                    Some("- [ ] Done"),
                )
                .is_ok()
            );
        }
    }

    // ===== Associated Resources =====

    mod resource_tests {
        use super::*;

        fn issue_with_next_id(next_resource_id: u64) -> Issue {
            Issue {
                id: IssueId::new("test-1"),
                title: "Test".to_string(),
                description: String::new(),
                status: IssueStatus::Open,
                priority: 2,
                issue_kind: IssueKind::Task,
                assignee: None,
                labels: vec![],
                design: None,
                acceptance_criteria: None,
                notes: vec![],
                resources: vec![],
                next_resource_id,
                dependencies: vec![],
                created_at: Utc::now(),
                updated_at: Utc::now(),
                closed_at: None,
            }
        }

        fn web_resource(url: &str, role: ResourceRole) -> NewResource {
            NewResource {
                target: ResourceTarget::web(WebUrl::new(url).expect("valid URL")),
                role,
                label: None,
            }
        }

        fn persisted_resource(id: &str, url: &str, role: ResourceRole) -> AssociatedResource {
            AssociatedResource::from_parts(
                ResourceId::new(id).expect("valid resource ID"),
                ResourceTarget::web(WebUrl::new(url).expect("valid URL")),
                role,
                None,
            )
        }

        #[test]
        fn add_resource_assigns_sequential_ids_in_insertion_order() {
            let mut issue = issue_with_next_id(1);
            let first = issue
                .add_resource(web_resource(
                    "https://a.example.com",
                    ResourceRole::Reference,
                ))
                .expect("add succeeds");
            let second = issue
                .add_resource(web_resource(
                    "https://b.example.com",
                    ResourceRole::Evidence,
                ))
                .expect("add succeeds");

            assert_eq!(first.as_str(), "r1");
            assert_eq!(second.as_str(), "r2");
            let ids: Vec<_> = issue.resources().iter().map(|r| r.id().as_str()).collect();
            assert_eq!(ids, ["r1", "r2"]);
        }

        #[test]
        fn add_resource_rejects_exact_target_role_duplicate() {
            let mut issue = issue_with_next_id(1);
            issue
                .add_resource(web_resource(
                    "https://a.example.com",
                    ResourceRole::Reference,
                ))
                .expect("add succeeds");

            let duplicate = issue.add_resource(web_resource(
                "https://a.example.com/",
                ResourceRole::Reference,
            ));
            assert!(matches!(
                duplicate,
                Err(ResourceError::DuplicateTargetRole { .. })
            ));
            assert_eq!(issue.resources().len(), 1);
        }

        #[test]
        fn add_resource_allows_same_target_with_distinct_roles() {
            let mut issue = issue_with_next_id(1);
            issue
                .add_resource(web_resource(
                    "https://a.example.com",
                    ResourceRole::Reference,
                ))
                .expect("add succeeds");
            issue
                .add_resource(web_resource(
                    "https://a.example.com",
                    ResourceRole::Documentation,
                ))
                .expect("distinct role is allowed");
            assert_eq!(issue.resources().len(), 2);
        }

        #[test]
        fn add_resource_never_reuses_ids_from_loaded_sequence() {
            let mut issue = issue_with_next_id(5);
            let id = issue
                .add_resource(web_resource(
                    "https://a.example.com",
                    ResourceRole::Reference,
                ))
                .expect("add succeeds");
            assert_eq!(id.as_str(), "r5");
        }

        #[test]
        fn rehydrate_resources_rejects_duplicate_ids() {
            let mut issue = issue_with_next_id(1);
            let result = issue.rehydrate_resources(
                vec![
                    persisted_resource("r1", "https://a.example.com", ResourceRole::Reference),
                    persisted_resource("r1", "https://b.example.com", ResourceRole::Evidence),
                ],
                2,
            );
            assert!(matches!(
                result,
                Err(ResourceError::DuplicateResourceId { .. })
            ));
            assert!(issue.resources().is_empty());
        }

        #[test]
        fn rehydrate_resources_rejects_duplicate_target_and_role() {
            let mut issue = issue_with_next_id(1);
            let result = issue.rehydrate_resources(
                vec![
                    persisted_resource("r1", "https://a.example.com", ResourceRole::Reference),
                    persisted_resource("r2", "https://a.example.com/", ResourceRole::Reference),
                ],
                3,
            );
            assert!(matches!(
                result,
                Err(ResourceError::DuplicateTargetRole { .. })
            ));
            assert!(issue.resources().is_empty());
        }

        #[test]
        fn rehydrate_resources_normalizes_stale_identifier_sequence() {
            let mut issue = issue_with_next_id(1);
            issue
                .rehydrate_resources(
                    vec![persisted_resource(
                        "r5",
                        "https://a.example.com",
                        ResourceRole::Reference,
                    )],
                    2,
                )
                .expect("rehydration should succeed");
            let id = issue
                .add_resource(web_resource(
                    "https://b.example.com",
                    ResourceRole::Evidence,
                ))
                .expect("add should use normalized sequence");
            assert_eq!(id.as_str(), "r6");
        }
        #[test]
        fn add_resource_rejects_exhausted_id_sequence() {
            let mut issue = issue_with_next_id(u64::MAX);
            let result = issue.add_resource(web_resource(
                "https://a.example.com",
                ResourceRole::Reference,
            ));
            assert_eq!(result, Err(ResourceError::IdSequenceExhausted));
            assert!(issue.resources().is_empty());
        }
    }
}
