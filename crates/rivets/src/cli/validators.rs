//! CLI input validation functions.
//!
//! These validators are used by clap's `value_parser` attribute to validate
//! user input at parse time, providing immediate feedback for invalid values.

use crate::domain::MAX_TITLE_LENGTH;

/// Validate issue ID prefix format.
///
/// Delegates to the domain validator in `commands::init` to maintain
/// a single source of truth for validation rules.
pub fn validate_prefix(s: &str) -> Result<String, String> {
    use crate::commands::init;

    let trimmed = s.trim();
    init::validate_prefix(trimmed).map_err(|e| e.to_string())?;
    Ok(trimmed.to_string())
}

/// Validate issue ID format.
///
/// Expected format: `prefix-suffix` where:
/// - prefix: 2-20 alphanumeric characters
/// - suffix: 1+ alphanumeric characters
///
/// Examples: `proj-abc`, `rivets-12x`, `test-1`
pub fn validate_issue_id(s: &str) -> Result<String, String> {
    let s = s.trim();

    if s.is_empty() {
        return Err("Issue ID cannot be empty".to_string());
    }

    // Check for the prefix-suffix format (must have at least one hyphen)
    let parts: Vec<&str> = s.splitn(2, '-').collect();
    if parts.len() != 2 {
        return Err(format!(
            "Invalid issue ID format: '{}'. Expected format: prefix-suffix (e.g., proj-abc or proj-abc-123)",
            s
        ));
    }

    let prefix = parts[0];
    let suffix = parts[1];

    // Validate prefix using shared validation logic
    validate_prefix(prefix).map_err(|e| format!("Issue ID {}", e.to_lowercase()))?;

    // Validate suffix
    //
    // Note: We use explicit checks instead of regex (e.g., `^[a-zA-Z0-9]+(-[a-zA-Z0-9]+)*$`)
    // to provide specific, actionable error messages and avoid adding regex as a dependency.
    // This approach is more maintainable for a CLI tool where user-facing errors matter.
    if suffix.is_empty() {
        return Err("Issue ID suffix cannot be empty".to_string());
    }

    // Suffix can contain alphanumerics and hyphens (for IDs like proj-abc-123)
    if !suffix
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err("Issue ID suffix must contain only alphanumerics and hyphens".to_string());
    }

    // Prevent edge cases: leading/trailing hyphens or consecutive hyphens
    // Equivalent to regex: ^[a-zA-Z0-9]+(-[a-zA-Z0-9]+)*$
    if suffix.starts_with('-') {
        return Err("Issue ID suffix cannot start with a hyphen".to_string());
    }

    if suffix.ends_with('-') {
        return Err("Issue ID suffix cannot end with a hyphen".to_string());
    }

    if suffix.contains("--") {
        return Err("Issue ID suffix cannot contain consecutive hyphens".to_string());
    }

    Ok(s.to_string())
}

/// Validate title length.
///
/// Title must not exceed MAX_TITLE_LENGTH (200 characters).
///
/// Examples: Valid titles under 200 chars
pub fn validate_title(s: &str) -> Result<String, String> {
    let s = s.trim();

    if s.is_empty() {
        return Err("Title cannot be empty".to_string());
    }

    if s.len() > MAX_TITLE_LENGTH {
        return Err(format!(
            "Title cannot exceed {} characters, got {} characters",
            MAX_TITLE_LENGTH,
            s.len()
        ));
    }

    // Check for newlines in title (titles should be single-line)
    if s.contains('\n') || s.contains('\r') {
        return Err("Title cannot contain newline characters".to_string());
    }

    // Check for control characters (0x00-0x1F except tab, and 0x7F-0x9F)
    // These can cause display issues and are likely user errors
    if let Some(pos) = s.chars().position(|c| {
        let code = c as u32;
        // Control characters excluding tab (0x09)
        (code < 0x20 && code != 0x09) || (0x7F..=0x9F).contains(&code)
    }) {
        return Err(format!(
            "Title contains invalid control character at position {}",
            pos
        ));
    }

    Ok(s.to_string())
}

/// Validate text field (description, notes, etc.)
///
/// Allows newlines but rejects control characters that could cause display issues.
/// Unlike titles, multi-line text is acceptable for descriptions and notes.
fn validate_text_field(s: &str, field_name: &str) -> Result<String, String> {
    // Check for control characters (0x00-0x1F except tab and newlines, and 0x7F-0x9F)
    if let Some(pos) = s.chars().position(|c| {
        let code = c as u32;
        // Control characters excluding tab (0x09), LF (0x0A), and CR (0x0D)
        (code < 0x20 && code != 0x09 && code != 0x0A && code != 0x0D)
            || (0x7F..=0x9F).contains(&code)
    }) {
        return Err(format!(
            "{} contains invalid control character at position {}",
            field_name, pos
        ));
    }

    Ok(s.to_string())
}

/// Validate description field
///
/// Wrapper for validate_text_field specifically for descriptions.
pub fn validate_description(s: &str) -> Result<String, String> {
    validate_text_field(s, "Description")
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========== Prefix Validation ==========

    #[test]
    fn test_validate_prefix_valid() {
        assert!(validate_prefix("proj").is_ok());
        assert!(validate_prefix("rivets").is_ok());
        assert!(validate_prefix("AB").is_ok());
        assert!(validate_prefix("test123").is_ok());
        assert!(validate_prefix("a1b2c3d4e5f6g7h8i9j0").is_ok()); // 20 chars
    }

    #[test]
    fn test_validate_prefix_too_short() {
        let result = validate_prefix("a");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("at least 2 characters"));
    }

    #[test]
    fn test_validate_prefix_too_long() {
        let result = validate_prefix("a".repeat(21).as_str());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot exceed 20"));
    }

    #[test]
    fn test_validate_prefix_invalid_chars() {
        assert!(validate_prefix("proj-test").is_err()); // hyphen
        assert!(validate_prefix("proj_test").is_err()); // underscore
        assert!(validate_prefix("proj test").is_err()); // space
        assert!(validate_prefix("proj.test").is_err()); // dot
    }

    #[test]
    fn test_validate_prefix_trims_whitespace() {
        assert_eq!(validate_prefix("  proj  ").unwrap(), "proj");
    }

    // ========== Issue ID Validation ==========

    #[test]
    fn test_validate_issue_id_valid() {
        assert!(validate_issue_id("proj-abc").is_ok());
        assert!(validate_issue_id("rivets-123").is_ok());
        assert!(validate_issue_id("ab-1").is_ok());
        assert!(validate_issue_id("TEST-xyz").is_ok());
    }

    #[test]
    fn test_validate_issue_id_empty() {
        let result = validate_issue_id("");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot be empty"));
    }

    #[test]
    fn test_validate_issue_id_no_hyphen() {
        let result = validate_issue_id("projabc");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Expected format"));
    }

    #[test]
    fn test_validate_issue_id_empty_suffix() {
        let result = validate_issue_id("proj-");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("suffix cannot be empty"));
    }

    #[test]
    fn test_validate_issue_id_prefix_too_short() {
        let result = validate_issue_id("a-123");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_lowercase()
            .contains("at least 2 characters"));
    }

    #[test]
    fn test_validate_issue_id_invalid_chars() {
        assert!(validate_issue_id("proj-abc_123").is_err()); // underscore in suffix
        assert!(validate_issue_id("proj_test-abc").is_err()); // underscore in prefix
    }

    #[test]
    fn test_validate_issue_id_multiple_hyphens() {
        // Issue IDs with multiple hyphens in suffix should now be valid
        assert!(validate_issue_id("proj-abc-123").is_ok());
        assert!(validate_issue_id("rivets-feature-xyz").is_ok());
        assert!(validate_issue_id("test-a-b-c-d").is_ok());
        assert_eq!(validate_issue_id("proj-abc-123").unwrap(), "proj-abc-123");
    }

    #[test]
    fn test_validate_issue_id_prefix_exactly_20_chars() {
        let prefix_20 = "a".repeat(20);
        let issue_id = format!("{}-xyz", prefix_20);
        assert!(validate_issue_id(&issue_id).is_ok());
    }

    #[test]
    fn test_validate_issue_id_prefix_21_chars() {
        let prefix_21 = "a".repeat(21);
        let issue_id = format!("{}-xyz", prefix_21);
        let result = validate_issue_id(&issue_id);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_lowercase()
            .contains("cannot exceed 20"));
    }

    #[test]
    fn test_validate_issue_id_leading_hyphen_suffix() {
        // `proj--abc` has a leading hyphen in the suffix (after the first hyphen)
        let result = validate_issue_id("proj--abc");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot start with a hyphen"));
    }

    #[test]
    fn test_validate_issue_id_trailing_hyphen_suffix() {
        let result = validate_issue_id("proj-abc-");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot end with a hyphen"));
    }

    #[test]
    fn test_validate_issue_id_consecutive_hyphens() {
        let result = validate_issue_id("proj-a--b");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("cannot contain consecutive hyphens"));
    }

    // ========== Title Validation ==========

    #[test]
    fn test_validate_title_valid() {
        assert!(validate_title("Short title").is_ok());
        assert!(validate_title("A".repeat(200).as_str()).is_ok()); // Exactly 200 chars
    }

    #[test]
    fn test_validate_title_empty() {
        let result = validate_title("");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot be empty"));
    }

    #[test]
    fn test_validate_title_too_long() {
        let long_title = "A".repeat(201);
        let result = validate_title(&long_title);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot exceed 200"));
    }

    #[test]
    fn test_validate_title_exactly_max_length() {
        let max_title = "A".repeat(200);
        assert!(validate_title(&max_title).is_ok());
        assert_eq!(validate_title(&max_title).unwrap().len(), 200);
    }

    #[test]
    fn test_validate_title_trims_whitespace() {
        assert_eq!(validate_title("  Test Title  ").unwrap(), "Test Title");
    }

    #[test]
    fn test_validate_title_whitespace_only() {
        let result = validate_title("   ");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot be empty"));
    }

    #[test]
    fn test_validate_title_with_newline() {
        let result = validate_title("Title with\nnewline");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("newline"));
    }

    #[test]
    fn test_validate_title_with_carriage_return() {
        let result = validate_title("Title with\rcarriage return");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("newline"));
    }

    #[test]
    fn test_validate_title_with_control_character() {
        // Test with null character (0x00)
        let result = validate_title("Title with\x00control");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("control character"));
    }

    #[test]
    fn test_validate_title_with_tab_allowed() {
        // Tab (0x09) should be allowed
        let result = validate_title("Title with\ttab");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Title with\ttab");
    }

    #[test]
    fn test_validate_title_with_delete_character() {
        // DEL character (0x7F)
        let result = validate_title("Title with\x7Fdelete");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("control character"));
    }

    // ========== Description Validation ==========

    #[test]
    fn test_validate_description_with_newline_allowed() {
        // Newlines should be allowed in descriptions
        let result = validate_description("Multi-line\ndescription");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Multi-line\ndescription");
    }

    #[test]
    fn test_validate_description_with_control_character() {
        // Control characters should be rejected
        let result = validate_description("Description with\x00control");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("control character"));
    }

    #[test]
    fn test_validate_description_with_tab_and_newline() {
        // Both tab and newline should be allowed
        let result = validate_description("Line1\n\tIndented line");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Line1\n\tIndented line");
    }

    #[test]
    fn test_validate_description_empty() {
        // Empty descriptions should be allowed
        let result = validate_description("");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "");
    }
}
