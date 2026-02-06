//! Output formatting for CLI commands.
//!
//! This module provides utilities for formatting command output in both
//! human-readable text format and JSON format for programmatic use.
//!
//! Submodules:
//! - [`color`]: Color and styling helpers (semantic colors, icons)
//! - [`json`]: JSON serialization for programmatic output
//! - [`tree`]: Dependency tree rendering with ASCII/Unicode connectors

pub mod color;
mod json;
pub mod tree;

use crate::domain::{Dependency, Issue};
use colored::Colorize;
use serde::Serialize;
use std::env;
use std::io::{self, Write};

// Re-export public items for backwards compatibility
pub use color::{error, info, success, warning};
pub use tree::{dep_tree_to_json_public, print_dep_tree, print_dep_tree_dependents, DepTreeNode};

use color::{
    bold, colored_status_icon, colored_type_icon, colorize_id, colorize_labels, colorize_priority,
    colorize_status, cyan, dimmed, yellow,
};
use json::{print_blocked_json, print_issue_details_json, print_issue_json, print_issues_json};

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

// ============================================================================
// Public Dispatch Functions
// ============================================================================

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
        colored_type_icon(issue.issue_type, config),
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
            colored_type_icon(issue.issue_type, config),
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
        colored_type_icon(issue.issue_type, config),
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
            colored_type_icon(issue.issue_type, config),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Dependency, DependencyType, IssueId, IssueStatus, IssueType};
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
        let text = "Check out https://example.com/very/long/path/to/resource for details";
        let wrapped = wrap_text(text, 30);
        assert!(!wrapped.is_empty());
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
    fn test_output_config_from_env() {
        with_env_lock(|| {
            env::remove_var("RIVETS_MAX_WIDTH");
            env::remove_var("RIVETS_ASCII");
            env::remove_var("NO_COLOR");
            env::remove_var("RIVETS_COLOR");

            env::set_var("RIVETS_MAX_WIDTH", "120");
            env::set_var("RIVETS_ASCII", "1");
            let config = OutputConfig::from_env();
            assert_eq!(config.max_width, 120);
            assert!(config.use_ascii);
            assert!(config.use_colors);

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
            assert!(config.use_colors);
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
        let config = OutputConfig::new(80, false, false);
        let mut buffer = Vec::new();

        print_text_section(&mut buffer, "Description", "", 80, &config).unwrap();
        assert!(buffer.is_empty(), "Empty content should produce no output");

        print_text_section(&mut buffer, "Description", "Some text", 80, &config).unwrap();
        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("Description:"));
        assert!(output.contains("Some text"));
    }

    #[test]
    fn test_print_optional_section_handles_none() {
        let config = OutputConfig::new(80, false, false);
        let mut buffer = Vec::new();

        print_optional_section(&mut buffer, "Notes", &None, 80, &config).unwrap();
        assert!(buffer.is_empty(), "None should produce no output");

        let empty: Option<String> = Some(String::new());
        print_optional_section(&mut buffer, "Notes", &empty, 80, &config).unwrap();
        assert!(buffer.is_empty(), "Empty Some should produce no output");

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
        assert!(
            !output.contains("Description:"),
            "Empty description should not show Description section"
        );
    }

    #[test]
    fn test_wrap_text_with_narrow_width() {
        let text = "Hello world";
        let wrapped = wrap_text(text, 5);
        assert!(!wrapped.is_empty());
        for line in &wrapped {
            assert!(line.len() <= 5, "Line '{}' exceeds width 5", line);
        }
    }

    #[test]
    fn test_wrap_text_with_wide_width() {
        let text = "Short";
        let wrapped = wrap_text(text, 100);
        assert_eq!(wrapped.len(), 1);
        assert_eq!(wrapped[0], "Short");
    }

    #[test]
    fn test_wrap_text_empty_input() {
        let wrapped = wrap_text("", 80);
        assert!(wrapped.is_empty() || (wrapped.len() == 1 && wrapped[0].is_empty()));
    }
}
