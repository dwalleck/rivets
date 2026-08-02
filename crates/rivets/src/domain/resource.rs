//! Associated Resource domain types.
//!
//! An Associated Resource is a typed reference from an Issue to relevant
//! information or an artifact. Resources form a mutable curated index with no
//! effect on workflow or readiness. See ADR-0003 and `CONTEXT.md`.

use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use std::sync::OnceLock;

use super::{find_control_char, join_canonical_names};

/// Opaque, stable identifier of an Associated Resource within its Issue.
///
/// Identifiers are assigned by the domain when a resource is added and are
/// never reused, so later update and removal operations can target a resource
/// without positional indices.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ResourceId(String);

impl ResourceId {
    /// Parse a non-empty, terminal-safe identifier.
    ///
    /// # Errors
    ///
    /// Returns [`ResourceError::EmptyResourceId`] when the value is empty
    /// after trimming, or [`ResourceError::ResourceIdControlCharacter`] when
    /// it contains a terminal-unsafe control character.
    pub fn new(id: impl Into<String>) -> Result<Self, ResourceError> {
        let id = id.into();
        if id.trim().is_empty() {
            return Err(ResourceError::EmptyResourceId);
        }
        if let Some(position) = id.chars().position(char::is_control) {
            return Err(ResourceError::ResourceIdControlCharacter { position });
        }
        Ok(Self(id))
    }

    /// Get the identifier as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ResourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A validated absolute HTTP or HTTPS URL.
///
/// Construction parses and normalizes the URL so that equality and duplicate
/// detection are insensitive to purely syntactic differences.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct WebUrl(String);

impl WebUrl {
    /// Parse an absolute HTTP or HTTPS URL.
    ///
    /// # Errors
    ///
    /// Returns [`ResourceError::MalformedWebUrl`] when syntax is malformed,
    /// [`ResourceError::MissingWebUrlAuthority`] when no host is present, or
    /// [`ResourceError::UnsupportedWebUrlScheme`] for non-HTTP(S) schemes.
    pub fn new(raw: impl Into<String>) -> Result<Self, ResourceError> {
        let raw = raw.into();
        // Match url::Url's preprocessing before checking the caller-supplied
        // authority marker, so leading C0 controls or spaces do not shift the
        // scheme offset relative to the parsed URL.
        let trimmed = raw.trim_matches(|character: char| character == ' ' || character <= '\u{1f}');
        let parsed = url::Url::parse(trimmed).map_err(|source| ResourceError::MalformedWebUrl {
            url: raw.clone(),
            source: WebUrlSyntaxError { source },
        })?;
        let has_explicit_authority = trimmed
            .get(parsed.scheme().len()..)
            .is_some_and(|rest| rest.starts_with("://"));
        match parsed.scheme() {
            "http" | "https" if has_explicit_authority && parsed.has_host() => {
                Ok(Self(parsed.into()))
            }
            "http" | "https" => Err(ResourceError::MissingWebUrlAuthority { url: raw }),
            scheme => Err(ResourceError::UnsupportedWebUrlScheme {
                url: raw,
                scheme: scheme.to_string(),
            }),
        }
    }

    /// Get the normalized URL as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for WebUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Opaque causal detail for malformed Web URL syntax.
///
/// The URL parser's concrete error remains private so third-party parser types
/// do not leak through the domain interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("malformed URL syntax")]
pub struct WebUrlSyntaxError {
    #[source]
    source: url::ParseError,
}

/// A normalized, workspace-relative path.
///
/// Construction applies purely lexical normalization (no filesystem access,
/// so the target need not exist): `.` and empty components are dropped,
/// in-bounds `..` components are resolved, and the result is stored in
/// canonical relative form. Absolute paths (including Windows drive-qualified
/// forms such as `C:...`), paths that escape the workspace root through
/// parent traversal, empty values, and terminal-unsafe control characters
/// are rejected. `/` is the only accepted separator: backslashes are
/// rejected rather than reinterpreted, because `\` is a separator on Windows
/// but an ordinary filename character on POSIX, and a portable
/// workspace-relative path must mean the same target on both. Tab is
/// permitted, matching the shared [`find_control_char`] convention used by
/// labels and identifiers.
///
/// Comparison (including duplicate detection) is byte-wise: no Unicode
/// normalization is applied, so NFC and NFD spellings of the same visual
/// name are distinct targets (tracked in rivets-yuom).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct WorkspacePath(String);

impl WorkspacePath {
    /// Parse and normalize a workspace-relative path.
    ///
    /// # Errors
    ///
    /// Returns [`ResourceError::EmptyPath`] when the value is empty or
    /// whitespace-only, [`ResourceError::PathControlCharacter`] when it
    /// contains a character unsafe for terminal output,
    /// [`ResourceError::WorkspacePathBackslash`] when it contains `\`,
    /// [`ResourceError::AbsoluteWorkspacePath`] when it starts with a path
    /// separator or a Windows drive prefix,
    /// [`ResourceError::WorkspacePathEscape`] when parent
    /// traversal leaves the workspace root, or
    /// [`ResourceError::EmptyNormalizedWorkspacePath`] when normalization
    /// leaves nothing (e.g. `a/..` or `.`).
    pub fn new(raw: impl Into<String>) -> Result<Self, ResourceError> {
        let raw = raw.into();
        if raw.trim().is_empty() {
            return Err(ResourceError::EmptyPath);
        }
        if let Some(position) = find_control_char(&raw) {
            return Err(ResourceError::PathControlCharacter { position });
        }
        if let Some(position) = raw.chars().position(|c| c == '\\') {
            return Err(ResourceError::WorkspacePathBackslash { position });
        }
        if raw.starts_with('/') || starts_with_drive_prefix(&raw) {
            return Err(ResourceError::AbsoluteWorkspacePath { path: raw });
        }
        let mut stack: Vec<&str> = Vec::new();
        for component in raw.split('/') {
            match component {
                "" | "." => {}
                ".." => {
                    if stack.pop().is_none() {
                        return Err(ResourceError::WorkspacePathEscape { path: raw });
                    }
                }
                component => stack.push(component),
            }
        }
        let normalized = stack.join("/");
        if normalized.is_empty() {
            return Err(ResourceError::EmptyNormalizedWorkspacePath { path: raw });
        }
        Ok(Self(normalized))
    }

    /// Get the normalized path as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for WorkspacePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A `C:`-style drive prefix anchors the path outside any workspace on
/// Windows even without a separator (`C:notes.txt` is drive-relative there).
fn starts_with_drive_prefix(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

/// The location of an Associated Resource.
///
/// A Web URL is absolute; a Workspace Path is normalized relative to its
/// Workspace root and cannot escape that boundary. See ADR-0003 and
/// `CONTEXT.md`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResourceTarget {
    /// An absolute HTTP or HTTPS URL.
    Web {
        /// The validated URL.
        url: WebUrl,
    },
    /// A normalized path relative to the Workspace root.
    Path {
        /// The validated, normalized path.
        path: WorkspacePath,
    },
}

impl ResourceTarget {
    /// Construct a Web URL target from a validated [`WebUrl`].
    pub fn web(url: WebUrl) -> Self {
        Self::Web { url }
    }

    /// Construct a Workspace Path target from a validated [`WorkspacePath`].
    pub fn path(path: WorkspacePath) -> Self {
        Self::Path { path }
    }
}

impl fmt::Display for ResourceTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Web { url } => write!(f, "{url}"),
            Self::Path { path } => write!(f, "{path}"),
        }
    }
}

/// The reason an Associated Resource matters to its Issue.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, ValueEnum,
)]
#[serde(rename_all = "snake_case")]
pub enum ResourceRole {
    /// Delivers work for the Issue (e.g., an implementation PR).
    Implementation,
    /// Explains the Issue or its context.
    Documentation,
    /// Supports a finding or decision recorded on the Issue.
    Evidence,
    /// Identifies where the Issue continues after migration.
    Successor,
    /// Generic external context; the fallback role.
    Reference,
}

impl fmt::Display for ResourceRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Implementation => write!(f, "implementation"),
            Self::Documentation => write!(f, "documentation"),
            Self::Evidence => write!(f, "evidence"),
            Self::Successor => write!(f, "successor"),
            Self::Reference => write!(f, "reference"),
        }
    }
}

impl ResourceRole {
    /// Comma-separated canonical role names, for error messages.
    ///
    /// Derived from the enum declaration rather than hand-written, so the
    /// listed values cannot drift from the accepted vocabulary.
    #[must_use]
    pub fn valid_values() -> &'static str {
        static VALUES: OnceLock<String> = OnceLock::new();
        VALUES.get_or_init(join_canonical_names::<Self>)
    }
}

impl FromStr for ResourceRole {
    type Err = ResourceError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "implementation" => Ok(Self::Implementation),
            "documentation" => Ok(Self::Documentation),
            "evidence" => Ok(Self::Evidence),
            "successor" => Ok(Self::Successor),
            "reference" => Ok(Self::Reference),
            _ => Err(ResourceError::UnknownRole {
                role: s.to_string(),
            }),
        }
    }
}

/// A validated human-readable label for an Associated Resource.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ResourceLabel(String);

impl ResourceLabel {
    /// Parse a non-empty, single-line label.
    ///
    /// # Errors
    ///
    /// Returns [`ResourceError::EmptyLabel`] when the value is empty after
    /// trimming, or [`ResourceError::LabelControlCharacter`] when it contains
    /// a character unsafe for terminal output.
    pub fn new(label: impl Into<String>) -> Result<Self, ResourceError> {
        let label = label.into();
        if label.trim().is_empty() {
            return Err(ResourceError::EmptyLabel);
        }
        if let Some(position) = find_control_char(&label) {
            return Err(ResourceError::LabelControlCharacter { position });
        }
        Ok(Self(label))
    }

    /// Get the label as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ResourceLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A typed reference from an Issue to relevant information or an artifact.
///
/// Constructed only through validated paths (`Issue::add_resource` or the
/// persistence boundary), so every instance carries a valid target, role,
/// and label.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AssociatedResource {
    id: ResourceId,
    target: ResourceTarget,
    role: ResourceRole,
    label: Option<ResourceLabel>,
}

impl AssociatedResource {
    pub(crate) fn from_parts(
        id: ResourceId,
        target: ResourceTarget,
        role: ResourceRole,
        label: Option<ResourceLabel>,
    ) -> Self {
        Self {
            id,
            target,
            role,
            label,
        }
    }

    /// The stable, opaque identifier of this resource within its Issue.
    pub fn id(&self) -> &ResourceId {
        &self.id
    }

    /// The location this resource points to.
    pub fn target(&self) -> &ResourceTarget {
        &self.target
    }

    /// Why this resource matters to its Issue.
    pub fn role(&self) -> ResourceRole {
        self.role
    }

    /// The optional human-readable label.
    pub fn label(&self) -> Option<&ResourceLabel> {
        self.label.as_ref()
    }
}

impl fmt::Display for AssociatedResource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {} ({})", self.id, self.target, self.role)?;
        if let Some(label) = &self.label {
            write!(f, " — {label}")?;
        }
        Ok(())
    }
}

/// Data for associating a new resource with an Issue.
///
/// The resource identifier is assigned by the domain when the resource is
/// added; callers supply only target, role, and optional label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewResource {
    /// Where the resource points.
    pub target: ResourceTarget,
    /// Why the resource matters.
    pub role: ResourceRole,
    /// Optional human-readable label.
    pub label: Option<ResourceLabel>,
}

/// Data for updating an existing Associated Resource, keyed by its stable
/// identifier.
///
/// Every `None` field leaves that property unchanged, so an update never
/// shifts the resource's position or reissues its identifier. The label uses
/// the double-Option pattern (same as `IssueUpdate::assignee`): `None` keeps
/// the current label, `Some(None)` clears it, and `Some(Some(label))` sets it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResourceUpdate {
    /// New target (if updating).
    pub target: Option<ResourceTarget>,
    /// New role (if updating).
    pub role: Option<ResourceRole>,
    /// New label (if updating); `Some(None)` clears the label.
    pub label: Option<Option<ResourceLabel>>,
}

/// A failure to construct or add an Associated Resource.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ResourceError {
    /// The target's URL syntax was malformed.
    #[error("Invalid web URL '{url}': {source}")]
    MalformedWebUrl {
        /// The rejected value.
        url: String,
        /// Opaque parser failure.
        #[source]
        source: WebUrlSyntaxError,
    },
    /// An HTTP(S) target omitted its explicit authority or host.
    #[error("Invalid web URL '{url}': URL must include an explicit host after '//'")]
    MissingWebUrlAuthority {
        /// The rejected value.
        url: String,
    },
    /// The parsed URL used a non-HTTP(S) scheme.
    #[error("Invalid web URL '{url}': unsupported scheme '{scheme}'")]
    UnsupportedWebUrlScheme {
        /// The rejected value.
        url: String,
        /// The rejected scheme.
        scheme: String,
    },
    /// The role was not one of the canonical Resource Roles.
    #[error(
        "Unknown resource role '{role}' (valid: implementation, documentation, evidence, successor, reference)"
    )]
    UnknownRole {
        /// The rejected value.
        role: String,
    },
    /// The label was empty or whitespace-only.
    #[error("Resource label cannot be empty")]
    EmptyLabel,
    /// The label included an unsafe control character.
    #[error("Resource label contains invalid control character at position {position}")]
    LabelControlCharacter {
        /// Character offset of the invalid value.
        position: usize,
    },
    /// The path was empty or whitespace-only.
    #[error("Workspace path cannot be empty")]
    EmptyPath,
    /// The path included a terminal-unsafe control character.
    #[error("Workspace path contains invalid control character at position {position}")]
    PathControlCharacter {
        /// Character offset of the invalid value.
        position: usize,
    },
    /// The path used a backslash, which is not portable across platforms.
    #[error("Workspace path contains '\\' at position {position}; use '/' as the separator")]
    WorkspacePathBackslash {
        /// Character offset of the backslash.
        position: usize,
    },
    /// The path was absolute instead of workspace-relative.
    #[error("Workspace path '{path}' must be relative to the workspace root")]
    AbsoluteWorkspacePath {
        /// The rejected value.
        path: String,
    },
    /// Parent traversal left the workspace root.
    #[error("Workspace path '{path}' escapes the workspace root")]
    WorkspacePathEscape {
        /// The rejected value.
        path: String,
    },
    /// Normalization left the path empty (e.g. `a/..` or `.`).
    #[error("Workspace path '{path}' does not refer to anything under the workspace root")]
    EmptyNormalizedWorkspacePath {
        /// The rejected value.
        path: String,
    },
    /// An identical target-and-role association already exists on the Issue.
    #[error("Resource with target '{target}' and role '{role}' already exists on this issue")]
    DuplicateTargetRole {
        /// The duplicated target.
        target: ResourceTarget,
        /// The duplicated role.
        role: ResourceRole,
    },
    /// A persisted resource identifier contained a terminal-unsafe character.
    #[error("Resource identifier contains invalid control character at position {position}")]
    ResourceIdControlCharacter {
        /// Character offset of the invalid value.
        position: usize,
    },
    /// Two persisted resources had the same stable identifier.
    #[error("Resource identifier '{id}' appears more than once on this issue")]
    DuplicateResourceId {
        /// The duplicated identifier.
        id: ResourceId,
    },
    /// The per-Issue resource identifier sequence reached its maximum.
    #[error("Resource identifier sequence exhausted for this issue")]
    IdSequenceExhausted,
    /// A persisted resource identifier was empty.
    #[error("Resource identifier cannot be empty")]
    EmptyResourceId,
    /// No field was provided to update.
    #[error("Resource update requires at least one field")]
    EmptyUpdate,
    /// The referenced resource does not exist on this Issue.
    #[error("Resource not found: {id}")]
    ResourceNotFound {
        /// The unknown identifier.
        id: ResourceId,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    // ===== WorkspacePath =====

    #[rstest]
    #[case::plain("docs/adr/0003.md", "docs/adr/0003.md")]
    #[case::single_component("src", "src")]
    #[case::drop_dot("a/./b", "a/b")]
    #[case::collapse_slashes("a//b", "a/b")]
    #[case::leading_dot("./x", "x")]
    #[case::trailing_dot("x/.", "x")]
    #[case::trailing_slash("src/", "src")]
    #[case::in_bounds_traversal("docs/../src/lib.rs", "src/lib.rs")]
    #[case::in_bounds_multi("a/b/../../c", "c")]
    #[case::dot_then_parent("x/./../y", "y")]
    #[case::unicode("é/文件.md", "é/文件.md")]
    #[case::embedded_space("with space/y", "with space/y")]
    #[case::hidden(".hidden", ".hidden")]
    #[case::tab_allowed("un\tdir/x", "un\tdir/x")]
    fn workspace_path_accepts_and_normalizes(#[case] input: &str, #[case] expected: &str) {
        let path = WorkspacePath::new(input).expect("valid workspace path");
        assert_eq!(path.as_str(), expected);
        assert_eq!(path.to_string(), expected);
    }

    #[rstest]
    #[case::empty("")]
    #[case::whitespace("   ")]
    #[case::tab("\t")]
    #[case::absolute("/etc/passwd")]
    #[case::double_absolute("//x")]
    #[case::escape_first("../escape.md")]
    #[case::escape_deep("a/../../b")]
    #[case::escape_nested("a/b/../../../c")]
    #[case::normalizes_to_root("a/..")]
    #[case::dot(".")]
    #[case::dot_slash("./")]
    #[case::control_escape("\u{1b}x")]
    #[case::backslash_traversal(r"..\..\secrets.txt")]
    #[case::backslash_separator(r"docs\readme.md")]
    #[case::backslash_rooted(r"\etc\passwd")]
    #[case::unc(r"\\server\share")]
    #[case::drive_backslash(r"C:\Windows")]
    #[case::drive_forward_slash("C:/Windows/system32")]
    #[case::drive_relative("C:relative.txt")]
    fn workspace_path_rejects(#[case] input: &str) {
        assert!(
            WorkspacePath::new(input).is_err(),
            "{input:?} should be rejected"
        );
    }

    #[test]
    fn workspace_path_matches_recorded_realpath_corpus() {
        let corpus = super::super::workspace_path_corpus::CORPUS;
        assert!(
            corpus.len() >= 400,
            "corpus should be substantial, got {}",
            corpus.len()
        );
        for (input, expected) in corpus {
            match WorkspacePath::new(*input) {
                Ok(path) => {
                    assert_eq!(
                        Some(path.as_str()),
                        *expected,
                        "normalization mismatch for {input:?}"
                    );
                }
                Err(_) => assert_eq!(
                    None, *expected,
                    "unexpected rejection for {input:?} (oracle accepts)"
                ),
            }
        }
    }

    #[test]
    fn workspace_path_backslash_reports_char_position() {
        assert_eq!(
            WorkspacePath::new(r"docs\readme.md"),
            Err(ResourceError::WorkspacePathBackslash { position: 4 })
        );
        // Char position, not byte offset: 'é' is one char but two bytes.
        assert_eq!(
            WorkspacePath::new("é\\x"),
            Err(ResourceError::WorkspacePathBackslash { position: 1 })
        );
    }

    #[test]
    fn workspace_path_drive_prefix_is_absolute() {
        for input in [
            "C:/Windows/system32",
            "C:relative.txt",
            "c:x",
            "a:notes.txt",
        ] {
            assert!(
                matches!(
                    WorkspacePath::new(input),
                    Err(ResourceError::AbsoluteWorkspacePath { .. })
                ),
                "{input:?} must be rejected as absolute"
            );
        }
    }

    #[test]
    fn workspace_path_drive_rule_is_prefix_only() {
        // Only a single-ASCII-letter-plus-colon prefix is drive-like; longer
        // first components and interior colons stay legal POSIX names.
        for input in ["ab:notes.txt", "dir:/file.rs", "src/C:/nested"] {
            assert!(
                WorkspacePath::new(input).is_ok(),
                "{input:?} must be accepted; colon rule is drive-prefix only"
            );
        }
    }

    #[test]
    fn workspace_path_accepts_nonexistent_targets() {
        // Purely lexical: branch-local and generated files are legal targets.
        let path = WorkspacePath::new("target/generated/does-not-exist/report.html")
            .expect("nonexistent target should be accepted");
        assert_eq!(path.as_str(), "target/generated/does-not-exist/report.html");
    }

    #[test]
    fn workspace_path_rejects_policy_cases() {
        for input in super::super::workspace_path_corpus::POLICY_REJECT {
            assert!(
                WorkspacePath::new(*input).is_err(),
                "policy rejection missing for {input:?}"
            );
        }
    }

    #[test]
    fn workspace_path_errors_are_typed_per_cause() {
        assert!(matches!(
            WorkspacePath::new("/x"),
            Err(ResourceError::AbsoluteWorkspacePath { .. })
        ));
        assert!(matches!(
            WorkspacePath::new("../x"),
            Err(ResourceError::WorkspacePathEscape { .. })
        ));
        assert!(matches!(
            WorkspacePath::new("a/.."),
            Err(ResourceError::EmptyNormalizedWorkspacePath { .. })
        ));
        assert!(matches!(
            WorkspacePath::new(""),
            Err(ResourceError::EmptyPath)
        ));
        assert!(matches!(
            WorkspacePath::new("x\u{1b}y"),
            Err(ResourceError::PathControlCharacter { .. })
        ));
    }

    // ===== WebUrl =====

    #[rstest]
    #[case::leading_space(" https://example.com/docs")]
    #[case::leading_control("\u{1f}https://example.com/docs")]
    #[case::http("http://example.com/docs")]
    #[case::https("https://example.com/docs")]
    #[case::https_with_port_and_query("https://example.com:8443/a?b=c#frag")]
    #[case::uppercase_scheme("HTTPS://example.com/x")]
    fn web_url_accepts_absolute_http_urls(#[case] input: &str) {
        assert!(WebUrl::new(input).is_ok());
    }

    #[rstest]
    #[case::no_scheme("example.com/docs")]
    #[case::relative_path("docs/adr/0003.md")]
    #[case::scheme_relative("//example.com/docs")]
    #[case::ftp("ftp://example.com/file")]
    #[case::file("file:///etc/passwd")]
    #[case::special_scheme_without_authority("https:relative")]
    #[case::mailto("mailto:a@example.com")]
    #[case::empty_scheme_host("http://")]
    #[case::whitespace("https://exa mple.com")]
    #[case::empty("")]
    fn web_url_rejects_non_absolute_or_non_http_values(#[case] input: &str) {
        assert!(WebUrl::new(input).is_err());
    }

    #[test]
    fn malformed_web_url_preserves_opaque_causal_error() {
        let error = WebUrl::new("example.com/docs").expect_err("relative URL should fail");
        match error {
            ResourceError::MalformedWebUrl { source, .. } => {
                assert!(std::error::Error::source(&source).is_some());
            }
            error => panic!("expected malformed URL error, got {error:?}"),
        }
    }

    #[test]
    fn unsupported_web_url_scheme_is_a_domain_error() {
        assert!(matches!(
            WebUrl::new("ftp://example.com/file"),
            Err(ResourceError::UnsupportedWebUrlScheme { .. })
        ));
    }

    #[test]
    fn web_url_normalizes_for_equality() {
        let a = WebUrl::new("HTTPS://EXAMPLE.com").expect("valid URL");
        let b = WebUrl::new("https://example.com/").expect("valid URL");
        assert_eq!(a, b);
    }

    // ===== ResourceRole =====

    #[rstest]
    #[case::implementation("implementation", ResourceRole::Implementation)]
    #[case::documentation("documentation", ResourceRole::Documentation)]
    #[case::evidence("evidence", ResourceRole::Evidence)]
    #[case::successor("successor", ResourceRole::Successor)]
    #[case::reference("reference", ResourceRole::Reference)]
    fn resource_role_parses_canonical_names(#[case] input: &str, #[case] expected: ResourceRole) {
        assert_eq!(input.parse::<ResourceRole>().unwrap(), expected);
        assert_eq!(expected.to_string(), input.to_lowercase());
    }

    #[test]
    fn resource_role_rejects_noncanonical_case() {
        assert!(matches!(
            "Evidence".parse::<ResourceRole>(),
            Err(ResourceError::UnknownRole { .. })
        ));
    }

    #[test]
    fn resource_role_rejects_unknown_names() {
        assert!(matches!(
            "provider".parse::<ResourceRole>(),
            Err(ResourceError::UnknownRole { .. })
        ));
    }

    #[test]
    fn resource_role_cli_value_name_matches_display() {
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
    }

    #[test]
    fn resource_role_serde_matches_display() {
        // Serde is the wire form of the same vocabulary: every variant's
        // JSON string must equal its Display string in both directions.
        for role in [
            ResourceRole::Implementation,
            ResourceRole::Documentation,
            ResourceRole::Evidence,
            ResourceRole::Successor,
            ResourceRole::Reference,
        ] {
            let json = serde_json::to_string(&role).expect("role serializes");
            assert_eq!(json, format!("\"{role}\""));
            let parsed: ResourceRole = serde_json::from_str(&json).expect("role deserializes");
            assert_eq!(parsed, role);
        }
    }

    #[test]
    fn resource_role_valid_values_lists_every_canonical_name() {
        // Pins the derived error-message list to the shipped wording.
        assert_eq!(
            ResourceRole::valid_values(),
            "implementation, documentation, evidence, successor, reference"
        );
    }

    // ===== ResourceLabel =====

    #[test]
    fn label_rejects_empty_and_whitespace() {
        assert_eq!(ResourceLabel::new(""), Err(ResourceError::EmptyLabel));
        assert_eq!(ResourceLabel::new(" \t "), Err(ResourceError::EmptyLabel));
    }

    #[test]
    fn label_rejects_control_characters() {
        assert!(matches!(
            ResourceLabel::new("bad\u{1b}label"),
            Err(ResourceError::LabelControlCharacter { .. })
        ));
    }

    #[test]
    fn label_accepts_non_empty_text() {
        let label = ResourceLabel::new("Implementation PR").expect("valid label");
        assert_eq!(label.as_str(), "Implementation PR");
    }

    // ===== ResourceId / AssociatedResource =====

    #[rstest]
    #[case::escape("r1\u{1b}", 2)]
    #[case::tab("r1\tspoof", 2)]
    fn resource_id_rejects_terminal_unsafe_control_characters(
        #[case] id: &str,
        #[case] position: usize,
    ) {
        assert!(matches!(
            ResourceId::new(id),
            Err(ResourceError::ResourceIdControlCharacter {
                position: actual
            }) if actual == position
        ));
    }

    #[test]
    fn associated_resource_display_includes_optional_label() {
        let resource = AssociatedResource::from_parts(
            ResourceId::new("r1").expect("valid resource ID"),
            ResourceTarget::web(WebUrl::new("https://example.com/pr/1").expect("valid URL")),
            ResourceRole::Implementation,
            Some(ResourceLabel::new("Implementation PR").expect("valid label")),
        );
        assert_eq!(
            resource.to_string(),
            "[r1] https://example.com/pr/1 (implementation) — Implementation PR"
        );
    }
}
