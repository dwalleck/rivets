//! # rivets-format
//!
//! Parser and serializer for the `.rivet` issue format.
//!
//! The `.rivet` format is a human-readable, git-friendly format for storing
//! issue tracking data. Each issue is stored in its own file, making merges
//! simple and diffs readable.
//!
//! ## Example
//!
//! ```rust
//! use rivets_format::{parse_document, serialize_document, RivetDocument, IssueMeta, IssueStatus};
//! use chrono::Utc;
//!
//! // Parse a .rivet file
//! let input = r#"
//! meta {
//!   id: rivets-a3f8
//!   status: open
//!   priority: 2
//!   created: 2025-01-15T10:30:00Z
//! }
//!
//! title {
//!   Fix the bug
//! }
//!
//! description {
//!   This needs to be fixed.
//! }
//! "#;
//!
//! let doc = parse_document(input).unwrap();
//! assert_eq!(doc.meta.id, "rivets-a3f8");
//! assert_eq!(doc.title, "Fix the bug");
//!
//! // Serialize back to .rivet format
//! let output = serialize_document(&doc);
//! assert!(output.contains("rivets-a3f8"));
//! ```
//!
//! ## Format Specification
//!
//! A `.rivet` file consists of named blocks:
//!
//! - `meta { ... }` - Required. Issue metadata (id, status, priority, dates)
//! - `title { ... }` - Required. The issue title
//! - `description { ... }` - Required. The issue description
//! - `labels [ ... ]` - Optional. List of labels, one per line
//! - `assignees [ ... ]` - Optional. List of assignees, one per line
//! - `depends-on [ ... ]` - Optional. Dependencies in `id: type` format
//! - `notes { ... }` - Optional. Free-form notes (supports markdown)
//! - `design { ... }` - Optional. Design documentation
//!
//! Comments start with `#` and are ignored.

mod parser;
mod serializer;
mod types;

pub use parser::{parse_document, ParseDocumentError};
pub use serializer::{serialize_document, serialize_document_with_options, SerializeOptions};
pub use types::*;

/// Read a .rivet file from the filesystem.
pub fn read_rivet_file(path: &std::path::Path) -> Result<RivetDocument, ReadError> {
    let content = std::fs::read_to_string(path).map_err(ReadError::Io)?;
    parse_document(&content).map_err(ReadError::Parse)
}

/// Write a .rivet file to the filesystem.
pub fn write_rivet_file(path: &std::path::Path, doc: &RivetDocument) -> Result<(), std::io::Error> {
    let content = serialize_document(doc);
    std::fs::write(path, content)
}

/// Errors that can occur when reading a .rivet file.
#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Parse error: {0}")]
    Parse(#[from] ParseDocumentError),
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use indoc::indoc;

    #[test]
    fn test_full_roundtrip() {
        let original = indoc! {r#"
            meta {
              id: rivets-test
              status: in-progress
              priority: 1
              created: 2025-01-15T10:30:00Z
              updated: 2025-01-20T14:22:00Z
            }

            title {
              Implement the feature
            }

            description {
              This is a multi-line description.
              
              It has paragraphs and stuff.
            }

            labels [
              backend
              urgent
            ]

            assignees [
              alice
              bob
            ]

            depends-on [
              rivets-other: blocks
            ]

            notes {
              ## Progress
              
              - Did the thing
              - Did another thing
            }
        "#};

        let doc = parse_document(original).unwrap();
        let serialized = serialize_document(&doc);
        let reparsed = parse_document(&serialized).unwrap();

        assert_eq!(doc.meta.id, reparsed.meta.id);
        assert_eq!(doc.meta.status, reparsed.meta.status);
        assert_eq!(doc.title, reparsed.title);
        assert_eq!(doc.description, reparsed.description);
        assert_eq!(doc.labels, reparsed.labels);
        assert_eq!(doc.assignees, reparsed.assignees);
        assert_eq!(doc.dependencies.len(), reparsed.dependencies.len());
        assert_eq!(doc.notes, reparsed.notes);
    }
}
