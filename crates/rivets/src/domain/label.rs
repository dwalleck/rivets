//! Canonical Issue Label value.

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use std::fmt;
use std::str::FromStr;

/// Maximum number of ASCII bytes in a canonical Issue Label.
pub const MAX_LABEL_LENGTH: usize = 50;

/// A canonical Workspace-defined classification applied to an Issue.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Label(String);

impl Label {
    /// Parse a canonical Issue Label.
    pub fn new(value: impl AsRef<str>) -> Result<Self, LabelError> {
        value.as_ref().parse()
    }

    /// Borrow the canonical Label spelling.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume the Label and return its canonical spelling.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

/// Why an Issue Label spelling is not canonical.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LabelError {
    /// The spelling has no bytes.
    #[error("Label cannot be empty")]
    Empty,
    /// The spelling exceeds [`MAX_LABEL_LENGTH`].
    #[error("Label cannot exceed {MAX_LABEL_LENGTH} bytes, got {length} bytes")]
    TooLong {
        /// Rejected byte length.
        length: usize,
    },
    /// An uppercase ASCII letter appeared.
    #[error("Label must be lowercase; uppercase character at position {position}")]
    Uppercase {
        /// Zero-based character position.
        position: usize,
    },
    /// A character is outside the canonical alphabet.
    #[error(
        "Label must contain only lowercase letters, numbers, hyphens, and underscores; invalid character at position {position}"
    )]
    InvalidCharacter {
        /// Zero-based character position.
        position: usize,
    },
    /// The spelling starts with a separator.
    #[error("Label must start with a letter or number")]
    InvalidStart,
    /// The spelling ends with a separator.
    #[error("Label must end with a letter or number")]
    InvalidEnd,
    /// Two separators are adjacent.
    #[error("Label cannot contain consecutive separators at position {position}")]
    ConsecutiveSeparators {
        /// Zero-based position of the second separator.
        position: usize,
    },
}

impl FromStr for Label {
    type Err = LabelError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.is_empty() {
            return Err(LabelError::Empty);
        }
        if value.len() > MAX_LABEL_LENGTH {
            return Err(LabelError::TooLong {
                length: value.len(),
            });
        }

        let mut previous_was_separator = false;
        for (position, character) in value.chars().enumerate() {
            if character.is_ascii_uppercase() {
                return Err(LabelError::Uppercase { position });
            }

            let is_separator = matches!(character, '-' | '_');
            if !character.is_ascii_lowercase() && !character.is_ascii_digit() && !is_separator {
                return Err(LabelError::InvalidCharacter { position });
            }
            if position == 0 && is_separator {
                return Err(LabelError::InvalidStart);
            }
            if previous_was_separator && is_separator {
                return Err(LabelError::ConsecutiveSeparators { position });
            }
            previous_was_separator = is_separator;
        }

        if previous_was_separator {
            return Err(LabelError::InvalidEnd);
        }

        Ok(Self(value.to_string()))
    }
}

impl fmt::Display for Label {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Serialize for Label {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Label {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case::one_byte("a")]
    #[case::maximum_length("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")]
    #[case::digits("p0")]
    #[case::hyphen("ready-for-agent")]
    #[case::underscore("type_safety")]
    #[case::both_nonadjacent("high-priority_v2")]
    fn parses_canonical_label(#[case] value: &str) {
        let label = value
            .parse::<Label>()
            .expect("canonical Label should parse");
        assert_eq!(label.as_str(), value);
        assert_eq!(label.to_string(), value);
        assert_eq!(label.into_string(), value);
    }

    #[rstest]
    #[case::empty("", LabelError::Empty)]
    #[case::too_long(
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        LabelError::TooLong { length: 51 }
    )]
    #[case::uppercase("Bug", LabelError::Uppercase { position: 0 })]
    #[case::space("high priority", LabelError::InvalidCharacter { position: 4 })]
    #[case::surrounding_whitespace(" bug ", LabelError::InvalidCharacter { position: 0 })]
    #[case::control("bug\u{1b}", LabelError::InvalidCharacter { position: 3 })]
    #[case::unicode("é", LabelError::InvalidCharacter { position: 0 })]
    #[case::dot("args.rs", LabelError::InvalidCharacter { position: 4 })]
    #[case::leading_hyphen("-bug", LabelError::InvalidStart)]
    #[case::leading_underscore("_bug", LabelError::InvalidStart)]
    #[case::trailing_hyphen("bug-", LabelError::InvalidEnd)]
    #[case::trailing_underscore("bug_", LabelError::InvalidEnd)]
    #[case::double_hyphen(
        "high--priority",
        LabelError::ConsecutiveSeparators { position: 5 }
    )]
    #[case::double_underscore(
        "needs__review",
        LabelError::ConsecutiveSeparators { position: 6 }
    )]
    #[case::mixed_hyphen_underscore(
        "high-_priority",
        LabelError::ConsecutiveSeparators { position: 5 }
    )]
    #[case::mixed_underscore_hyphen(
        "high_-priority",
        LabelError::ConsecutiveSeparators { position: 5 }
    )]
    fn rejects_noncanonical_label(#[case] value: &str, #[case] expected: LabelError) {
        assert_eq!(value.parse::<Label>(), Err(expected));
    }

    #[test]
    fn serde_round_trip_uses_string_spelling() {
        let label = Label::new("ready-for-agent").expect("canonical Label");
        let json = serde_json::to_string(&label).expect("Label should serialize");
        assert_eq!(json, "\"ready-for-agent\"");
        assert_eq!(
            serde_json::from_str::<Label>(&json).expect("Label should deserialize"),
            label
        );
    }

    #[test]
    fn serde_rejects_noncanonical_spelling() {
        let error = serde_json::from_str::<Label>("\"DRY\"")
            .expect_err("noncanonical Label should be rejected");
        assert!(error.to_string().contains("Label must be lowercase"));
    }

    #[test]
    fn collection_budget_is_one_bounded_parse_per_label() {
        let labels = (0..1_000)
            .map(|index| format!("label-{index}"))
            .map(|value| Label::new(value).expect("generated Label should parse"))
            .collect::<Vec<_>>();
        assert_eq!(labels.len(), 1_000);
        assert!(labels.iter().all(|label| label.as_str().len() <= 50));
    }
}
