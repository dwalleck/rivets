//! MCP models.
//!
//! This module contains tool input parameter types and the few response
//! envelopes that have no domain counterpart (context, stats, blocked
//! groupings). Domain records (Issue, Note, Resource, Dependency) serialize
//! directly through their own serde derives per ADR-0004; nothing here
//! mirrors them.

use rivets::domain::{Issue, IssueKind};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ============================================================================
// Tool Input Parameters
// ============================================================================

/// Schema-only stand-in for the domain [`IssueKind`] wire vocabulary.
///
/// schemars cannot derive `JsonSchema` for the domain type from another
/// crate (orphan rule), and the rivets crate must not gain a schemars
/// dependency, so this local type renders the `issue_kind` enum values in
/// tool schemas. It performs no parsing; its variant list is a fenced
/// duplicate of the domain vocabulary — the schema fence test
/// (`issue_kind_schema_matches_domain_display`) pins its values to the
/// domain enum's Display strings so a new Kind cannot silently drift.
#[derive(JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum McpIssueKindSchema {
    /// Bug fix.
    Bug,
    /// New feature.
    Feature,
    /// General task.
    Task,
    /// Epic grouping.
    Epic,
    /// Maintenance chore.
    Chore,
}

/// Canonical and migration-only names for an MCP Issue Kind input.
///
/// `issue_type` remains accepted for compatibility but is omitted from the
/// generated schema and from serialized canonical requests.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct IssueKindInput {
    /// Canonical Issue Kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<McpIssueKindSchema>")]
    pub issue_kind: Option<IssueKind>,

    #[serde(default, rename = "issue_type", skip_serializing)]
    #[schemars(skip)]
    legacy_issue_type: Option<IssueKind>,
}

impl IssueKindInput {
    /// Construct a canonical MCP Issue Kind input.
    #[must_use]
    pub const fn canonical(issue_kind: Option<IssueKind>) -> Self {
        Self {
            issue_kind,
            legacy_issue_type: None,
        }
    }

    /// Resolve compatibility fields, preferring `issue_kind` on conflict.
    #[must_use]
    pub fn resolve(self, operation: &'static str) -> Option<IssueKind> {
        match (self.issue_kind, self.legacy_issue_type) {
            (Some(issue_kind), Some(issue_type)) if issue_kind != issue_type => {
                tracing::warn!(
                    operation,
                    issue_kind = ?issue_kind,
                    issue_type = ?issue_type,
                    "Conflicting MCP Issue Kind fields; using issue_kind"
                );
                Some(issue_kind)
            }
            (Some(issue_kind), _) | (None, Some(issue_kind)) => Some(issue_kind),
            (None, None) => None,
        }
    }
}

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

    /// Filter by Issue Kind.
    #[serde(flatten)]
    pub kind: IssueKindInput,

    /// Filter by assignee.
    pub assignee: Option<String>,

    /// Include Issues regardless of Assignment.
    #[serde(default)]
    pub all_assignees: bool,

    /// Filter by label.
    pub label: Option<String>,

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

    /// Filter by Issue Kind.
    #[serde(flatten)]
    pub kind: IssueKindInput,

    /// Filter by assignee.
    pub assignee: Option<String>,

    /// Filter by label.
    pub label: Option<String>,

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

    /// Priority level (0-4, default 2).
    pub priority: Option<u8>,

    /// Issue Kind (bug, feature, task, epic, chore).
    #[serde(flatten)]
    pub kind: IssueKindInput,

    /// Assignee.
    pub assignee: Option<String>,

    /// Labels.
    pub labels: Option<Vec<String>>,

    /// Design notes.
    pub design: Option<String>,

    /// Acceptance criteria.
    pub acceptance: Option<String>,

    /// Initial Note.
    pub initial_note: Option<String>,

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

    /// New Issue Kind.
    #[serde(flatten)]
    pub kind: IssueKindInput,

    /// New title.
    pub title: Option<String>,

    /// New description.
    pub description: Option<String>,

    /// New design notes.
    pub design: Option<String>,

    /// New acceptance criteria.
    pub acceptance_criteria: Option<String>,

    /// New labels (replaces existing labels).
    pub labels: Option<Vec<String>>,

    /// Optional workspace root (uses current context if not specified).
    pub workspace_root: Option<String>,
}

/// Parameters for atomic Assignment Claim and Release tools.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AssignmentParams {
    /// Issue ID whose Assignment changes.
    pub issue_id: String,

    /// Exact Assignee identity to claim as or release.
    pub assignee: String,

    /// Optional workspace root (uses current context if not specified).
    pub workspace_root: Option<String>,
}

/// Parameters for the `add_note` tool.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AddNoteParams {
    /// The Issue receiving the Note.
    pub issue_id: String,

    /// Immutable Note content.
    pub content: String,

    /// Optional workspace root (uses current context if not specified).
    pub workspace_root: Option<String>,
}

/// Parameters for the `resource_add` tool.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ResourceAddParams {
    /// The Issue receiving the Associated Resource.
    pub issue_id: String,

    /// Absolute HTTP or HTTPS URL (exactly one of url/path is required).
    pub url: Option<String>,

    /// Path relative to the workspace root (exactly one of url/path is required).
    pub path: Option<String>,

    /// Resource Role (implementation, documentation, evidence, successor, reference).
    pub role: String,

    /// Optional human-readable label.
    pub label: Option<String>,

    /// Optional workspace root (uses current context if not specified).
    pub workspace_root: Option<String>,
}

/// Parameters for the `resource_update` tool.
///
/// Only the provided fields change; the resource keeps its stable identifier
/// and position. At least one of `url`/`path`/`role`/`label`/`clear_label` is
/// required.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ResourceUpdateParams {
    /// The Issue whose Associated Resource should be updated.
    pub issue_id: String,

    /// Stable resource identifier (e.g. "r3").
    pub resource_id: String,

    /// New absolute HTTP or HTTPS URL (at most one of url/path).
    pub url: Option<String>,

    /// New path relative to the workspace root (at most one of url/path).
    pub path: Option<String>,

    /// New Resource Role.
    pub role: Option<String>,

    /// New human-readable label (at most one of `label`/`clear_label`).
    pub label: Option<String>,

    /// Clear the resource's label.
    #[serde(default)]
    pub clear_label: bool,

    /// Optional workspace root (uses current context if not specified).
    pub workspace_root: Option<String>,
}

/// Parameters for the `resource_remove` tool.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ResourceRemoveParams {
    /// The Issue whose Associated Resource should be removed.
    pub issue_id: String,

    /// Stable resource identifier (e.g. "r3").
    pub resource_id: String,

    /// Optional workspace root (uses current context if not specified).
    pub workspace_root: Option<String>,
}

/// Parameters for the `resource_list` tool.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ResourceListParams {
    /// The Issue whose Associated Resources should be listed.
    pub issue_id: String,

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

/// Parameters shared by Blocking Dependency add and remove tools.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BlockingDependencyPairParams {
    /// Issue that depends on the prerequisite.
    pub dependent_id: String,
    /// Issue that must be completed first.
    pub prerequisite_id: String,
    /// Optional workspace root (uses current context if not specified).
    pub workspace_root: Option<String>,
}

/// One valid perspective for listing Blocking Dependencies.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BlockingDependencyListQuery {
    /// List prerequisites required by one dependent.
    PrerequisitesOf {
        /// Issue whose prerequisite edges are requested.
        dependent_id: String,
    },
    /// List Issues that depend on one prerequisite.
    DependentsOf {
        /// Issue whose incoming dependent edges are requested.
        prerequisite_id: String,
    },
}

/// Parameters for the canonical Blocking Dependency list tool.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BlockingDependencyListParams {
    /// Role-safe endpoint perspective.
    pub query: BlockingDependencyListQuery,
    /// Optional workspace root (uses current context if not specified).
    pub workspace_root: Option<String>,
}

/// Parameters for the canonical Blocking Dependency tree tool.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BlockingDependencyTreeParams {
    /// Root dependent Issue.
    pub dependent_id: String,
    /// Maximum depth; zero means unlimited.
    pub depth: Option<usize>,
    /// Optional workspace root (uses current context if not specified).
    pub workspace_root: Option<String>,
}

/// Parameters for the `reopen` tool.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReopenParams {
    /// The issue ID to reopen.
    pub issue_id: String,

    /// Reason for reopening.
    pub reason: Option<String>,

    /// Optional workspace root (uses current context if not specified).
    pub workspace_root: Option<String>,
}

/// Parameters for the `stale` tool.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct StaleParams {
    /// Number of days since last update to consider stale (default: 30).
    pub days: Option<u32>,

    /// Filter by status.
    pub status: Option<String>,

    /// Maximum number of issues to return.
    pub limit: Option<usize>,

    /// Optional workspace root (uses current context if not specified).
    pub workspace_root: Option<String>,
}

/// Parameters for the `label_add` tool.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LabelAddParams {
    /// The issue ID to add the label to.
    pub issue_id: String,

    /// The label to add.
    pub label: String,

    /// Optional workspace root (uses current context if not specified).
    pub workspace_root: Option<String>,
}

/// Parameters for the `label_remove` tool.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LabelRemoveParams {
    /// The issue ID to remove the label from.
    pub issue_id: String,

    /// The label to remove.
    pub label: String,

    /// Optional workspace root (uses current context if not specified).
    pub workspace_root: Option<String>,
}

/// Parameters for the `label_list` tool.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LabelListParams {
    /// The issue ID to list labels for.
    pub issue_id: String,

    /// Optional workspace root (uses current context if not specified).
    pub workspace_root: Option<String>,
}

/// Parameters for the `label_list_all` tool.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct LabelListAllParams {
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

    /// The issue ID prefix (e.g., "proj" for "proj-abc"), if available.
    pub issue_prefix: Option<String>,
}

/// Blocked issue response.
#[derive(Debug, Clone, Serialize)]
pub struct BlockedIssueResponse {
    /// The blocked issue.
    pub issue: Issue,

    /// Issues blocking this one.
    pub blockers: Vec<Issue>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[test]
    fn ready_params_read_legacy_issue_type() {
        let params: ReadyParams = serde_json::from_value(serde_json::json!({
            "issue_type": "bug"
        }))
        .expect("legacy issue_type should deserialize");

        assert_eq!(params.kind.resolve("ready"), Some(IssueKind::Bug));
    }

    #[test]
    fn ready_params_default_and_explicit_all_assignees() {
        let default: ReadyParams =
            serde_json::from_value(serde_json::json!({})).expect("empty Ready input should parse");
        assert!(!default.all_assignees);

        let all: ReadyParams = serde_json::from_value(serde_json::json!({
            "all_assignees": true
        }))
        .expect("explicit all_assignees should parse");
        assert!(all.all_assignees);
    }

    #[test]
    fn list_params_read_legacy_issue_type() {
        let params: ListParams = serde_json::from_value(serde_json::json!({
            "issue_type": "feature"
        }))
        .expect("legacy issue_type should deserialize");

        assert_eq!(params.kind.resolve("list"), Some(IssueKind::Feature));
    }

    #[test]
    fn create_params_read_legacy_issue_type() {
        let params: CreateParams = serde_json::from_value(serde_json::json!({
            "title": "Legacy input",
            "issue_type": "epic"
        }))
        .expect("legacy issue_type should deserialize");

        assert_eq!(params.kind.resolve("create"), Some(IssueKind::Epic));
    }

    #[test]
    fn update_params_read_legacy_issue_type() {
        let params: UpdateParams = serde_json::from_value(serde_json::json!({
            "issue_id": "rivets-test",
            "issue_type": "chore"
        }))
        .expect("legacy issue_type should deserialize");

        assert_eq!(params.kind.resolve("update"), Some(IssueKind::Chore));
    }

    #[test]
    fn conflicting_mcp_kind_fields_use_canonical_kind() {
        let params: CreateParams = serde_json::from_value(serde_json::json!({
            "title": "Conflicting input",
            "issue_kind": "feature",
            "issue_type": "task"
        }))
        .expect("conflicting compatibility fields should deserialize");

        assert_eq!(params.kind.resolve("create"), Some(IssueKind::Feature));
    }

    #[rstest]
    #[case::bug("bug", IssueKind::Bug)]
    #[case::feature("feature", IssueKind::Feature)]
    #[case::task("task", IssueKind::Task)]
    #[case::epic("epic", IssueKind::Epic)]
    #[case::chore("chore", IssueKind::Chore)]
    fn mcp_kind_input_accepts_canonical_names(#[case] input: &str, #[case] expected: IssueKind) {
        // MCP accepts exactly the canonical domain strings.
        let params: ReadyParams =
            serde_json::from_value(serde_json::json!({ "issue_kind": input }))
                .expect("canonical Issue Kind should deserialize");
        assert_eq!(params.kind.resolve("ready"), Some(expected));
    }

    #[rstest]
    #[case::uppercase("BUG")]
    #[case::mixed_case("bUg")]
    #[case::unknown("bogus")]
    #[case::empty("")]
    fn mcp_kind_input_rejects_noncanonical_names(#[case] input: &str) {
        // The former case-insensitive leniency is gone; non-canonical
        // spellings fail with serde's unknown-variant error.
        let result: Result<ReadyParams, _> =
            serde_json::from_value(serde_json::json!({ "issue_kind": input }));
        let error = result.expect_err("non-canonical Issue Kind must be rejected");
        assert!(
            error.to_string().contains("unknown variant"),
            "error should be serde's unknown-variant shape: {error}"
        );
    }

    #[test]
    fn issue_kind_schema_matches_domain_display() {
        // Fence: the schema mirror's enum values must equal the domain
        // enum's Display strings, so a new Kind cannot silently drift from
        // the tool schema.
        use schemars::schema_for;

        let schema = schema_for!(IssueKindInput);
        let schema_json = serde_json::to_value(&schema).expect("schema serializes");
        let kind_schema = &schema_json["properties"]["issue_kind"];
        // Option<T> renders as `anyOf: [{ $ref: #/$defs/... }, { type: null }]`;
        // resolve the reference; schemars 1.x lists enum values as `oneOf`
        // const entries inside the referenced definition.
        let reference = kind_schema["anyOf"][0]["$ref"]
            .as_str()
            .expect("issue_kind schema references its definition");
        let def_name = reference
            .rsplit('/')
            .next()
            .expect("reference names a definition");
        let enum_values: Vec<String> = schema_json["$defs"][def_name]["oneOf"]
            .as_array()
            .expect("issue_kind definition lists oneOf const values")
            .iter()
            .map(|entry| {
                entry["const"]
                    .as_str()
                    .expect("const value is a string")
                    .to_string()
            })
            .collect();
        let expected: Vec<String> = [
            IssueKind::Bug,
            IssueKind::Feature,
            IssueKind::Task,
            IssueKind::Epic,
            IssueKind::Chore,
        ]
        .iter()
        .map(ToString::to_string)
        .collect();
        assert_eq!(enum_values, expected);
    }
}

/// One role-named entry in a Blocking prerequisite tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockingDependencyTreeEntry {
    /// Issue that depends on the prerequisite.
    pub dependent_id: String,
    /// Issue that must be completed first.
    pub prerequisite_id: String,
    /// One-based distance from the requested root dependent.
    pub depth: usize,
}

/// Structured Blocking prerequisite tree response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockingDependencyTreeResponse {
    /// Root dependent used for the traversal.
    pub root_dependent_id: String,
    /// Blocking edges in deterministic breadth-first order.
    pub prerequisites: Vec<BlockingDependencyTreeEntry>,
}
