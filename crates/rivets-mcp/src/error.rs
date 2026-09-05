//! Error types for the rivets MCP server.

use rivets::error::Error as RivetsError;
use rmcp::ErrorData as McpError;
use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur in the rivets MCP server.
#[derive(Debug, Error)]
pub enum Error {
    /// No workspace context has been set.
    #[error("No workspace context set. Provide workspace_root or call set_context.")]
    NoContext,

    /// Another writer currently owns the Workspace mutation transaction.
    #[error(
        "Workspace is busy: '{}'; retry the operation",
        workspace_root.display()
    )]
    WorkspaceBusy {
        /// Canonical root of the contended Workspace.
        workspace_root: PathBuf,
    },

    /// Invalid argument value provided.
    #[error("Invalid {field}: '{value}'. Valid values: {valid_values}")]
    InvalidArgument {
        /// The field name that had an invalid value.
        field: &'static str,
        /// The invalid value that was provided.
        value: String,
        /// Description of valid values.
        valid_values: &'static str,
    },

    /// Issue ID input failed domain parsing.
    #[error(transparent)]
    InvalidIssueId(#[from] rivets::domain::IssueIdError),

    /// Issue Label input failed domain parsing.
    #[error(transparent)]
    InvalidLabel(#[from] rivets::domain::LabelError),

    /// Note content failed domain validation.
    #[error("Invalid note: {0}")]
    InvalidNote(#[from] rivets::domain::NoteError),

    /// Associated Resource input failed domain validation.
    #[error("Invalid resource: {0}")]
    InvalidResource(#[from] rivets::domain::ResourceError),

    /// A status change violated the domain transition rules.
    ///
    /// Transparent so MCP rejects a transition with the same observable
    /// error as the CLI (ADR-0005: the domain owns transition rules).
    #[error(transparent)]
    InvalidStatusTransition(#[from] rivets::domain::StatusTransitionError),

    /// An Assignment Claim or Release violated the domain contract.
    #[error(transparent)]
    Assignment(#[from] rivets::domain::AssignmentError),

    /// The requested issue was not found.
    #[error("Issue not found: {0}")]
    IssueNotFound(String),

    /// The specified workspace was not found or path is invalid.
    #[error("Workspace not found: {path}")]
    WorkspaceNotFound {
        /// The path that was not found.
        path: String,
        /// The underlying IO error, if any.
        #[source]
        source: Option<std::io::Error>,
    },

    /// Workspace exists but is not present in the context cache.
    #[error("Workspace not initialized: {0}")]
    WorkspaceNotInitialized(String),

    /// Failed to discover a rivets workspace.
    #[error("No .rivets directory found in {0} or parent directories")]
    NoRivetsDirectory(String),

    /// Failed to load workspace configuration.
    #[error(
        "Failed to load config from '{path}': {reason}. Run 'rivets init' to create a valid configuration."
    )]
    ConfigLoad {
        /// The path to the config file.
        path: String,
        /// The reason for the failure.
        reason: String,
    },
    /// Blocking Dependency endpoint roles failed domain validation.
    #[error("Invalid Blocking Dependency: {0}")]
    InvalidBlockingDependency(#[from] rivets::domain::BlockingDependencyError),

    /// Related Association endpoints failed domain validation.
    #[error("Invalid Related Association: {0}")]
    InvalidRelatedAssociation(#[from] rivets::domain::RelatedAssociationError),

    /// Discovery Origin endpoint roles failed domain validation.
    #[error("Invalid Discovery Origin: {0}")]
    InvalidDiscoveryOrigin(#[from] rivets::domain::DiscoveryOriginError),

    /// The requested Related Association does not exist.
    #[error("Related association not found: {left_issue_id} <-> {right_issue_id}")]
    RelatedAssociationNotFound {
        /// Canonical left endpoint.
        left_issue_id: String,
        /// Canonical right endpoint.
        right_issue_id: String,
    },

    /// The requested Discovery Origin already exists.
    #[error("Discovery origin already exists: {discovered_issue_id} -> {source_issue_id}")]
    DuplicateDiscoveryOrigin {
        /// Discovered Issue.
        discovered_issue_id: String,
        /// Source Issue.
        source_issue_id: String,
    },

    /// The requested Discovery Origin does not exist.
    #[error("Discovery origin not found: {discovered_issue_id} -> {source_issue_id}")]
    DiscoveryOriginNotFound {
        /// Discovered Issue.
        discovered_issue_id: String,
        /// Source Issue.
        source_issue_id: String,
    },

    /// Adding a Discovery Origin would create a cycle.
    #[error(
        "Discovery origin cycle detected: adding origin from {discovered_issue_id} to {source_issue_id} would create a cycle"
    )]
    CircularDiscoveryOrigin {
        /// Discovered Issue.
        discovered_issue_id: String,
        /// Source Issue.
        source_issue_id: String,
    },

    /// A Parentage invariant or transition was rejected.
    #[error(transparent)]
    InvalidParentage(#[from] rivets::domain::ParentageError),

    /// An error from the rivets storage layer.
    #[error("Storage error: {0}")]
    Storage(#[source] RivetsError),

    /// An I/O error occurred.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization/deserialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

impl Error {
    pub(crate) fn to_mcp_error(&self) -> McpError {
        match self {
            Self::WorkspaceBusy { workspace_root } => McpError::internal_error(
                self.to_string(),
                Some(serde_json::json!({
                    "retryable": true,
                    "workspace_root": workspace_root,
                })),
            ),
            Self::NoContext
            | Self::InvalidArgument { .. }
            | Self::InvalidIssueId(_)
            | Self::InvalidLabel(_)
            | Self::InvalidResource(_)
            | Self::InvalidNote(_)
            | Self::InvalidBlockingDependency(_)
            | Self::InvalidParentage(_)
            | Self::InvalidStatusTransition(_)
            | Self::Assignment(_)
            | Self::InvalidRelatedAssociation(_)
            | Self::InvalidDiscoveryOrigin(_)
            | Self::RelatedAssociationNotFound { .. }
            | Self::DuplicateDiscoveryOrigin { .. }
            | Self::DiscoveryOriginNotFound { .. }
            | Self::CircularDiscoveryOrigin { .. }
            | Self::IssueNotFound(_) => McpError::invalid_params(self.to_string(), None),
            Self::WorkspaceNotFound { .. }
            | Self::WorkspaceNotInitialized(_)
            | Self::NoRivetsDirectory(_)
            | Self::ConfigLoad { .. }
            | Self::Storage(_)
            | Self::Io(_)
            | Self::Json(_) => McpError::internal_error(self.to_string(), None),
        }
    }
}

/// Result type for rivets MCP operations.
pub type Result<T> = std::result::Result<T, Error>;

impl From<RivetsError> for Error {
    fn from(error: RivetsError) -> Self {
        match error {
            RivetsError::IssueNotFound(issue_id) => Self::IssueNotFound(issue_id.to_string()),
            RivetsError::WorkspaceBusy { workspace_root } => Self::WorkspaceBusy { workspace_root },
            RivetsError::RelatedAssociationNotFound {
                left_issue_id,
                right_issue_id,
            } => Self::RelatedAssociationNotFound {
                left_issue_id: left_issue_id.to_string(),
                right_issue_id: right_issue_id.to_string(),
            },
            RivetsError::DuplicateDiscoveryOrigin {
                discovered_issue_id,
                source_issue_id,
            } => Self::DuplicateDiscoveryOrigin {
                discovered_issue_id: discovered_issue_id.to_string(),
                source_issue_id: source_issue_id.to_string(),
            },
            RivetsError::DiscoveryOriginNotFound {
                discovered_issue_id,
                source_issue_id,
            } => Self::DiscoveryOriginNotFound {
                discovered_issue_id: discovered_issue_id.to_string(),
                source_issue_id: source_issue_id.to_string(),
            },
            RivetsError::CircularDiscoveryOrigin {
                discovered_issue_id,
                source_issue_id,
            } => Self::CircularDiscoveryOrigin {
                discovered_issue_id: discovered_issue_id.to_string(),
                source_issue_id: source_issue_id.to_string(),
            },
            RivetsError::Storage(storage_error) => match storage_error.try_into_resource_error() {
                Ok(source) => Self::InvalidResource(source),
                Err(storage_error) => match storage_error.try_into_status_transition_error() {
                    Ok(source) => Self::InvalidStatusTransition(source),
                    Err(storage_error) => match storage_error.try_into_assignment_error() {
                        Ok(source) => Self::Assignment(source),
                        Err(storage_error) => Self::Storage(RivetsError::Storage(storage_error)),
                    },
                },
            },
            RivetsError::InvalidParentage(source) => Self::InvalidParentage(source),
            error @ (RivetsError::Io(_)
            | RivetsError::WorkspaceLock { .. }
            | RivetsError::Config(_)
            | RivetsError::Validation { .. }
            | RivetsError::HasDependents { .. }
            | RivetsError::CircularDependency { .. }
            | RivetsError::InvalidIssueId(_)
            | RivetsError::InvalidPriority(_)
            | RivetsError::DependencyNotFound { .. }
            | RivetsError::IssueAlreadyExists(_)
            | RivetsError::Json(_)) => Self::Storage(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivets::domain::{
        AssignmentError, IssueId, IssueKind, IssueStatus, ParentageError, ResourceError,
        StatusTransitionError,
    };
    use rivets::error::StorageError;

    #[test]
    fn core_issue_not_found_maps_to_mcp_issue_not_found() {
        let error = Error::from(RivetsError::IssueNotFound(IssueId::new("test-missing")));
        assert!(matches!(
            error,
            Error::IssueNotFound(issue_id) if issue_id == "test-missing"
        ));
    }

    #[test]
    fn core_workspace_busy_maps_to_retryable_mcp_variant() {
        let workspace_root = PathBuf::from("/tmp/workspace");
        let error = Error::from(RivetsError::WorkspaceBusy {
            workspace_root: workspace_root.clone(),
        });
        assert!(matches!(
            error,
            Error::WorkspaceBusy {
                workspace_root: actual
            } if actual == workspace_root
        ));
    }

    #[test]
    fn workspace_busy_protocol_error_is_retryable() {
        use rmcp::model::ErrorCode;

        let error = Error::WorkspaceBusy {
            workspace_root: PathBuf::from("/tmp/workspace"),
        }
        .to_mcp_error();
        assert_eq!(error.code, ErrorCode::INTERNAL_ERROR);
        assert_eq!(
            error.data,
            Some(serde_json::json!({
                "retryable": true,
                "workspace_root": "/tmp/workspace",
            }))
        );
    }

    #[test]
    fn storage_resource_error_maps_to_invalid_resource() {
        let error = Error::from(RivetsError::Storage(StorageError::Resource(
            ResourceError::EmptyLabel,
        )));
        assert!(matches!(
            error,
            Error::InvalidResource(ResourceError::EmptyLabel)
        ));
    }

    #[test]
    fn storage_transition_error_maps_to_invalid_status_transition() {
        let error = Error::from(RivetsError::Storage(StorageError::InvalidStatusTransition(
            StatusTransitionError::NotClosed {
                current: IssueStatus::Open,
            },
        )));

        assert!(matches!(
            error,
            Error::InvalidStatusTransition(StatusTransitionError::NotClosed {
                current: IssueStatus::Open
            })
        ));
    }
    #[test]
    fn assignment_errors_preserve_retry_classification() {
        use rmcp::model::ErrorCode;

        let issue_id = IssueId::new("test-claimed");
        let assignment_errors = [
            AssignmentError::NotOpen {
                issue_id: issue_id.clone(),
                status: IssueStatus::InProgress,
            },
            AssignmentError::Blocked {
                issue_id: issue_id.clone(),
            },
            AssignmentError::AlreadyClaimed {
                issue_id: issue_id.clone(),
                assignee: "alice".to_string(),
            },
            AssignmentError::NotClaimed {
                issue_id: issue_id.clone(),
            },
            AssignmentError::AssigneeMismatch {
                issue_id: issue_id.clone(),
                expected: "bob".to_string(),
                actual: "alice".to_string(),
            },
            AssignmentError::AssigneeRequired {
                issue_id: issue_id.clone(),
            },
            AssignmentError::BlankAssignee {
                issue_id: issue_id.clone(),
            },
            AssignmentError::ClosedCannotBeAssigned { issue_id },
        ];

        for source in assignment_errors {
            let error = Error::from(RivetsError::Storage(StorageError::Assignment(source)));
            assert!(matches!(error, Error::Assignment(_)));
            let protocol = error.to_mcp_error();
            assert_eq!(protocol.code, ErrorCode::INVALID_PARAMS);
            assert_eq!(protocol.data, None);
        }
    }

    #[test]
    fn non_resource_storage_error_remains_a_storage_error() {
        let error = Error::from(RivetsError::Storage(StorageError::InvalidFormat(
            "bad record".to_string(),
        )));
        assert!(matches!(
            error,
            Error::Storage(RivetsError::Storage(StorageError::InvalidFormat(message)))
                if message == "bad record"
        ));
    }

    #[test]
    fn parentage_tool_errors_are_invalid_params() {
        use rmcp::model::ErrorCode;

        let child_id = IssueId::new("test-child");
        let parent_id = IssueId::new("test-parent");
        let errors = [
            ParentageError::SelfReference {
                issue_id: child_id.clone(),
            },
            ParentageError::ParentNotEpic {
                parent_id: parent_id.clone(),
                actual_kind: IssueKind::Task,
            },
            ParentageError::AlreadyParented {
                child_id: child_id.clone(),
                parent_id: parent_id.clone(),
            },
            ParentageError::NoParent {
                child_id: child_id.clone(),
            },
            ParentageError::Cycle {
                child_id: child_id.clone(),
                parent_id: parent_id.clone(),
            },
            ParentageError::ClosedParent {
                child_id: child_id.clone(),
                parent_id: parent_id.clone(),
            },
            ParentageError::ActiveChildren {
                epic_id: parent_id.clone(),
                child_ids: vec![child_id.clone()],
            },
            ParentageError::ParentHasChildren {
                parent_id,
                child_ids: vec![child_id],
            },
        ];

        for error in errors {
            assert_eq!(
                Error::InvalidParentage(error).to_mcp_error().code,
                ErrorCode::INVALID_PARAMS
            );
        }
    }
}
