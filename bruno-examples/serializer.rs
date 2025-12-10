//! Serializer for writing .rivet documents.

use crate::types::*;
use std::fmt::Write;

/// Serialize a RivetDocument to the .rivet format string.
pub fn serialize_document(doc: &RivetDocument) -> String {
    let mut output = String::new();

    // Meta block
    write_meta_block(&mut output, &doc.meta);

    // Title block
    write_text_block(&mut output, "title", &doc.title);

    // Description block
    write_text_block(&mut output, "description", &doc.description);

    // Labels block (only if non-empty)
    if !doc.labels.is_empty() {
        write_list_block(&mut output, "labels", &doc.labels);
    }

    // Assignees block (only if non-empty)
    if !doc.assignees.is_empty() {
        write_list_block(&mut output, "assignees", &doc.assignees);
    }

    // Dependencies block (only if non-empty)
    if !doc.dependencies.is_empty() {
        write_dependencies_block(&mut output, &doc.dependencies);
    }

    // Notes block (only if present)
    if let Some(notes) = &doc.notes {
        write_text_block(&mut output, "notes", notes);
    }

    // Design block (only if present)
    if let Some(design) = &doc.design {
        write_text_block(&mut output, "design", design);
    }

    output
}

fn write_meta_block(output: &mut String, meta: &IssueMeta) {
    output.push_str("meta {\n");
    writeln!(output, "  id: {}", meta.id).unwrap();
    writeln!(output, "  status: {}", meta.status.as_str()).unwrap();
    writeln!(output, "  priority: {}", meta.priority).unwrap();
    writeln!(output, "  created: {}", meta.created.format("%Y-%m-%dT%H:%M:%SZ")).unwrap();

    if let Some(updated) = &meta.updated {
        writeln!(output, "  updated: {}", updated.format("%Y-%m-%dT%H:%M:%SZ")).unwrap();
    }

    if let Some(closed) = &meta.closed {
        writeln!(output, "  closed: {}", closed.format("%Y-%m-%dT%H:%M:%SZ")).unwrap();
    }

    output.push_str("}\n\n");
}

fn write_text_block(output: &mut String, name: &str, content: &str) {
    output.push_str(name);
    output.push_str(" {\n");

    // Indent each line of the content
    for line in content.lines() {
        if line.is_empty() {
            output.push('\n');
        } else {
            output.push_str("  ");
            output.push_str(line);
            output.push('\n');
        }
    }

    output.push_str("}\n\n");
}

fn write_list_block(output: &mut String, name: &str, items: &[String]) {
    output.push_str(name);
    output.push_str(" [\n");

    for item in items {
        output.push_str("  ");
        output.push_str(item);
        output.push('\n');
    }

    output.push_str("]\n\n");
}

fn write_dependencies_block(output: &mut String, deps: &[Dependency]) {
    output.push_str("depends-on [\n");

    for dep in deps {
        writeln!(output, "  {}: {}", dep.issue_id, dep.dep_type.as_str()).unwrap();
    }

    output.push_str("]\n\n");
}

/// Options for controlling serialization output.
#[derive(Debug, Clone, Default)]
pub struct SerializeOptions {
    /// Include empty optional blocks (notes, design) as empty blocks.
    pub include_empty_optional: bool,

    /// Use compact single-line format for short titles.
    pub compact_short_titles: bool,
}

/// Serialize with custom options.
pub fn serialize_document_with_options(doc: &RivetDocument, opts: &SerializeOptions) -> String {
    let mut output = String::new();

    write_meta_block(&mut output, &doc.meta);

    // Compact title for short single-line titles
    if opts.compact_short_titles && !doc.title.contains('\n') && doc.title.len() < 60 {
        writeln!(output, "title {{ {} }}\n", doc.title).unwrap();
    } else {
        write_text_block(&mut output, "title", &doc.title);
    }

    write_text_block(&mut output, "description", &doc.description);

    if !doc.labels.is_empty() {
        write_list_block(&mut output, "labels", &doc.labels);
    }

    if !doc.assignees.is_empty() {
        write_list_block(&mut output, "assignees", &doc.assignees);
    }

    if !doc.dependencies.is_empty() {
        write_dependencies_block(&mut output, &doc.dependencies);
    }

    if let Some(notes) = &doc.notes {
        write_text_block(&mut output, "notes", notes);
    } else if opts.include_empty_optional {
        output.push_str("notes {\n}\n\n");
    }

    if let Some(design) = &doc.design {
        write_text_block(&mut output, "design", design);
    } else if opts.include_empty_optional {
        output.push_str("design {\n}\n\n");
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_document;
    use chrono::TimeZone;

    fn sample_document() -> RivetDocument {
        RivetDocument {
            meta: IssueMeta {
                id: "rivets-a3f8".to_string(),
                status: IssueStatus::InProgress,
                priority: 1,
                created: Utc.with_ymd_and_hms(2025, 1, 15, 10, 30, 0).unwrap(),
                updated: Some(Utc.with_ymd_and_hms(2025, 1, 20, 14, 22, 0).unwrap()),
                closed: None,
            },
            title: "Implement storage backend".to_string(),
            description: "Add a new storage backend.\n\nThis is important.".to_string(),
            labels: vec!["backend".to_string(), "storage".to_string()],
            assignees: vec!["dwalleck".to_string()],
            dependencies: vec![
                Dependency {
                    issue_id: "rivets-x9k2".to_string(),
                    dep_type: DependencyType::Blocks,
                },
                Dependency {
                    issue_id: "rivets-b2c9".to_string(),
                    dep_type: DependencyType::Related,
                },
            ],
            notes: Some("Some notes here.".to_string()),
            design: None,
        }
    }

    #[test]
    fn test_roundtrip() {
        let original = sample_document();
        let serialized = serialize_document(&original);
        let parsed = parse_document(&serialized).expect("should parse");

        assert_eq!(parsed.meta.id, original.meta.id);
        assert_eq!(parsed.meta.status, original.meta.status);
        assert_eq!(parsed.meta.priority, original.meta.priority);
        assert_eq!(parsed.title, original.title);
        assert_eq!(parsed.description, original.description);
        assert_eq!(parsed.labels, original.labels);
        assert_eq!(parsed.assignees, original.assignees);
        assert_eq!(parsed.dependencies.len(), original.dependencies.len());
        assert_eq!(parsed.notes, original.notes);
    }

    #[test]
    fn test_serialize_output_format() {
        let doc = sample_document();
        let output = serialize_document(&doc);

        // Check structure
        assert!(output.contains("meta {"));
        assert!(output.contains("id: rivets-a3f8"));
        assert!(output.contains("status: in-progress"));
        assert!(output.contains("title {"));
        assert!(output.contains("Implement storage backend"));
        assert!(output.contains("labels ["));
        assert!(output.contains("backend"));
        assert!(output.contains("depends-on ["));
        assert!(output.contains("rivets-x9k2: blocks"));
    }

    #[test]
    fn test_serialize_minimal() {
        let doc = RivetDocument {
            meta: IssueMeta {
                id: "test-123".to_string(),
                status: IssueStatus::Open,
                priority: 2,
                created: Utc::now(),
                updated: None,
                closed: None,
            },
            title: "Simple issue".to_string(),
            description: "A simple description.".to_string(),
            labels: vec![],
            assignees: vec![],
            dependencies: vec![],
            notes: None,
            design: None,
        };

        let output = serialize_document(&doc);

        // Should NOT contain empty blocks
        assert!(!output.contains("labels ["));
        assert!(!output.contains("assignees ["));
        assert!(!output.contains("depends-on ["));
        assert!(!output.contains("notes {"));
        assert!(!output.contains("design {"));
    }

    #[test]
    fn test_multiline_description() {
        let doc = RivetDocument {
            meta: IssueMeta {
                id: "test-456".to_string(),
                status: IssueStatus::Open,
                priority: 2,
                created: Utc::now(),
                ..Default::default()
            },
            title: "Multiline test".to_string(),
            description: "Line one.\n\nLine two with gap.\nLine three.".to_string(),
            ..Default::default()
        };

        let output = serialize_document(&doc);
        let parsed = parse_document(&output).unwrap();

        assert_eq!(parsed.description, doc.description);
    }
}
