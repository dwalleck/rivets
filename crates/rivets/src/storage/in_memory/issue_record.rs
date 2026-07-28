//! Compatibility boundary between persisted JSONL issue records and the domain model.

use crate::domain::{
    Dependency, Issue, IssueId, IssueKind, IssueStatus, Note, NoteContent, NoteError,
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

#[derive(Debug)]
pub(super) enum IssueRecordError {
    MigrationConflict {
        issue_id: IssueId,
        field: MigrationField,
    },
    InvalidData {
        issue_id: IssueId,
        error: String,
    },
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

/// The current on-disk Issue shape.
///
/// This DTO owns all serde behavior for JSONL persistence. The domain [`Issue`]
/// remains independently serializable for JSON output, but loading persisted
/// records must pass through [`IssueRecord::into_domain`].
#[derive(Debug, Serialize, Deserialize)]
pub(super) struct IssueRecord {
    id: IssueId,
    title: String,
    description: String,
    status: IssueStatus,
    priority: u8,
    /// Canonical field. Optional while decoding so legacy-only records reach
    /// `into_domain`; canonical writes always populate it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    issue_kind: Option<IssueKind>,
    /// Legacy read-only field accepted during migration and never written back.
    #[serde(default, skip_serializing)]
    issue_type: Option<IssueKind>,
    assignee: Option<String>,
    labels: Vec<String>,
    design: Option<String>,
    acceptance_criteria: Option<String>,
    #[serde(default)]
    notes: PersistedNotes,
    external_ref: Option<String>,
    dependencies: Vec<Dependency>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    closed_at: Option<DateTime<Utc>>,
}

impl IssueRecord {
    pub(super) fn into_domain(self) -> Result<Issue, IssueRecordError> {
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
            external_ref,
            dependencies,
            created_at,
            updated_at,
            closed_at,
        } = self;
        let issue_kind = match (issue_kind, issue_type) {
            (Some(issue_kind), None) | (None, Some(issue_kind)) => issue_kind,
            (Some(issue_kind), Some(issue_type)) if issue_kind == issue_type => issue_kind,
            (Some(_), Some(_)) => {
                return Err(IssueRecordError::MigrationConflict {
                    issue_id: id,
                    field: MigrationField::IssueKind,
                });
            }
            (None, None) => {
                return Err(IssueRecordError::InvalidData {
                    issue_id: id,
                    error: "missing issue kind (`issue_kind` or legacy `issue_type`)".to_string(),
                });
            }
        };

        let note_error = |error: NoteError| IssueRecordError::InvalidData {
            issue_id: id.clone(),
            error: error.to_string(),
        };
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

        let issue = Issue {
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
            external_ref,
            dependencies,
            created_at,
            updated_at,
            closed_at,
        };
        issue
            .validate()
            .map_err(|error| IssueRecordError::InvalidData {
                issue_id: issue.id.clone(),
                error,
            })?;

        Ok(issue)
    }
}

impl From<Issue> for IssueRecord {
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
            external_ref,
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
            issue_kind: Some(issue_kind),
            // Accepted only during loading; canonical writes never emit `issue_type`.
            issue_type: None,
            assignee,
            labels,
            design,
            acceptance_criteria,
            notes: PersistedNotes::Canonical(notes.into_iter().map(Into::into).collect()),
            external_ref,
            dependencies,
            created_at,
            updated_at,
            closed_at,
        }
    }
}
