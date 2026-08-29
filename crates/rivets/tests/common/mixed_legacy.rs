#![allow(dead_code)] // Each integration-test binary uses a different subset.

use std::path::Path;

pub const MIXED_ISSUE_COUNT: usize = 8;
pub const LEGACY_NOTE_ID: &str = "test-note";
pub const LEGACY_URL_ID: &str = "test-url";
pub const LEGACY_OPAQUE_ID: &str = "test-opaque";
pub const CONFLICT_ID: &str = "test-kind-conflict";
pub const MIXED_LEGACY_JSONL: &str = include_str!("../fixtures/mixed_legacy_issues.jsonl");

pub fn seed_mixed_workspace(workspace_root: &Path) {
    std::fs::write(
        workspace_root.join(".rivets/issues.jsonl"),
        MIXED_LEGACY_JSONL,
    )
    .expect("mixed legacy fixture should be seeded");
}

pub fn read_records(path: &Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(path)
        .expect("Issue records should be readable")
        .lines()
        .map(|line| serde_json::from_str(line).expect("Issue record should be JSON"))
        .collect()
}

pub fn fixture_records() -> Vec<serde_json::Value> {
    MIXED_LEGACY_JSONL
        .lines()
        .map(|line| serde_json::from_str(line).expect("fixture record should be JSON"))
        .collect()
}

pub fn record<'a>(records: &'a [serde_json::Value], issue_id: &str) -> &'a serde_json::Value {
    records
        .iter()
        .find(|record| record["id"] == issue_id)
        .unwrap_or_else(|| panic!("Issue {issue_id} should be present"))
}

pub fn assert_canonical_records(records: &[serde_json::Value]) {
    assert_eq!(records.len(), MIXED_ISSUE_COUNT);
    for record in records {
        assert!(record.get("issue_type").is_none());
        assert!(record.get("external_ref").is_none());
        assert!(record["issue_kind"].is_string());
        assert!(record["notes"].is_array());
        assert!(record["resources"].is_array());
    }
}
