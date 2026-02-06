//! Color and styling helpers for CLI output.
//!
//! Semantic Color Theme:
//!   - Success/Done:  green   (closed status, completed actions)
//!   - Warning/Active: yellow (in_progress, P1 priority)
//!   - Error/Blocked: red     (blocked status, P0 priority, bugs)
//!   - Info/Reference: cyan   (issue IDs, root tree node)
//!   - Accent:        magenta (labels, epics)
//!   - Muted:         dimmed  (field labels, connectors, chores)
//!   - Emphasis:      bold    (section headers, P0)
//!   - Default:       white   (open status)

use crate::domain::{IssueStatus, IssueType};
use colored::Colorize;

use super::OutputConfig;

/// Apply semantic "success" color (green) to text.
pub fn success(text: &str, config: &OutputConfig) -> String {
    if !config.use_colors {
        return text.to_string();
    }
    text.green().to_string()
}

/// Apply semantic "error" color (red) to text.
pub fn error(text: &str, config: &OutputConfig) -> String {
    if !config.use_colors {
        return text.to_string();
    }
    text.red().to_string()
}

/// Apply semantic "warning" color (yellow) to text.
pub fn warning(text: &str, config: &OutputConfig) -> String {
    if !config.use_colors {
        return text.to_string();
    }
    text.yellow().to_string()
}

/// Apply semantic "info" color (cyan) to text.
pub fn info(text: &str, config: &OutputConfig) -> String {
    if !config.use_colors {
        return text.to_string();
    }
    text.cyan().to_string()
}

/// Apply color to status text based on issue status.
pub(crate) fn colorize_status(status: IssueStatus, config: &OutputConfig) -> String {
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
pub(crate) fn colorize_priority(priority: u8, config: &OutputConfig) -> String {
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
pub(crate) fn colorize_id(id: &str, config: &OutputConfig) -> String {
    if !config.use_colors {
        return id.to_string();
    }
    id.cyan().to_string()
}

/// Colorize labels (magenta).
pub(crate) fn colorize_labels(labels: &[String], config: &OutputConfig) -> String {
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
pub(crate) fn colored_status_icon(status: IssueStatus, config: &OutputConfig) -> String {
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
pub(crate) fn dimmed(text: &str, config: &OutputConfig) -> String {
    if !config.use_colors {
        return text.to_string();
    }
    text.dimmed().to_string()
}

/// Apply bold style to text (for section headers).
pub(crate) fn bold(text: &str, config: &OutputConfig) -> String {
    if !config.use_colors {
        return text.to_string();
    }
    text.bold().to_string()
}

/// Apply cyan color to text (for arrows/connectors).
pub(crate) fn cyan(text: &str, config: &OutputConfig) -> String {
    if !config.use_colors {
        return text.to_string();
    }
    text.cyan().to_string()
}

/// Apply yellow color to text (for arrows/connectors).
pub(crate) fn yellow(text: &str, config: &OutputConfig) -> String {
    if !config.use_colors {
        return text.to_string();
    }
    text.yellow().to_string()
}

/// Get a type icon for issue types, with ASCII fallback support.
pub(crate) fn type_icon(issue_type: IssueType, config: &OutputConfig) -> &'static str {
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

/// Get a colored type icon for issue types.
pub(crate) fn colored_type_icon(issue_type: IssueType, config: &OutputConfig) -> String {
    let icon = type_icon(issue_type, config);
    if !config.use_colors {
        return icon.to_string();
    }
    match issue_type {
        IssueType::Bug => icon.red().to_string(),
        IssueType::Feature => icon.green().to_string(),
        IssueType::Epic => icon.magenta().bold().to_string(),
        IssueType::Task => icon.blue().to_string(),
        IssueType::Chore => icon.dimmed().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::IssueType;
    use colored::control::set_override;
    use std::sync::{Mutex, MutexGuard};

    static GLOBAL_STATE_MUTEX: Mutex<()> = Mutex::new(());

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

    fn with_colors_enabled<F, R>(f: F) -> R
    where
        F: FnOnce() -> R,
    {
        let _guard = ColorGuard::new();
        f()
    }

    #[test]
    fn test_colorize_status_contains_ansi_codes() {
        with_colors_enabled(|| {
            let config = OutputConfig::new(80, false, true);
            let open = colorize_status(IssueStatus::Open, &config);
            let in_progress = colorize_status(IssueStatus::InProgress, &config);
            let blocked = colorize_status(IssueStatus::Blocked, &config);
            let closed = colorize_status(IssueStatus::Closed, &config);

            assert!(open.contains("open"));
            assert!(in_progress.contains("in_progress"));
            assert!(blocked.contains("blocked"));
            assert!(closed.contains("closed"));

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
        let config = OutputConfig::new(80, false, false);
        let open = colorize_status(IssueStatus::Open, &config);
        let in_progress = colorize_status(IssueStatus::InProgress, &config);

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

            assert!(p0.contains("P0"));
            assert!(p1.contains("P1"));
            assert!(p2.contains("P2"));

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
        assert_eq!(type_icon(IssueType::Task, &config), "◇");
        assert_eq!(type_icon(IssueType::Bug, &config), "●");
        assert_eq!(type_icon(IssueType::Feature, &config), "★");
        assert_eq!(type_icon(IssueType::Epic, &config), "◆");
        assert_eq!(type_icon(IssueType::Chore, &config), "○");
    }

    #[test]
    fn test_ascii_fallback_icons() {
        let config = OutputConfig::new(80, true, true);

        assert_eq!(type_icon(IssueType::Task, &config), "-");
        assert_eq!(type_icon(IssueType::Bug, &config), "*");
        assert_eq!(type_icon(IssueType::Feature, &config), "+");
        assert_eq!(type_icon(IssueType::Epic, &config), "#");
        assert_eq!(type_icon(IssueType::Chore, &config), ".");

        let config_no_color = OutputConfig::new(80, true, false);
        let open = colored_status_icon(IssueStatus::Open, &config_no_color);
        let closed = colored_status_icon(IssueStatus::Closed, &config_no_color);
        assert!(open.contains("o"));
        assert!(closed.contains("+"));
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
    fn test_colored_type_icon_without_colors() {
        let config = OutputConfig::new(80, false, false);
        let bug = colored_type_icon(IssueType::Bug, &config);
        assert_eq!(bug, "●");
        assert!(
            !bug.contains("\x1b["),
            "Bug icon should NOT have ANSI codes"
        );

        let feature = colored_type_icon(IssueType::Feature, &config);
        assert_eq!(feature, "★");
        assert!(
            !feature.contains("\x1b["),
            "Feature icon should NOT have ANSI codes"
        );
    }

    #[test]
    fn test_colored_type_icon_with_colors() {
        with_colors_enabled(|| {
            let config = OutputConfig::new(80, false, true);
            let bug = colored_type_icon(IssueType::Bug, &config);
            assert!(bug.contains("●"), "Bug icon should contain the icon");
            assert!(
                bug.contains("\x1b["),
                "Bug icon should have ANSI codes when colors enabled"
            );

            let feature = colored_type_icon(IssueType::Feature, &config);
            assert!(
                feature.contains("★"),
                "Feature icon should contain the icon"
            );
            assert!(
                feature.contains("\x1b["),
                "Feature icon should have ANSI codes when colors enabled"
            );

            let epic = colored_type_icon(IssueType::Epic, &config);
            assert!(epic.contains("◆"), "Epic icon should contain the icon");
            assert!(
                epic.contains("\x1b["),
                "Epic icon should have ANSI codes when colors enabled"
            );
        });
    }

    #[test]
    fn test_colored_type_icon_ascii_mode() {
        let config = OutputConfig::new(80, true, false);
        assert_eq!(colored_type_icon(IssueType::Bug, &config), "*");
        assert_eq!(colored_type_icon(IssueType::Feature, &config), "+");
        assert_eq!(colored_type_icon(IssueType::Epic, &config), "#");
        assert_eq!(colored_type_icon(IssueType::Task, &config), "-");
        assert_eq!(colored_type_icon(IssueType::Chore, &config), ".");
    }

    #[test]
    fn test_semantic_colors_with_colors_enabled() {
        with_colors_enabled(|| {
            let config = OutputConfig::new(80, false, true);
            let s = success("done", &config);
            assert!(s.contains("done"));
            assert!(s.contains("\x1b["), "success should have ANSI codes");

            let e = error("fail", &config);
            assert!(e.contains("fail"));
            assert!(e.contains("\x1b["), "error should have ANSI codes");

            let w = warning("caution", &config);
            assert!(w.contains("caution"));
            assert!(w.contains("\x1b["), "warning should have ANSI codes");

            let i = info("note", &config);
            assert!(i.contains("note"));
            assert!(i.contains("\x1b["), "info should have ANSI codes");
        });
    }

    #[test]
    fn test_semantic_colors_without_colors() {
        let config = OutputConfig::new(80, false, false);
        assert_eq!(success("done", &config), "done");
        assert_eq!(error("fail", &config), "fail");
        assert_eq!(warning("caution", &config), "caution");
        assert_eq!(info("note", &config), "note");
    }
}
