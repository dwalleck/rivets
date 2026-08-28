//! Domain types for issue tracking.
//!
//! This module contains the core domain types for the rivets issue tracker.

use chrono::{DateTime, Utc};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use std::sync::OnceLock;

mod relationship;
mod resource;
#[cfg(test)]
mod workspace_path_corpus;

pub use relationship::{BlockingDependency, BlockingDependencyError};
pub use resource::{
    AssociatedResource, NewResource, ResourceError, ResourceId, ResourceLabel, ResourceRole,
    ResourceTarget, ResourceUpdate, WebUrl, WorkspacePath,
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
    #[serde(skip)]
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

    /// Update an existing resource by its stable identifier.
    ///
    /// Only the provided fields change; the resource keeps its identifier and
    /// position. The duplicate check runs against the post-update state,
    /// excluding the resource itself.
    ///
    /// # Errors
    ///
    /// Returns [`ResourceError::EmptyUpdate`] when no field is provided,
    /// [`ResourceError::ResourceNotFound`] when the identifier is unknown, or
    /// [`ResourceError::DuplicateTargetRole`] when the post-update target and
    /// role already exist on another resource.
    pub fn update_resource(
        &mut self,
        id: &ResourceId,
        update: ResourceUpdate,
    ) -> Result<(), ResourceError> {
        if update.target.is_none() && update.role.is_none() && update.label.is_none() {
            return Err(ResourceError::EmptyUpdate);
        }
        let Some(index) = self
            .resources
            .iter()
            .position(|resource| resource.id() == id)
        else {
            return Err(ResourceError::ResourceNotFound { id: id.clone() });
        };
        let ResourceUpdate {
            target,
            role,
            label,
        } = update;
        let current = &self.resources[index];
        let target = target.unwrap_or_else(|| current.target().clone());
        let role = role.unwrap_or(current.role());
        let label = match label {
            Some(label) => label,
            None => current.label().cloned(),
        };
        if self
            .resources
            .iter()
            .enumerate()
            .any(|(candidate_index, candidate)| {
                candidate_index != index
                    && *candidate.target() == target
                    && candidate.role() == role
            })
        {
            return Err(ResourceError::DuplicateTargetRole { target, role });
        }
        self.resources[index] = AssociatedResource::from_parts(id.clone(), target, role, label);
        Ok(())
    }

    /// Remove a resource by its stable identifier.
    ///
    /// The remaining resources keep their identifiers and positions, and the
    /// per-Issue identifier sequence is untouched, so identifiers are never
    /// reused after a removal.
    ///
    /// # Errors
    ///
    /// Returns [`ResourceError::ResourceNotFound`] when the identifier is
    /// unknown.
    pub fn remove_resource(&mut self, id: &ResourceId) -> Result<(), ResourceError> {
        let Some(index) = self
            .resources
            .iter()
            .position(|resource| resource.id() == id)
        else {
            return Err(ResourceError::ResourceNotFound { id: id.clone() });
        };
        self.resources.remove(index);
        Ok(())
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

/// Join every canonical value name of a vocabulary enum with `", "`.
///
/// Derived from the `clap` value names so a valid-values error string can
/// never drift from the enum declaration it describes.
pub(crate) fn join_canonical_names<T: ValueEnum + fmt::Display>() -> String {
    T::value_variants()
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Status of an issue
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum IssueStatus {
    /// Issue is open and ready to work on
    Open,

    /// Issue is currently being worked on
    #[serde(rename = "in_progress")]
    #[value(name = "in_progress", alias = "in-progress")]
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

impl IssueStatus {
    /// Comma-separated canonical status names, for error messages.
    ///
    /// Derived from the enum declaration rather than hand-written, so the
    /// listed values cannot drift from the accepted vocabulary.
    #[must_use]
    pub fn valid_values() -> &'static str {
        static VALUES: OnceLock<String> = OnceLock::new();
        VALUES.get_or_init(join_canonical_names::<Self>)
    }

    /// Validate a status transition per the domain rules (ADR-0005).
    ///
    /// The domain owns these rules; adapters and storage implementations
    /// must not re-validate them.
    ///
    /// # Invalid Transitions
    ///
    /// - `Closed` → `Closed`: an Issue cannot be closed twice.
    /// - Any non-`Closed` status → `Open`: only closed Issues can be reopened.
    ///
    /// Every other transition is allowed.
    ///
    /// # Errors
    ///
    /// Returns a [`StatusTransitionError`] describing the rejected transition.
    pub const fn validate_transition(self, target: Self) -> Result<(), StatusTransitionError> {
        match (self, target) {
            (Self::Closed, Self::Closed) => {
                Err(StatusTransitionError::AlreadyClosed { current: self })
            }
            (Self::Closed, _) => Ok(()),
            (current, Self::Open) => Err(StatusTransitionError::NotClosed { current }),
            _ => Ok(()),
        }
    }
}

/// A status change rejected by the domain transition rules.
///
/// Display output is the full user-facing message (no adapter prefix), so
/// CLI and MCP surface the identical observable rejection (rivets-rb3h).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum StatusTransitionError {
    /// Closing an Issue that is already closed.
    #[error("Issue is already closed (status: {current})")]
    AlreadyClosed {
        /// The Issue's status when the close was rejected.
        current: IssueStatus,
    },
    /// Reopening an Issue that is not closed.
    #[error("Issue is not closed (status: {current})")]
    NotClosed {
        /// The Issue's status when the reopen was rejected.
        current: IssueStatus,
    },
}

/// A failure to parse an [`IssueStatus`] from a string.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IssueStatusError {
    /// The string was not a canonical Issue Status name.
    #[error("Unknown issue status '{status}'")]
    UnknownStatus {
        /// The rejected input string.
        status: String,
    },
}

impl FromStr for IssueStatus {
    type Err = IssueStatusError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "open" => Ok(Self::Open),
            "in_progress" => Ok(Self::InProgress),
            "blocked" => Ok(Self::Blocked),
            "closed" => Ok(Self::Closed),
            _ => Err(IssueStatusError::UnknownStatus {
                status: s.to_string(),
            }),
        }
    }
}

/// Current classification of an issue
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
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

/// A failure to parse an [`IssueKind`] from a string.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IssueKindError {
    /// The string was not a canonical Issue Kind name.
    #[error("Unknown issue kind '{kind}'")]
    UnknownKind {
        /// The rejected input string.
        kind: String,
    },
}

impl FromStr for IssueKind {
    type Err = IssueKindError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "bug" => Ok(Self::Bug),
            "feature" => Ok(Self::Feature),
            "task" => Ok(Self::Task),
            "epic" => Ok(Self::Epic),
            "chore" => Ok(Self::Chore),
            _ => Err(IssueKindError::UnknownKind {
                kind: s.to_string(),
            }),
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, ValueEnum)]
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

impl DependencyType {
    /// Comma-separated canonical dependency-type names, for error messages.
    ///
    /// Derived from the enum declaration rather than hand-written, so the
    /// listed values cannot drift from the accepted vocabulary.
    #[must_use]
    pub fn valid_values() -> &'static str {
        static VALUES: OnceLock<String> = OnceLock::new();
        VALUES.get_or_init(join_canonical_names::<Self>)
    }
}

/// A failure to parse a [`DependencyType`] from a string.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DependencyTypeError {
    /// The string was not a canonical Dependency Type name.
    #[error("Unknown dependency type '{dependency_type}'")]
    UnknownDependencyType {
        /// The rejected input string.
        dependency_type: String,
    },
}

impl FromStr for DependencyType {
    type Err = DependencyTypeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "blocks" => Ok(Self::Blocks),
            "related" => Ok(Self::Related),
            "parent-child" => Ok(Self::ParentChild),
            "discovered-from" => Ok(Self::DiscoveredFrom),
            _ => Err(DependencyTypeError::UnknownDependencyType {
                dependency_type: s.to_string(),
            }),
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

/// Whether a character is unsafe in a multiline domain text value.
pub(crate) fn is_unsafe_multiline_control(character: char) -> bool {
    let code = character as u32;
    (code < 0x20 && code != 0x09 && code != 0x0A && code != 0x0D) || (0x7F..=0x9F).contains(&code)
}

/// Check a multi-line field for control characters, allowing tab, LF, and CR.
///
/// Returns the position of the first offending character, if any.
fn find_control_char_multiline(s: &str) -> Option<usize> {
    s.chars().position(is_unsafe_multiline_control)
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

    /// Blocking prerequisite Issues to attach atomically at creation.
    pub prerequisites: Vec<IssueId>,
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
            prerequisites: vec![],
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
        assert!(issue.prerequisites.is_empty());
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

    // ===== FromStr Roundtrip Tests =====
    //
    // parse(display(variant)) == variant for every variant; non-canonical
    // spellings (uppercase, empty, CLI/MCP aliases, unknown) are rejected
    // with the typed error carrying the offending string.

    #[test]
    fn test_issue_status_from_str_roundtrip() {
        for status in [
            IssueStatus::Open,
            IssueStatus::InProgress,
            IssueStatus::Blocked,
            IssueStatus::Closed,
        ] {
            assert_eq!(status.to_string().parse::<IssueStatus>(), Ok(status));
        }
    }

    #[test]
    fn test_issue_status_from_str_rejects_noncanonical() {
        for invalid in ["", "OPEN", "in-progress", "in_progress ", "bogus"] {
            let error = invalid.parse::<IssueStatus>().unwrap_err();
            assert!(matches!(
                error,
                IssueStatusError::UnknownStatus { status } if status == invalid
            ));
        }
    }

    #[test]
    fn test_issue_kind_from_str_roundtrip() {
        for kind in [
            IssueKind::Bug,
            IssueKind::Feature,
            IssueKind::Task,
            IssueKind::Epic,
            IssueKind::Chore,
        ] {
            assert_eq!(kind.to_string().parse::<IssueKind>(), Ok(kind));
        }
    }

    #[test]
    fn test_issue_kind_from_str_rejects_noncanonical() {
        for invalid in ["", "BUG", "task ", "bogus"] {
            let error = invalid.parse::<IssueKind>().unwrap_err();
            assert!(matches!(
                error,
                IssueKindError::UnknownKind { kind } if kind == invalid
            ));
        }
    }

    #[test]
    fn test_dependency_type_from_str_roundtrip() {
        for dep_type in [
            DependencyType::Blocks,
            DependencyType::Related,
            DependencyType::ParentChild,
            DependencyType::DiscoveredFrom,
        ] {
            assert_eq!(dep_type.to_string().parse::<DependencyType>(), Ok(dep_type));
        }
    }

    #[test]
    fn test_dependency_type_from_str_rejects_noncanonical() {
        for invalid in ["", "BLOCKS", "parent_child", "discovered_from", "bogus"] {
            let error = invalid.parse::<DependencyType>().unwrap_err();
            assert!(matches!(
                error,
                DependencyTypeError::UnknownDependencyType { dependency_type } if dependency_type == invalid
            ));
        }
    }

    // ===== Serde Wire-Format Fence Tests =====
    //
    // Serde is the wire form of the same vocabulary: every variant's JSON
    // string must equal its Display string in both directions, so JSON
    // output and human-readable output cannot diverge.

    #[test]
    fn test_issue_status_serde_matches_display() {
        for status in [
            IssueStatus::Open,
            IssueStatus::InProgress,
            IssueStatus::Blocked,
            IssueStatus::Closed,
        ] {
            let json = serde_json::to_string(&status).expect("status serializes");
            assert_eq!(json, format!("\"{status}\""));
            let parsed: IssueStatus = serde_json::from_str(&json).expect("status deserializes");
            assert_eq!(parsed, status);
        }
    }

    #[test]
    fn test_issue_kind_serde_matches_display() {
        for kind in [
            IssueKind::Bug,
            IssueKind::Feature,
            IssueKind::Task,
            IssueKind::Epic,
            IssueKind::Chore,
        ] {
            let json = serde_json::to_string(&kind).expect("kind serializes");
            assert_eq!(json, format!("\"{kind}\""));
            let parsed: IssueKind = serde_json::from_str(&json).expect("kind deserializes");
            assert_eq!(parsed, kind);
        }
    }

    #[test]
    fn test_dependency_type_serde_matches_display() {
        for dep_type in [
            DependencyType::Blocks,
            DependencyType::Related,
            DependencyType::ParentChild,
            DependencyType::DiscoveredFrom,
        ] {
            let json = serde_json::to_string(&dep_type).expect("dep type serializes");
            assert_eq!(json, format!("\"{dep_type}\""));
            let parsed: DependencyType =
                serde_json::from_str(&json).expect("dep type deserializes");
            assert_eq!(parsed, dep_type);
        }
    }

    // ===== Valid-Values Fence Tests =====

    #[test]
    fn test_valid_values_list_every_canonical_name() {
        // Pins the derived error-message lists to the shipped wording.
        assert_eq!(
            IssueStatus::valid_values(),
            "open, in_progress, blocked, closed"
        );
        assert_eq!(
            DependencyType::valid_values(),
            "blocks, related, parent-child, discovered-from"
        );
    }

    // ===== CLI ValueEnum Vocabulary Tests =====
    //
    // The clap value name of every variant equals its Display string, and
    // the canonical name/alias pair of IssueStatus::InProgress is preserved
    // (the CLI contract recorded in .rivets-bkjj/baseline-cli-contract.txt).

    #[test]
    fn test_cli_value_names_match_display() {
        for status in [
            IssueStatus::Open,
            IssueStatus::InProgress,
            IssueStatus::Blocked,
            IssueStatus::Closed,
        ] {
            let possible = status.to_possible_value().expect("possible value");
            assert_eq!(possible.get_name(), status.to_string());
        }
        for kind in [
            IssueKind::Bug,
            IssueKind::Feature,
            IssueKind::Task,
            IssueKind::Epic,
            IssueKind::Chore,
        ] {
            let possible = kind.to_possible_value().expect("possible value");
            assert_eq!(possible.get_name(), kind.to_string());
        }
        for role in [
            ResourceRole::Implementation,
            ResourceRole::Documentation,
            ResourceRole::Evidence,
            ResourceRole::Successor,
            ResourceRole::Reference,
        ] {
            let possible = role.to_possible_value().expect("possible value");
            assert_eq!(possible.get_name(), role.to_string());
        }
        for dep_type in [
            DependencyType::Blocks,
            DependencyType::Related,
            DependencyType::ParentChild,
            DependencyType::DiscoveredFrom,
        ] {
            let possible = dep_type.to_possible_value().expect("possible value");
            assert_eq!(possible.get_name(), dep_type.to_string());
        }
    }

    #[test]
    fn test_in_progress_alias_preserved() {
        let possible = IssueStatus::InProgress
            .to_possible_value()
            .expect("possible value");
        assert_eq!(possible.get_name(), "in_progress");
        assert_eq!(
            possible.get_name_and_aliases().collect::<Vec<_>>(),
            vec!["in_progress", "in-progress"]
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

        fn path_resource(path: &str, role: ResourceRole) -> NewResource {
            NewResource {
                target: ResourceTarget::path(
                    WorkspacePath::new(path).expect("valid workspace path"),
                ),
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
        fn duplicate_detection_normalizes_equivalent_paths() {
            let mut issue = issue_with_next_id(1);
            issue
                .add_resource(path_resource("src/lib.rs", ResourceRole::Reference))
                .expect("add succeeds");
            let duplicate =
                issue.add_resource(path_resource("docs/../src/lib.rs", ResourceRole::Reference));
            assert!(matches!(
                duplicate,
                Err(ResourceError::DuplicateTargetRole { .. })
            ));
            assert_eq!(
                issue.resources().len(),
                1,
                "equivalent normalized path must be a duplicate"
            );
        }

        #[test]
        fn duplicate_detection_distinguishes_path_from_web() {
            let mut issue = issue_with_next_id(1);
            issue
                .add_resource(path_resource("docs/adr/0003.md", ResourceRole::Reference))
                .expect("add succeeds");
            // Same textual value as a URL is a different target kind.
            issue
                .add_resource(web_resource(
                    "https://example.com/docs/adr/0003.md",
                    ResourceRole::Reference,
                ))
                .expect("web and path targets are distinct");
            assert_eq!(issue.resources().len(), 2);
        }

        #[test]
        fn duplicate_detection_applies_to_paths_with_distinct_roles() {
            let mut issue = issue_with_next_id(1);
            issue
                .add_resource(path_resource("src/lib.rs", ResourceRole::Reference))
                .expect("add succeeds");
            issue
                .add_resource(path_resource("src/lib.rs", ResourceRole::Documentation))
                .expect("same target with distinct role is allowed");
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

        // ===== update_resource =====

        fn three_resource_issue() -> Issue {
            let mut issue = issue_with_next_id(1);
            issue
                .add_resource(path_resource("src/lib.rs", ResourceRole::Implementation))
                .expect("add succeeds");
            issue
                .add_resource(web_resource(
                    "https://b.example.com",
                    ResourceRole::Evidence,
                ))
                .expect("add succeeds");
            issue
                .add_resource(path_resource("docs/adr/0003.md", ResourceRole::Reference))
                .expect("add succeeds");
            issue
        }

        #[test]
        fn update_resource_changes_only_provided_fields_and_preserves_position() {
            let mut issue = three_resource_issue();
            let r2 = issue.resources()[1].id().clone();
            issue
                .update_resource(
                    &r2,
                    ResourceUpdate {
                        target: Some(ResourceTarget::path(
                            WorkspacePath::new("src/../src/main.rs").expect("normalizes"),
                        )),
                        role: None,
                        label: Some(Some(ResourceLabel::new("main entry").expect("label"))),
                    },
                )
                .expect("update succeeds");
            let resources = issue.resources();
            assert_eq!(resources.len(), 3);
            assert_eq!(resources[1].id(), &r2, "id must not change");
            assert_eq!(resources[0].id().as_str(), "r1", "position must not shift");
            assert_eq!(resources[2].id().as_str(), "r3", "position must not shift");
            assert_eq!(resources[1].target().to_string(), "src/main.rs");
            assert_eq!(
                resources[1].role(),
                ResourceRole::Evidence,
                "role unchanged"
            );
            assert_eq!(resources[1].label().map(|l| l.as_str()), Some("main entry"));
        }

        #[test]
        fn update_resource_changes_role_and_clears_label() {
            let mut issue = three_resource_issue();
            issue
                .add_resource(web_resource(
                    "https://c.example.com",
                    ResourceRole::Documentation,
                ))
                .expect("add succeeds");
            let r4 = issue.resources()[3].id().clone();
            issue
                .update_resource(
                    &r4,
                    ResourceUpdate {
                        target: None,
                        role: Some(ResourceRole::Successor),
                        label: Some(Some(ResourceLabel::new("temp").expect("label"))),
                    },
                )
                .expect("set label");
            issue
                .update_resource(
                    &r4,
                    ResourceUpdate {
                        target: None,
                        role: None,
                        label: Some(None),
                    },
                )
                .expect("clear label");
            let resources = issue.resources();
            assert_eq!(resources[3].role(), ResourceRole::Successor);
            assert!(resources[3].label().is_none(), "label must be cleared");
        }

        #[test]
        fn update_resource_last_position_is_stable() {
            let mut issue = three_resource_issue();
            let r3 = issue.resources()[2].id().clone();
            issue
                .update_resource(
                    &r3,
                    ResourceUpdate {
                        role: Some(ResourceRole::Successor),
                        ..ResourceUpdate::default()
                    },
                )
                .expect("update succeeds");
            let ids: Vec<_> = issue.resources().iter().map(|r| r.id().as_str()).collect();
            assert_eq!(
                ids,
                ["r1", "r2", "r3"],
                "updating the last resource must not reorder"
            );
            assert_eq!(issue.resources()[2].role(), ResourceRole::Successor);
        }

        #[test]
        fn update_resource_allows_web_to_path_and_back() {
            let mut issue = issue_with_next_id(1);
            issue
                .add_resource(web_resource(
                    "https://a.example.com",
                    ResourceRole::Reference,
                ))
                .expect("add succeeds");
            let r1 = issue.resources()[0].id().clone();
            issue
                .update_resource(
                    &r1,
                    ResourceUpdate {
                        target: Some(ResourceTarget::path(
                            WorkspacePath::new("docs/guide.md").expect("path"),
                        )),
                        role: None,
                        label: None,
                    },
                )
                .expect("web to path");
            assert_eq!(issue.resources()[0].target().to_string(), "docs/guide.md");
            issue
                .update_resource(
                    &r1,
                    ResourceUpdate {
                        target: Some(ResourceTarget::web(
                            WebUrl::new("https://a.example.com").expect("url"),
                        )),
                        role: None,
                        label: None,
                    },
                )
                .expect("path to web");
            assert_eq!(
                issue.resources()[0].target().to_string(),
                "https://a.example.com/"
            );
        }

        #[test]
        fn update_resource_rejects_empty_update() {
            let mut issue = three_resource_issue();
            let r1 = issue.resources()[0].id().clone();
            assert_eq!(
                issue.update_resource(&r1, ResourceUpdate::default()),
                Err(ResourceError::EmptyUpdate)
            );
        }

        #[test]
        fn update_resource_rejects_unknown_id() {
            let mut issue = three_resource_issue();
            let unknown = ResourceId::new("r99").expect("valid id");
            assert_eq!(
                issue.update_resource(
                    &unknown,
                    ResourceUpdate {
                        target: None,
                        role: Some(ResourceRole::Successor),
                        label: None,
                    },
                ),
                Err(ResourceError::ResourceNotFound { id: unknown })
            );
        }

        #[test]
        fn update_resource_checks_duplicates_against_post_update_state() {
            let mut issue = three_resource_issue();
            // r1 is (src/lib.rs, implementation); changing r2's target to
            // src/lib.rs with role implementation must collide via normalization.
            let r2 = issue.resources()[1].id().clone();
            let duplicate = issue.update_resource(
                &r2,
                ResourceUpdate {
                    target: Some(ResourceTarget::path(
                        WorkspacePath::new("src/../src/lib.rs").expect("normalizes"),
                    )),
                    role: Some(ResourceRole::Implementation),
                    label: None,
                },
            );
            assert!(matches!(
                duplicate,
                Err(ResourceError::DuplicateTargetRole { .. })
            ));
            assert_eq!(
                issue.resources()[1].target().to_string(),
                "https://b.example.com/"
            );

            // Same target with a distinct role is fine.
            issue
                .update_resource(
                    &r2,
                    ResourceUpdate {
                        target: Some(ResourceTarget::path(
                            WorkspacePath::new("src/lib.rs").expect("path"),
                        )),
                        role: Some(ResourceRole::Documentation),
                        label: None,
                    },
                )
                .expect("distinct role update succeeds");
            assert_eq!(issue.resources()[1].role(), ResourceRole::Documentation);
        }

        #[test]
        fn update_resource_does_not_consume_identifier_sequence() {
            let mut issue = issue_with_next_id(1);
            issue
                .add_resource(web_resource(
                    "https://a.example.com",
                    ResourceRole::Reference,
                ))
                .expect("add succeeds");
            let r1 = issue.resources()[0].id().clone();
            issue
                .update_resource(
                    &r1,
                    ResourceUpdate {
                        target: None,
                        role: Some(ResourceRole::Successor),
                        label: None,
                    },
                )
                .expect("update succeeds");
            let next = issue
                .add_resource(web_resource(
                    "https://b.example.com",
                    ResourceRole::Evidence,
                ))
                .expect("add succeeds");
            assert_eq!(
                next.as_str(),
                "r2",
                "update must not advance the id sequence"
            );
        }

        // ===== remove_resource =====

        #[test]
        fn remove_resource_keeps_remaining_ids_and_positions() {
            let mut issue = three_resource_issue();
            let r2 = issue.resources()[1].id().clone();
            issue.remove_resource(&r2).expect("remove succeeds");
            let resources = issue.resources();
            assert_eq!(resources.len(), 2);
            assert_eq!(resources[0].id().as_str(), "r1");
            assert_eq!(resources[0].role(), ResourceRole::Implementation);
            assert_eq!(resources[1].id().as_str(), "r3");
            assert_eq!(resources[1].role(), ResourceRole::Reference);
        }

        #[test]
        fn remove_resource_first_middle_last_and_only() {
            for position in 0..3 {
                let mut issue = three_resource_issue();
                let target = issue.resources()[position].id().clone();
                issue.remove_resource(&target).expect("remove succeeds");
                let expected: Vec<&str> = ["r1", "r2", "r3"]
                    .into_iter()
                    .enumerate()
                    .filter(|(index, _)| *index != position)
                    .map(|(_, id)| id)
                    .collect();
                let actual: Vec<&str> = issue.resources().iter().map(|r| r.id().as_str()).collect();
                assert_eq!(actual, expected, "removing position {position}");
            }
            let mut issue = three_resource_issue();
            for resource in issue.resources().to_vec() {
                issue
                    .remove_resource(resource.id())
                    .expect("remove succeeds");
            }
            assert!(issue.resources().is_empty());
        }

        #[test]
        fn remove_resource_rejects_unknown_id() {
            let mut issue = three_resource_issue();
            let unknown = ResourceId::new("r99").expect("valid id");
            assert_eq!(
                issue.remove_resource(&unknown),
                Err(ResourceError::ResourceNotFound { id: unknown })
            );
            assert_eq!(issue.resources().len(), 3, "failed removal must not mutate");
        }

        #[test]
        fn remove_resource_from_empty_issue_is_not_found() {
            let mut issue = issue_with_next_id(1);
            let id = ResourceId::new("r1").expect("valid id");
            assert!(matches!(
                issue.remove_resource(&id),
                Err(ResourceError::ResourceNotFound { .. })
            ));
        }

        #[test]
        fn remove_resource_never_reuses_identifiers() {
            let mut issue = three_resource_issue();
            let r2 = issue.resources()[1].id().clone();
            issue.remove_resource(&r2).expect("remove succeeds");
            let next = issue
                .add_resource(web_resource(
                    "https://new.example.com",
                    ResourceRole::Evidence,
                ))
                .expect("add succeeds");
            assert_eq!(
                next.as_str(),
                "r4",
                "next id must continue the sequence, never reuse r2"
            );
            let ids: Vec<_> = issue.resources().iter().map(|r| r.id().as_str()).collect();
            assert_eq!(ids, ["r1", "r3", "r4"]);
        }
    }

    mod status_transition_tests {
        use super::*;
        use rstest::rstest;

        #[rstest]
        #[case::open_to_closed(IssueStatus::Open, IssueStatus::Closed, true)]
        #[case::in_progress_to_closed(IssueStatus::InProgress, IssueStatus::Closed, true)]
        #[case::blocked_to_closed(IssueStatus::Blocked, IssueStatus::Closed, true)]
        #[case::closed_to_closed(IssueStatus::Closed, IssueStatus::Closed, false)]
        #[case::closed_to_open(IssueStatus::Closed, IssueStatus::Open, true)]
        #[case::closed_to_in_progress(IssueStatus::Closed, IssueStatus::InProgress, true)]
        #[case::closed_to_blocked(IssueStatus::Closed, IssueStatus::Blocked, true)]
        #[case::open_to_open(IssueStatus::Open, IssueStatus::Open, false)]
        #[case::in_progress_to_open(IssueStatus::InProgress, IssueStatus::Open, false)]
        #[case::blocked_to_open(IssueStatus::Blocked, IssueStatus::Open, false)]
        #[case::open_to_in_progress(IssueStatus::Open, IssueStatus::InProgress, true)]
        #[case::open_to_blocked(IssueStatus::Open, IssueStatus::Blocked, true)]
        #[case::in_progress_to_blocked(IssueStatus::InProgress, IssueStatus::Blocked, true)]
        #[case::blocked_to_in_progress(IssueStatus::Blocked, IssueStatus::InProgress, true)]
        fn transition_matrix(
            #[case] current: IssueStatus,
            #[case] target: IssueStatus,
            #[case] should_succeed: bool,
        ) {
            let result = current.validate_transition(target);
            assert_eq!(
                result.is_ok(),
                should_succeed,
                "Transition {current:?} -> {target:?} expected success={should_succeed}, got {result:?}"
            );
        }

        #[test]
        fn closing_a_closed_issue_yields_already_closed() {
            let error = IssueStatus::Closed
                .validate_transition(IssueStatus::Closed)
                .expect_err("Closed -> Closed must be rejected");
            assert_eq!(
                error,
                StatusTransitionError::AlreadyClosed {
                    current: IssueStatus::Closed
                }
            );
            assert_eq!(
                error.to_string(),
                "Issue is already closed (status: closed)"
            );
        }

        #[rstest]
        #[case::open(IssueStatus::Open)]
        #[case::in_progress(IssueStatus::InProgress)]
        #[case::blocked(IssueStatus::Blocked)]
        fn reopening_a_non_closed_issue_yields_not_closed(#[case] current: IssueStatus) {
            let error = current
                .validate_transition(IssueStatus::Open)
                .expect_err("non-Closed -> Open must be rejected");
            assert_eq!(error, StatusTransitionError::NotClosed { current });
            assert_eq!(
                error.to_string(),
                format!("Issue is not closed (status: {current})")
            );
        }
    }
}
