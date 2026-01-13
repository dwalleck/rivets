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
/// including terminal width limits, ASCII fallback mode, and color output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputConfig {
    /// Maximum content width for text wrapping.
    pub max_width: usize,
    /// Whether to use ASCII-only icons instead of Unicode.
    pub use_ascii: bool,
    /// Whether to use colors in output.
    pub use_colors: bool,
}

impl OutputConfig {
    /// Create a new OutputConfig with explicit values.
    pub fn new(max_width: usize, use_ascii: bool, use_colors: bool) -> Self {
        Self {
            max_width,
            use_ascii,
            use_colors,
        }
    }

    /// Create an OutputConfig by reading from environment variables.
    ///
    /// Reads:
    /// - `RIVETS_MAX_WIDTH`: Maximum content width (default: 80)
    /// - `RIVETS_ASCII`: Set to "1" or "true" for ASCII-only icons (default: false)
    /// - `NO_COLOR`: Standard env var to disable colors (any value disables colors)
    /// - `RIVETS_COLOR`: Set to "0" or "false" to disable colors (default: true)
    pub fn from_env() -> Self {
        let max_width = match env::var("RIVETS_MAX_WIDTH") {
            Ok(s) if !s.is_empty() => match s.parse() {
                Ok(width) => width,
                Err(_) => {
                    tracing::warn!(
                        env_var = "RIVETS_MAX_WIDTH",
                        value = %s,
                        default = DEFAULT_MAX_CONTENT_WIDTH,
                        "Invalid value, using default"
                    );
                    DEFAULT_MAX_CONTENT_WIDTH
                }
            },
            _ => DEFAULT_MAX_CONTENT_WIDTH,
        };

        let use_ascii = match env::var("RIVETS_ASCII") {
            Ok(v) if v == "1" || v.eq_ignore_ascii_case("true") => true,
            Ok(v) if v == "0" || v.eq_ignore_ascii_case("false") || v.is_empty() => false,
            Ok(v) => {
                tracing::warn!(
                    env_var = "RIVETS_ASCII",
                    value = %v,
                    "Invalid value (expected '1', 'true', '0', or 'false'), using default"
                );
                false
            }
            Err(_) => false,
        };

        // Respect NO_COLOR standard (https://no-color.org/)
        // Also support RIVETS_COLOR for explicit control
        let use_colors = env::var("NO_COLOR").is_err()
            && env::var("RIVETS_COLOR")
                .map(|v| v != "0" && !v.eq_ignore_ascii_case("false"))
                .unwrap_or(true);

        Self {
            max_width,
            use_ascii,
            use_colors,
        }
    }
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            max_width: DEFAULT_MAX_CONTENT_WIDTH,
            use_ascii: false,
            use_colors: true,
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
fn colorize_status(status: IssueStatus, config: &OutputConfig) -> String {
    let text = format!("{status}");
    if !config.use_colors {
        return text;
    }
    match status {
        IssueStatus::Open => text.white().to_string(),
        IssueStatus::InProgress => text.yellow().to_string(),
        IssueStatus::Blocked => text.red().to_string(),
        IssueStatus::Closed => text.green().to_string(),
    }
}

/// Apply color to priority text based on priority level.
fn colorize_priority(priority: u8, config: &OutputConfig) -> String {
    let text = format!("P{priority}");
    if !config.use_colors {
        return text;
    }
    match priority {
        0 => text.red().bold().to_string(),
        1 => text.yellow().to_string(),
        _ => text.to_string(),
    }
}

/// Colorize an issue ID (cyan).
fn colorize_id(id: &str, config: &OutputConfig) -> String {
    if !config.use_colors {
        return id.to_string();
    }
    id.cyan().to_string()
}

/// Colorize labels (magenta).
fn colorize_labels(labels: &[String], config: &OutputConfig) -> String {
    if labels.is_empty() {
        return String::new();
    }
    let text = labels.join(", ");
    if !config.use_colors {
        return text;
    }
    text.magenta().to_string()
}

/// Get a colored status icon, with ASCII fallback support.
fn colored_status_icon(status: IssueStatus, config: &OutputConfig) -> String {
    let icon = if config.use_ascii {
        match status {
            IssueStatus::Open => "o",
            IssueStatus::InProgress => ">",
            IssueStatus::Blocked => "x",
            IssueStatus::Closed => "+",
        }
    } else {
        match status {
            IssueStatus::Open => "○",
            IssueStatus::InProgress => "▶",
            IssueStatus::Blocked => "✗",
            IssueStatus::Closed => "✓",
        }
    };

    if !config.use_colors {
        return icon.to_string();
    }

    match status {
        IssueStatus::Open => icon.white().to_string(),
        IssueStatus::InProgress => icon.yellow().to_string(),
        IssueStatus::Blocked => icon.red().to_string(),
        IssueStatus::Closed => icon.green().to_string(),
    }
}

/// Apply dimmed style to text (for labels/field names).
fn dimmed(text: &str, config: &OutputConfig) -> String {
    if !config.use_colors {
        return text.to_string();
    }
    text.dimmed().to_string()
}

/// Apply bold style to text (for section headers).
fn bold(text: &str, config: &OutputConfig) -> String {
    if !config.use_colors {
        return text.to_string();
    }
    text.bold().to_string()
}

/// Apply cyan color to text (for arrows/connectors).
fn cyan(text: &str, config: &OutputConfig) -> String {
    if !config.use_colors {
        return text.to_string();
    }
    text.cyan().to_string()
}

/// Apply yellow color to text (for arrows/connectors).
fn yellow(text: &str, config: &OutputConfig) -> String {
    if !config.use_colors {
        return text.to_string();
    }
    text.yellow().to_string()
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
    if config.use_colors {
        writeln!(w, "{}:", title.bold())?;
    } else {
        writeln!(w, "{}:", title)?;
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
        colorize_id(issue.id.as_str(), config),
        type_icon(issue.issue_type, config),
        colorize_priority(issue.priority, config),
        issue.title
    )?;

    if let Some(ref assignee) = issue.assignee {
        writeln!(w, "  {} {}", dimmed("Assignee:", config), assignee)?;
    }

    if !issue.labels.is_empty() {
        writeln!(
            w,
            "  {} {}",
            dimmed("Labels:", config),
            colorize_labels(&issue.labels, config)
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
            colorize_id(issue.id.as_str(), config),
            type_icon(issue.issue_type, config),
            colorize_priority(issue.priority, config),
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
        colorize_id(issue.id.as_str(), config),
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
        dimmed("Type:", config),
        type_display,
        dimmed("Status:", config),
        colorize_status(issue.status, config),
        dimmed("Priority:", config),
        colorize_priority(issue.priority, config)
    )?;

    // Optional fields
    if let Some(ref assignee) = issue.assignee {
        writeln!(w, "{} {}", dimmed("Assignee:", config), assignee)?;
    }

    if !issue.labels.is_empty() {
        writeln!(
            w,
            "{} {}",
            dimmed("Labels:", config),
            colorize_labels(&issue.labels, config)
        )?;
    }

    if let Some(ref ext_ref) = issue.external_ref {
        writeln!(w, "{} {}", dimmed("Ref:", config), ext_ref)?;
    }

    // Timestamps
    writeln!(
        w,
        "{} {}    {} {}",
        dimmed("Created:", config),
        issue.created_at.format("%Y-%m-%d %H:%M"),
        dimmed("Updated:", config),
        issue.updated_at.format("%Y-%m-%d %H:%M")
    )?;

    if let Some(closed_at) = issue.closed_at {
        writeln!(
            w,
            "{} {}",
            dimmed("Closed:", config),
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
        writeln!(w, "{} ({}):", bold("Dependencies", config), deps.len())?;
        for dep in deps {
            writeln!(
                w,
                "  {} {} ({})",
                cyan("→", config),
                colorize_id(dep.depends_on_id.as_str(), config),
                dep.dep_type
            )?;
        }
    }

    // Dependents section
    if !dependents.is_empty() {
        writeln!(w)?;
        writeln!(w, "{} ({}):", bold("Dependents", config), dependents.len())?;
        for dep in dependents {
            writeln!(
                w,
                "  {} {} ({})",
                yellow("←", config),
                colorize_id(dep.depends_on_id.as_str(), config),
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
            colorize_id(issue.id.as_str(), config),
            type_icon(issue.issue_type, config),
            colorize_priority(issue.priority, config),
            issue.title
        )?;

        let blocked_by: Vec<String> = blockers
            .iter()
            .map(|b| {
                format!(
                    "{} ({})",
                    colorize_id(b.id.as_str(), config),
                    colorize_status(b.status, config)
                )
            })
            .collect();
        writeln!(
            w,
            "  {} {}",
            dimmed("Blocked by:", config),
            blocked_by.join(", ")
        )?;
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
    use std::sync::{Mutex, MutexGuard};

    // Mutex to protect global state in tests:
    // - colored crate's set_override() is process-global
    // - Environment variables are process-global
    // Tests modifying either must hold this mutex.
    static GLOBAL_STATE_MUTEX: Mutex<()> = Mutex::new(());

    /// RAII guard that enables colors via set_override and resets on drop.
    /// Holds the mutex guard to prevent parallel color tests.
    struct ColorGuard<'a> {
        _guard: MutexGuard<'a, ()>,
    }

    impl<'a> ColorGuard<'a> {
        fn new() -> Self {
            let guard = GLOBAL_STATE_MUTEX.lock().unwrap();
            set_override(true);
            Self { _guard: guard }
        }
    }

    impl Drop for ColorGuard<'_> {
        fn drop(&mut self) {
            set_override(false);
        }
    }

    /// Helper for tests that need colors enabled.
    /// Acquires mutex, enables colors, runs closure, then disables colors.
    fn with_colors_enabled<F, R>(f: F) -> R
    where
        F: FnOnce() -> R,
    {
        let _guard = ColorGuard::new();
        f()
    }

    /// Helper for tests that modify environment variables.
    /// Acquires mutex to prevent parallel env var modifications.
    fn with_env_lock<F, R>(f: F) -> R
    where
        F: FnOnce() -> R,
    {
        let _guard = GLOBAL_STATE_MUTEX.lock().unwrap();
        f()
    }

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
        with_colors_enabled(|| {
            let config = OutputConfig::new(80, false, true);
            let open = colorize_status(IssueStatus::Open, &config);
            let in_progress = colorize_status(IssueStatus::InProgress, &config);
            let blocked = colorize_status(IssueStatus::Blocked, &config);
            let closed = colorize_status(IssueStatus::Closed, &config);

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
        });
    }

    #[test]
    fn test_colorize_status_without_colors() {
        // Test that colors are disabled when use_colors is false
        let config = OutputConfig::new(80, false, false);
        let open = colorize_status(IssueStatus::Open, &config);
        let in_progress = colorize_status(IssueStatus::InProgress, &config);

        // Should contain status text but NO ANSI codes
        assert!(open.contains("open"));
        assert!(!open.contains("\x1b["), "Open should NOT have ANSI codes");
        assert!(in_progress.contains("in_progress"));
        assert!(
            !in_progress.contains("\x1b["),
            "InProgress should NOT have ANSI codes"
        );
    }

    #[test]
    fn test_colorize_priority_contains_ansi_codes() {
        with_colors_enabled(|| {
            let config = OutputConfig::new(80, false, true);
            let p0 = colorize_priority(0, &config);
            let p1 = colorize_priority(1, &config);
            let p2 = colorize_priority(2, &config);

            // Verify priority text is present
            assert!(p0.contains("P0"));
            assert!(p1.contains("P1"));
            assert!(p2.contains("P2"));

            // P0 (bold+red) and P1 (yellow) should have ANSI codes
            assert!(p0.contains("\x1b["), "P0 should have ANSI codes");
            assert!(p1.contains("\x1b["), "P1 should have ANSI codes");
            // P2 and higher have no color styling
            assert!(!p2.contains("\x1b["), "P2 should not have ANSI codes");
        });
    }

    #[test]
    fn test_colorize_priority_without_colors() {
        let config = OutputConfig::new(80, false, false);
        let p0 = colorize_priority(0, &config);
        let p1 = colorize_priority(1, &config);

        // Should contain priority text but NO ANSI codes
        assert!(p0.contains("P0"));
        assert!(!p0.contains("\x1b["), "P0 should NOT have ANSI codes");
        assert!(p1.contains("P1"));
        assert!(!p1.contains("\x1b["), "P1 should NOT have ANSI codes");
    }

    #[test]
    fn test_colorize_id_contains_ansi_codes() {
        with_colors_enabled(|| {
            let config = OutputConfig::new(80, false, true);
            let id = colorize_id("test-123", &config);
            assert!(id.contains("test-123"));
            // Cyan color adds ANSI codes
            assert!(id.contains("\x1b["), "ID should have ANSI codes");
        });
    }

    #[test]
    fn test_colorize_id_without_colors() {
        let config = OutputConfig::new(80, false, false);
        let id = colorize_id("test-123", &config);
        assert_eq!(id, "test-123");
        assert!(!id.contains("\x1b["), "ID should NOT have ANSI codes");
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
        let config = OutputConfig::new(80, true, true);

        assert_eq!(type_icon(IssueType::Task, &config), "-");
        assert_eq!(type_icon(IssueType::Bug, &config), "*");
        assert_eq!(type_icon(IssueType::Feature, &config), "+");
        assert_eq!(type_icon(IssueType::Epic, &config), "#");
        assert_eq!(type_icon(IssueType::Chore, &config), ".");

        // Status icons should also be ASCII
        let config_no_color = OutputConfig::new(80, true, false);
        let open = colored_status_icon(IssueStatus::Open, &config_no_color);
        let closed = colored_status_icon(IssueStatus::Closed, &config_no_color);
        assert!(open.contains("o"));
        assert!(closed.contains("+"));
        // With colors disabled, no ANSI codes
        assert!(
            !open.contains("\x1b["),
            "ASCII open should NOT have ANSI codes"
        );
        assert!(
            !closed.contains("\x1b["),
            "ASCII closed should NOT have ANSI codes"
        );
    }

    #[test]
    fn test_output_config_from_env() {
        with_env_lock(|| {
            // Clean up any existing env vars
            env::remove_var("RIVETS_MAX_WIDTH");
            env::remove_var("RIVETS_ASCII");
            env::remove_var("NO_COLOR");
            env::remove_var("RIVETS_COLOR");

            // Test that OutputConfig::from_env respects env vars
            env::set_var("RIVETS_MAX_WIDTH", "120");
            env::set_var("RIVETS_ASCII", "1");
            let config = OutputConfig::from_env();
            assert_eq!(config.max_width, 120);
            assert!(config.use_ascii);
            assert!(config.use_colors); // Colors default to true

            env::set_var("RIVETS_MAX_WIDTH", "invalid");
            env::set_var("RIVETS_ASCII", "false");
            let config = OutputConfig::from_env();
            assert_eq!(config.max_width, DEFAULT_MAX_CONTENT_WIDTH);
            assert!(!config.use_ascii);

            // Test NO_COLOR standard
            env::set_var("NO_COLOR", "1");
            let config = OutputConfig::from_env();
            assert!(!config.use_colors, "NO_COLOR should disable colors");

            env::remove_var("NO_COLOR");

            // Test RIVETS_COLOR=0 disables colors
            env::set_var("RIVETS_COLOR", "0");
            let config = OutputConfig::from_env();
            assert!(!config.use_colors, "RIVETS_COLOR=0 should disable colors");

            env::set_var("RIVETS_COLOR", "false");
            let config = OutputConfig::from_env();
            assert!(
                !config.use_colors,
                "RIVETS_COLOR=false should disable colors"
            );

            // Clean up
            env::remove_var("RIVETS_MAX_WIDTH");
            env::remove_var("RIVETS_ASCII");
            env::remove_var("RIVETS_COLOR");
            let config = OutputConfig::from_env();
            assert_eq!(config.max_width, DEFAULT_MAX_CONTENT_WIDTH);
            assert!(!config.use_ascii);
            assert!(config.use_colors); // Colors default to true
        });
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
        // Use explicit config with colors disabled to avoid race conditions
        // with colored crate's global state in parallel tests
        let config = OutputConfig::new(80, false, false);
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
        // Use explicit config with colors disabled to avoid race conditions
        // with colored crate's global state in parallel tests
        let config = OutputConfig::new(80, false, false);
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
