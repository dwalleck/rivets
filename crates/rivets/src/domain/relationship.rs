//! Typed Issue Relationship values.

use super::IssueId;
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
}
