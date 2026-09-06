//! Validated updates to the mutable, descriptive fields of an [`Issue`].

use super::{
    Issue, IssueKind, MAX_PRIORITY, MIN_PRIORITY, validate_text_fields, validate_title_and_priority,
};
use thiserror::Error;

/// A failure to construct a canonical issue update.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum UpdateError {
    /// No mutable field was supplied.
    #[error("Issue update requires at least one field")]
    EmptyUpdate,
    /// The title failed the shared title grammar.
    #[error("{0}")]
    InvalidTitle(String),
    /// The description failed the shared multiline text grammar.
    #[error("{0}")]
    InvalidDescription(String),
    /// The design failed the shared multiline text grammar.
    #[error("{0}")]
    InvalidDesign(String),
    /// The acceptance criteria failed the shared multiline text grammar.
    #[error("{0}")]
    InvalidAcceptanceCriteria(String),
    /// Priority was outside the canonical P0-P4 range.
    #[error("Invalid priority value: {0} (must be 0-4)")]
    InvalidPriority(u8),
}

/// A validated, nonempty update containing only canonical mutable fields.
///
/// Values can only cross the storage boundary after construction through
/// [`IssueUpdate::builder`]. In particular, this type intentionally has no
/// `Default` implementation and no public fields.
///
/// ```compile_fail
/// use rivets::domain::IssueUpdate;
/// let _ = IssueUpdate::default();
/// ```
///
/// ```compile_fail
/// use rivets::domain::IssueUpdate;
/// let _ = IssueUpdate { title: None };
/// ```
#[derive(Debug, Clone)]
pub struct IssueUpdate {
    title: Option<String>,
    description: Option<String>,
    priority: Option<u8>,
    issue_kind: Option<IssueKind>,
    design: Option<String>,
    acceptance_criteria: Option<String>,
}

impl IssueUpdate {
    /// Start constructing a validated update.
    #[must_use]
    pub fn builder() -> IssueUpdateBuilder {
        IssueUpdateBuilder::default()
    }

    /// Apply this update's supplied fields to an issue candidate.
    pub(crate) fn apply(self, issue: &mut Issue) {
        let Self {
            title,
            description,
            priority,
            issue_kind,
            design,
            acceptance_criteria,
        } = self;
        if let Some(title) = title {
            issue.title = title;
        }
        if let Some(description) = description {
            issue.description = description;
        }
        if let Some(priority) = priority {
            issue.priority = priority;
        }
        if let Some(issue_kind) = issue_kind {
            issue.issue_kind = issue_kind;
        }
        if let Some(design) = design {
            issue.design = Some(design);
        }
        if let Some(acceptance_criteria) = acceptance_criteria {
            issue.acceptance_criteria = Some(acceptance_criteria);
        }
    }
}

/// Builder for [`IssueUpdate`].
///
/// The fields are deliberately private: callers must use [`Self::build`] so
/// empty requests and every supplied value are validated consistently.
#[derive(Debug, Clone, Default)]
pub struct IssueUpdateBuilder {
    title: Option<String>,
    description: Option<String>,
    priority: Option<u8>,
    issue_kind: Option<IssueKind>,
    design: Option<String>,
    acceptance_criteria: Option<String>,
}

impl IssueUpdateBuilder {
    /// Supply a replacement title, or `None` to leave it unchanged.
    #[must_use]
    pub fn title(mut self, value: Option<String>) -> Self {
        self.title = value;
        self
    }

    /// Supply a replacement description, or `None` to leave it unchanged.
    #[must_use]
    pub fn description(mut self, value: Option<String>) -> Self {
        self.description = value;
        self
    }

    /// Supply a replacement priority, or `None` to leave it unchanged.
    #[must_use]
    pub fn priority(mut self, value: Option<u8>) -> Self {
        self.priority = value;
        self
    }

    /// Supply a replacement kind, or `None` to leave it unchanged.
    #[must_use]
    pub fn issue_kind(mut self, value: Option<IssueKind>) -> Self {
        self.issue_kind = value;
        self
    }

    /// Supply replacement design text, or `None` to leave it unchanged.
    #[must_use]
    pub fn design(mut self, value: Option<String>) -> Self {
        self.design = value;
        self
    }

    /// Supply replacement acceptance criteria, or `None` to leave it unchanged.
    #[must_use]
    pub fn acceptance_criteria(mut self, value: Option<String>) -> Self {
        self.acceptance_criteria = value;
        self
    }

    /// Validate all supplied fields and consume the builder.
    pub fn build(self) -> Result<IssueUpdate, UpdateError> {
        let Self {
            title,
            description,
            priority,
            issue_kind,
            design,
            acceptance_criteria,
        } = self;

        if title.is_none()
            && description.is_none()
            && priority.is_none()
            && issue_kind.is_none()
            && design.is_none()
            && acceptance_criteria.is_none()
        {
            return Err(UpdateError::EmptyUpdate);
        }

        if let Some(title) = title.as_deref() {
            validate_title_and_priority(title, MIN_PRIORITY).map_err(UpdateError::InvalidTitle)?;
        }
        if let Some(description) = description.as_deref() {
            validate_text_fields(description, None, None, None)
                .map_err(UpdateError::InvalidDescription)?;
        }
        if let Some(priority) = priority
            && !(MIN_PRIORITY..=MAX_PRIORITY).contains(&priority)
        {
            return Err(UpdateError::InvalidPriority(priority));
        }
        if let Some(design) = design.as_deref() {
            validate_text_fields("", None, Some(design), None)
                .map_err(UpdateError::InvalidDesign)?;
        }
        if let Some(acceptance_criteria) = acceptance_criteria.as_deref() {
            validate_text_fields("", None, None, Some(acceptance_criteria))
                .map_err(UpdateError::InvalidAcceptanceCriteria)?;
        }

        Ok(IssueUpdate {
            title,
            description,
            priority,
            issue_kind,
            design,
            acceptance_criteria,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_update_is_rejected() {
        assert_eq!(
            IssueUpdate::builder().build().unwrap_err(),
            UpdateError::EmptyUpdate
        );
    }

    #[test]
    fn invalid_priority_is_typed() {
        assert_eq!(
            IssueUpdate::builder()
                .priority(Some(MAX_PRIORITY + 1))
                .build()
                .unwrap_err(),
            UpdateError::InvalidPriority(MAX_PRIORITY + 1)
        );
    }
}
