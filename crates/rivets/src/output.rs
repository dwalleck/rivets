//! Output formatting for CLI commands.
//!
//! This module provides utilities for formatting command output in both
//! human-readable text format and JSON format for programmatic use.

use crate::domain::{Dependency, Issue, IssueStatus, IssueType};
use colored::Colorize;
use serde::Serialize;
use std::io::{self, Write};

// ============================================================================
// Terminal Width Detection
// ============================================================================

const DEFAULT_TERMINAL_WIDTH: u16 = 80;

/// Get the current terminal width, falling back to default if detection fails.
fn get_terminal_width() -> usize {
    terminal_size::terminal_size()
        .map(|(w, _)| w.0 as usize)
        .unwrap_or(DEFAULT_TERMINAL_WIDTH as usize)
}

// ============================================================================
// Color Helpers
// ============================================================================

/// Apply color to status text based on issue status.
fn colorize_status(status: IssueStatus) -> String {
    let text = format!("{status}");
    match status {
        IssueStatus::Open => text.white().to_string(),
        IssueStatus::InProgress => text.yellow().to_string(),
        IssueStatus::Blocked => text.red().to_string(),
        IssueStatus::Closed => text.green().to_string(),
    }
}

/// Apply color to priority text based on priority level.
fn colorize_priority(priority: u8) -> String {
    let text = format!("P{priority}");
    match priority {
        0 => text.red().bold().to_string(),
        1 => text.yellow().to_string(),
        _ => text.to_string(),
    }
}

/// Colorize an issue ID (cyan).
fn colorize_id(id: &str) -> String {
    id.cyan().to_string()
}

/// Colorize labels (magenta).
fn colorize_labels(labels: &[String]) -> String {
    if labels.is_empty() {
        String::new()
    } else {
        labels.join(", ").magenta().to_string()
    }
}

/// Get a colored status icon.
fn colored_status_icon(status: IssueStatus) -> String {
    match status {
        IssueStatus::Open => "○".white().to_string(),
        IssueStatus::InProgress => "▶".yellow().to_string(),
        IssueStatus::Blocked => "✗".red().to_string(),
        IssueStatus::Closed => "✓".green().to_string(),
    }
}

/// Get a type icon for issue types.
fn type_icon(issue_type: IssueType) -> &'static str {
    match issue_type {
        IssueType::Task => "◇",
        IssueType::Bug => "●",
        IssueType::Feature => "★",
        IssueType::Epic => "◆",
        IssueType::Chore => "○",
    }
}

/// Output format mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    /// Human-readable text format
    Text,
    /// JSON format for programmatic use
    Json,
}

/// Print an issue in the specified format
pub fn print_issue(issue: &Issue, mode: OutputMode) -> io::Result<()> {
    let stdout = io::stdout();
    let mut handle = stdout.lock();

    match mode {
        OutputMode::Text => print_issue_text(&mut handle, issue),
        OutputMode::Json => print_issue_json(&mut handle, issue),
    }
}

/// Print a list of issues in the specified format
pub fn print_issues(issues: &[Issue], mode: OutputMode) -> io::Result<()> {
    let stdout = io::stdout();
    let mut handle = stdout.lock();

    match mode {
        OutputMode::Text => print_issues_text(&mut handle, issues),
        OutputMode::Json => print_issues_json(&mut handle, issues),
    }
}

/// Print an issue with full details (for show command)
pub fn print_issue_details(
    issue: &Issue,
    deps: &[Dependency],
    dependents: &[Dependency],
    mode: OutputMode,
) -> io::Result<()> {
    let stdout = io::stdout();
    let mut handle = stdout.lock();

    match mode {
        OutputMode::Text => print_issue_details_text(&mut handle, issue, deps, dependents),
        OutputMode::Json => print_issue_details_json(&mut handle, issue, deps, dependents),
    }
}

/// Print blocked issues with their blockers
pub fn print_blocked_issues(blocked: &[(Issue, Vec<Issue>)], mode: OutputMode) -> io::Result<()> {
    let stdout = io::stdout();
    let mut handle = stdout.lock();

    match mode {
        OutputMode::Text => print_blocked_text(&mut handle, blocked),
        OutputMode::Json => print_blocked_json(&mut handle, blocked),
    }
}

/// Print a simple message
pub fn print_message(msg: &str) -> io::Result<()> {
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    writeln!(handle, "{}", msg)
}

/// Print a JSON-formatted result for any serializable value
pub fn print_json<T: Serialize>(value: &T) -> io::Result<()> {
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    let json = serde_json::to_string_pretty(value)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    writeln!(handle, "{}", json)
}

// ============================================================================
// Text Formatting
// ============================================================================

fn print_issue_text<W: Write>(w: &mut W, issue: &Issue) -> io::Result<()> {
    writeln!(
        w,
        "{} {} {} {} {}",
        colored_status_icon(issue.status),
        colorize_id(issue.id.as_str()),
        type_icon(issue.issue_type),
        colorize_priority(issue.priority),
        issue.title
    )?;

    if let Some(ref assignee) = issue.assignee {
        writeln!(w, "  {} {}", "Assignee:".dimmed(), assignee)?;
    }

    if !issue.labels.is_empty() {
        writeln!(
            w,
            "  {} {}",
            "Labels:".dimmed(),
            colorize_labels(&issue.labels)
        )?;
    }

    Ok(())
}

fn print_issues_text<W: Write>(w: &mut W, issues: &[Issue]) -> io::Result<()> {
    if issues.is_empty() {
        writeln!(w, "No issues found.")?;
        return Ok(());
    }

    writeln!(w, "Found {} issue(s):", issues.len())?;
    writeln!(w)?;

    for issue in issues {
        writeln!(
            w,
            "{} {}  {}  {}  {}",
            colored_status_icon(issue.status),
            colorize_id(issue.id.as_str()),
            type_icon(issue.issue_type),
            colorize_priority(issue.priority),
            issue.title
        )?;
    }

    Ok(())
}

fn print_issue_details_text<W: Write>(
    w: &mut W,
    issue: &Issue,
    deps: &[Dependency],
    dependents: &[Dependency],
) -> io::Result<()> {
    let terminal_width = get_terminal_width();
    let content_width = terminal_width.min(80);

    // Header: status icon, ID, and title
    writeln!(
        w,
        "{} {}: {}",
        colored_status_icon(issue.status),
        colorize_id(issue.id.as_str()),
        issue.title
    )?;

    // Metadata line
    let type_display = format!("{} {}", type_icon(issue.issue_type), issue.issue_type);
    writeln!(
        w,
        "{}  {}    {}  {}    {}  {}",
        "Type:".dimmed(),
        type_display,
        "Status:".dimmed(),
        colorize_status(issue.status),
        "Priority:".dimmed(),
        colorize_priority(issue.priority)
    )?;

    // Optional fields
    if let Some(ref assignee) = issue.assignee {
        writeln!(w, "{} {}", "Assignee:".dimmed(), assignee)?;
    }

    if !issue.labels.is_empty() {
        writeln!(
            w,
            "{} {}",
            "Labels:".dimmed(),
            colorize_labels(&issue.labels)
        )?;
    }

    if let Some(ref ext_ref) = issue.external_ref {
        writeln!(w, "{} {}", "Ref:".dimmed(), ext_ref)?;
    }

    // Timestamps
    writeln!(
        w,
        "{} {}    {} {}",
        "Created:".dimmed(),
        issue.created_at.format("%Y-%m-%d %H:%M"),
        "Updated:".dimmed(),
        issue.updated_at.format("%Y-%m-%d %H:%M")
    )?;

    if let Some(closed_at) = issue.closed_at {
        writeln!(
            w,
            "{} {}",
            "Closed:".dimmed(),
            closed_at.format("%Y-%m-%d %H:%M")
        )?;
    }

    // Description section
    if !issue.description.is_empty() {
        writeln!(w)?;
        writeln!(w, "{}:", "Description".bold())?;
        for line in wrap_text(&issue.description, content_width.saturating_sub(2)) {
            writeln!(w, "  {line}")?;
        }
    }

    // Design Notes section
    if let Some(ref design) = issue.design {
        writeln!(w)?;
        writeln!(w, "{}:", "Design Notes".bold())?;
        for line in wrap_text(design, content_width.saturating_sub(2)) {
            writeln!(w, "  {line}")?;
        }
    }

    // Acceptance Criteria section
    if let Some(ref acceptance) = issue.acceptance_criteria {
        writeln!(w)?;
        writeln!(w, "{}:", "Acceptance Criteria".bold())?;
        for line in wrap_text(acceptance, content_width.saturating_sub(2)) {
            writeln!(w, "  {line}")?;
        }
    }

    // Notes section
    if let Some(ref notes) = issue.notes {
        writeln!(w)?;
        writeln!(w, "{}:", "Notes".bold())?;
        for line in wrap_text(notes, content_width.saturating_sub(2)) {
            writeln!(w, "  {line}")?;
        }
    }

    // Dependencies section
    if !deps.is_empty() {
        writeln!(w)?;
        writeln!(w, "{} ({}):", "Dependencies".bold(), deps.len())?;
        for dep in deps {
            writeln!(
                w,
                "  {} {} ({})",
                "→".cyan(),
                colorize_id(dep.depends_on_id.as_str()),
                dep.dep_type
            )?;
        }
    }

    // Dependents section
    if !dependents.is_empty() {
        writeln!(w)?;
        writeln!(w, "{} ({}):", "Dependents".bold(), dependents.len())?;
        for dep in dependents {
            writeln!(
                w,
                "  {} {} ({})",
                "←".yellow(),
                colorize_id(dep.depends_on_id.as_str()),
                dep.dep_type
            )?;
        }
    }

    Ok(())
}

/// Wrap text to fit within a given width, preserving existing line breaks.
fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for line in text.lines() {
        if line.len() <= max_width {
            lines.push(line.to_string());
        } else {
            // Simple word-based wrapping
            let mut current_line = String::new();
            for word in line.split_whitespace() {
                if current_line.is_empty() {
                    current_line = word.to_string();
                } else if current_line.len() + 1 + word.len() <= max_width {
                    current_line.push(' ');
                    current_line.push_str(word);
                } else {
                    lines.push(current_line);
                    current_line = word.to_string();
                }
            }
            if !current_line.is_empty() {
                lines.push(current_line);
            }
        }
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn print_blocked_text<W: Write>(w: &mut W, blocked: &[(Issue, Vec<Issue>)]) -> io::Result<()> {
    if blocked.is_empty() {
        writeln!(w, "No blocked issues found.")?;
        return Ok(());
    }

    writeln!(w, "Found {} blocked issue(s):", blocked.len())?;
    writeln!(w)?;

    for (issue, blockers) in blocked {
        writeln!(
            w,
            "{} {}  {}  {}  {}",
            colored_status_icon(issue.status),
            colorize_id(issue.id.as_str()),
            type_icon(issue.issue_type),
            colorize_priority(issue.priority),
            issue.title
        )?;

        let blocked_by: Vec<String> = blockers
            .iter()
            .map(|b| {
                format!(
                    "{} ({})",
                    colorize_id(b.id.as_str()),
                    colorize_status(b.status)
                )
            })
            .collect();
        writeln!(w, "  {} {}", "Blocked by:".dimmed(), blocked_by.join(", "))?;
    }

    Ok(())
}

// ============================================================================
// JSON Formatting
// ============================================================================

fn print_issue_json<W: Write>(w: &mut W, issue: &Issue) -> io::Result<()> {
    let json = serde_json::to_string_pretty(issue)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    writeln!(w, "{}", json)
}

fn print_issues_json<W: Write>(w: &mut W, issues: &[Issue]) -> io::Result<()> {
    let json = serde_json::to_string_pretty(issues)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    writeln!(w, "{}", json)
}

#[derive(Serialize)]
struct IssueDetails<'a> {
    #[serde(flatten)]
    issue: &'a Issue,
    dependency_details: Vec<&'a Dependency>,
    dependent_details: Vec<&'a Dependency>,
}

fn print_issue_details_json<W: Write>(
    w: &mut W,
    issue: &Issue,
    deps: &[Dependency],
    dependents: &[Dependency],
) -> io::Result<()> {
    let details = IssueDetails {
        issue,
        dependency_details: deps.iter().collect(),
        dependent_details: dependents.iter().collect(),
    };

    let json = serde_json::to_string_pretty(&details)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    writeln!(w, "{}", json)
}

#[derive(Serialize)]
struct BlockedIssue<'a> {
    issue: &'a Issue,
    blocked_by: Vec<&'a Issue>,
}

fn print_blocked_json<W: Write>(w: &mut W, blocked: &[(Issue, Vec<Issue>)]) -> io::Result<()> {
    let items: Vec<BlockedIssue> = blocked
        .iter()
        .map(|(issue, blockers)| BlockedIssue {
            issue,
            blocked_by: blockers.iter().collect(),
        })
        .collect();

    let json = serde_json::to_string_pretty(&items)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    writeln!(w, "{}", json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{DependencyType, IssueId, IssueType};
    use chrono::Utc;

    fn test_issue() -> Issue {
        Issue {
            id: IssueId::new("test-abc"),
            title: "Test Issue".to_string(),
            description: "A test description".to_string(),
            status: IssueStatus::Open,
            priority: 1,
            issue_type: IssueType::Task,
            assignee: Some("alice".to_string()),
            labels: vec!["urgent".to_string()],
            design: None,
            acceptance_criteria: None,
            notes: None,
            external_ref: None,
            dependencies: vec![],
            created_at: Utc::now(),
            updated_at: Utc::now(),
            closed_at: None,
        }
    }

    #[test]
    fn test_wrap_text() {
        let text = "This is a test of text wrapping functionality";
        let wrapped = wrap_text(text, 20);
        assert!(!wrapped.is_empty());
        for line in &wrapped {
            assert!(line.len() <= 20 || !line.contains(' '));
        }
    }

    #[test]
    fn test_wrap_text_preserves_newlines() {
        let text = "Line one\nLine two\nLine three";
        let wrapped = wrap_text(text, 50);
        assert_eq!(wrapped.len(), 3);
    }

    #[test]
    fn test_colorize_status() {
        // Just verify they produce non-empty output
        assert!(!colorize_status(IssueStatus::Open).is_empty());
        assert!(!colorize_status(IssueStatus::InProgress).is_empty());
        assert!(!colorize_status(IssueStatus::Blocked).is_empty());
        assert!(!colorize_status(IssueStatus::Closed).is_empty());
    }

    #[test]
    fn test_colorize_priority() {
        assert!(colorize_priority(0).contains("P0"));
        assert!(colorize_priority(1).contains("P1"));
        assert!(colorize_priority(2).contains("P2"));
    }

    #[test]
    fn test_type_icon() {
        assert_eq!(type_icon(IssueType::Task), "◇");
        assert_eq!(type_icon(IssueType::Bug), "●");
        assert_eq!(type_icon(IssueType::Feature), "★");
        assert_eq!(type_icon(IssueType::Epic), "◆");
    }

    #[test]
    fn test_print_issue_text() {
        let issue = test_issue();
        let mut buffer = Vec::new();

        print_issue_text(&mut buffer, &issue).unwrap();

        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("test-abc"));
        assert!(output.contains("Test Issue"));
        assert!(output.contains("P1"));
        assert!(output.contains("alice"));
    }

    #[test]
    fn test_print_issue_json() {
        let issue = test_issue();
        let mut buffer = Vec::new();

        print_issue_json(&mut buffer, &issue).unwrap();

        let output = String::from_utf8(buffer).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["id"], "test-abc");
        assert_eq!(parsed["title"], "Test Issue");
    }

    #[test]
    fn test_print_issue_details_text() {
        let issue = test_issue();
        let deps = vec![Dependency {
            depends_on_id: IssueId::new("test-xyz"),
            dep_type: DependencyType::Blocks,
        }];
        let dependents = vec![];

        let mut buffer = Vec::new();
        print_issue_details_text(&mut buffer, &issue, &deps, &dependents).unwrap();

        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("test-abc"));
        assert!(output.contains("Dependencies"));
        assert!(output.contains("test-xyz"));
        assert!(output.contains("blocks"));
    }

    #[test]
    fn test_print_issues_table() {
        let issues = vec![test_issue()];
        let mut buffer = Vec::new();

        print_issues_text(&mut buffer, &issues).unwrap();

        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("Found 1 issue"));
        assert!(output.contains("test-abc"));
    }
}
