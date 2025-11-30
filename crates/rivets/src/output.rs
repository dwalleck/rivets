//! Output formatting for CLI commands.
//!
//! This module provides utilities for formatting command output in both
//! human-readable text format and JSON format for programmatic use.

use crate::domain::{Dependency, Issue, IssueStatus};
use serde::Serialize;
use std::io::{self, Write};

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
        "{} {} [{}] P{} {}",
        status_icon(issue.status),
        issue.id,
        issue.issue_type,
        issue.priority,
        issue.title
    )?;

    if let Some(ref assignee) = issue.assignee {
        writeln!(w, "  Assignee: {}", assignee)?;
    }

    if !issue.labels.is_empty() {
        writeln!(w, "  Labels: {}", issue.labels.join(", "))?;
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
        print_issue_text(w, issue)?;
    }

    Ok(())
}

fn print_issue_details_text<W: Write>(
    w: &mut W,
    issue: &Issue,
    deps: &[Dependency],
    dependents: &[Dependency],
) -> io::Result<()> {
    writeln!(w, "{}", "=".repeat(60))?;
    writeln!(w, "{} {}", status_icon(issue.status), issue.id)?;
    writeln!(w, "{}", "=".repeat(60))?;
    writeln!(w)?;

    writeln!(w, "Title:    {}", issue.title)?;
    writeln!(w, "Type:     {}", issue.issue_type)?;
    writeln!(w, "Status:   {}", issue.status)?;
    writeln!(w, "Priority: P{}", issue.priority)?;

    if let Some(ref assignee) = issue.assignee {
        writeln!(w, "Assignee: {}", assignee)?;
    }

    if !issue.labels.is_empty() {
        writeln!(w, "Labels:   {}", issue.labels.join(", "))?;
    }

    if let Some(ref ext_ref) = issue.external_ref {
        writeln!(w, "Ref:      {}", ext_ref)?;
    }

    writeln!(w)?;
    writeln!(
        w,
        "Created:  {}",
        issue.created_at.format("%Y-%m-%d %H:%M:%S UTC")
    )?;
    writeln!(
        w,
        "Updated:  {}",
        issue.updated_at.format("%Y-%m-%d %H:%M:%S UTC")
    )?;

    if let Some(closed_at) = issue.closed_at {
        writeln!(w, "Closed:   {}", closed_at.format("%Y-%m-%d %H:%M:%S UTC"))?;
    }

    if !issue.description.is_empty() {
        writeln!(w)?;
        writeln!(w, "Description:")?;
        writeln!(w, "{}", indent_text(&issue.description, "  "))?;
    }

    if let Some(ref design) = issue.design {
        writeln!(w)?;
        writeln!(w, "Design Notes:")?;
        writeln!(w, "{}", indent_text(design, "  "))?;
    }

    if let Some(ref acceptance) = issue.acceptance_criteria {
        writeln!(w)?;
        writeln!(w, "Acceptance Criteria:")?;
        writeln!(w, "{}", indent_text(acceptance, "  "))?;
    }

    if let Some(ref notes) = issue.notes {
        writeln!(w)?;
        writeln!(w, "Notes:")?;
        writeln!(w, "{}", indent_text(notes, "  "))?;
    }

    if !deps.is_empty() {
        writeln!(w)?;
        writeln!(w, "Dependencies ({}):", deps.len())?;
        for dep in deps {
            writeln!(w, "  -> {} ({})", dep.depends_on_id, dep.dep_type)?;
        }
    }

    if !dependents.is_empty() {
        writeln!(w)?;
        writeln!(w, "Dependents ({}):", dependents.len())?;
        for dep in dependents {
            writeln!(w, "  <- {} ({})", dep.depends_on_id, dep.dep_type)?;
        }
    }

    writeln!(w)?;
    writeln!(w, "{}", "=".repeat(60))?;

    Ok(())
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
            "{} {} P{} {}",
            status_icon(issue.status),
            issue.id,
            issue.priority,
            issue.title
        )?;
        writeln!(w, "  Blocked by:")?;
        for blocker in blockers {
            writeln!(w, "    - {} ({})", blocker.id, blocker.status)?;
        }
        writeln!(w)?;
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

// ============================================================================
// Helpers
// ============================================================================

fn status_icon(status: IssueStatus) -> &'static str {
    match status {
        IssueStatus::Open => "[ ]",
        IssueStatus::InProgress => "[>]",
        IssueStatus::Blocked => "[X]",
        IssueStatus::Closed => "[+]",
    }
}

fn indent_text(text: &str, indent: &str) -> String {
    text.lines()
        .map(|line| format!("{}{}", indent, line))
        .collect::<Vec<_>>()
        .join("\n")
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
    fn test_status_icon() {
        assert_eq!(status_icon(IssueStatus::Open), "[ ]");
        assert_eq!(status_icon(IssueStatus::InProgress), "[>]");
        assert_eq!(status_icon(IssueStatus::Blocked), "[X]");
        assert_eq!(status_icon(IssueStatus::Closed), "[+]");
    }

    #[test]
    fn test_indent_text() {
        let text = "line 1\nline 2\nline 3";
        let indented = indent_text(text, "  ");
        assert_eq!(indented, "  line 1\n  line 2\n  line 3");
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
        assert!(output.contains("Dependencies (1)"));
        assert!(output.contains("test-xyz"));
        assert!(output.contains("blocks"));
    }
}
