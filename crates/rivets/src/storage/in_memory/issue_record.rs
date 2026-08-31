//! Compatibility boundary between persisted JSONL issue records and the domain model.

use crate::domain::{
    AssociatedResource, Dependency, Issue, IssueId, IssueKind, IssueStatus, NewResource, Note,
    NoteContent, NoteError, ResourceError, ResourceId, ResourceLabel, ResourceRole, ResourceTarget,
    WebUrl, WorkspacePath, is_unsafe_multiline_control,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A domain field with emitted and migration-only persisted names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationField {
    /// The issue's kind, emitted as `issue_kind` while accepting legacy `issue_type`.
    IssueKind,
}

impl MigrationField {
    /// The persisted name written by the save path.
    pub const fn emitted_name(self) -> &'static str {
        match self {
            Self::IssueKind => "issue_kind",
        }
    }

    /// The migration-only persisted name accepted during loading.
    pub const fn accepted_migration_name(self) -> &'static str {
        match self {
            Self::IssueKind => "issue_type",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub(super) enum IssueRecordError {
    #[error("invalid data for Issue '{issue_id}': {error}")]
    InvalidData {
        issue_id: IssueId,
        error: String,
        migration_conflict: Option<MigrationField>,
    },
    #[error("invalid Associated Resource for Issue '{issue_id}': {source}")]
    InvalidResource {
        issue_id: IssueId,
        #[source]
        source: ResourceError,
        migration_conflict: Option<MigrationField>,
    },
}

fn invalid_resource_error(
    issue_id: &IssueId,
    migration_conflict: Option<MigrationField>,
    source: ResourceError,
) -> IssueRecordError {
    IssueRecordError::InvalidResource {
        issue_id: issue_id.clone(),
        source,
        migration_conflict,
    }
}

fn invalid_data_error(
    issue_id: &IssueId,
    migration_conflict: Option<MigrationField>,
    error: impl ToString,
) -> IssueRecordError {
    IssueRecordError::InvalidData {
        issue_id: issue_id.clone(),
        error: error.to_string(),
        migration_conflict,
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct NoteRecord {
    content: String,
    created_at: DateTime<Utc>,
}

impl NoteRecord {
    fn into_domain(self) -> Result<Note, NoteError> {
        Ok(Note::from_parts(
            NoteContent::new(self.content)?,
            self.created_at,
        ))
    }
}

impl From<Note> for NoteRecord {
    fn from(note: Note) -> Self {
        let (content, created_at) = note.into_parts();
        Self {
            content,
            created_at,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum PersistedNotes {
    Legacy(String),
    Canonical(Vec<NoteRecord>),
    Empty(()),
}

impl Default for PersistedNotes {
    fn default() -> Self {
        Self::Empty(())
    }
}

/// Persisted form of a Resource Target.
///
/// Stringly typed on purpose: the record is an adapter DTO, and conversion
/// into the domain revalidates through the [`WebUrl`] and [`WorkspacePath`]
/// constructors.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ResourceTargetRecord {
    Web { url: String },
    Path { path: String },
}

/// Persisted form of an Associated Resource.
#[derive(Debug, Serialize, Deserialize)]
struct ResourceRecord {
    id: String,
    target: ResourceTargetRecord,
    role: ResourceRole,
    label: Option<String>,
}

impl ResourceRecord {
    fn into_domain(self) -> Result<AssociatedResource, crate::domain::ResourceError> {
        let target = match self.target {
            ResourceTargetRecord::Web { url } => ResourceTarget::web(WebUrl::new(url)?),
            ResourceTargetRecord::Path { path } => ResourceTarget::path(WorkspacePath::new(path)?),
        };
        let label = self.label.map(ResourceLabel::new).transpose()?;
        Ok(AssociatedResource::from_parts(
            ResourceId::new(self.id)?,
            target,
            self.role,
            label,
        ))
    }
}

impl From<AssociatedResource> for ResourceRecord {
    fn from(resource: AssociatedResource) -> Self {
        let target = match resource.target() {
            ResourceTarget::Web { url } => ResourceTargetRecord::Web {
                url: url.as_str().to_string(),
            },
            ResourceTarget::Path { path } => ResourceTargetRecord::Path {
                path: path.as_str().to_string(),
            },
        };
        Self {
            id: resource.id().as_str().to_string(),
            target,
            role: resource.role(),
            label: resource.label().map(|l| l.as_str().to_string()),
        }
    }
}

/// Default next resource identifier for records that never held resources.
const DEFAULT_NEXT_RESOURCE_ID: u64 = 1;

const MIGRATED_EXTERNAL_REF_NOTE_PREFIX: &str = "Migrated legacy external reference: ";

fn migrated_external_ref_note_text(external_ref: &str) -> String {
    let escape_controls = external_ref.chars().any(is_unsafe_multiline_control);
    let mut content =
        String::with_capacity(MIGRATED_EXTERNAL_REF_NOTE_PREFIX.len() + external_ref.len());
    content.push_str(MIGRATED_EXTERNAL_REF_NOTE_PREFIX);
    if escape_controls {
        for character in external_ref.chars() {
            if is_unsafe_multiline_control(character) || character == '\\' {
                content.extend(character.escape_default());
            } else {
                content.push(character);
            }
        }
    } else {
        content.push_str(external_ref);
    }
    content
}

/// Classifies a legacy `external_ref` as a migratable Web URL or opaque text.
///
/// The error arms are enumerated exhaustively on purpose: adding a
/// [`ResourceError`] variant must force an explicit decision about whether it
/// means "not a URL, keep as a Note" or "corrupt data, skip the Issue loudly".
fn legacy_external_ref_url(external_ref: &str) -> Result<Option<WebUrl>, ResourceError> {
    match WebUrl::new(external_ref) {
        Ok(url) => Ok(Some(url)),
        Err(
            ResourceError::MalformedWebUrl { .. }
            | ResourceError::MissingWebUrlAuthority { .. }
            | ResourceError::UnsupportedWebUrlScheme { .. },
        ) => Ok(None),
        Err(
            error @ (ResourceError::UnknownRole { .. }
            | ResourceError::EmptyLabel
            | ResourceError::LabelControlCharacter { .. }
            | ResourceError::DuplicateTargetRole { .. }
            | ResourceError::ResourceIdControlCharacter { .. }
            | ResourceError::DuplicateResourceId { .. }
            | ResourceError::IdSequenceExhausted
            | ResourceError::EmptyResourceId
            | ResourceError::EmptyPath
            | ResourceError::PathControlCharacter { .. }
            | ResourceError::WorkspacePathBackslash { .. }
            | ResourceError::AbsoluteWorkspacePath { .. }
            | ResourceError::WorkspacePathEscape { .. }
            | ResourceError::EmptyNormalizedWorkspacePath { .. }
            | ResourceError::EmptyUpdate
            | ResourceError::ResourceNotFound { .. }),
        ) => Err(error),
    }
}

/// Attaches a migrated resource, tolerating only an already-migrated duplicate.
///
/// Exhaustive for the same reason as [`legacy_external_ref_url`]: a new
/// [`ResourceError`] variant must be explicitly sorted into "benign for
/// re-migration" or "fail the record".
fn add_migrated_resource(issue: &mut Issue, resource: NewResource) -> Result<(), ResourceError> {
    match issue.add_resource(resource) {
        Ok(_) | Err(ResourceError::DuplicateTargetRole { .. }) => Ok(()),
        Err(
            error @ (ResourceError::MalformedWebUrl { .. }
            | ResourceError::MissingWebUrlAuthority { .. }
            | ResourceError::UnsupportedWebUrlScheme { .. }
            | ResourceError::UnknownRole { .. }
            | ResourceError::EmptyLabel
            | ResourceError::LabelControlCharacter { .. }
            | ResourceError::ResourceIdControlCharacter { .. }
            | ResourceError::DuplicateResourceId { .. }
            | ResourceError::IdSequenceExhausted
            | ResourceError::EmptyResourceId
            | ResourceError::EmptyPath
            | ResourceError::PathControlCharacter { .. }
            | ResourceError::WorkspacePathBackslash { .. }
            | ResourceError::AbsoluteWorkspacePath { .. }
            | ResourceError::WorkspacePathEscape { .. }
            | ResourceError::EmptyNormalizedWorkspacePath { .. }
            | ResourceError::EmptyUpdate
            | ResourceError::ResourceNotFound { .. }),
        ) => Err(error),
    }
}

fn default_next_resource_id() -> u64 {
    DEFAULT_NEXT_RESOURCE_ID
}

fn is_default_next_resource_id(value: &u64) -> bool {
    *value == DEFAULT_NEXT_RESOURCE_ID
}

pub(super) struct IssueRecordConversion {
    pub(super) issue: Issue,
    pub(super) migration_conflict: Option<MigrationField>,
    pub(super) assignment_migration: Option<String>,
}

/// A compatibility DTO for decoding persisted Issue records.
///
/// Optional canonical and legacy Kind fields are confined to this read seam.
/// Canonical writes use [`CanonicalIssueRecord`], whose Kind is required.
#[derive(Debug, Deserialize)]
pub(super) struct IssueRecord {
    id: IssueId,
    title: String,
    description: String,
    status: IssueStatus,
    priority: u8,
    /// Canonical field. Optional only while decoding so legacy-only records
    /// can reach `into_domain`.
    #[serde(default)]
    issue_kind: Option<IssueKind>,
    /// Legacy read-only field accepted during migration.
    #[serde(default)]
    issue_type: Option<IssueKind>,
    assignee: Option<String>,
    labels: Vec<String>,
    design: Option<String>,
    acceptance_criteria: Option<String>,
    #[serde(default)]
    notes: PersistedNotes,
    /// Canonical Associated Resource collection.
    #[serde(default)]
    resources: Vec<ResourceRecord>,
    /// Monotonic resource identifier sequence. Emitted only once resources
    /// exist so resource-free records do not grow a sequence field.
    #[serde(
        default = "default_next_resource_id",
        skip_serializing_if = "is_default_next_resource_id"
    )]
    next_resource_id: u64,
    /// Legacy read-only field accepted during migration and never written back.
    #[serde(default, skip_serializing)]
    external_ref: Option<String>,
    dependencies: Vec<Dependency>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    closed_at: Option<DateTime<Utc>>,
}

impl IssueRecord {
    pub(super) fn into_domain(self) -> Result<IssueRecordConversion, IssueRecordError> {
        let Self {
            id,
            title,
            description,
            status,
            priority,
            issue_kind,
            issue_type,
            assignee,
            labels,
            design,
            acceptance_criteria,
            notes,
            resources,
            next_resource_id,
            external_ref,
            dependencies,
            created_at,
            updated_at,
            closed_at,
        } = self;
        let (issue_kind, migration_conflict) = match (issue_kind, issue_type) {
            (Some(issue_kind), None) | (None, Some(issue_kind)) => (issue_kind, None),
            (Some(issue_kind), Some(issue_type)) if issue_kind == issue_type => (issue_kind, None),
            (Some(issue_kind), Some(_)) => (issue_kind, Some(MigrationField::IssueKind)),
            (None, None) => {
                return Err(invalid_data_error(
                    &id,
                    None,
                    "missing issue kind (`issue_kind` or legacy `issue_type`)",
                ));
            }
        };

        let note_error = |error: NoteError| invalid_data_error(&id, migration_conflict, error);
        let notes = match notes {
            PersistedNotes::Empty(()) => Vec::new(),
            PersistedNotes::Legacy(content) if content.trim().is_empty() => Vec::new(),
            PersistedNotes::Legacy(content) => {
                let content = NoteContent::new(content).map_err(note_error)?;
                vec![Note::from_parts(content, updated_at)]
            }
            PersistedNotes::Canonical(records) => records
                .into_iter()
                .map(NoteRecord::into_domain)
                .collect::<Result<Vec<_>, _>>()
                .map_err(note_error)?,
        };

        let resources = resources
            .into_iter()
            .map(ResourceRecord::into_domain)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|source| invalid_resource_error(&id, migration_conflict, source))?;

        let mut issue = Issue {
            id,
            title,
            description,
            status,
            priority,
            issue_kind,
            assignee,
            labels,
            design,
            acceptance_criteria,
            notes,
            resources: Vec::new(),
            next_resource_id: DEFAULT_NEXT_RESOURCE_ID,
            dependencies,
            created_at,
            updated_at,
            closed_at,
        };
        issue
            .rehydrate_resources(resources, next_resource_id)
            .map_err(|source| invalid_resource_error(&issue.id, migration_conflict, source))?;

        // Legacy `external_ref` migration (ADR-0003). Only a truly empty value
        // carries no context. Absolute Web URLs become Reference resources;
        // every other non-empty value is preserved visibly in a migration Note
        // (with terminal-unsafe control characters and backslashes escaped)
        // rather than guessed or discarded.
        if let Some(external_ref) = external_ref
            && !external_ref.is_empty()
        {
            match legacy_external_ref_url(&external_ref)
                .map_err(|source| invalid_resource_error(&issue.id, migration_conflict, source))?
            {
                Some(url) => {
                    let resource = NewResource {
                        target: ResourceTarget::web(url),
                        role: ResourceRole::Reference,
                        label: None,
                    };
                    add_migrated_resource(&mut issue, resource).map_err(|source| {
                        invalid_resource_error(&issue.id, migration_conflict, source)
                    })?;
                }
                None => {
                    let content = NoteContent::new(migrated_external_ref_note_text(&external_ref))
                        .map_err(|error| {
                            invalid_data_error(&issue.id, migration_conflict, error)
                        })?;
                    issue.append_note(content, updated_at);
                }
            }
        }
        let assignment_migration = match (issue.status, issue.assignee.as_deref()) {
            (IssueStatus::InProgress, None) => {
                issue.status = IssueStatus::Open;
                Some(
                    "Migration: changed unassigned In Progress Issue to Open because active work requires an Assignee"
                        .to_string(),
                )
            }
            (IssueStatus::Closed, Some(assignee)) => {
                let assignee = assignee.to_string();
                issue.assignee = None;
                Some(format!(
                    "Migration: cleared Assignee '{assignee}' from Closed Issue because Closed Issues cannot remain assigned"
                ))
            }
            (IssueStatus::Open | IssueStatus::InProgress | IssueStatus::Closed, None | Some(_)) => {
                None
            }
        };
        if let Some(message) = &assignment_migration {
            let content = NoteContent::new(message.clone())
                .map_err(|error| invalid_data_error(&issue.id, migration_conflict, error))?;
            issue.append_note(content, updated_at);
        }
        issue
            .validate_assignment_state()
            .map_err(|error| invalid_data_error(&issue.id, migration_conflict, error))?;
        issue
            .validate()
            .map_err(|error| invalid_data_error(&issue.id, migration_conflict, error))?;

        Ok(IssueRecordConversion {
            issue,
            migration_conflict,
            assignment_migration,
        })
    }
}

/// The canonical Issue shape written to disk.
#[derive(Debug, Serialize)]
pub(super) struct CanonicalIssueRecord {
    id: IssueId,
    title: String,
    description: String,
    status: IssueStatus,
    priority: u8,
    issue_kind: IssueKind,
    assignee: Option<String>,
    labels: Vec<String>,
    design: Option<String>,
    acceptance_criteria: Option<String>,
    notes: PersistedNotes,
    /// Canonical Associated Resource collection.
    resources: Vec<ResourceRecord>,
    /// Monotonic resource identifier sequence. Emitted only once resources
    /// exist so resource-free records do not grow a sequence field.
    #[serde(
        default = "default_next_resource_id",
        skip_serializing_if = "is_default_next_resource_id"
    )]
    next_resource_id: u64,
    dependencies: Vec<Dependency>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    closed_at: Option<DateTime<Utc>>,
}

impl From<Issue> for CanonicalIssueRecord {
    fn from(issue: Issue) -> Self {
        let Issue {
            id,
            title,
            description,
            status,
            priority,
            issue_kind,
            assignee,
            labels,
            design,
            acceptance_criteria,
            notes,
            resources,
            next_resource_id,
            dependencies,
            created_at,
            updated_at,
            closed_at,
        } = issue;

        Self {
            id,
            title,
            description,
            status,
            priority,
            issue_kind,
            assignee,
            labels,
            design,
            acceptance_criteria,
            notes: PersistedNotes::Canonical(notes.into_iter().map(Into::into).collect()),
            resources: resources.into_iter().map(Into::into).collect(),
            next_resource_id,
            dependencies,
            created_at,
            updated_at,
            closed_at,
        }
    }
}
