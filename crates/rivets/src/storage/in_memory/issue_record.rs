//! Compatibility boundary between persisted JSONL issue records and the domain model.

use crate::domain::{Dependency, Issue, IssueId, IssueStatus, IssueType};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A domain field with legacy and canonical persisted representations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationField {
    IssueKind,
}

impl MigrationField {
    pub const fn name(self) -> &'static str {
        match self {
            Self::IssueKind => "issue_kind",
        }
    }

    pub const fn legacy_name(self) -> &'static str {
        match self {
            Self::IssueKind => "issue_type",
        }
    }

    pub const fn canonical_name(self) -> &'static str {
        match self {
            Self::IssueKind => "issue_kind",
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
    issue_type: IssueType,
    #[serde(default, skip_serializing)]
    issue_kind: Option<IssueType>,
    assignee: Option<String>,
    labels: Vec<String>,
    design: Option<String>,
    acceptance_criteria: Option<String>,
    notes: Option<String>,
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
            issue_type,
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
        } = self;
        let issue_type = match issue_kind {
            None => issue_type,
            Some(issue_kind) if issue_kind == issue_type => issue_kind,
            Some(_) => {
                return Err(IssueRecordError::MigrationConflict {
                    issue_id: id,
                    field: MigrationField::IssueKind,
                });
            }
        };

        let issue = Issue {
            id,
            title,
            description,
            status,
            priority,
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
        } = issue;

        Self {
            id,
            title,
            description,
            status,
            priority,
            issue_type,
            issue_kind: None,
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
        }
    }
}
