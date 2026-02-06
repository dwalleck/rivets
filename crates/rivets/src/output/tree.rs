//! Dependency tree rendering for `rivets dep tree` output.

use std::io::{self, Write};

use colored::Colorize;

use super::color::{bold, colored_status_icon, colorize_id, colorize_priority};
use super::{OutputConfig, OutputMode};

/// A node in a dependency tree for rendering purposes.
#[derive(Debug, Clone)]
pub struct DepTreeNode {
    /// Issue ID of this node.
    pub id: String,
    /// Dependency type relationship to the parent (if any).
    pub dep_type: Option<crate::domain::DependencyType>,
    /// Issue status (for status icon rendering).
    pub status: Option<crate::domain::IssueStatus>,
    /// Issue title (optional, for the root node).
    pub title: Option<String>,
    /// Issue priority (optional).
    pub priority: Option<u8>,
    /// Children of this node in the dependency tree.
    pub children: Vec<DepTreeNode>,
}

/// Print a dependency tree with ASCII/Unicode connectors.
///
/// Renders a tree like:
/// ```text
/// ◆ rivets-abc [P2] Fix the bug
/// ├── rivets-def (blocks)
/// │   └── rivets-ghi (blocks)
/// └── rivets-jkl (related)
/// ```
pub fn print_dep_tree(root: &DepTreeNode, mode: OutputMode) -> io::Result<()> {
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    let config = OutputConfig::from_env();

    match mode {
        OutputMode::Text => print_dep_tree_text(&mut handle, root, &config),
        OutputMode::Json => {
            let json = dep_tree_to_json(root);
            let output = serde_json::to_string_pretty(&json).map_err(io::Error::other)?;
            writeln!(handle, "{}", output)
        }
    }
}

/// Render the dependency tree with ASCII art connectors.
fn print_dep_tree_text<W: Write>(
    w: &mut W,
    root: &DepTreeNode,
    config: &OutputConfig,
) -> io::Result<()> {
    // Print root node
    let root_icon = if config.use_ascii { "*" } else { "◆" };
    let root_icon_str = if config.use_colors {
        root_icon.cyan().bold().to_string()
    } else {
        root_icon.to_string()
    };

    let id_str = colorize_id(&root.id, config);
    let priority_str = root
        .priority
        .map(|p| format!(" {}", colorize_priority(p, config)))
        .unwrap_or_default();
    let title_str = root
        .title
        .as_deref()
        .map(|t| format!(" {}", t))
        .unwrap_or_default();

    writeln!(
        w,
        "{} {}{}{}",
        root_icon_str, id_str, priority_str, title_str
    )?;

    // Print children recursively
    print_dep_tree_children(w, &root.children, &[], config)
}

/// Recursively render tree children with proper connector lines.
///
/// `prefix_segments` tracks which ancestor levels still have siblings below,
/// used to draw the vertical continuation lines (`│`).
fn print_dep_tree_children<W: Write>(
    w: &mut W,
    children: &[DepTreeNode],
    prefix_segments: &[bool],
    config: &OutputConfig,
) -> io::Result<()> {
    let (branch, corner, pipe, space) = if config.use_ascii {
        ("|-- ", "`-- ", "|   ", "    ")
    } else {
        ("├── ", "└── ", "│   ", "    ")
    };

    for (i, child) in children.iter().enumerate() {
        let is_last = i == children.len() - 1;

        // Build prefix from ancestor continuation lines
        let mut prefix = String::new();
        for &has_more in prefix_segments {
            let segment = if has_more { pipe } else { space };
            if config.use_colors {
                prefix.push_str(&segment.dimmed().to_string());
            } else {
                prefix.push_str(segment);
            }
        }

        // Add branch or corner connector
        let connector = if is_last { corner } else { branch };
        let connector_str = if config.use_colors {
            connector.dimmed().to_string()
        } else {
            connector.to_string()
        };

        // Format child node
        let id_str = colorize_id(&child.id, config);
        let dep_type_str = child
            .dep_type
            .map(|dt| {
                let text = format!("({})", dt);
                if config.use_colors {
                    text.dimmed().to_string()
                } else {
                    text
                }
            })
            .unwrap_or_default();
        let status_str = child
            .status
            .map(|s| format!(" {}", colored_status_icon(s, config)))
            .unwrap_or_default();

        writeln!(
            w,
            "{}{}{} {}{}",
            prefix, connector_str, id_str, dep_type_str, status_str
        )?;

        // Recurse into children
        if !child.children.is_empty() {
            let mut next_segments = prefix_segments.to_vec();
            next_segments.push(!is_last);
            print_dep_tree_children(w, &child.children, &next_segments, config)?;
        }
    }

    Ok(())
}

/// Convert a dependency tree to a JSON value for programmatic output.
fn dep_tree_to_json(node: &DepTreeNode) -> serde_json::Value {
    let mut obj = serde_json::json!({
        "id": node.id,
    });

    if let Some(dt) = node.dep_type {
        obj["dep_type"] = serde_json::json!(format!("{}", dt));
    }
    if let Some(title) = &node.title {
        obj["title"] = serde_json::json!(title);
    }
    if let Some(p) = node.priority {
        obj["priority"] = serde_json::json!(p);
    }
    if let Some(s) = node.status {
        obj["status"] = serde_json::json!(format!("{}", s));
    }
    obj["dependencies"] = serde_json::json!(node
        .children
        .iter()
        .map(dep_tree_to_json)
        .collect::<Vec<_>>());

    obj
}

/// Convert a dependency tree to JSON including dependents for the `dep tree` command.
pub fn dep_tree_to_json_public(
    root: &DepTreeNode,
    dependents: &[crate::domain::Dependency],
) -> serde_json::Value {
    let mut json = dep_tree_to_json(root);
    json["dependents"] = serde_json::json!(dependents
        .iter()
        .map(|dep| {
            serde_json::json!({
                "depends_on_id": dep.depends_on_id.to_string(),
                "dep_type": format!("{}", dep.dep_type)
            })
        })
        .collect::<Vec<_>>());
    json
}

/// Print the "depended on by" (reverse dependencies) section for tree output.
pub fn print_dep_tree_dependents<W: Write>(
    w: &mut W,
    dependents: &[crate::domain::Dependency],
    config: &OutputConfig,
) -> io::Result<()> {
    if dependents.is_empty() {
        return Ok(());
    }

    writeln!(w)?;
    writeln!(
        w,
        "{} ({}):",
        bold("Depended on by", config),
        dependents.len()
    )?;

    let (corner, _pipe) = if config.use_ascii {
        ("`-- ", "|   ")
    } else {
        ("└── ", "│   ")
    };

    for dep in dependents {
        let connector_str = if config.use_colors {
            corner.dimmed().to_string()
        } else {
            corner.to_string()
        };
        let dep_type_str = format!("({})", dep.dep_type);
        let dep_type_display = if config.use_colors {
            dep_type_str.dimmed().to_string()
        } else {
            dep_type_str
        };

        writeln!(
            w,
            "  {}{}  {}",
            connector_str,
            colorize_id(dep.depends_on_id.as_str(), config),
            dep_type_display
        )?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Dependency, DependencyType, IssueId, IssueStatus};

    fn leaf_node(id: &str, dep_type: DependencyType) -> DepTreeNode {
        DepTreeNode {
            id: id.to_string(),
            dep_type: Some(dep_type),
            status: Some(IssueStatus::Open),
            title: None,
            priority: None,
            children: vec![],
        }
    }

    fn root_node(id: &str, children: Vec<DepTreeNode>) -> DepTreeNode {
        DepTreeNode {
            id: id.to_string(),
            dep_type: None,
            status: Some(IssueStatus::Open),
            title: Some("Root issue".to_string()),
            priority: Some(2),
            children,
        }
    }

    #[test]
    fn test_tree_single_root_no_children() {
        let config = OutputConfig::new(80, false, false);
        let root = root_node("test-abc", vec![]);
        let mut buffer = Vec::new();

        print_dep_tree_text(&mut buffer, &root, &config).expect("tree rendering should succeed");

        let output = String::from_utf8(buffer).expect("output should be valid UTF-8");
        assert!(output.contains("test-abc"), "should contain root ID");
        assert!(output.contains("P2"), "should contain priority");
        assert!(output.contains("Root issue"), "should contain title");
    }

    #[test]
    fn test_tree_single_child_unicode() {
        let config = OutputConfig::new(80, false, false);
        let root = root_node(
            "test-root",
            vec![leaf_node("test-child", DependencyType::Blocks)],
        );
        let mut buffer = Vec::new();

        print_dep_tree_text(&mut buffer, &root, &config).expect("tree rendering should succeed");

        let output = String::from_utf8(buffer).expect("output should be valid UTF-8");
        assert!(
            output.contains("└── test-child"),
            "single child should use corner connector, got: {}",
            output
        );
        assert!(
            output.contains("(blocks)"),
            "should show dep type, got: {}",
            output
        );
    }

    #[test]
    fn test_tree_single_child_ascii() {
        let config = OutputConfig::new(80, true, false);
        let root = root_node(
            "test-root",
            vec![leaf_node("test-child", DependencyType::Blocks)],
        );
        let mut buffer = Vec::new();

        print_dep_tree_text(&mut buffer, &root, &config).expect("tree rendering should succeed");

        let output = String::from_utf8(buffer).expect("output should be valid UTF-8");
        assert!(
            output.contains("`-- test-child"),
            "ASCII mode should use backtick connector, got: {}",
            output
        );
    }

    #[test]
    fn test_tree_multiple_children_connectors() {
        let config = OutputConfig::new(80, false, false);
        let root = root_node(
            "test-root",
            vec![
                leaf_node("child-1", DependencyType::Blocks),
                leaf_node("child-2", DependencyType::Related),
                leaf_node("child-3", DependencyType::ParentChild),
            ],
        );
        let mut buffer = Vec::new();

        print_dep_tree_text(&mut buffer, &root, &config).expect("tree rendering should succeed");

        let output = String::from_utf8(buffer).expect("output should be valid UTF-8");
        assert!(
            output.contains("├── child-1"),
            "non-last child should use branch connector, got: {}",
            output
        );
        assert!(
            output.contains("├── child-2"),
            "non-last child should use branch connector, got: {}",
            output
        );
        assert!(
            output.contains("└── child-3"),
            "last child should use corner connector, got: {}",
            output
        );
    }

    #[test]
    fn test_tree_nested_children() {
        let config = OutputConfig::new(80, false, false);
        let grandchild = leaf_node("grandchild", DependencyType::Blocks);
        let child = DepTreeNode {
            id: "child".to_string(),
            dep_type: Some(DependencyType::Blocks),
            status: Some(IssueStatus::InProgress),
            title: None,
            priority: None,
            children: vec![grandchild],
        };
        let root = root_node("root", vec![child]);
        let mut buffer = Vec::new();

        print_dep_tree_text(&mut buffer, &root, &config).expect("tree rendering should succeed");

        let output = String::from_utf8(buffer).expect("output should be valid UTF-8");
        assert!(output.contains("child"), "should contain child");
        assert!(output.contains("grandchild"), "should contain grandchild");
        let child_line = output
            .lines()
            .find(|l| l.contains("child") && !l.contains("grandchild"))
            .expect("should find child line");
        let grandchild_line = output
            .lines()
            .find(|l| l.contains("grandchild"))
            .expect("should find grandchild line");
        assert!(
            grandchild_line.len() > child_line.len(),
            "grandchild should have more indentation"
        );
    }

    #[test]
    fn test_tree_continuation_lines() {
        let config = OutputConfig::new(80, false, false);
        let grandchild = leaf_node("grandchild-1", DependencyType::Blocks);
        let child1 = DepTreeNode {
            id: "child-1".to_string(),
            dep_type: Some(DependencyType::Blocks),
            status: None,
            title: None,
            priority: None,
            children: vec![grandchild],
        };
        let child2 = leaf_node("child-2", DependencyType::Related);
        let root = root_node("root", vec![child1, child2]);
        let mut buffer = Vec::new();

        print_dep_tree_text(&mut buffer, &root, &config).expect("tree rendering should succeed");

        let output = String::from_utf8(buffer).expect("output should be valid UTF-8");
        assert!(
            output.contains("│   └── grandchild-1"),
            "grandchild should have continuation pipe, got:\n{}",
            output
        );
    }

    #[test]
    fn test_dep_tree_to_json_structure() {
        let grandchild = leaf_node("gc-1", DependencyType::Blocks);
        let child = DepTreeNode {
            id: "child-1".to_string(),
            dep_type: Some(DependencyType::Blocks),
            status: Some(IssueStatus::Open),
            title: None,
            priority: None,
            children: vec![grandchild],
        };
        let root = root_node("root", vec![child]);

        let json = dep_tree_to_json(&root);
        assert_eq!(json["id"], "root");
        assert_eq!(json["title"], "Root issue");
        assert_eq!(json["priority"], 2);

        let deps = json["dependencies"]
            .as_array()
            .expect("should have dependencies array");
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0]["id"], "child-1");

        let nested = deps[0]["dependencies"]
            .as_array()
            .expect("should have nested deps");
        assert_eq!(nested.len(), 1);
        assert_eq!(nested[0]["id"], "gc-1");
    }

    #[test]
    fn test_dep_tree_to_json_public_includes_dependents() {
        let root = root_node("root", vec![]);
        let dependents = vec![Dependency {
            depends_on_id: IssueId::new("dep-1"),
            dep_type: DependencyType::Blocks,
        }];

        let json = dep_tree_to_json_public(&root, &dependents);
        assert_eq!(json["id"], "root");

        let deps = json["dependents"]
            .as_array()
            .expect("should have dependents");
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0]["depends_on_id"], "dep-1");
    }

    #[test]
    fn test_print_dep_tree_dependents_section() {
        let config = OutputConfig::new(80, false, false);
        let dependents = vec![
            Dependency {
                depends_on_id: IssueId::new("dep-1"),
                dep_type: DependencyType::Blocks,
            },
            Dependency {
                depends_on_id: IssueId::new("dep-2"),
                dep_type: DependencyType::Related,
            },
        ];
        let mut buffer = Vec::new();

        print_dep_tree_dependents(&mut buffer, &dependents, &config)
            .expect("dependents rendering should succeed");

        let output = String::from_utf8(buffer).expect("output should be valid UTF-8");
        assert!(
            output.contains("Depended on by"),
            "should have section header"
        );
        assert!(output.contains("dep-1"), "should contain first dependent");
        assert!(output.contains("dep-2"), "should contain second dependent");
        assert!(output.contains("(blocks)"), "should show dep type");
        assert!(output.contains("(related)"), "should show dep type");
    }

    #[test]
    fn test_print_dep_tree_dependents_empty() {
        let config = OutputConfig::new(80, false, false);
        let mut buffer = Vec::new();

        print_dep_tree_dependents(&mut buffer, &[], &config)
            .expect("empty dependents should succeed");

        assert!(
            buffer.is_empty(),
            "empty dependents should produce no output"
        );
    }

    #[test]
    fn test_tree_root_icon_ascii_vs_unicode() {
        let config_unicode = OutputConfig::new(80, false, false);
        let root = root_node("test", vec![]);
        let mut buf_unicode = Vec::new();
        print_dep_tree_text(&mut buf_unicode, &root, &config_unicode).expect("should render");
        let out_unicode = String::from_utf8(buf_unicode).expect("valid UTF-8");
        assert!(
            out_unicode.contains('\u{25C6}'),
            "Unicode mode should use diamond, got: {}",
            out_unicode
        );

        let config_ascii = OutputConfig::new(80, true, false);
        let mut buf_ascii = Vec::new();
        print_dep_tree_text(&mut buf_ascii, &root, &config_ascii).expect("should render");
        let out_ascii = String::from_utf8(buf_ascii).expect("valid UTF-8");
        assert!(
            out_ascii.contains('*'),
            "ASCII mode should use asterisk, got: {}",
            out_ascii
        );
    }
}
