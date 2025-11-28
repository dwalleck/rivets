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
use rivets::domain::{DependencyType, Issue, IssueId, IssueStatus, IssueType, NewIssue};
use rivets::storage::in_memory::{
    load_from_jsonl, new_in_memory_storage, save_to_jsonl, LoadWarning,
};
use std::io::Write;
use tempfile::NamedTempFile;

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
        issue_type: IssueType::Task,
        assignee: None,
        labels: vec![],
        design: None,
        acceptance_criteria: None,
        notes: None,
        external_ref: None,
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
        assert!(storage
            .get(&IssueId::new("test-invalid"))
            .await
            .unwrap()
            .is_none());
        assert!(storage
            .get(&IssueId::new("test-valid"))
            .await
            .unwrap()
            .is_some());
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
                LoadWarning::CircularDependency { .. } => {}
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
        let now = Utc::now();
        let issue = Issue {
            id: IssueId::new("test-full"),
            title: "Full Issue".to_string(),
            description: "Complete description".to_string(),
            status: IssueStatus::InProgress,
            priority: 1,
            issue_type: IssueType::Feature,
            assignee: Some("alice".to_string()),
            labels: vec!["backend".to_string(), "urgent".to_string()],
            design: Some("Design notes here".to_string()),
            acceptance_criteria: Some("- Criterion 1\n- Criterion 2".to_string()),
            notes: Some("Implementation notes".to_string()),
            external_ref: Some("GH-123".to_string()),
            dependencies: vec![],
            created_at: now,
            updated_at: now,
            closed_at: None,
        };

        let json = serde_json::to_string(&issue).unwrap();
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
        assert_eq!(loaded.issue_type, IssueType::Feature);
        assert_eq!(loaded.assignee, Some("alice".to_string()));
        assert_eq!(loaded.labels, vec!["backend", "urgent"]);
        assert_eq!(loaded.design, Some("Design notes here".to_string()));
        assert_eq!(loaded.external_ref, Some("GH-123".to_string()));
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
