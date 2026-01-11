//! Output formatting for CLI commands.
//!
//! This module provides utilities for formatting command output in both
//! human-readable text format and JSON format for programmatic use.

use crate::domain::{Dependency, Issue, IssueStatus, IssueType};
use colored::Colorize;
use serde::Serialize;
use std::env;
use std::io::{self, Write};

// ============================================================================
// Output Configuration
// ============================================================================

const DEFAULT_TERMINAL_WIDTH: u16 = 80;
const DEFAULT_MAX_CONTENT_WIDTH: usize = 80;

/// Configuration for output formatting.
///
/// This struct holds settings that control how output is formatted,
/// including terminal width limits and ASCII fallback mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputConfig {
    /// Maximum content width for text wrapping.
    pub max_width: usize,
    /// Whether to use ASCII-only icons instead of Unicode.
    pub use_ascii: bool,
}

impl OutputConfig {
    /// Create a new OutputConfig with explicit values.
    pub fn new(max_width: usize, use_ascii: bool) -> Self {
        Self {
            max_width,
            use_ascii,
        }
    }

    /// Create an OutputConfig by reading from environment variables.
    ///
    /// Reads:
    /// - `RIVETS_MAX_WIDTH`: Maximum content width (default: 80)
    /// - `RIVETS_ASCII`: Set to "1" or "true" for ASCII-only icons (default: false)
    pub fn from_env() -> Self {
        let max_width = env::var("RIVETS_MAX_WIDTH")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_MAX_CONTENT_WIDTH);

        let use_ascii = env::var("RIVETS_ASCII")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        Self {
            max_width,
            use_ascii,
        }
    }
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            max_width: DEFAULT_MAX_CONTENT_WIDTH,
            use_ascii: false,
        }
    }
}

// ============================================================================
// Terminal Width Detection
// ============================================================================

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

/// Get a colored status icon, with ASCII fallback support.
fn colored_status_icon(status: IssueStatus, config: &OutputConfig) -> String {
    if config.use_ascii {
        match status {
            IssueStatus::Open => "o".white().to_string(),
            IssueStatus::InProgress => ">".yellow().to_string(),
            IssueStatus::Blocked => "x".red().to_string(),
            IssueStatus::Closed => "+".green().to_string(),
        }
    } else {
        match status {
            IssueStatus::Open => "○".white().to_string(),
            IssueStatus::InProgress => "▶".yellow().to_string(),
            IssueStatus::Blocked => "✗".red().to_string(),
            IssueStatus::Closed => "✓".green().to_string(),
        }
    }
}

/// Get a type icon for issue types, with ASCII fallback support.
fn type_icon(issue_type: IssueType, config: &OutputConfig) -> &'static str {
    if config.use_ascii {
        match issue_type {
            IssueType::Task => "-",
            IssueType::Bug => "*",
            IssueType::Feature => "+",
            IssueType::Epic => "#",
            IssueType::Chore => ".",
        }
    } else {
        match issue_type {
            IssueType::Task => "◇",
            IssueType::Bug => "●",
            IssueType::Feature => "★",
            IssueType::Epic => "◆",
            IssueType::Chore => "○",
        }
    }
}

// ============================================================================
// Section Printing Helpers
// ============================================================================

/// Print a text section with a bold title and wrapped, indented content.
fn print_text_section<W: Write>(
    w: &mut W,
    title: &str,
    content: &str,
    width: usize,
    config: &OutputConfig,
) -> io::Result<()> {
    if content.is_empty() {
        return Ok(());
    }
    writeln!(w)?;
    if config.use_ascii {
        writeln!(w, "{}:", title)?;
    } else {
        writeln!(w, "{}:", title.bold())?;
    }
    for line in wrap_text(content, width.saturating_sub(2)) {
        writeln!(w, "  {line}")?;
    }
    Ok(())
}

/// Print an optional text section (only if Some and non-empty).
fn print_optional_section<W: Write>(
    w: &mut W,
    title: &str,
    content: &Option<String>,
    width: usize,
    config: &OutputConfig,
) -> io::Result<()> {
    if let Some(text) = content {
        print_text_section(w, title, text, width, config)?;
    }
    Ok(())
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
    let config = OutputConfig::from_env();

    match mode {
        OutputMode::Text => print_issue_text(&mut handle, issue, &config),
        OutputMode::Json => print_issue_json(&mut handle, issue),
    }
}

/// Print a list of issues in the specified format
pub fn print_issues(issues: &[Issue], mode: OutputMode) -> io::Result<()> {
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    let config = OutputConfig::from_env();

    match mode {
        OutputMode::Text => print_issues_text(&mut handle, issues, &config),
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
    let config = OutputConfig::from_env();

    match mode {
        OutputMode::Text => print_issue_details_text(&mut handle, issue, deps, dependents, &config),
        OutputMode::Json => print_issue_details_json(&mut handle, issue, deps, dependents),
    }
}

/// Print blocked issues with their blockers
pub fn print_blocked_issues(blocked: &[(Issue, Vec<Issue>)], mode: OutputMode) -> io::Result<()> {
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    let config = OutputConfig::from_env();

    match mode {
        OutputMode::Text => print_blocked_text(&mut handle, blocked, &config),
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

fn print_issue_text<W: Write>(w: &mut W, issue: &Issue, config: &OutputConfig) -> io::Result<()> {
    writeln!(
        w,
        "{} {} {} {} {}",
        colored_status_icon(issue.status, config),
        colorize_id(issue.id.as_str()),
        type_icon(issue.issue_type, config),
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

fn print_issues_text<W: Write>(
    w: &mut W,
    issues: &[Issue],
    config: &OutputConfig,
) -> io::Result<()> {
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
            colored_status_icon(issue.status, config),
            colorize_id(issue.id.as_str()),
            type_icon(issue.issue_type, config),
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
    config: &OutputConfig,
) -> io::Result<()> {
    let terminal_width = get_terminal_width();
    let content_width = terminal_width.min(config.max_width);

    // Header: status icon, ID, and title
    writeln!(
        w,
        "{} {}: {}",
        colored_status_icon(issue.status, config),
        colorize_id(issue.id.as_str()),
        issue.title
    )?;

    // Metadata line
    let type_display = format!(
        "{} {}",
        type_icon(issue.issue_type, config),
        issue.issue_type
    );
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

    // Long-form content sections
    print_text_section(w, "Description", &issue.description, content_width, config)?;
    print_optional_section(w, "Design Notes", &issue.design, content_width, config)?;
    print_optional_section(
        w,
        "Acceptance Criteria",
        &issue.acceptance_criteria,
        content_width,
        config,
    )?;
    print_optional_section(w, "Notes", &issue.notes, content_width, config)?;

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
/// Uses textwrap to handle edge cases like long words (URLs, file paths).
fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    text.lines()
        .flat_map(|line| {
            if line.trim().is_empty() {
                vec![String::new()]
            } else {
                textwrap::wrap(line, max_width)
                    .into_iter()
                    .map(|s| s.into_owned())
                    .collect()
            }
        })
        .collect()
}

fn print_blocked_text<W: Write>(
    w: &mut W,
    blocked: &[(Issue, Vec<Issue>)],
    config: &OutputConfig,
) -> io::Result<()> {
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
            colored_status_icon(issue.status, config),
            colorize_id(issue.id.as_str()),
            type_icon(issue.issue_type, config),
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
    use colored::control::set_override;
    use std::env;
    use std::sync::Mutex;

    // Mutex to prevent color tests from running in parallel (set_override is global state)
    static COLOR_TEST_MUTEX: Mutex<()> = Mutex::new(());

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
            assert!(
                line.len() <= 20,
                "Line too long: '{}' ({} chars)",
                line,
                line.len()
            );
        }
    }

    #[test]
    fn test_wrap_text_preserves_newlines() {
        let text = "Line one\nLine two\nLine three";
        let wrapped = wrap_text(text, 50);
        assert_eq!(wrapped.len(), 3);
    }

    #[test]
    fn test_wrap_text_handles_long_words() {
        // Long URL that exceeds max_width
        let text = "Check out https://example.com/very/long/path/to/resource for details";
        let wrapped = wrap_text(text, 30);
        assert!(!wrapped.is_empty());
        // textwrap will break long words to fit
        for line in &wrapped {
            assert!(
                line.len() <= 30,
                "Line too long: '{}' ({} chars)",
                line,
                line.len()
            );
        }
    }

    #[test]
    fn test_colorize_status_contains_ansi_codes() {
        // Lock mutex to prevent race conditions with other color tests
        let _guard = COLOR_TEST_MUTEX.lock().unwrap();
        set_override(true);

        let open = colorize_status(IssueStatus::Open);
        let in_progress = colorize_status(IssueStatus::InProgress);
        let blocked = colorize_status(IssueStatus::Blocked);
        let closed = colorize_status(IssueStatus::Closed);

        // All should contain the status text
        assert!(open.contains("open"));
        assert!(in_progress.contains("in_progress"));
        assert!(blocked.contains("blocked"));
        assert!(closed.contains("closed"));

        // All should contain ANSI escape codes (\x1b[)
        assert!(open.contains("\x1b["), "Open status should have ANSI codes");
        assert!(
            in_progress.contains("\x1b["),
            "InProgress status should have ANSI codes"
        );
        assert!(
            blocked.contains("\x1b["),
            "Blocked status should have ANSI codes"
        );
        assert!(
            closed.contains("\x1b["),
            "Closed status should have ANSI codes"
        );

        set_override(false);
    }

    #[test]
    fn test_colorize_priority_contains_ansi_codes() {
        let _guard = COLOR_TEST_MUTEX.lock().unwrap();
        set_override(true);

        let p0 = colorize_priority(0);
        let p1 = colorize_priority(1);
        let p2 = colorize_priority(2);

        // Verify priority text is present
        assert!(p0.contains("P0"));
        assert!(p1.contains("P1"));
        assert!(p2.contains("P2"));

        // P0 (bold+red) and P1 (yellow) should have ANSI codes
        assert!(p0.contains("\x1b["), "P0 should have ANSI codes");
        assert!(p1.contains("\x1b["), "P1 should have ANSI codes");
        // P2 and higher have no color styling
        assert!(!p2.contains("\x1b["), "P2 should not have ANSI codes");

        set_override(false);
    }

    #[test]
    fn test_colorize_id_contains_ansi_codes() {
        let _guard = COLOR_TEST_MUTEX.lock().unwrap();
        set_override(true);

        let id = colorize_id("test-123");
        assert!(id.contains("test-123"));
        // Cyan color adds ANSI codes
        assert!(id.contains("\x1b["), "ID should have ANSI codes");

        set_override(false);
    }

    #[test]
    fn test_type_icon() {
        let config = OutputConfig::default();
        // Test all issue types including Chore (Unicode mode)
        assert_eq!(type_icon(IssueType::Task, &config), "◇");
        assert_eq!(type_icon(IssueType::Bug, &config), "●");
        assert_eq!(type_icon(IssueType::Feature, &config), "★");
        assert_eq!(type_icon(IssueType::Epic, &config), "◆");
        assert_eq!(type_icon(IssueType::Chore, &config), "○");
    }

    #[test]
    fn test_ascii_fallback_icons() {
        // Test ASCII mode using explicit config (no env var needed)
        let config = OutputConfig::new(80, true);

        assert_eq!(type_icon(IssueType::Task, &config), "-");
        assert_eq!(type_icon(IssueType::Bug, &config), "*");
        assert_eq!(type_icon(IssueType::Feature, &config), "+");
        assert_eq!(type_icon(IssueType::Epic, &config), "#");
        assert_eq!(type_icon(IssueType::Chore, &config), ".");

        // Status icons should also be ASCII
        let open = colored_status_icon(IssueStatus::Open, &config);
        let closed = colored_status_icon(IssueStatus::Closed, &config);
        assert!(open.contains("o"));
        assert!(closed.contains("+"));
    }

    #[test]
    fn test_output_config_from_env() {
        // Lock mutex since we read global env state
        let _guard = COLOR_TEST_MUTEX.lock().unwrap();

        // Test that OutputConfig::from_env respects env vars
        env::set_var("RIVETS_MAX_WIDTH", "120");
        env::set_var("RIVETS_ASCII", "1");
        let config = OutputConfig::from_env();
        assert_eq!(config.max_width, 120);
        assert!(config.use_ascii);

        env::set_var("RIVETS_MAX_WIDTH", "invalid");
        env::set_var("RIVETS_ASCII", "false");
        let config = OutputConfig::from_env();
        assert_eq!(config.max_width, DEFAULT_MAX_CONTENT_WIDTH);
        assert!(!config.use_ascii);

        env::remove_var("RIVETS_MAX_WIDTH");
        env::remove_var("RIVETS_ASCII");
        let config = OutputConfig::from_env();
        assert_eq!(config.max_width, DEFAULT_MAX_CONTENT_WIDTH);
        assert!(!config.use_ascii);
    }

    #[test]
    fn test_print_issue_text() {
        let issue = test_issue();
        let config = OutputConfig::default();
        let mut buffer = Vec::new();

        print_issue_text(&mut buffer, &issue, &config).unwrap();

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
        let config = OutputConfig::default();
        let deps = vec![Dependency {
            depends_on_id: IssueId::new("test-xyz"),
            dep_type: DependencyType::Blocks,
        }];
        let dependents = vec![];

        let mut buffer = Vec::new();
        print_issue_details_text(&mut buffer, &issue, &deps, &dependents, &config).unwrap();

        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("test-abc"));
        assert!(output.contains("Dependencies"));
        assert!(output.contains("test-xyz"));
        assert!(output.contains("blocks"));
    }

    #[test]
    fn test_print_issues_list_format() {
        let issues = vec![test_issue()];
        let config = OutputConfig::default();
        let mut buffer = Vec::new();

        print_issues_text(&mut buffer, &issues, &config).unwrap();

        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("Found 1 issue"));
        assert!(output.contains("test-abc"));
    }

    #[test]
    fn test_print_text_section_skips_empty_content() {
        let config = OutputConfig::default();
        let mut buffer = Vec::new();

        // Empty content should produce no output
        print_text_section(&mut buffer, "Description", "", 80, &config).unwrap();
        assert!(buffer.is_empty(), "Empty content should produce no output");

        // Non-empty content should produce output
        print_text_section(&mut buffer, "Description", "Some text", 80, &config).unwrap();
        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("Description:"));
        assert!(output.contains("Some text"));
    }

    #[test]
    fn test_print_optional_section_handles_none() {
        let config = OutputConfig::default();
        let mut buffer = Vec::new();

        // None should produce no output
        print_optional_section(&mut buffer, "Notes", &None, 80, &config).unwrap();
        assert!(buffer.is_empty(), "None should produce no output");

        // Some with empty string should also produce no output
        let empty: Option<String> = Some(String::new());
        print_optional_section(&mut buffer, "Notes", &empty, 80, &config).unwrap();
        assert!(buffer.is_empty(), "Empty Some should produce no output");

        // Some with content should produce output
        let content: Option<String> = Some("Important note".to_string());
        print_optional_section(&mut buffer, "Notes", &content, 80, &config).unwrap();
        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("Notes:"));
        assert!(output.contains("Important note"));
    }

    #[test]
    fn test_issue_with_empty_description() {
        let mut issue = test_issue();
        issue.description = String::new();
        let config = OutputConfig::default();

        let mut buffer = Vec::new();
        print_issue_details_text(&mut buffer, &issue, &[], &[], &config).unwrap();

        let output = String::from_utf8(buffer).unwrap();
        // Should not contain "Description:" section when empty
        assert!(
            !output.contains("Description:"),
            "Empty description should not show Description section"
        );
    }

    #[test]
    fn test_wrap_text_with_narrow_width() {
        // Edge case: very narrow width
        let text = "Hello world";
        let wrapped = wrap_text(text, 5);
        assert!(!wrapped.is_empty());
        for line in &wrapped {
            assert!(line.len() <= 5, "Line '{}' exceeds width 5", line);
        }
    }

    #[test]
    fn test_wrap_text_with_wide_width() {
        // Edge case: width wider than content
        let text = "Short";
        let wrapped = wrap_text(text, 100);
        assert_eq!(wrapped.len(), 1);
        assert_eq!(wrapped[0], "Short");
    }

    #[test]
    fn test_wrap_text_empty_input() {
        let wrapped = wrap_text("", 80);
        // Empty string has no lines, so result is empty
        assert!(wrapped.is_empty() || (wrapped.len() == 1 && wrapped[0].is_empty()));
    }
}
