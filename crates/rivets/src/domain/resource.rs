//! Associated Resource domain types.
//!
//! An Associated Resource is a typed reference from an Issue to relevant
//! information or an artifact. Resources form a mutable curated index with no
//! effect on workflow or readiness. See ADR-0003 and `CONTEXT.md`.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

use super::find_control_char;

/// Opaque, stable identifier of an Associated Resource within its Issue.
///
/// Identifiers are assigned by the domain when a resource is added and are
/// never reused, so later update and removal operations can target a resource
/// without positional indices.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ResourceId(String);

impl ResourceId {
    pub(crate) fn new(id: impl Into<String>) -> Result<Self, ResourceError> {
        let id = id.into();
        if id.trim().is_empty() {
            return Err(ResourceError::EmptyResourceId);
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
    /// Returns [`ResourceError::InvalidWebUrl`] when the value is not a
    /// well-formed absolute URL with an `http` or `https` scheme.
    pub fn new(raw: impl Into<String>) -> Result<Self, ResourceError> {
        let raw = raw.into();
        let parsed = url::Url::parse(&raw).map_err(|error| ResourceError::InvalidWebUrl {
            url: raw.clone(),
            reason: error.to_string(),
        })?;
        let has_explicit_authority = raw
            .get(parsed.scheme().len()..)
            .is_some_and(|rest| rest.starts_with("://"));
        match parsed.scheme() {
            "http" | "https" if has_explicit_authority && parsed.has_host() => {
                Ok(Self(parsed.into()))
            }
            "http" | "https" => Err(ResourceError::InvalidWebUrl {
                url: raw,
                reason: "URL must include an explicit host after '//'".to_string(),
            }),
            _ => Err(ResourceError::InvalidWebUrl {
                url: raw,
                reason: "scheme must be http or https".to_string(),
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

/// The location of an Associated Resource.
///
/// Discriminated so that Web URLs and Workspace Paths travel through the core
/// as distinct, validated concepts rather than untyped strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResourceTarget {
    /// An absolute HTTP or HTTPS URL.
    Web {
        /// The validated URL.
        url: WebUrl,
    },
}

impl ResourceTarget {
    /// Construct a Web URL target from a validated [`WebUrl`].
    pub fn web(url: WebUrl) -> Self {
        Self::Web { url }
    }
}

impl fmt::Display for ResourceTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Web { url } => write!(f, "{url}"),
        }
    }
}

/// The reason an Associated Resource matters to its Issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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

/// A failure to construct or add an Associated Resource.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ResourceError {
    /// The target was not an absolute HTTP or HTTPS URL.
    #[error("Invalid web URL '{url}': {reason}")]
    InvalidWebUrl {
        /// The rejected value.
        url: String,
        /// Why parsing failed.
        reason: String,
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
    /// An identical target-and-role association already exists on the Issue.
    #[error("Resource with target '{target}' and role '{role}' already exists on this issue")]
    DuplicateTargetRole {
        /// The duplicated target.
        target: String,
        /// The duplicated role.
        role: ResourceRole,
    },
    /// The per-Issue resource identifier sequence reached its maximum.
    #[error("Resource identifier sequence exhausted for this issue")]
    IdSequenceExhausted,
    /// A persisted resource identifier was empty.
    #[error("Resource identifier cannot be empty")]
    EmptyResourceId,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    // ===== WebUrl =====

    #[rstest]
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
        assert!(matches!(
            WebUrl::new(input),
            Err(ResourceError::InvalidWebUrl { .. })
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
}
