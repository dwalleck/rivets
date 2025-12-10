//! Parser for the .rivet issue format using winnow.

use chrono::{DateTime, Utc};
use winnow::{
    ascii::{line_ending, space0, space1, till_line_ending},
    combinator::{alt, delimited, opt, preceded, repeat, terminated},
    error::{ContextError, ParseError},
    prelude::*,
    token::{any, take_till, take_until, take_while},
};

use crate::types::*;

/// Parse a complete .rivet document.
pub fn parse_document(input: &str) -> Result<RivetDocument, ParseDocumentError> {
    let mut doc = RivetDocument::default();
    let blocks = parse_blocks
        .parse(input)
        .map_err(|e| ParseDocumentError::Parse(e.to_string()))?;

    for block in blocks {
        match block.name.as_str() {
            "meta" => {
                doc.meta = parse_meta_content(&block.content)?;
            }
            "title" => {
                doc.title = block.content.trim().to_string();
            }
            "description" => {
                doc.description = block.content.trim().to_string();
            }
            "labels" => {
                doc.labels = parse_list_content(&block.content);
            }
            "assignees" => {
                doc.assignees = parse_list_content(&block.content);
            }
            "depends-on" => {
                doc.dependencies = parse_dependencies_content(&block.content)?;
            }
            "notes" => {
                doc.notes = Some(block.content.trim().to_string());
            }
            "design" => {
                doc.design = Some(block.content.trim().to_string());
            }
            name => {
                return Err(ParseDocumentError::UnknownBlock(name.to_string()));
            }
        }
    }

    // Validate required fields
    if doc.meta.id.is_empty() {
        return Err(ParseDocumentError::MissingField("meta.id".to_string()));
    }
    if doc.title.is_empty() {
        return Err(ParseDocumentError::MissingField("title".to_string()));
    }

    Ok(doc)
}

/// Errors that can occur when parsing a .rivet document.
#[derive(Debug, thiserror::Error)]
pub enum ParseDocumentError {
    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Unknown block type: {0}")]
    UnknownBlock(String),

    #[error("Missing required field: {0}")]
    MissingField(String),

    #[error("Invalid value for {field}: {value}")]
    InvalidValue { field: String, value: String },

    #[error("Invalid date format for {field}: {value}")]
    InvalidDate { field: String, value: String },
}

/// A raw parsed block before semantic interpretation.
#[derive(Debug)]
struct Block {
    name: String,
    content: String,
}

/// Parse all blocks from the document.
fn parse_blocks(input: &mut &str) -> PResult<Vec<Block>> {
    let mut blocks = Vec::new();

    loop {
        // Skip whitespace between blocks
        let _: () = skip_whitespace_and_comments.parse_next(input)?;

        if input.is_empty() {
            break;
        }

        // Parse block name
        let name: String = parse_block_name.parse_next(input)?;

        // Skip whitespace after name
        let _: () = space0.void().parse_next(input)?;

        // Parse block body (either {...} or [...])
        let content: String = parse_block_body.parse_next(input)?;

        blocks.push(Block { name, content });
    }

    Ok(blocks)
}

/// Skip whitespace, newlines, and comments.
fn skip_whitespace_and_comments(input: &mut &str) -> PResult<()> {
    loop {
        let before = *input;

        // Skip whitespace
        let _: &str = take_while(0.., |c: char| c.is_whitespace()).parse_next(input)?;

        // Skip line comments
        if input.starts_with('#') {
            let _: &str = till_line_ending.parse_next(input)?;
            let _: Option<&str> = opt(line_ending).parse_next(input)?;
            continue;
        }

        if *input == before {
            break;
        }
    }
    Ok(())
}

/// Parse a block name (e.g., "meta", "title", "depends-on").
fn parse_block_name(input: &mut &str) -> PResult<String> {
    let name: &str =
        take_while(1.., |c: char| c.is_alphanumeric() || c == '-' || c == '_').parse_next(input)?;
    Ok(name.to_string())
}

/// Parse a block body - either brace-delimited or bracket-delimited.
fn parse_block_body(input: &mut &str) -> PResult<String> {
    alt((parse_brace_body, parse_bracket_body)).parse_next(input)
}

/// Parse content within braces { ... }, handling nested braces.
fn parse_brace_body(input: &mut &str) -> PResult<String> {
    let _: char = '{'.parse_next(input)?;
    let content = parse_nested_content(input, '{', '}')?;
    let _: char = '}'.parse_next(input)?;
    Ok(content)
}

/// Parse content within brackets [ ... ], handling nested brackets.
fn parse_bracket_body(input: &mut &str) -> PResult<String> {
    let _: char = '['.parse_next(input)?;
    let content = parse_nested_content(input, '[', ']')?;
    let _: char = ']'.parse_next(input)?;
    Ok(content)
}

/// Parse content handling nested delimiters.
fn parse_nested_content(input: &mut &str, open: char, close: char) -> PResult<String> {
    let mut content = String::new();
    let mut depth = 1;

    while depth > 0 {
        if input.is_empty() {
            return Err(winnow::error::ErrMode::Cut(ContextError::new()));
        }

        let c: char = any.parse_next(input)?;

        if c == open {
            depth += 1;
            content.push(c);
        } else if c == close {
            depth -= 1;
            if depth > 0 {
                content.push(c);
            }
        } else {
            content.push(c);
        }
    }

    Ok(content)
}

/// Parse the content of a meta block into IssueMeta.
fn parse_meta_content(content: &str) -> Result<IssueMeta, ParseDocumentError> {
    let mut meta = IssueMeta::default();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            let value = value.trim();

            match key {
                "id" => meta.id = value.to_string(),
                "status" => {
                    meta.status =
                        IssueStatus::from_str(value).ok_or_else(|| ParseDocumentError::InvalidValue {
                            field: "status".to_string(),
                            value: value.to_string(),
                        })?;
                }
                "priority" => {
                    meta.priority = value.parse().map_err(|_| ParseDocumentError::InvalidValue {
                        field: "priority".to_string(),
                        value: value.to_string(),
                    })?;
                }
                "created" => {
                    meta.created = parse_datetime(value).ok_or_else(|| ParseDocumentError::InvalidDate {
                        field: "created".to_string(),
                        value: value.to_string(),
                    })?;
                }
                "updated" => {
                    meta.updated =
                        Some(parse_datetime(value).ok_or_else(|| ParseDocumentError::InvalidDate {
                            field: "updated".to_string(),
                            value: value.to_string(),
                        })?);
                }
                "closed" => {
                    meta.closed =
                        Some(parse_datetime(value).ok_or_else(|| ParseDocumentError::InvalidDate {
                            field: "closed".to_string(),
                            value: value.to_string(),
                        })?);
                }
                _ => {
                    // Ignore unknown fields for forward compatibility
                }
            }
        }
    }

    Ok(meta)
}

/// Parse a datetime string in ISO 8601 format.
fn parse_datetime(s: &str) -> Option<DateTime<Utc>> {
    // Try parsing with timezone
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc));
    }

    // Try parsing without timezone (assume UTC)
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
        return Some(dt.and_utc());
    }

    // Try date only
    if let Ok(date) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Some(
            date.and_hms_opt(0, 0, 0)
                .expect("midnight is valid")
                .and_utc(),
        );
    }

    None
}

/// Parse a list block content into a Vec<String>.
fn parse_list_content(content: &str) -> Vec<String> {
    content
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.to_string())
        .collect()
}

/// Parse a depends-on block content into Vec<Dependency>.
fn parse_dependencies_content(content: &str) -> Result<Vec<Dependency>, ParseDocumentError> {
    let mut deps = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some((id, dep_type_str)) = line.split_once(':') {
            let dep_type = DependencyType::from_str(dep_type_str).ok_or_else(|| {
                ParseDocumentError::InvalidValue {
                    field: "dependency type".to_string(),
                    value: dep_type_str.trim().to_string(),
                }
            })?;

            deps.push(Dependency {
                issue_id: id.trim().to_string(),
                dep_type,
            });
        } else {
            // Allow bare issue IDs, default to Related
            deps.push(Dependency {
                issue_id: line.to_string(),
                dep_type: DependencyType::Related,
            });
        }
    }

    Ok(deps)
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_parse_minimal_document() {
        let input = indoc! {r#"
            meta {
              id: rivets-a3f8
              status: open
              priority: 2
              created: 2025-01-15T10:30:00Z
            }

            title {
              Fix the bug
            }

            description {
              This is the description.
            }
        "#};

        let doc = parse_document(input).unwrap();

        assert_eq!(doc.meta.id, "rivets-a3f8");
        assert_eq!(doc.meta.status, IssueStatus::Open);
        assert_eq!(doc.meta.priority, 2);
        assert_eq!(doc.title, "Fix the bug");
        assert_eq!(doc.description, "This is the description.");
    }

    #[test]
    fn test_parse_full_document() {
        let input = indoc! {r#"
            meta {
              id: rivets-a3f8
              status: in-progress
              priority: 1
              created: 2025-01-15T10:30:00Z
              updated: 2025-01-20T14:22:00Z
            }

            title {
              Implement Automerge storage backend
            }

            description {
              Add an alternative storage backend using Automerge CRDTs
              to enable concurrent access by multiple agents.
              
              This should be behind a feature flag initially.
            }

            labels [
              backend
              storage
              enhancement
            ]

            assignees [
              dwalleck
              claude
            ]

            depends-on [
              rivets-x9k2: blocks
              rivets-b2c9: related
            ]

            notes {
              ## 2025-01-20 - Reconsidering approach
              
              After discussion, file-per-issue might be simpler.
            }
        "#};

        let doc = parse_document(input).unwrap();

        assert_eq!(doc.meta.id, "rivets-a3f8");
        assert_eq!(doc.meta.status, IssueStatus::InProgress);
        assert_eq!(doc.meta.priority, 1);
        assert!(doc.meta.updated.is_some());

        assert_eq!(doc.title, "Implement Automerge storage backend");
        assert!(doc.description.contains("Automerge CRDTs"));
        assert!(doc.description.contains("feature flag"));

        assert_eq!(doc.labels, vec!["backend", "storage", "enhancement"]);
        assert_eq!(doc.assignees, vec!["dwalleck", "claude"]);

        assert_eq!(doc.dependencies.len(), 2);
        assert_eq!(doc.dependencies[0].issue_id, "rivets-x9k2");
        assert_eq!(doc.dependencies[0].dep_type, DependencyType::Blocks);
        assert_eq!(doc.dependencies[1].issue_id, "rivets-b2c9");
        assert_eq!(doc.dependencies[1].dep_type, DependencyType::Related);

        assert!(doc.notes.is_some());
        assert!(doc.notes.as_ref().unwrap().contains("Reconsidering"));
    }

    #[test]
    fn test_parse_with_comments() {
        let input = indoc! {r#"
            # This is a file-level comment
            
            meta {
              id: rivets-test
              # This is a comment inside meta
              status: open
              priority: 2
              created: 2025-01-15T10:30:00Z
            }

            title {
              Test issue
            }

            description {
              Description here.
            }

            labels [
              # Comment in list
              bug
              urgent
            ]
        "#};

        let doc = parse_document(input).unwrap();

        assert_eq!(doc.meta.id, "rivets-test");
        assert_eq!(doc.labels, vec!["bug", "urgent"]);
    }

    #[test]
    fn test_parse_nested_braces_in_notes() {
        let input = indoc! {r#"
            meta {
              id: rivets-nested
              status: open
              priority: 2
              created: 2025-01-15T10:30:00Z
            }

            title {
              Test nested braces
            }

            description {
              Has code block.
            }

            notes {
              Here's some code:
              
              ```rust
              fn main() {
                  println!("Hello");
              }
              ```
            }
        "#};

        let doc = parse_document(input).unwrap();

        assert!(doc.notes.is_some());
        let notes = doc.notes.unwrap();
        assert!(notes.contains("fn main()"));
        assert!(notes.contains("println!"));
    }

    #[test]
    fn test_missing_required_field() {
        let input = indoc! {r#"
            meta {
              status: open
              priority: 2
              created: 2025-01-15T10:30:00Z
            }

            title {
              Missing ID
            }

            description {
              Oops.
            }
        "#};

        let result = parse_document(input);
        assert!(matches!(result, Err(ParseDocumentError::MissingField(_))));
    }

    #[test]
    fn test_invalid_status() {
        let input = indoc! {r#"
            meta {
              id: rivets-test
              status: invalid-status
              priority: 2
              created: 2025-01-15T10:30:00Z
            }

            title {
              Bad status
            }

            description {
              Oops.
            }
        "#};

        let result = parse_document(input);
        assert!(matches!(result, Err(ParseDocumentError::InvalidValue { .. })));
    }

    #[test]
    fn test_various_date_formats() {
        let input = indoc! {r#"
            meta {
              id: rivets-dates
              status: open
              priority: 2
              created: 2025-01-15
            }

            title {
              Date format test
            }

            description {
              Testing date-only format.
            }
        "#};

        let doc = parse_document(input).unwrap();
        assert_eq!(doc.meta.created.format("%Y-%m-%d").to_string(), "2025-01-15");
    }
}
