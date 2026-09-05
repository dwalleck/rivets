//! Typed Issue Relationship values.

use super::{IssueId, IssueKind};
use serde::{Deserialize, Serialize};

/// A directed relationship from an Issue that depends on work to its prerequisite.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "RawBlockingDependency")]
pub struct BlockingDependency {
    dependent_id: IssueId,
    prerequisite_id: IssueId,
}

#[derive(Deserialize)]
struct RawBlockingDependency {
    dependent_id: IssueId,
    prerequisite_id: IssueId,
}

impl TryFrom<RawBlockingDependency> for BlockingDependency {
    type Error = BlockingDependencyError;

    fn try_from(raw: RawBlockingDependency) -> Result<Self, Self::Error> {
        Self::new(raw.dependent_id, raw.prerequisite_id)
    }
}

impl BlockingDependency {
    /// Constructs a Blocking Dependency with explicit endpoint roles.
    ///
    /// # Errors
    ///
    /// Returns [`BlockingDependencyError::SelfReference`] when both roles name
    /// the same Issue.
    pub fn new(
        dependent_id: IssueId,
        prerequisite_id: IssueId,
    ) -> Result<Self, BlockingDependencyError> {
        if dependent_id == prerequisite_id {
            return Err(BlockingDependencyError::SelfReference {
                issue_id: dependent_id,
            });
        }

        Ok(Self {
            dependent_id,
            prerequisite_id,
        })
    }
    pub(crate) fn from_valid_parts(dependent_id: IssueId, prerequisite_id: IssueId) -> Self {
        debug_assert_ne!(dependent_id, prerequisite_id);
        Self {
            dependent_id,
            prerequisite_id,
        }
    }

    /// Returns the Issue that depends on the prerequisite.
    #[must_use]
    pub const fn dependent_id(&self) -> &IssueId {
        &self.dependent_id
    }

    /// Returns the Issue that must be completed first.
    #[must_use]
    pub const fn prerequisite_id(&self) -> &IssueId {
        &self.prerequisite_id
    }
}

/// A rejected Blocking Dependency value.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BlockingDependencyError {
    /// A Blocking Dependency cannot point from an Issue to itself.
    #[error("Issue {issue_id} cannot depend on itself")]
    SelfReference {
        /// The Issue that cannot depend on itself.
        issue_id: IssueId,
    },
}

/// A symmetric, non-blocking association between two Issues.
///
/// Endpoints are stored in lexical order so either caller order constructs
/// and serializes the same value.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct RelatedAssociation {
    left_issue_id: IssueId,
    right_issue_id: IssueId,
}

impl<'de> Deserialize<'de> for RelatedAssociation {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Wire {
            left_issue_id: IssueId,
            right_issue_id: IssueId,
        }

        let wire = Wire::deserialize(deserializer)?;
        Self::new(wire.left_issue_id, wire.right_issue_id).map_err(serde::de::Error::custom)
    }
}

impl RelatedAssociation {
    /// Constructs a Related Association with deterministic endpoint ordering.
    ///
    /// # Errors
    ///
    /// Returns [`RelatedAssociationError::SelfReference`] when both endpoints
    /// name the same Issue.
    pub fn new(
        issue_id: IssueId,
        related_issue_id: IssueId,
    ) -> Result<Self, RelatedAssociationError> {
        match issue_id.cmp(&related_issue_id) {
            std::cmp::Ordering::Less => Ok(Self {
                left_issue_id: issue_id,
                right_issue_id: related_issue_id,
            }),
            std::cmp::Ordering::Greater => Ok(Self {
                left_issue_id: related_issue_id,
                right_issue_id: issue_id,
            }),
            std::cmp::Ordering::Equal => Err(RelatedAssociationError::SelfReference { issue_id }),
        }
    }

    /// Returns the lexically first endpoint.
    #[must_use]
    pub const fn left_issue_id(&self) -> &IssueId {
        &self.left_issue_id
    }

    /// Returns the lexically second endpoint.
    #[must_use]
    pub const fn right_issue_id(&self) -> &IssueId {
        &self.right_issue_id
    }
}

/// A rejected Related Association value.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RelatedAssociationError {
    /// An Issue cannot be related to itself.
    #[error("Issue {issue_id} cannot be related to itself")]
    SelfReference { issue_id: IssueId },
}

/// Directed provenance from a discovered Issue to the source Issue whose work
/// surfaced it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct DiscoveryOrigin {
    discovered_issue_id: IssueId,
    source_issue_id: IssueId,
}

impl<'de> Deserialize<'de> for DiscoveryOrigin {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Wire {
            discovered_issue_id: IssueId,
            source_issue_id: IssueId,
        }

        let wire = Wire::deserialize(deserializer)?;
        Self::new(wire.discovered_issue_id, wire.source_issue_id).map_err(serde::de::Error::custom)
    }
}

impl DiscoveryOrigin {
    /// Constructs a Discovery Origin with explicit endpoint roles.
    ///
    /// # Errors
    ///
    /// Returns [`DiscoveryOriginError::SelfReference`] when both roles name
    /// the same Issue.
    pub fn new(
        discovered_issue_id: IssueId,
        source_issue_id: IssueId,
    ) -> Result<Self, DiscoveryOriginError> {
        if discovered_issue_id == source_issue_id {
            return Err(DiscoveryOriginError::SelfReference {
                issue_id: discovered_issue_id,
            });
        }

        Ok(Self {
            discovered_issue_id,
            source_issue_id,
        })
    }

    /// Returns the Issue that was discovered.
    #[must_use]
    pub const fn discovered_issue_id(&self) -> &IssueId {
        &self.discovered_issue_id
    }

    /// Returns the Issue whose work surfaced the discovered Issue.
    #[must_use]
    pub const fn source_issue_id(&self) -> &IssueId {
        &self.source_issue_id
    }
}

/// A rejected Discovery Origin value.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DiscoveryOriginError {
    /// An Issue cannot record itself as its own Discovery source.
    #[error("Issue {issue_id} cannot be its own Discovery source")]
    SelfReference { issue_id: IssueId },
}

/// A directed ownership relationship from a child Issue to one Epic parent.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Parentage {
    child_id: IssueId,
    parent_id: IssueId,
}

impl<'de> Deserialize<'de> for Parentage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Wire {
            child_id: IssueId,
            parent_id: IssueId,
        }

        let wire = Wire::deserialize(deserializer)?;
        Self::new(wire.child_id, wire.parent_id).map_err(serde::de::Error::custom)
    }
}

impl Parentage {
    /// Constructs Parentage with explicit child and parent roles.
    ///
    /// # Errors
    ///
    /// Returns [`ParentageError::SelfReference`] when both roles name the same
    /// Issue. Workspace-level parent Kind, cardinality, cycle, and lifecycle
    /// invariants are enforced by the storage interface.
    pub fn new(child_id: IssueId, parent_id: IssueId) -> Result<Self, ParentageError> {
        if child_id == parent_id {
            return Err(ParentageError::SelfReference { issue_id: child_id });
        }

        Ok(Self {
            child_id,
            parent_id,
        })
    }

    pub(crate) fn from_valid_parts(child_id: IssueId, parent_id: IssueId) -> Self {
        debug_assert_ne!(child_id, parent_id);
        Self {
            child_id,
            parent_id,
        }
    }

    /// Returns the Issue owned by the parent Epic.
    #[must_use]
    pub const fn child_id(&self) -> &IssueId {
        &self.child_id
    }

    /// Returns the Epic that owns the child.
    #[must_use]
    pub const fn parent_id(&self) -> &IssueId {
        &self.parent_id
    }
}

/// A rejected Parentage value or Workspace Parentage transition.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParentageError {
    /// An Issue cannot own itself.
    #[error("Issue {issue_id} cannot be its own parent")]
    SelfReference {
        /// The Issue occupying both endpoint roles.
        issue_id: IssueId,
    },

    /// Only an Epic may own child Issues.
    #[error("Issue {parent_id} cannot be a parent because its kind is {actual_kind}, not epic")]
    ParentNotEpic {
        /// The rejected candidate parent.
        parent_id: IssueId,
        /// The candidate parent's current Kind.
        actual_kind: IssueKind,
    },

    /// A set operation tried to assign a second parent.
    #[error("Issue {child_id} already has parent {parent_id}; use parent move to change it")]
    AlreadyParented {
        /// The child that already has a parent.
        child_id: IssueId,
        /// The child's current parent.
        parent_id: IssueId,
    },

    /// An operation requires Parentage that is absent.
    #[error("Issue {child_id} has no parent")]
    NoParent {
        /// The unparented child.
        child_id: IssueId,
    },

    /// A proposed edge would make Parentage cyclic.
    #[error(
        "Parentage cycle detected: setting parent of {child_id} to {parent_id} would create a cycle"
    )]
    Cycle {
        /// The proposed child.
        child_id: IssueId,
        /// The proposed parent.
        parent_id: IssueId,
    },

    /// A non-Closed child cannot be owned by a Closed Epic.
    #[error("Cannot attach non-Closed child {child_id} to Closed Epic {parent_id}")]
    ClosedParent {
        /// The non-Closed child.
        child_id: IssueId,
        /// The Closed candidate parent.
        parent_id: IssueId,
    },

    /// An Epic cannot close while direct children remain non-Closed.
    #[error("Cannot close Epic {epic_id}: non-Closed direct children: {child_ids:?}")]
    ActiveChildren {
        /// The Epic whose close was rejected.
        epic_id: IssueId,
        /// Deterministically ordered non-Closed direct children.
        child_ids: Vec<IssueId>,
    },

    /// An Epic with children cannot become a non-Epic Kind.
    #[error(
        "Cannot change Epic {parent_id} to a non-Epic kind while it has children: {child_ids:?}"
    )]
    ParentHasChildren {
        /// The owning Epic whose reclassification was rejected.
        parent_id: IssueId,
        /// Deterministically ordered direct children.
        child_ids: Vec<IssueId>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocking_dependency_preserves_direction_and_rejects_self() {
        let dependency = BlockingDependency::new(
            IssueId::new("test-dependent"),
            IssueId::new("test-prerequisite"),
        )
        .expect("distinct endpoint roles should be valid");

        assert_eq!(dependency.dependent_id().as_str(), "test-dependent");
        assert_eq!(dependency.prerequisite_id().as_str(), "test-prerequisite");
        assert_eq!(
            serde_json::to_value(&dependency).expect("relationship should serialize"),
            serde_json::json!({
                "dependent_id": "test-dependent",
                "prerequisite_id": "test-prerequisite"
            })
        );

        assert_eq!(
            BlockingDependency::new(IssueId::new("test-a"), IssueId::new("test-a")),
            Err(BlockingDependencyError::SelfReference {
                issue_id: IssueId::new("test-a")
            })
        );

        let deserialization_error =
            serde_json::from_value::<BlockingDependency>(serde_json::json!({
                "dependent_id": "test-self",
                "prerequisite_id": "test-self"
            }))
            .expect_err("deserialization must enforce the self-reference invariant");
        assert!(
            deserialization_error
                .to_string()
                .contains("cannot depend on itself")
        );
    }

    #[test]
    fn relationship_values_preserve_semantics_and_reject_self() {
        let issue_a = IssueId::new("test-a");
        let issue_b = IssueId::new("test-b");

        let forward = RelatedAssociation::new(issue_a.clone(), issue_b.clone())
            .expect("distinct Related endpoints should be valid");
        let reverse = RelatedAssociation::new(issue_b.clone(), issue_a.clone())
            .expect("Related endpoint order should not matter");
        assert_eq!(forward, reverse);
        assert_eq!(forward.left_issue_id(), &issue_a);
        assert_eq!(forward.right_issue_id(), &issue_b);
        assert_eq!(
            serde_json::to_value(&forward).expect("Related should serialize"),
            serde_json::json!({
                "left_issue_id": "test-a",
                "right_issue_id": "test-b"
            })
        );
        assert_eq!(
            RelatedAssociation::new(issue_a.clone(), issue_a.clone()),
            Err(RelatedAssociationError::SelfReference {
                issue_id: issue_a.clone()
            })
        );

        let origin = DiscoveryOrigin::new(issue_a.clone(), issue_b.clone())
            .expect("distinct Discovery endpoints should be valid");
        assert_eq!(origin.discovered_issue_id(), &issue_a);
        assert_eq!(origin.source_issue_id(), &issue_b);
        assert_eq!(
            serde_json::to_value(&origin).expect("Discovery should serialize"),
            serde_json::json!({
                "discovered_issue_id": "test-a",
                "source_issue_id": "test-b"
            })
        );
        assert_eq!(
            DiscoveryOrigin::new(issue_a.clone(), issue_a.clone()),
            Err(DiscoveryOriginError::SelfReference { issue_id: issue_a })
        );
    }

    #[test]
    fn relationship_deserialization_enforces_self_reference_invariants() {
        let related = serde_json::from_value::<RelatedAssociation>(serde_json::json!({
            "left_issue_id": "test-a",
            "right_issue_id": "test-a"
        }));
        assert!(
            related.is_err(),
            "Related self-reference must not deserialize"
        );

        let discovery = serde_json::from_value::<DiscoveryOrigin>(serde_json::json!({
            "discovered_issue_id": "test-a",
            "source_issue_id": "test-a"
        }));
        assert!(
            discovery.is_err(),
            "Discovery self-reference must not deserialize"
        );
    }

    #[test]
    fn blocking_dependency_deserialization_enforces_invariant() {
        let dependency: BlockingDependency = serde_json::from_value(serde_json::json!({
            "dependent_id": "test-dependent",
            "prerequisite_id": "test-prerequisite"
        }))
        .expect("distinct endpoint roles should deserialize");
        assert_eq!(dependency.dependent_id().as_str(), "test-dependent");
        assert_eq!(dependency.prerequisite_id().as_str(), "test-prerequisite");

        let self_reference = serde_json::from_value::<BlockingDependency>(serde_json::json!({
            "dependent_id": "test-self",
            "prerequisite_id": "test-self"
        }));
        assert!(
            self_reference.is_err(),
            "deserialization must preserve the self-reference invariant"
        );
    }

    #[test]
    fn parentage_constructor_and_deserialization_reject_self_reference() {
        let parentage = Parentage::new(IssueId::new("test-child"), IssueId::new("test-parent"))
            .expect("distinct endpoint roles should be valid");

        assert_eq!(parentage.child_id().as_str(), "test-child");
        assert_eq!(parentage.parent_id().as_str(), "test-parent");
        assert_eq!(
            serde_json::to_value(&parentage).expect("Parentage should serialize"),
            serde_json::json!({
                "child_id": "test-child",
                "parent_id": "test-parent"
            })
        );
        assert_eq!(
            Parentage::new(IssueId::new("test-self"), IssueId::new("test-self")),
            Err(ParentageError::SelfReference {
                issue_id: IssueId::new("test-self")
            })
        );

        let deserialization_error = serde_json::from_value::<Parentage>(serde_json::json!({
            "child_id": "test-self",
            "parent_id": "test-self"
        }))
        .expect_err("deserialization must enforce the self-reference invariant");
        assert!(
            deserialization_error
                .to_string()
                .contains("cannot be its own parent")
        );
    }
}
