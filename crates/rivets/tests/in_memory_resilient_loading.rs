//! Integration tests for in_memory storage resilient loading.
//!
//! These tests verify the integration between the rivets-jsonl library's
//! resilient loading functionality and the rivets in_memory storage backend.
//!
//! # Test Coverage
//!
//! - LoadWarning types and their behavior
//! - load_from_jsonl() with corrupted files
//! - Warning propagation from rivets-jsonl to rivets
//! - Storage functionality after resilient loading
//! - Round-trip persistence through save and load

use chrono::Utc;
use rivets::domain::{
    BlockingDependency, DependencyType, IssueId, IssueKind, IssueStatus, NewIssue, ResourceError,
    ResourceRole,
};
use rivets::storage::in_memory::{
    LoadWarning, MigrationField, load_from_jsonl, new_in_memory_storage, save_to_jsonl,
};
use rivets::storage::{StorageBackend, create_storage};
use std::io::Write;
use tempfile::NamedTempFile;

#[path = "common/mixed_legacy.rs"]
mod mixed_legacy;

use mixed_legacy::{
    CONFLICT_ID, LEGACY_NOTE_ID, LEGACY_OPAQUE_ID, LEGACY_URL_ID, MIXED_ISSUE_COUNT,
    MIXED_LEGACY_JSONL, assert_canonical_records, fixture_records, read_records, record,
};

// =============================================================================
// Test Helpers
// =============================================================================

fn create_temp_jsonl_file(content: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().expect("Failed to create temp file");
    file.write_all(content.as_bytes())
        .expect("Failed to write to temp file");
    file.flush().expect("Failed to flush temp file");
    file
}

fn create_test_issue(title: &str) -> NewIssue {
    NewIssue {
        title: title.to_string(),
        description: "Test description".to_string(),
        priority: 2,
        issue_kind: IssueKind::Task,
        assignee: None,
        labels: vec![],
        design: None,
        acceptance_criteria: None,
        initial_note: None,
        dependencies: vec![],
    }
}

fn create_valid_issue_json(id: &str, title: &str) -> String {
    let now = Utc::now().to_rfc3339();
    format!(
        r#"{{"id":"{}","title":"{}","description":"Test","status":"open","priority":2,"issue_type":"task","assignee":null,"labels":[],"design":null,"acceptance_criteria":null,"notes":null,"external_ref":null,"dependencies":[],"created_at":"{}","updated_at":"{}","closed_at":null}}"#,
        id, title, now, now
    )
}

fn create_issue_with_dependency_json(
    id: &str,
    title: &str,
    dep_id: &str,
    dep_type: &str,
) -> String {
    let now = Utc::now().to_rfc3339();
    format!(
        r#"{{"id":"{}","title":"{}","description":"Test","status":"open","priority":2,"issue_type":"task","assignee":null,"labels":[],"design":null,"acceptance_criteria":null,"notes":null,"external_ref":null,"dependencies":[{{"depends_on_id":"{}","dep_type":"{}"}}],"created_at":"{}","updated_at":"{}","closed_at":null}}"#,
        id, title, dep_id, dep_type, now, now
    )
}

// =============================================================================
// LoadWarning Tests
// =============================================================================

mod load_warning_tests {
    use super::*;

    #[test]
    fn load_warning_malformed_json_contains_line_number() {
        let warning = LoadWarning::MalformedJson {
            line_number: 42,
            error: "unexpected end of input".to_string(),
        };

        match warning {
            LoadWarning::MalformedJson { line_number, error } => {
                assert_eq!(line_number, 42);
                assert!(!error.is_empty());
            }
            _ => panic!("Expected MalformedJson variant"),
        }
    }

    #[test]
    fn load_warning_orphaned_dependency_contains_ids() {
        let warning = LoadWarning::OrphanedDependency {
            from: IssueId::new("test-1"),
            to: IssueId::new("nonexistent"),
        };

        match warning {
            LoadWarning::OrphanedDependency { from, to } => {
                assert_eq!(from.as_str(), "test-1");
                assert_eq!(to.as_str(), "nonexistent");
            }
            _ => panic!("Expected OrphanedDependency variant"),
        }
    }

    #[test]
    fn load_warning_circular_dependency_contains_ids() {
        let warning = LoadWarning::CircularDependency {
            from: IssueId::new("test-1"),
            to: IssueId::new("test-2"),
        };

        match warning {
            LoadWarning::CircularDependency { from, to } => {
                assert_eq!(from.as_str(), "test-1");
                assert_eq!(to.as_str(), "test-2");
            }
            _ => panic!("Expected CircularDependency variant"),
        }
    }

    #[test]
    fn load_warning_invalid_issue_data_contains_details() {
        let warning = LoadWarning::InvalidIssueData {
            issue_id: IssueId::new("test-invalid"),
            line_number: 5,
            error: "Priority exceeds maximum".to_string(),
        };

        match warning {
            LoadWarning::InvalidIssueData {
                issue_id,
                line_number,
                error,
            } => {
                assert_eq!(issue_id.as_str(), "test-invalid");
                assert_eq!(line_number, 5);
                assert!(error.contains("Priority"));
            }
            _ => panic!("Expected InvalidIssueData variant"),
        }
    }

    #[test]
    fn load_warning_invalid_resource_data_contains_details() {
        let warning = LoadWarning::InvalidResourceData {
            issue_id: IssueId::new("test-resource"),
            line_number: 8,
            source: ResourceError::EmptyResourceId,
        };

        match warning {
            LoadWarning::InvalidResourceData {
                issue_id,
                line_number,
                source,
            } => {
                assert_eq!(issue_id.as_str(), "test-resource");
                assert_eq!(line_number, 8);
                assert!(matches!(source, ResourceError::EmptyResourceId));
            }
            _ => panic!("Expected InvalidResourceData variant"),
        }
    }

    #[test]
    fn load_warning_migration_conflict_display_names_both_fields() {
        let warning = LoadWarning::MigrationConflict {
            issue_id: IssueId::new("test-conflict"),
            line_number: 4,
            field: MigrationField::IssueKind,
        };
        assert_eq!(
            warning.to_string(),
            "line 4: Issue test-conflict has legacy field issue_type conflicting with canonical issue_kind"
        );
    }

    #[test]
    fn load_warning_is_clone() {
        let warning = LoadWarning::MalformedJson {
            line_number: 1,
            error: "test".to_string(),
        };
        let cloned = warning.clone();

        match cloned {
            LoadWarning::MalformedJson { line_number, .. } => {
                assert_eq!(line_number, 1);
            }
            _ => panic!("Clone failed"),
        }
    }

    #[test]
    fn load_warning_is_debug() {
        let warning = LoadWarning::MalformedJson {
            line_number: 1,
            error: "test".to_string(),
        };
        let debug_str = format!("{:?}", warning);
        assert!(debug_str.contains("MalformedJson"));
    }
}

// =============================================================================
// load_from_jsonl() Tests
// =============================================================================

mod load_from_jsonl_tests {
    use super::*;

    #[tokio::test]
    async fn load_empty_file() {
        let file = create_temp_jsonl_file("");
        let (storage, warnings) = load_from_jsonl(file.path(), "test".to_string())
            .await
            .unwrap();

        let all_issues = storage.export_all().await.unwrap();
        assert!(all_issues.is_empty());
        assert!(warnings.is_empty());
    }

    #[tokio::test]
    async fn load_single_valid_issue() {
        let content = create_valid_issue_json("test-1", "Valid Issue");
        let file = create_temp_jsonl_file(&content);

        let (storage, warnings) = load_from_jsonl(file.path(), "test".to_string())
            .await
            .unwrap();

        assert!(warnings.is_empty());

        let issue = storage.get(&IssueId::new("test-1")).await.unwrap().unwrap();
        assert_eq!(issue.title, "Valid Issue");
    }

    #[tokio::test]
    async fn load_multiple_valid_issues() {
        let content = format!(
            "{}\n{}\n{}",
            create_valid_issue_json("test-1", "Issue 1"),
            create_valid_issue_json("test-2", "Issue 2"),
            create_valid_issue_json("test-3", "Issue 3")
        );
        let file = create_temp_jsonl_file(&content);

        let (storage, warnings) = load_from_jsonl(file.path(), "test".to_string())
            .await
            .unwrap();

        assert!(warnings.is_empty());

        let all_issues = storage.export_all().await.unwrap();
        assert_eq!(all_issues.len(), 3);
    }

    #[tokio::test]
    async fn load_with_malformed_json() {
        let line1 = create_valid_issue_json("test-1", "Valid 1");
        let line3 = create_valid_issue_json("test-3", "Valid 2");
        let content = format!("{}\n{{invalid json}}\n{}", line1, line3);
        let file = create_temp_jsonl_file(&content);

        let (storage, warnings) = load_from_jsonl(file.path(), "test".to_string())
            .await
            .unwrap();

        // Should have 1 warning for malformed JSON
        assert_eq!(warnings.len(), 1);
        match &warnings[0] {
            LoadWarning::MalformedJson { line_number, .. } => {
                assert_eq!(*line_number, 2);
            }
            _ => panic!("Expected MalformedJson warning"),
        }

        // Should have loaded 2 valid issues
        let all_issues = storage.export_all().await.unwrap();
        assert_eq!(all_issues.len(), 2);
    }

    #[tokio::test]
    async fn load_with_multiple_malformed_lines() {
        let line2 = create_valid_issue_json("test-2", "Valid 1");
        let line5 = create_valid_issue_json("test-5", "Valid 2");
        let content = format!(
            "{{invalid1}}\n{}\n{{invalid2}}\n{{invalid3}}\n{}",
            line2, line5
        );
        let file = create_temp_jsonl_file(&content);

        let (storage, warnings) = load_from_jsonl(file.path(), "test".to_string())
            .await
            .unwrap();

        // Should have 3 warnings
        assert_eq!(warnings.len(), 3);

        // All should be MalformedJson
        for warning in &warnings {
            match warning {
                LoadWarning::MalformedJson { .. } => {}
                _ => panic!("Expected MalformedJson warning"),
            }
        }

        // Should have loaded 2 valid issues
        let all_issues = storage.export_all().await.unwrap();
        assert_eq!(all_issues.len(), 2);
    }

    #[tokio::test]
    async fn load_with_orphaned_dependency() {
        let content = format!(
            "{}\n{}",
            create_valid_issue_json("test-1", "Valid Issue"),
            create_issue_with_dependency_json("test-2", "With Orphan", "nonexistent", "blocks")
        );
        let file = create_temp_jsonl_file(&content);

        let (storage, warnings) = load_from_jsonl(file.path(), "test".to_string())
            .await
            .unwrap();

        // Should have 1 warning for orphaned dependency
        assert_eq!(warnings.len(), 1);
        match &warnings[0] {
            LoadWarning::OrphanedDependency { from, to } => {
                assert_eq!(from.as_str(), "test-2");
                assert_eq!(to.as_str(), "nonexistent");
            }
            _ => panic!("Expected OrphanedDependency warning"),
        }

        // Both issues should be loaded
        let all_issues = storage.export_all().await.unwrap();
        assert_eq!(all_issues.len(), 2);

        // But the dependency should not exist in the graph
        let deps = storage
            .get_dependencies(&IssueId::new("test-2"))
            .await
            .unwrap();
        assert!(deps.is_empty());
    }

    #[tokio::test]
    async fn load_with_circular_dependency() {
        // Create two issues that depend on each other
        let now = Utc::now().to_rfc3339();
        let issue1 = format!(
            r#"{{"id":"test-1","title":"Issue 1","description":"Test","status":"open","priority":2,"issue_type":"task","assignee":null,"labels":[],"design":null,"acceptance_criteria":null,"notes":null,"external_ref":null,"dependencies":[{{"depends_on_id":"test-2","dep_type":"blocks"}}],"created_at":"{}","updated_at":"{}","closed_at":null}}"#,
            now, now
        );
        let issue2 = format!(
            r#"{{"id":"test-2","title":"Issue 2","description":"Test","status":"open","priority":2,"issue_type":"task","assignee":null,"labels":[],"design":null,"acceptance_criteria":null,"notes":null,"external_ref":null,"dependencies":[{{"depends_on_id":"test-1","dep_type":"blocks"}}],"created_at":"{}","updated_at":"{}","closed_at":null}}"#,
            now, now
        );
        let content = format!("{}\n{}", issue1, issue2);
        let file = create_temp_jsonl_file(&content);

        let (storage, warnings) = load_from_jsonl(file.path(), "test".to_string())
            .await
            .unwrap();

        // Should have 1 warning for circular dependency (one edge broken)
        assert_eq!(warnings.len(), 1);
        match &warnings[0] {
            LoadWarning::CircularDependency { from, to } => {
                // One of the circular edges should be flagged
                assert!(
                    (from.as_str() == "test-1" && to.as_str() == "test-2")
                        || (from.as_str() == "test-2" && to.as_str() == "test-1")
                );
            }
            _ => panic!("Expected CircularDependency warning"),
        }

        // Both issues should be loaded
        let all_issues = storage.export_all().await.unwrap();
        assert_eq!(all_issues.len(), 2);

        // Only one dependency should exist (cycle broken)
        let deps1 = storage
            .get_dependencies(&IssueId::new("test-1"))
            .await
            .unwrap();
        let deps2 = storage
            .get_dependencies(&IssueId::new("test-2"))
            .await
            .unwrap();
        assert_eq!(deps1.len() + deps2.len(), 1);
    }

    #[tokio::test]
    async fn load_with_invalid_priority() {
        let now = Utc::now().to_rfc3339();
        // Priority 10 is invalid (max is 4)
        let invalid_issue = format!(
            r#"{{"id":"test-invalid","title":"Invalid Priority","description":"Test","status":"open","priority":10,"issue_type":"task","assignee":null,"labels":[],"design":null,"acceptance_criteria":null,"notes":null,"external_ref":null,"dependencies":[],"created_at":"{}","updated_at":"{}","closed_at":null}}"#,
            now, now
        );
        let valid_issue = create_valid_issue_json("test-valid", "Valid Issue");
        let content = format!("{}\n{}", invalid_issue, valid_issue);
        let file = create_temp_jsonl_file(&content);

        let (storage, warnings) = load_from_jsonl(file.path(), "test".to_string())
            .await
            .unwrap();

        // Should have 1 warning for invalid issue data
        assert_eq!(warnings.len(), 1);
        match &warnings[0] {
            LoadWarning::InvalidIssueData {
                issue_id,
                line_number,
                error,
            } => {
                assert_eq!(issue_id.as_str(), "test-invalid");
                assert_eq!(*line_number, 1);
                assert!(error.contains("Priority"));
            }
            _ => panic!("Expected InvalidIssueData warning, got {:?}", warnings[0]),
        }

        // Only valid issue should be loaded
        let all_issues = storage.export_all().await.unwrap();
        assert_eq!(all_issues.len(), 1);
        assert!(
            storage
                .get(&IssueId::new("test-invalid"))
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            storage
                .get(&IssueId::new("test-valid"))
                .await
                .unwrap()
                .is_some()
        );
    }

    #[tokio::test]
    async fn load_with_mixed_warnings() {
        let now = Utc::now().to_rfc3339();

        // Valid issue
        let valid1 = create_valid_issue_json("test-1", "Valid 1");
        // Malformed JSON
        let malformed = "{invalid json}";
        // Valid issue
        let valid2 = create_valid_issue_json("test-3", "Valid 2");
        // Issue with orphaned dependency
        let orphan = format!(
            r#"{{"id":"test-4","title":"Orphan Dep","description":"Test","status":"open","priority":2,"issue_type":"task","assignee":null,"labels":[],"design":null,"acceptance_criteria":null,"notes":null,"external_ref":null,"dependencies":[{{"depends_on_id":"nonexistent","dep_type":"blocks"}}],"created_at":"{}","updated_at":"{}","closed_at":null}}"#,
            now, now
        );
        // Invalid priority
        let invalid_priority = format!(
            r#"{{"id":"test-5","title":"Bad Priority","description":"Test","status":"open","priority":99,"issue_type":"task","assignee":null,"labels":[],"design":null,"acceptance_criteria":null,"notes":null,"external_ref":null,"dependencies":[],"created_at":"{}","updated_at":"{}","closed_at":null}}"#,
            now, now
        );

        let content = format!(
            "{}\n{}\n{}\n{}\n{}",
            valid1, malformed, valid2, orphan, invalid_priority
        );
        let file = create_temp_jsonl_file(&content);

        let (storage, warnings) = load_from_jsonl(file.path(), "test".to_string())
            .await
            .unwrap();

        // Should have 3 warnings: malformed, invalid priority, orphaned dependency
        assert_eq!(warnings.len(), 3, "Warnings: {:?}", warnings);

        // Check warning types
        let mut has_malformed = false;
        let mut has_orphaned = false;
        let mut has_invalid = false;

        for warning in &warnings {
            match warning {
                LoadWarning::MalformedJson { .. } => has_malformed = true,
                LoadWarning::OrphanedDependency { .. } => has_orphaned = true,
                LoadWarning::InvalidIssueData { .. } => has_invalid = true,
                LoadWarning::CircularDependency { .. }
                | LoadWarning::MigrationConflict { .. }
                | LoadWarning::InvalidResourceData { .. } => {}
            }
        }

        assert!(has_malformed, "Should have MalformedJson warning");
        assert!(has_orphaned, "Should have OrphanedDependency warning");
        assert!(has_invalid, "Should have InvalidIssueData warning");

        // Should have loaded 3 valid issues (test-1, test-3, test-4)
        let all_issues = storage.export_all().await.unwrap();
        assert_eq!(all_issues.len(), 3);
    }

    #[tokio::test]
    async fn load_with_empty_lines() {
        let content = format!(
            "\n{}\n\n{}\n",
            create_valid_issue_json("test-1", "Issue 1"),
            create_valid_issue_json("test-2", "Issue 2")
        );
        let file = create_temp_jsonl_file(&content);

        let (storage, warnings) = load_from_jsonl(file.path(), "test".to_string())
            .await
            .unwrap();

        // Empty lines should not generate warnings
        assert!(warnings.is_empty());

        let all_issues = storage.export_all().await.unwrap();
        assert_eq!(all_issues.len(), 2);
    }

    #[tokio::test]
    async fn load_preserves_all_issue_fields() {
        let now = Utc::now().to_rfc3339();
        let json = format!(
            r#"{{"id":"test-full","title":"Full Issue","description":"Complete description","status":"in_progress","priority":1,"issue_kind":"feature","assignee":"alice","labels":["backend","urgent"],"design":"Design notes here","acceptance_criteria":"- Criterion 1\n- Criterion 2","notes":[{{"content":"Implementation notes","created_at":"{now}"}}],"external_ref":"GH-123","dependencies":[],"created_at":"{now}","updated_at":"{now}","closed_at":null}}"#
        );
        let file = create_temp_jsonl_file(&json);

        let (storage, warnings) = load_from_jsonl(file.path(), "test".to_string())
            .await
            .unwrap();

        assert!(warnings.is_empty());

        let loaded = storage
            .get(&IssueId::new("test-full"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(loaded.title, "Full Issue");
        assert_eq!(loaded.description, "Complete description");
        assert_eq!(loaded.status, IssueStatus::InProgress);
        assert_eq!(loaded.priority, 1);
        assert_eq!(loaded.issue_kind, IssueKind::Feature);
        assert_eq!(loaded.assignee, Some("alice".to_string()));
        assert_eq!(loaded.labels, vec!["backend", "urgent"]);
        assert_eq!(loaded.design, Some("Design notes here".to_string()));
        assert!(loaded.resources().is_empty());
        assert_eq!(loaded.notes().len(), 2);
        assert_eq!(loaded.notes()[0].content(), "Implementation notes");
        assert_eq!(loaded.notes()[0].created_at().to_rfc3339(), now);
        assert_eq!(
            loaded.notes()[1].content(),
            "Migrated legacy external reference: GH-123"
        );
        assert_eq!(loaded.notes()[1].created_at().to_rfc3339(), now);
    }

    #[tokio::test]
    async fn legacy_web_external_ref_migrates_to_reference_resource_idempotently() {
        let json = r#"{"id":"test-web-ref","title":"Legacy Web Ref","description":"Test","status":"open","priority":2,"issue_type":"task","assignee":null,"labels":[],"design":null,"acceptance_criteria":null,"notes":null,"external_ref":"HTTPS://EXAMPLE.com/pr/1","dependencies":[],"created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-02T03:04:05Z","closed_at":null}"#;
        let file = create_temp_jsonl_file(json);

        let (storage, warnings) = load_from_jsonl(file.path(), "test".to_string())
            .await
            .expect("legacy Web URL should load");
        assert!(warnings.is_empty());
        let issue = storage
            .get(&IssueId::new("test-web-ref"))
            .await
            .expect("lookup should succeed")
            .expect("Issue should load");
        assert!(issue.notes().is_empty());
        assert_eq!(issue.resources().len(), 1);
        let resource = &issue.resources()[0];
        assert_eq!(resource.id().as_str(), "r1");
        assert_eq!(resource.target().to_string(), "https://example.com/pr/1");
        assert_eq!(resource.role(), ResourceRole::Reference);
        assert!(resource.label().is_none());

        save_to_jsonl(storage.as_ref(), file.path())
            .await
            .expect("canonical save should succeed");
        let canonical = std::fs::read(file.path()).expect("canonical file should be readable");
        let record: serde_json::Value =
            serde_json::from_slice(&canonical).expect("canonical record should be JSON");
        assert!(record.get("external_ref").is_none());
        assert_eq!(record["resources"][0]["id"], "r1");
        assert_eq!(record["resources"][0]["target"]["type"], "web");
        assert_eq!(
            record["resources"][0]["target"]["url"],
            "https://example.com/pr/1"
        );
        assert_eq!(record["resources"][0]["role"], "reference");
        assert_eq!(record["next_resource_id"], 2);

        let (reloaded, warnings) = load_from_jsonl(file.path(), "test".to_string())
            .await
            .expect("canonical record should reload");
        assert!(warnings.is_empty());
        save_to_jsonl(reloaded.as_ref(), file.path())
            .await
            .expect("repeat canonical save should succeed");
        assert_eq!(
            std::fs::read(file.path()).expect("repeat saved file should be readable"),
            canonical
        );
    }

    #[tokio::test]
    async fn opaque_legacy_external_ref_becomes_migration_note_idempotently() {
        let json = r#"{"id":"test-opaque-ref","title":"Opaque Ref","description":"Test","status":"open","priority":2,"issue_type":"task","assignee":null,"labels":[],"design":null,"acceptance_criteria":null,"notes":null,"external_ref":"  GH-123  ","dependencies":[],"created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-02T03:04:05Z","closed_at":null}"#;
        let file = create_temp_jsonl_file(json);

        let (storage, warnings) = load_from_jsonl(file.path(), "test".to_string())
            .await
            .expect("opaque legacy reference should load");
        assert!(warnings.is_empty());
        let issue = storage
            .get(&IssueId::new("test-opaque-ref"))
            .await
            .expect("lookup should succeed")
            .expect("Issue should load");
        assert!(issue.resources().is_empty());
        assert_eq!(issue.notes().len(), 1);
        assert_eq!(
            issue.notes()[0].content(),
            "Migrated legacy external reference:   GH-123  "
        );
        assert_eq!(
            issue.notes()[0].created_at().to_rfc3339(),
            "2026-01-02T03:04:05+00:00"
        );

        save_to_jsonl(storage.as_ref(), file.path())
            .await
            .expect("canonical save should succeed");
        let canonical = std::fs::read(file.path()).expect("canonical file should be readable");
        let record: serde_json::Value =
            serde_json::from_slice(&canonical).expect("canonical record should be JSON");
        assert!(record.get("external_ref").is_none());
        assert_eq!(record["resources"], serde_json::json!([]));
        assert_eq!(
            record["notes"][0]["content"],
            "Migrated legacy external reference:   GH-123  "
        );
        assert_eq!(record["notes"][0]["created_at"], "2026-01-02T03:04:05Z");

        let (reloaded, warnings) = load_from_jsonl(file.path(), "test".to_string())
            .await
            .expect("canonical record should reload");
        assert!(warnings.is_empty());
        save_to_jsonl(reloaded.as_ref(), file.path())
            .await
            .expect("repeat canonical save should succeed");
        assert_eq!(
            std::fs::read(file.path()).expect("repeat saved file should be readable"),
            canonical
        );
    }

    #[tokio::test]
    async fn whitespace_only_legacy_external_ref_is_preserved_as_a_note() {
        let json = r#"{"id":"test-whitespace-ref","title":"Whitespace Ref","description":"Test","status":"open","priority":2,"issue_type":"task","assignee":null,"labels":[],"design":null,"acceptance_criteria":null,"notes":null,"external_ref":" \t ","dependencies":[],"created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-02T03:04:05Z","closed_at":null}"#;
        let file = create_temp_jsonl_file(json);

        let (storage, warnings) = load_from_jsonl(file.path(), "test".to_string())
            .await
            .expect("whitespace-only legacy reference should load");
        assert!(warnings.is_empty());
        let issue = storage
            .get(&IssueId::new("test-whitespace-ref"))
            .await
            .expect("lookup should succeed")
            .expect("Issue should load");
        assert!(issue.resources().is_empty());
        assert_eq!(issue.notes().len(), 1);
        assert_eq!(
            issue.notes()[0].content(),
            "Migrated legacy external reference:  \t "
        );

        save_to_jsonl(storage.as_ref(), file.path())
            .await
            .expect("canonical save should succeed");
        let record: serde_json::Value =
            serde_json::from_reader(std::fs::File::open(file.path()).unwrap())
                .expect("canonical record should be JSON");
        assert!(record.get("external_ref").is_none());
        assert_eq!(
            record["notes"][0]["content"],
            "Migrated legacy external reference:  \t "
        );
    }

    #[tokio::test]
    async fn unsafe_controls_in_opaque_external_ref_are_preserved_visibly() {
        let json = r#"{"id":"test-control-ref","title":"Control Ref","description":"Test","status":"open","priority":2,"issue_type":"task","assignee":null,"labels":[],"design":null,"acceptance_criteria":null,"notes":null,"external_ref":"\u001bC:\\tmp\n","dependencies":[],"created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-02T03:04:05Z","closed_at":null}"#;
        let file = create_temp_jsonl_file(json);

        let (storage, warnings) = load_from_jsonl(file.path(), "test".to_string())
            .await
            .expect("unsafe controls should migrate visibly");
        assert!(warnings.is_empty());
        let issue = storage
            .get(&IssueId::new("test-control-ref"))
            .await
            .expect("lookup should succeed")
            .expect("Issue should load");
        assert!(issue.resources().is_empty());
        assert_eq!(issue.notes().len(), 1);
        assert_eq!(
            issue.notes()[0].content(),
            "Migrated legacy external reference: \\u{1b}C:\\\\tmp\n"
        );

        save_to_jsonl(storage.as_ref(), file.path())
            .await
            .expect("canonical save should succeed");
        let record: serde_json::Value =
            serde_json::from_reader(std::fs::File::open(file.path()).unwrap())
                .expect("canonical record should be JSON");
        assert!(record.get("external_ref").is_none());
        assert_eq!(
            record["notes"][0]["content"],
            "Migrated legacy external reference: \\u{1b}C:\\\\tmp\n"
        );
    }

    #[tokio::test]
    async fn empty_legacy_external_ref_loads_without_resources_or_notes() {
        let json = r#"{"id":"test-empty-ref","title":"Empty Ref","description":"Test","status":"open","priority":2,"issue_type":"task","assignee":null,"labels":[],"design":null,"acceptance_criteria":null,"notes":null,"external_ref":"","dependencies":[],"created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z","closed_at":null}"#;
        let file = create_temp_jsonl_file(json);

        let (storage, warnings) = load_from_jsonl(file.path(), "test".to_string())
            .await
            .expect("empty legacy reference should load");
        assert!(warnings.is_empty());
        let issue = storage
            .get(&IssueId::new("test-empty-ref"))
            .await
            .expect("lookup should succeed")
            .expect("Issue should load");
        assert!(issue.resources().is_empty());
        assert!(issue.notes().is_empty());
    }

    #[tokio::test]
    async fn empty_canonical_resource_id_is_rejected_with_visible_warning() {
        let json = r#"{"id":"test-empty-resource-id","title":"Bad Resource","description":"Test","status":"open","priority":2,"issue_kind":"task","assignee":null,"labels":[],"design":null,"acceptance_criteria":null,"notes":[],"resources":[{"id":"","target":{"type":"web","url":"https://example.com"},"role":"reference","label":null}],"dependencies":[],"created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z","closed_at":null}"#;
        let file = create_temp_jsonl_file(json);

        let (storage, warnings) = load_from_jsonl(file.path(), "test".to_string())
            .await
            .expect("resilient load should report invalid resource ID");
        assert_eq!(warnings.len(), 1);
        match &warnings[0] {
            LoadWarning::InvalidResourceData { source, .. } => {
                assert!(matches!(source, ResourceError::EmptyResourceId));
            }
            warning => panic!("expected InvalidResourceData warning, got {warning:?}"),
        }
        assert!(
            storage
                .export_all()
                .await
                .expect("export should succeed")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn control_character_resource_id_is_rejected_with_visible_warning() {
        let json = r#"{"id":"test-control-resource-id","title":"Bad Resource","description":"Test","status":"open","priority":2,"issue_kind":"task","assignee":null,"labels":[],"design":null,"acceptance_criteria":null,"notes":[],"resources":[{"id":"r1\u001b","target":{"type":"web","url":"https://example.com"},"role":"reference","label":null}],"dependencies":[],"created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z","closed_at":null}"#;
        let file = create_temp_jsonl_file(json);

        let (storage, warnings) = load_from_jsonl(file.path(), "test".to_string())
            .await
            .expect("resilient load should report unsafe resource ID");
        assert_eq!(warnings.len(), 1);
        match &warnings[0] {
            LoadWarning::InvalidResourceData { source, .. } => {
                assert!(matches!(
                    source,
                    ResourceError::ResourceIdControlCharacter { .. }
                ));
            }
            warning => panic!("expected InvalidResourceData warning, got {warning:?}"),
        }
        assert!(
            storage
                .export_all()
                .await
                .expect("export should succeed")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn escaping_workspace_path_resource_is_rejected_with_visible_warning() {
        let json = r#"{"id":"test-escape-path","title":"Bad Path","description":"Test","status":"open","priority":2,"issue_kind":"task","assignee":null,"labels":[],"design":null,"acceptance_criteria":null,"notes":[],"resources":[{"id":"r1","target":{"type":"path","path":"../../etc/passwd"},"role":"reference","label":null}],"dependencies":[],"created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z","closed_at":null}"#;
        let file = create_temp_jsonl_file(json);

        let (storage, warnings) = load_from_jsonl(file.path(), "test".to_string())
            .await
            .expect("resilient load should report an escaping workspace path");
        assert_eq!(warnings.len(), 1);
        match &warnings[0] {
            LoadWarning::InvalidResourceData { source, .. } => {
                assert!(matches!(source, ResourceError::WorkspacePathEscape { .. }));
            }
            warning => panic!("expected InvalidResourceData warning, got {warning:?}"),
        }
        assert!(
            storage
                .export_all()
                .await
                .expect("export should succeed")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn workspace_path_resource_round_trips_normalized() {
        let json = r#"{"id":"test-path-resource","title":"Path Resource","description":"Test","status":"open","priority":2,"issue_kind":"task","assignee":null,"labels":[],"design":null,"acceptance_criteria":null,"notes":[],"resources":[{"id":"r1","target":{"type":"path","path":"docs/../docs/adr/0003.md"},"role":"documentation","label":"ADR"}],"next_resource_id":2,"dependencies":[],"created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z","closed_at":null}"#;
        let file = create_temp_jsonl_file(json);

        let (storage, warnings) = load_from_jsonl(file.path(), "test".to_string())
            .await
            .expect("valid path resource should load");
        assert!(warnings.is_empty());
        let issue = storage
            .get(&IssueId::new("test-path-resource"))
            .await
            .expect("get should succeed")
            .expect("issue should exist");
        assert_eq!(issue.resources().len(), 1);
        // The persisted raw form is re-validated and normalized on load.
        assert_eq!(
            issue.resources()[0].target().to_string(),
            "docs/adr/0003.md"
        );
        assert_eq!(issue.resources()[0].role(), ResourceRole::Documentation);

        // Saving writes the canonical normalized path record.
        let out = create_temp_jsonl_file("");
        save_to_jsonl(storage.as_ref(), out.path())
            .await
            .expect("save should succeed");
        let written = std::fs::read_to_string(out.path()).expect("saved file should be readable");
        let record: serde_json::Value =
            serde_json::from_str(written.lines().next().expect("one record"))
                .expect("record should be JSON");
        assert_eq!(
            record["resources"][0]["target"],
            serde_json::json!({"type":"path","path":"docs/adr/0003.md"})
        );
    }

    #[tokio::test]
    async fn load_preserves_valid_dependencies() {
        let content = format!(
            "{}\n{}",
            create_valid_issue_json("test-1", "Dependency Target"),
            create_issue_with_dependency_json("test-2", "Has Dependency", "test-1", "blocks")
        );
        let file = create_temp_jsonl_file(&content);

        let (storage, warnings) = load_from_jsonl(file.path(), "test".to_string())
            .await
            .unwrap();

        assert!(warnings.is_empty());

        let deps = storage
            .get_dependencies(&IssueId::new("test-2"))
            .await
            .unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].depends_on_id.as_str(), "test-1");
        assert_eq!(deps[0].dep_type, DependencyType::Blocks);
    }

    #[tokio::test]
    async fn conflicting_canonical_and_legacy_kind_fields_keep_canonical_record() {
        let content = r#"{"id":"test-conflict","title":"Conflict","description":"Test","status":"open","priority":2,"issue_kind":"feature","issue_type":"task","assignee":null,"labels":[],"design":null,"acceptance_criteria":null,"notes":null,"external_ref":null,"dependencies":[],"created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z","closed_at":null}"#;
        let file = create_temp_jsonl_file(content);

        let (storage, warnings) = load_from_jsonl(file.path(), "test".to_string())
            .await
            .expect("load should succeed and report the conflict as a warning");

        let issues = storage
            .export_all()
            .await
            .expect("export_all should succeed");
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].id.as_str(), "test-conflict");
        assert_eq!(issues[0].issue_kind, IssueKind::Feature);
        assert_eq!(warnings.len(), 1);
        match &warnings[0] {
            LoadWarning::MigrationConflict {
                issue_id,
                line_number,
                field,
            } => {
                assert_eq!(issue_id.as_str(), "test-conflict");
                assert_eq!(*line_number, 1);
                assert_eq!(*field, MigrationField::IssueKind);
                assert_eq!(field.emitted_name(), "issue_kind");
                assert_eq!(field.accepted_migration_name(), "issue_type");
                assert_ne!(field.emitted_name(), field.accepted_migration_name());
            }
            warning => panic!("Expected MigrationConflict warning, got {warning:?}"),
        }
    }

    #[tokio::test]
    async fn legacy_note_preserves_exact_content_and_update_timestamp() {
        let content = r#"{"id":"test-note","title":"Legacy Note","description":"Test","status":"open","priority":2,"issue_type":"task","assignee":null,"labels":[],"design":null,"acceptance_criteria":null,"notes":"Line 1\n\nLine 2  ","external_ref":null,"dependencies":[],"created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-02T03:04:05Z","closed_at":null}"#;
        let file = create_temp_jsonl_file(content);

        let (storage, warnings) = load_from_jsonl(file.path(), "test".to_string())
            .await
            .expect("legacy Note should load");
        assert!(warnings.is_empty());

        let issue = storage
            .get(&IssueId::new("test-note"))
            .await
            .expect("lookup should succeed")
            .expect("legacy Issue should load");
        assert_eq!(issue.notes().len(), 1);
        assert_eq!(issue.notes()[0].content(), "Line 1\n\nLine 2  ");
        assert_eq!(
            issue.notes()[0].created_at().to_rfc3339(),
            "2026-01-02T03:04:05+00:00"
        );
    }

    #[tokio::test]
    async fn missing_and_null_legacy_notes_load_as_empty_histories() {
        let content = concat!(
            r#"{"id":"test-missing","title":"Missing","description":"Test","status":"open","priority":2,"issue_type":"task","assignee":null,"labels":[],"design":null,"acceptance_criteria":null,"external_ref":null,"dependencies":[],"created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z","closed_at":null}"#,
            "\n",
            r#"{"id":"test-null","title":"Null","description":"Test","status":"open","priority":2,"issue_type":"task","assignee":null,"labels":[],"design":null,"acceptance_criteria":null,"notes":null,"external_ref":null,"dependencies":[],"created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z","closed_at":null}"#,
            "\n",
            r#"{"id":"test-whitespace","title":"Whitespace","description":"Test","status":"open","priority":2,"issue_type":"task","assignee":null,"labels":[],"design":null,"acceptance_criteria":null,"notes":" \t ","external_ref":null,"dependencies":[],"created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z","closed_at":null}"#
        );
        let file = create_temp_jsonl_file(content);

        let (storage, warnings) = load_from_jsonl(file.path(), "test".to_string())
            .await
            .expect("legacy empty histories should load");
        assert!(warnings.is_empty());
        for id in ["test-missing", "test-null", "test-whitespace"] {
            let issue = storage
                .get(&IssueId::new(id))
                .await
                .expect("lookup should succeed")
                .expect("Issue should load");
            assert!(issue.notes().is_empty());
        }
    }

    #[tokio::test]
    async fn malformed_note_shape_is_rejected_with_a_warning() {
        let content = r#"{"id":"test-malformed","title":"Malformed","description":"Test","status":"open","priority":2,"issue_type":"task","assignee":null,"labels":[],"design":null,"acceptance_criteria":null,"notes":{},"external_ref":null,"dependencies":[],"created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z","closed_at":null}"#;
        let file = create_temp_jsonl_file(content);

        let (storage, warnings) = load_from_jsonl(file.path(), "test".to_string())
            .await
            .expect("resilient load should report malformed Note data");
        assert_eq!(warnings.len(), 1);
        match &warnings[0] {
            LoadWarning::MalformedJson { line_number, error } => {
                assert_eq!(*line_number, 1);
                assert!(
                    !error.is_empty(),
                    "warning should describe malformed Note data"
                );
            }
            warning => panic!("expected malformed Note warning, got {warning:?}"),
        }
        assert!(
            storage
                .export_all()
                .await
                .expect("export should succeed")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn domain_conversion_warning_preserves_physical_line_number() {
        let content = concat!(
            "not valid JSON\n",
            "\n",
            r#"{"id":"test-invalid","title":"Invalid","description":"Test","status":"open","priority":2,"assignee":null,"labels":[],"design":null,"acceptance_criteria":null,"notes":null,"external_ref":null,"dependencies":[],"created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z","closed_at":null}"#
        );
        let file = create_temp_jsonl_file(content);

        let (_, warnings) = load_from_jsonl(file.path(), "test".to_string())
            .await
            .expect("resilient load should report both skipped records");
        assert_eq!(warnings.len(), 2);
        assert!(matches!(
            &warnings[1],
            LoadWarning::InvalidIssueData {
                issue_id,
                line_number: 3,
                ..
            } if issue_id == &IssueId::new("test-invalid")
        ));
    }

    #[tokio::test]
    async fn invalid_record_with_conflicting_kind_fields_reports_both_warnings() {
        let content = r#"{"id":"test-conflict","title":"Conflict","description":"Test","status":"open","priority":10,"issue_kind":"feature","issue_type":"task","assignee":null,"labels":[],"design":null,"acceptance_criteria":null,"notes":null,"external_ref":null,"dependencies":[],"created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z","closed_at":null}"#;
        let file = create_temp_jsonl_file(content);

        let (storage, warnings) = load_from_jsonl(file.path(), "test".to_string())
            .await
            .expect("load should succeed and report both data problems");

        assert!(
            storage
                .export_all()
                .await
                .expect("export_all should succeed")
                .is_empty()
        );
        assert_eq!(warnings.len(), 2);
        assert!(warnings.iter().any(|warning| matches!(
            warning,
            LoadWarning::MigrationConflict {
                issue_id,
                field: MigrationField::IssueKind,
                ..
            } if issue_id.as_str() == "test-conflict"
        )));
        assert!(warnings.iter().any(|warning| matches!(
            warning,
            LoadWarning::InvalidIssueData { issue_id, .. }
                if issue_id.as_str() == "test-conflict"
        )));
    }

    #[tokio::test]
    async fn resolvable_kind_conflict_does_not_block_subsequent_writes() {
        let content = r#"{"id":"test-conflict","title":"Conflict","description":"Test","status":"open","priority":2,"issue_kind":"feature","issue_type":"task","assignee":null,"labels":[],"design":null,"acceptance_criteria":null,"notes":null,"external_ref":null,"dependencies":[],"created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z","closed_at":null}"#;
        let file = create_temp_jsonl_file(content);

        let mut storage = create_storage(
            StorageBackend::Jsonl(file.path().to_path_buf()),
            "test".to_string(),
        )
        .await
        .expect("storage should open with a MigrationConflict warning");

        // The conflicting record is loaded with the canonical field winning.
        let issue = storage
            .get(&IssueId::new("test-conflict"))
            .await
            .expect("lookup should succeed")
            .expect("conflicting record should still load");
        assert_eq!(issue.issue_kind, IssueKind::Feature);

        // Writes must not be blocked: the warning is informational, not a skip.
        storage
            .create(NewIssue {
                title: "Post-load write".to_string(),
                description: String::new(),
                priority: 2,
                issue_kind: IssueKind::Task,
                assignee: None,
                labels: Vec::new(),
                design: None,
                acceptance_criteria: None,
                initial_note: None,
                dependencies: Vec::new(),
            })
            .await
            .expect("create should succeed after a MigrationConflict warning");
        storage
            .save()
            .await
            .expect("save should succeed after a MigrationConflict warning");

        // The saved file still contains the migrated record.
        let (reloaded, _) = load_from_jsonl(file.path(), "test".to_string())
            .await
            .expect("reload should succeed");
        let issues = reloaded
            .export_all()
            .await
            .expect("export_all should succeed");
        assert_eq!(issues.len(), 2);
    }

    #[tokio::test]
    async fn load_nonexistent_file_returns_error() {
        let result = load_from_jsonl(
            std::path::Path::new("/nonexistent/file.jsonl"),
            "test".to_string(),
        )
        .await;
        assert!(result.is_err());
    }
}

// =============================================================================
// Storage Operations After Resilient Loading
// =============================================================================

mod storage_after_load_tests {
    use super::*;

    #[tokio::test]
    async fn can_create_new_issues_after_resilient_load() {
        let line1 = create_valid_issue_json("test-1", "Existing 1");
        let line3 = create_valid_issue_json("test-3", "Existing 2");
        let content = format!("{}\n{{invalid}}\n{}", line1, line3);
        let file = create_temp_jsonl_file(&content);

        let (mut storage, _) = load_from_jsonl(file.path(), "test".to_string())
            .await
            .unwrap();

        // Create a new issue
        let new_issue = create_test_issue("New Issue");
        let created = storage.create(new_issue).await.unwrap();

        assert!(created.id.as_str().starts_with("test-"));
        assert_eq!(created.title, "New Issue");

        // Verify all issues exist
        let all_issues = storage.export_all().await.unwrap();
        assert_eq!(all_issues.len(), 3);
    }

    #[tokio::test]
    async fn can_add_dependencies_after_resilient_load() {
        let content = format!(
            "{}\n{}",
            create_valid_issue_json("test-1", "Issue 1"),
            create_valid_issue_json("test-2", "Issue 2")
        );
        let file = create_temp_jsonl_file(&content);

        let (mut storage, _) = load_from_jsonl(file.path(), "test".to_string())
            .await
            .unwrap();

        // Add a dependency
        storage
            .add_dependency(
                &IssueId::new("test-2"),
                &IssueId::new("test-1"),
                DependencyType::Blocks,
            )
            .await
            .unwrap();

        let deps = storage
            .get_dependencies(&IssueId::new("test-2"))
            .await
            .unwrap();
        assert_eq!(deps.len(), 1);
    }

    #[tokio::test]
    async fn can_update_issues_after_resilient_load() {
        let content = create_valid_issue_json("test-1", "Original Title");
        let file = create_temp_jsonl_file(&content);

        let (mut storage, _) = load_from_jsonl(file.path(), "test".to_string())
            .await
            .unwrap();

        // Update the issue
        let update = rivets::domain::IssueUpdate {
            title: Some("Updated Title".to_string()),
            status: Some(IssueStatus::InProgress),
            ..Default::default()
        };

        storage
            .update(&IssueId::new("test-1"), update)
            .await
            .unwrap();

        let updated = storage.get(&IssueId::new("test-1")).await.unwrap().unwrap();
        assert_eq!(updated.title, "Updated Title");
        assert_eq!(updated.status, IssueStatus::InProgress);
    }

    #[tokio::test]
    async fn id_generator_registered_after_resilient_load() {
        let content = format!(
            "{}\n{}",
            create_valid_issue_json("test-abc1", "Issue 1"),
            create_valid_issue_json("test-xyz2", "Issue 2")
        );
        let file = create_temp_jsonl_file(&content);

        let (mut storage, _) = load_from_jsonl(file.path(), "test".to_string())
            .await
            .unwrap();

        // Create new issues and verify IDs don't collide
        let new1 = storage.create(create_test_issue("New 1")).await.unwrap();
        let new2 = storage.create(create_test_issue("New 2")).await.unwrap();

        assert_ne!(new1.id.as_str(), "test-abc1");
        assert_ne!(new1.id.as_str(), "test-xyz2");
        assert_ne!(new2.id.as_str(), "test-abc1");
        assert_ne!(new2.id.as_str(), "test-xyz2");
        assert_ne!(new1.id.as_str(), new2.id.as_str());
    }
}

// =============================================================================
// Round-Trip Persistence Tests
// =============================================================================

mod round_trip_tests {
    use super::*;

    #[tokio::test]
    async fn save_and_reload_preserves_issues() {
        // Create storage and add issues
        let mut storage = new_in_memory_storage("test".to_string());

        let issue1 = storage.create(create_test_issue("Issue 1")).await.unwrap();
        let issue2 = storage.create(create_test_issue("Issue 2")).await.unwrap();

        // Save to file
        let file = NamedTempFile::new().unwrap();
        save_to_jsonl(storage.as_ref(), file.path()).await.unwrap();

        // Reload
        let (reloaded, warnings) = load_from_jsonl(file.path(), "test".to_string())
            .await
            .unwrap();

        assert!(warnings.is_empty());

        let loaded1 = reloaded.get(&issue1.id).await.unwrap().unwrap();
        let loaded2 = reloaded.get(&issue2.id).await.unwrap().unwrap();

        assert_eq!(loaded1.title, "Issue 1");
        assert_eq!(loaded2.title, "Issue 2");
    }

    #[tokio::test]
    async fn save_and_reload_preserves_dependencies() {
        let mut storage = new_in_memory_storage("test".to_string());

        let issue1 = storage.create(create_test_issue("Blocker")).await.unwrap();
        let issue2 = storage.create(create_test_issue("Blocked")).await.unwrap();

        storage
            .add_dependency(&issue2.id, &issue1.id, DependencyType::Blocks)
            .await
            .unwrap();

        let file = NamedTempFile::new().unwrap();
        save_to_jsonl(storage.as_ref(), file.path()).await.unwrap();

        let (reloaded, warnings) = load_from_jsonl(file.path(), "test".to_string())
            .await
            .unwrap();

        assert!(warnings.is_empty());

        let deps = reloaded.get_dependencies(&issue2.id).await.unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].depends_on_id, issue1.id);
        assert_eq!(deps[0].dep_type, DependencyType::Blocks);
    }

    #[tokio::test]
    async fn corrupted_file_gracefully_loads_valid_data() {
        // Create storage with issues
        let mut storage = new_in_memory_storage("test".to_string());
        let issue1 = storage.create(create_test_issue("Valid 1")).await.unwrap();
        let issue2 = storage.create(create_test_issue("Valid 2")).await.unwrap();

        // Save to file
        let file = NamedTempFile::new().unwrap();
        save_to_jsonl(storage.as_ref(), file.path()).await.unwrap();

        // Corrupt the file by appending invalid JSON
        {
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(file.path())
                .unwrap();
            writeln!(f, "{{invalid json}}").unwrap();
        }

        // Reload should still work with warnings
        let (reloaded, warnings) = load_from_jsonl(file.path(), "test".to_string())
            .await
            .unwrap();

        assert_eq!(warnings.len(), 1);

        // Valid issues should still be there
        let loaded1 = reloaded.get(&issue1.id).await.unwrap();
        let loaded2 = reloaded.get(&issue2.id).await.unwrap();
        assert!(loaded1.is_some());
        assert!(loaded2.is_some());
    }

    #[tokio::test]
    async fn multiple_round_trips_preserve_data() {
        let mut storage = new_in_memory_storage("test".to_string());

        let issue1 = storage.create(create_test_issue("Issue 1")).await.unwrap();

        // First save and reload
        let file1 = NamedTempFile::new().unwrap();
        save_to_jsonl(storage.as_ref(), file1.path()).await.unwrap();

        let (mut storage2, _) = load_from_jsonl(file1.path(), "test".to_string())
            .await
            .unwrap();

        // Add more data
        let issue2 = storage2.create(create_test_issue("Issue 2")).await.unwrap();
        storage2
            .add_dependency(&issue2.id, &issue1.id, DependencyType::Related)
            .await
            .unwrap();

        // Second save and reload
        let file2 = NamedTempFile::new().unwrap();
        save_to_jsonl(storage2.as_ref(), file2.path())
            .await
            .unwrap();

        let (storage3, warnings) = load_from_jsonl(file2.path(), "test".to_string())
            .await
            .unwrap();

        assert!(warnings.is_empty());

        let all_issues = storage3.export_all().await.unwrap();
        assert_eq!(all_issues.len(), 2);

        let deps = storage3.get_dependencies(&issue2.id).await.unwrap();
        assert_eq!(deps.len(), 1);
    }
    #[tokio::test]
    async fn saving_legacy_kind_records_canonicalizes_once() {
        let original = concat!(
            r#"{"id":"test-a","title":"First","description":"Test","status":"open","priority":1,"issue_type":"bug","assignee":"alice","labels":["backend"],"design":"Plan","acceptance_criteria":"Done","notes":"History","external_ref":"GH-1","dependencies":[],"created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-02T00:00:00Z","closed_at":null}"#,
            "\n",
            r#"{"id":"test-b","title":"Second","description":"Test","status":"in_progress","priority":2,"issue_type":"task","assignee":null,"labels":[],"design":null,"acceptance_criteria":null,"notes":null,"external_ref":null,"dependencies":[{"depends_on_id":"test-a","dep_type":"blocks"}],"created_at":"2026-01-03T00:00:00Z","updated_at":"2026-01-04T00:00:00Z","closed_at":null}"#,
            "\n",
        );
        let file = create_temp_jsonl_file(original);

        let (storage, warnings) = load_from_jsonl(file.path(), "test".to_string())
            .await
            .expect("legacy records should load without error");
        assert!(warnings.is_empty());

        save_to_jsonl(storage.as_ref(), file.path())
            .await
            .expect("save should succeed");

        let canonical =
            std::fs::read(file.path()).expect("canonical saved file should be readable");
        let canonical_text = String::from_utf8(canonical.clone()).expect("JSONL should be UTF-8");
        assert!(!canonical_text.contains("\"issue_type\""));
        assert!(canonical_text.contains("\"issue_kind\":\"bug\""));
        assert!(canonical_text.contains("\"issue_kind\":\"task\""));
        let records: Vec<serde_json::Value> = canonical_text
            .lines()
            .map(|line| serde_json::from_str(line).expect("canonical record should be JSON"))
            .collect();
        assert_eq!(records[0]["notes"][0]["content"], "History");
        assert_eq!(records[0]["notes"][0]["created_at"], "2026-01-02T00:00:00Z");
        assert_eq!(records[1]["notes"], serde_json::json!([]));

        let (reloaded, warnings) = load_from_jsonl(file.path(), "test".to_string())
            .await
            .expect("canonical records should reload");
        assert!(warnings.is_empty());
        save_to_jsonl(reloaded.as_ref(), file.path())
            .await
            .expect("repeat canonical save should succeed");
        assert_eq!(
            std::fs::read(file.path()).expect("repeat saved file should be readable"),
            canonical
        );
    }
}

// =============================================================================
// Large Dataset Tests
// =============================================================================

mod large_dataset_tests {
    use super::*;

    #[tokio::test]
    async fn load_large_file_with_sparse_errors() {
        const TOTAL_ISSUES: usize = 100;
        const ERROR_RATE: usize = 10; // 1 in 10 lines is an error

        let mut lines = Vec::new();
        let mut valid_count = 0;

        for i in 0..TOTAL_ISSUES {
            if i % ERROR_RATE == 5 {
                lines.push("{invalid json}".to_string());
            } else {
                lines.push(create_valid_issue_json(
                    &format!("test-{}", valid_count),
                    &format!("Issue {}", valid_count),
                ));
                valid_count += 1;
            }
        }

        let content = lines.join("\n");
        let file = create_temp_jsonl_file(&content);

        let (storage, warnings) = load_from_jsonl(file.path(), "test".to_string())
            .await
            .unwrap();

        // Should have warnings for each error line
        assert_eq!(warnings.len(), TOTAL_ISSUES / ERROR_RATE);

        // Should have loaded all valid issues
        let all_issues = storage.export_all().await.unwrap();
        assert_eq!(all_issues.len(), valid_count);
    }

    #[tokio::test]
    async fn load_performance_with_many_issues() {
        use std::time::Instant;

        const ISSUE_COUNT: usize = 1000;

        let lines: Vec<String> = (0..ISSUE_COUNT)
            .map(|i| create_valid_issue_json(&format!("test-{}", i), &format!("Issue {}", i)))
            .collect();

        let content = lines.join("\n");
        let file = create_temp_jsonl_file(&content);

        let start = Instant::now();
        let (storage, warnings) = load_from_jsonl(file.path(), "test".to_string())
            .await
            .unwrap();
        let duration = start.elapsed();

        assert!(warnings.is_empty());

        let all_issues = storage.export_all().await.unwrap();
        assert_eq!(all_issues.len(), ISSUE_COUNT);

        // Should complete in reasonable time (< 5 seconds even in CI)
        assert!(
            duration.as_secs() < 5,
            "Loading {} issues took {:?}, expected < 5s",
            ISSUE_COUNT,
            duration
        );

        println!("Loaded {} issues in {:?}", ISSUE_COUNT, duration);
    }
}
#[tokio::test]
async fn mixed_legacy_fixture_round_trips_to_stable_canonical_jsonl() {
    let file = create_temp_jsonl_file(MIXED_LEGACY_JSONL);
    let (storage, warnings) = load_from_jsonl(file.path(), "test".to_string())
        .await
        .expect("mixed legacy fixture should load");

    let issues = storage
        .export_all()
        .await
        .expect("all loaded Issues should be exportable");
    assert_eq!(issues.len(), MIXED_ISSUE_COUNT);

    assert_eq!(warnings.len(), 1);
    match &warnings[0] {
        LoadWarning::MigrationConflict {
            issue_id,
            line_number,
            field,
        } => {
            assert_eq!(issue_id.as_str(), CONFLICT_ID);
            assert_eq!(*line_number, 8);
            assert_eq!(*field, MigrationField::IssueKind);
        }
        warning => panic!("Expected MigrationConflict warning, got {warning:?}"),
    }

    for (id, expected_kind) in [
        ("test-missing", IssueKind::Bug),
        ("test-null", IssueKind::Feature),
        ("test-note", IssueKind::Task),
        ("test-url", IssueKind::Epic),
        ("test-opaque", IssueKind::Chore),
    ] {
        let issue = storage
            .get(&IssueId::new(id))
            .await
            .expect("Issue lookup should succeed")
            .expect("fixture Issue should be loaded");
        assert_eq!(issue.issue_kind, expected_kind);
    }

    let conflict = storage
        .get(&IssueId::new(CONFLICT_ID))
        .await
        .expect("conflict Issue lookup should succeed")
        .expect("conflict Issue should be loaded");
    assert_eq!(conflict.issue_kind, IssueKind::Feature);

    let fixture = fixture_records();
    let expected_note = record(&fixture, LEGACY_NOTE_ID)["notes"]
        .as_str()
        .expect("legacy Note fixture should be a string");
    let legacy_note = storage
        .get(&IssueId::new(LEGACY_NOTE_ID))
        .await
        .expect("legacy Note Issue lookup should succeed")
        .expect("legacy Note Issue should be loaded");
    assert_eq!(
        legacy_note.notes()[0].content().as_bytes(),
        expected_note.as_bytes()
    );
    assert_eq!(legacy_note.notes()[0].created_at(), &legacy_note.updated_at);

    let legacy_url = storage
        .get(&IssueId::new(LEGACY_URL_ID))
        .await
        .expect("legacy URL Issue lookup should succeed")
        .expect("legacy URL Issue should be loaded");
    assert!(legacy_url.notes().is_empty());
    assert_eq!(legacy_url.resources().len(), 1);
    assert_eq!(legacy_url.resources()[0].id().as_str(), "r1");
    assert_eq!(
        legacy_url.resources()[0].target().to_string(),
        "https://example.com/legacy/pr/7"
    );
    assert_eq!(legacy_url.resources()[0].role(), ResourceRole::Reference);
    assert!(legacy_url.resources()[0].label().is_none());

    let opaque = storage
        .get(&IssueId::new(LEGACY_OPAQUE_ID))
        .await
        .expect("opaque reference Issue lookup should succeed")
        .expect("opaque reference Issue should be loaded");
    assert!(opaque.resources().is_empty());
    assert_eq!(opaque.notes().len(), 1);
    assert_eq!(
        opaque.notes()[0].content(),
        "Migrated legacy external reference:   GH-opaque-42  "
    );
    assert_eq!(opaque.notes()[0].created_at(), &opaque.updated_at);

    let canonical_issue = storage
        .get(&IssueId::new("test-canonical"))
        .await
        .expect("canonical Issue lookup should succeed")
        .expect("canonical Issue should be loaded");
    assert_eq!(
        canonical_issue
            .notes()
            .iter()
            .map(|note| note.content())
            .collect::<Vec<_>>(),
        ["Canonical first Note", "Canonical second Note"]
    );
    assert_eq!(canonical_issue.resources().len(), 2);
    assert_eq!(canonical_issue.resources()[0].id().as_str(), "r1");
    assert_eq!(
        canonical_issue.resources()[0].target().to_string(),
        "https://example.com/canonical"
    );
    assert_eq!(
        canonical_issue.resources()[0].role(),
        ResourceRole::Evidence
    );
    assert_eq!(canonical_issue.resources()[1].id().as_str(), "r2");
    assert_eq!(
        canonical_issue.resources()[1].target().to_string(),
        "docs/adr/0001-multiple-notes.md"
    );
    assert_eq!(
        canonical_issue.resources()[1].role(),
        ResourceRole::Documentation
    );

    save_to_jsonl(storage.as_ref(), file.path())
        .await
        .expect("canonical save should succeed");
    let canonical_bytes = std::fs::read(file.path()).expect("canonical file should be readable");
    let canonical_records = read_records(file.path());
    assert_canonical_records(&canonical_records);

    for (id, expected_kind) in [
        ("test-missing", "bug"),
        ("test-null", "feature"),
        (LEGACY_NOTE_ID, "task"),
        (LEGACY_URL_ID, "epic"),
        (LEGACY_OPAQUE_ID, "chore"),
    ] {
        assert_eq!(record(&canonical_records, id)["issue_kind"], expected_kind);
    }
    assert_eq!(
        record(&canonical_records, CONFLICT_ID)["issue_kind"],
        "feature"
    );
    assert_eq!(
        record(&canonical_records, LEGACY_NOTE_ID)["notes"][0]["content"],
        expected_note
    );
    assert_eq!(
        record(&canonical_records, LEGACY_NOTE_ID)["notes"][0]["created_at"],
        record(&canonical_records, LEGACY_NOTE_ID)["updated_at"]
    );
    let canonical_url = record(&canonical_records, LEGACY_URL_ID);
    assert!(canonical_url.get("external_ref").is_none());
    assert_eq!(canonical_url["resources"][0]["id"], "r1");
    assert_eq!(
        canonical_url["resources"][0]["target"]["url"],
        "https://example.com/legacy/pr/7"
    );
    assert_eq!(canonical_url["resources"][0]["role"], "reference");
    let canonical_opaque = record(&canonical_records, LEGACY_OPAQUE_ID);
    assert!(canonical_opaque.get("external_ref").is_none());
    assert_eq!(
        canonical_opaque["notes"][0]["content"],
        "Migrated legacy external reference:   GH-opaque-42  "
    );
    assert_eq!(
        canonical_opaque["notes"][0]["created_at"],
        canonical_opaque["updated_at"]
    );
    let canonical_history = record(&canonical_records, "test-canonical");
    assert_eq!(
        canonical_history["notes"][0]["content"],
        "Canonical first Note"
    );
    assert_eq!(
        canonical_history["notes"][1]["content"],
        "Canonical second Note"
    );
    assert_eq!(canonical_history["resources"][0]["id"], "r1");
    assert_eq!(canonical_history["resources"][1]["id"], "r2");

    let (reloaded, reload_warnings) = load_from_jsonl(file.path(), "test".to_string())
        .await
        .expect("canonical records should reload");
    assert!(reload_warnings.is_empty());
    save_to_jsonl(reloaded.as_ref(), file.path())
        .await
        .expect("repeat canonical save should succeed");
    assert_eq!(
        std::fs::read(file.path()).expect("repeat saved file should be readable"),
        canonical_bytes
    );
}

#[tokio::test]
async fn legacy_relationships_survive_blocking_mutations() {
    let content = concat!(
        r#"{"id":"test-dependent","title":"Dependent","description":"Test","status":"open","priority":2,"issue_type":"task","assignee":null,"labels":[],"design":null,"acceptance_criteria":null,"notes":null,"external_ref":null,"dependencies":[{"depends_on_id":"test-prerequisite","dep_type":"related"}],"created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z","closed_at":null}"#,
        "\n",
        r#"{"id":"test-prerequisite","title":"Prerequisite","description":"Test","status":"open","priority":2,"issue_type":"task","assignee":null,"labels":[],"design":null,"acceptance_criteria":null,"notes":null,"external_ref":null,"dependencies":[],"created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z","closed_at":null}"#,
        "\n",
    );
    let file = create_temp_jsonl_file(content);
    let (mut storage, warnings) = load_from_jsonl(file.path(), "test".to_string())
        .await
        .expect("legacy relationship fixture should load");
    assert!(warnings.is_empty());
    let dependency = BlockingDependency::new(
        IssueId::new("test-dependent"),
        IssueId::new("test-prerequisite"),
    )
    .unwrap();

    storage
        .add_blocking_dependency(dependency.clone())
        .await
        .expect("Blocking and Related should coexist");
    save_to_jsonl(storage.as_ref(), file.path()).await.unwrap();
    let records = std::fs::read_to_string(file.path()).unwrap();
    let dependent: serde_json::Value = records
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .find(|record: &serde_json::Value| record["id"] == "test-dependent")
        .unwrap();
    assert_eq!(
        dependent["dependencies"],
        serde_json::json!([
            {"depends_on_id": "test-prerequisite", "dep_type": "blocks"},
            {"depends_on_id": "test-prerequisite", "dep_type": "related"}
        ])
    );

    let (mut reloaded, warnings) = load_from_jsonl(file.path(), "test".to_string())
        .await
        .expect("mixed relationship record should reload");
    assert!(warnings.is_empty());
    reloaded
        .remove_blocking_dependency(&dependency)
        .await
        .expect("typed removal should find only Blocking");
    save_to_jsonl(reloaded.as_ref(), file.path()).await.unwrap();
    let records = std::fs::read_to_string(file.path()).unwrap();
    let dependent: serde_json::Value = records
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .find(|record: &serde_json::Value| record["id"] == "test-dependent")
        .unwrap();
    assert_eq!(
        dependent["dependencies"],
        serde_json::json!([
            {"depends_on_id": "test-prerequisite", "dep_type": "related"}
        ])
    );
}
