//! Integration tests for in-memory storage.
//!
//! These tests verify the full functionality of the in-memory storage backend,
//! including CRUD operations, dependency management, cycle detection, blocking
//! semantics, and sort policies.

use rivets::domain::{
    DependencyType, IssueFilter, IssueId, IssueStatus, IssueType, IssueUpdate, NewIssue, SortPolicy,
};
use rivets::error::Error;
use rivets::storage::in_memory::{load_from_jsonl, new_in_memory_storage, save_to_jsonl};
use rivets::storage::IssueStorage;
use rstest::rstest;
use tempfile::tempdir;

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

fn create_test_issue_with_priority(title: &str, priority: u8) -> NewIssue {
    NewIssue {
        title: title.to_string(),
        description: "Test description".to_string(),
        priority,
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

// ========== Basic CRUD Tests ==========

#[tokio::test]
async fn test_create_issue() {
    let mut storage = new_in_memory_storage("test".to_string());

    let new_issue = create_test_issue("Test Issue");
    let issue = storage.create(new_issue).await.unwrap();

    assert!(issue.id.as_str().starts_with("test-"));
    assert_eq!(issue.title, "Test Issue");
    assert_eq!(issue.status, IssueStatus::Open);
    assert_eq!(issue.priority, 2);
}

#[tokio::test]
async fn test_get_issue() {
    let mut storage = new_in_memory_storage("test".to_string());

    let new_issue = create_test_issue("Test Issue");
    let created = storage.create(new_issue).await.unwrap();

    // Get existing issue
    let retrieved = storage.get(&created.id).await.unwrap();
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().title, "Test Issue");

    // Get non-existing issue
    let non_existing = storage
        .get(&IssueId::new("test-nonexistent"))
        .await
        .unwrap();
    assert!(non_existing.is_none());
}

#[tokio::test]
async fn test_update_issue() {
    let mut storage = new_in_memory_storage("test".to_string());

    let new_issue = create_test_issue("Original Title");
    let created = storage.create(new_issue).await.unwrap();

    let updates = IssueUpdate {
        title: Some("Updated Title".to_string()),
        status: Some(IssueStatus::InProgress),
        priority: Some(1),
        ..Default::default()
    };

    let updated = storage.update(&created.id, updates).await.unwrap();
    assert_eq!(updated.title, "Updated Title");
    assert_eq!(updated.status, IssueStatus::InProgress);
    assert_eq!(updated.priority, 1);
}

#[tokio::test]
async fn test_delete_issue() {
    let mut storage = new_in_memory_storage("test".to_string());

    let new_issue = create_test_issue("To Delete");
    let created = storage.create(new_issue).await.unwrap();

    // Delete should succeed
    storage.delete(&created.id).await.unwrap();

    // Issue should no longer exist
    let retrieved = storage.get(&created.id).await.unwrap();
    assert!(retrieved.is_none());
}

#[tokio::test]
async fn test_delete_with_dependents() {
    let mut storage = new_in_memory_storage("test".to_string());

    let issue1 = storage.create(create_test_issue("Issue 1")).await.unwrap();
    let issue2 = storage.create(create_test_issue("Issue 2")).await.unwrap();

    // Issue 2 depends on Issue 1
    storage
        .add_dependency(&issue2.id, &issue1.id, DependencyType::Blocks)
        .await
        .unwrap();

    // Deleting issue1 should fail because issue2 depends on it
    let result = storage.delete(&issue1.id).await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), Error::HasDependents { .. }));
}

// ========== Dependency Tests ==========

#[tokio::test]
async fn test_add_dependency() {
    let mut storage = new_in_memory_storage("test".to_string());

    let issue1 = storage.create(create_test_issue("Issue 1")).await.unwrap();
    let issue2 = storage.create(create_test_issue("Issue 2")).await.unwrap();

    // Add dependency: issue2 depends on issue1
    storage
        .add_dependency(&issue2.id, &issue1.id, DependencyType::Blocks)
        .await
        .unwrap();

    // Get dependencies for issue2
    let deps = storage.get_dependencies(&issue2.id).await.unwrap();
    assert_eq!(deps.len(), 1);
    assert_eq!(deps[0].depends_on_id, issue1.id);
    assert_eq!(deps[0].dep_type, DependencyType::Blocks);

    // Get dependents for issue1
    let dependents = storage.get_dependents(&issue1.id).await.unwrap();
    assert_eq!(dependents.len(), 1);
    assert_eq!(dependents[0].depends_on_id, issue2.id);
}

#[tokio::test]
async fn test_remove_dependency() {
    let mut storage = new_in_memory_storage("test".to_string());

    let issue1 = storage.create(create_test_issue("Issue 1")).await.unwrap();
    let issue2 = storage.create(create_test_issue("Issue 2")).await.unwrap();

    storage
        .add_dependency(&issue2.id, &issue1.id, DependencyType::Blocks)
        .await
        .unwrap();

    // Remove the dependency
    storage
        .remove_dependency(&issue2.id, &issue1.id)
        .await
        .unwrap();

    // Dependency should be gone
    let deps = storage.get_dependencies(&issue2.id).await.unwrap();
    assert_eq!(deps.len(), 0);
}

#[tokio::test]
async fn test_all_dependency_types() {
    let mut storage = new_in_memory_storage("test".to_string());

    let issue1 = storage.create(create_test_issue("Blocker")).await.unwrap();
    let issue2 = storage.create(create_test_issue("Related")).await.unwrap();
    let issue3 = storage.create(create_test_issue("Parent")).await.unwrap();
    let issue4 = storage
        .create(create_test_issue("Discovered"))
        .await
        .unwrap();
    let main_issue = storage
        .create(create_test_issue("Main Issue"))
        .await
        .unwrap();

    // Add all 4 dependency types
    storage
        .add_dependency(&main_issue.id, &issue1.id, DependencyType::Blocks)
        .await
        .unwrap();
    storage
        .add_dependency(&main_issue.id, &issue2.id, DependencyType::Related)
        .await
        .unwrap();
    storage
        .add_dependency(&main_issue.id, &issue3.id, DependencyType::ParentChild)
        .await
        .unwrap();
    storage
        .add_dependency(&main_issue.id, &issue4.id, DependencyType::DiscoveredFrom)
        .await
        .unwrap();

    // Verify all dependencies
    let deps = storage.get_dependencies(&main_issue.id).await.unwrap();
    assert_eq!(deps.len(), 4);

    // Verify each type
    assert!(deps
        .iter()
        .any(|d| d.depends_on_id == issue1.id && d.dep_type == DependencyType::Blocks));
    assert!(deps
        .iter()
        .any(|d| d.depends_on_id == issue2.id && d.dep_type == DependencyType::Related));
    assert!(deps
        .iter()
        .any(|d| d.depends_on_id == issue3.id && d.dep_type == DependencyType::ParentChild));
    assert!(deps
        .iter()
        .any(|d| d.depends_on_id == issue4.id && d.dep_type == DependencyType::DiscoveredFrom));
}

// ========== Cycle Detection Tests ==========

#[tokio::test]
async fn test_cycle_detection() {
    let mut storage = new_in_memory_storage("test".to_string());

    let issue1 = storage.create(create_test_issue("Issue 1")).await.unwrap();
    let issue2 = storage.create(create_test_issue("Issue 2")).await.unwrap();
    let issue3 = storage.create(create_test_issue("Issue 3")).await.unwrap();

    // Create chain: 1 -> 2 -> 3
    storage
        .add_dependency(&issue1.id, &issue2.id, DependencyType::Blocks)
        .await
        .unwrap();
    storage
        .add_dependency(&issue2.id, &issue3.id, DependencyType::Blocks)
        .await
        .unwrap();

    // Adding 3 -> 1 would create a cycle
    let result = storage
        .add_dependency(&issue3.id, &issue1.id, DependencyType::Blocks)
        .await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        Error::CircularDependency { .. }
    ));
}

#[tokio::test]
async fn test_self_dependency_cycle() {
    let mut storage = new_in_memory_storage("test".to_string());

    let issue = storage
        .create(create_test_issue("Self Referencing"))
        .await
        .unwrap();

    // Try to add self-dependency
    let result = storage
        .add_dependency(&issue.id, &issue.id, DependencyType::Blocks)
        .await;

    // Self-dependency should fail as a cycle
    assert!(result.is_err());
}

#[tokio::test]
async fn test_has_cycle_method() {
    let mut storage = new_in_memory_storage("test".to_string());

    let issue1 = storage.create(create_test_issue("Issue 1")).await.unwrap();
    let issue2 = storage.create(create_test_issue("Issue 2")).await.unwrap();
    let issue3 = storage.create(create_test_issue("Issue 3")).await.unwrap();

    // 1 -> 2 -> 3
    storage
        .add_dependency(&issue1.id, &issue2.id, DependencyType::Blocks)
        .await
        .unwrap();
    storage
        .add_dependency(&issue2.id, &issue3.id, DependencyType::Blocks)
        .await
        .unwrap();

    // 3 -> 1 would create cycle
    assert!(storage.has_cycle(&issue3.id, &issue1.id).await.unwrap());

    // 1 -> 3 would NOT create cycle
    assert!(!storage.has_cycle(&issue1.id, &issue3.id).await.unwrap());
}

// ========== Dependency Tree Tests ==========

#[tokio::test]
async fn test_dependency_tree_simple_chain() {
    let mut storage = new_in_memory_storage("test".to_string());

    let issue_a = storage.create(create_test_issue("A")).await.unwrap();
    let issue_b = storage.create(create_test_issue("B")).await.unwrap();
    let issue_c = storage.create(create_test_issue("C")).await.unwrap();

    // A -> B -> C
    storage
        .add_dependency(&issue_a.id, &issue_b.id, DependencyType::Blocks)
        .await
        .unwrap();
    storage
        .add_dependency(&issue_b.id, &issue_c.id, DependencyType::Blocks)
        .await
        .unwrap();

    // Get tree from A
    let tree = storage
        .get_dependency_tree(&issue_a.id, None)
        .await
        .unwrap();
    assert_eq!(tree.len(), 2);

    // B should be at depth 1
    assert!(tree
        .iter()
        .any(|(d, depth)| d.depends_on_id == issue_b.id && *depth == 1));

    // C should be at depth 2
    assert!(tree
        .iter()
        .any(|(d, depth)| d.depends_on_id == issue_c.id && *depth == 2));
}

#[tokio::test]
async fn test_dependency_tree_with_max_depth() {
    let mut storage = new_in_memory_storage("test".to_string());

    let issue_a = storage.create(create_test_issue("A")).await.unwrap();
    let issue_b = storage.create(create_test_issue("B")).await.unwrap();
    let issue_c = storage.create(create_test_issue("C")).await.unwrap();
    let issue_d = storage.create(create_test_issue("D")).await.unwrap();

    // A -> B -> C -> D
    storage
        .add_dependency(&issue_a.id, &issue_b.id, DependencyType::Blocks)
        .await
        .unwrap();
    storage
        .add_dependency(&issue_b.id, &issue_c.id, DependencyType::Blocks)
        .await
        .unwrap();
    storage
        .add_dependency(&issue_c.id, &issue_d.id, DependencyType::Blocks)
        .await
        .unwrap();

    // Get tree with max_depth = 2
    let tree = storage
        .get_dependency_tree(&issue_a.id, Some(2))
        .await
        .unwrap();

    // Should only include B and C
    assert_eq!(tree.len(), 2);
    assert!(tree.iter().any(|(d, _)| d.depends_on_id == issue_b.id));
    assert!(tree.iter().any(|(d, _)| d.depends_on_id == issue_c.id));
    assert!(!tree.iter().any(|(d, _)| d.depends_on_id == issue_d.id));
}

// ========== Ready to Work Tests ==========

#[tokio::test]
async fn test_ready_to_work() {
    let mut storage = new_in_memory_storage("test".to_string());

    let issue1 = storage.create(create_test_issue("Blocker")).await.unwrap();
    let issue2 = storage.create(create_test_issue("Blocked")).await.unwrap();
    let _issue3 = storage.create(create_test_issue("Ready")).await.unwrap();

    // issue2 is blocked by issue1
    storage
        .add_dependency(&issue2.id, &issue1.id, DependencyType::Blocks)
        .await
        .unwrap();

    // Get ready issues
    let ready = storage.ready_to_work(None, None).await.unwrap();

    // issue3 and issue1 should be ready, issue2 should be blocked
    assert_eq!(ready.len(), 2);
    let ready_titles: Vec<_> = ready.iter().map(|i| i.title.as_str()).collect();
    assert!(ready_titles.contains(&"Blocker"));
    assert!(ready_titles.contains(&"Ready"));
    assert!(!ready_titles.contains(&"Blocked"));
}

#[tokio::test]
async fn test_ready_to_work_closed_blocker_unblocks() {
    let mut storage = new_in_memory_storage("test".to_string());

    let blocker = storage
        .create(create_test_issue("Blocker Issue"))
        .await
        .unwrap();
    let blocked = storage
        .create(create_test_issue("Blocked Issue"))
        .await
        .unwrap();

    storage
        .add_dependency(&blocked.id, &blocker.id, DependencyType::Blocks)
        .await
        .unwrap();

    // Initially blocked should not be ready
    let ready = storage.ready_to_work(None, None).await.unwrap();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].id, blocker.id);

    // Close the blocker
    storage
        .update(
            &blocker.id,
            IssueUpdate {
                status: Some(IssueStatus::Closed),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    // Now blocked should be ready
    let ready = storage.ready_to_work(None, None).await.unwrap();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].id, blocked.id);
}

#[tokio::test]
async fn test_ready_to_work_parent_child_transitive_blocking() {
    let mut storage = new_in_memory_storage("test".to_string());

    let blocker = storage.create(create_test_issue("Blocker")).await.unwrap();
    let epic = storage.create(create_test_issue("Epic")).await.unwrap();
    let parent_task = storage
        .create(create_test_issue("Parent Task"))
        .await
        .unwrap();
    let child_task = storage
        .create(create_test_issue("Child Task"))
        .await
        .unwrap();

    // Epic is blocked by blocker
    storage
        .add_dependency(&epic.id, &blocker.id, DependencyType::Blocks)
        .await
        .unwrap();

    // parent_task is child of epic
    storage
        .add_dependency(&parent_task.id, &epic.id, DependencyType::ParentChild)
        .await
        .unwrap();

    // child_task is child of parent_task
    storage
        .add_dependency(&child_task.id, &parent_task.id, DependencyType::ParentChild)
        .await
        .unwrap();

    let ready = storage.ready_to_work(None, None).await.unwrap();

    // Only blocker should be ready; epic, parent_task, and child_task are all blocked
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].id, blocker.id);
}

#[rstest]
#[case::related(DependencyType::Related)]
#[case::discovered_from(DependencyType::DiscoveredFrom)]
#[tokio::test]
async fn test_ready_to_work_non_blocking_dependency_types(#[case] dep_type: DependencyType) {
    let mut storage = new_in_memory_storage("test".to_string());

    let issue1 = storage.create(create_test_issue("Issue 1")).await.unwrap();
    let issue2 = storage.create(create_test_issue("Issue 2")).await.unwrap();

    storage
        .add_dependency(&issue2.id, &issue1.id, dep_type)
        .await
        .unwrap();

    let ready = storage.ready_to_work(None, None).await.unwrap();

    // Both should be ready since these dependency types don't block
    assert_eq!(ready.len(), 2, "{:?} should not block", dep_type);
}

// ========== Sort Policy Tests ==========

#[tokio::test]
async fn test_sort_policy_priority() {
    let mut storage = new_in_memory_storage("test".to_string());

    let p4 = storage
        .create(create_test_issue_with_priority("P4 Issue", 4))
        .await
        .unwrap();
    let p0 = storage
        .create(create_test_issue_with_priority("P0 Issue", 0))
        .await
        .unwrap();
    let p2 = storage
        .create(create_test_issue_with_priority("P2 Issue", 2))
        .await
        .unwrap();
    let p1 = storage
        .create(create_test_issue_with_priority("P1 Issue", 1))
        .await
        .unwrap();

    let ready = storage
        .ready_to_work(None, Some(SortPolicy::Priority))
        .await
        .unwrap();

    // Should be sorted P0 -> P1 -> P2 -> P4
    assert_eq!(ready[0].id, p0.id);
    assert_eq!(ready[1].id, p1.id);
    assert_eq!(ready[2].id, p2.id);
    assert_eq!(ready[3].id, p4.id);
}

#[tokio::test]
async fn test_sort_policy_oldest() {
    let mut storage = new_in_memory_storage("test".to_string());

    let first = storage
        .create(create_test_issue_with_priority("First (P4)", 4))
        .await
        .unwrap();
    let second = storage
        .create(create_test_issue_with_priority("Second (P0)", 0))
        .await
        .unwrap();
    let third = storage
        .create(create_test_issue_with_priority("Third (P2)", 2))
        .await
        .unwrap();

    let ready = storage
        .ready_to_work(None, Some(SortPolicy::Oldest))
        .await
        .unwrap();

    // Should be sorted by creation time regardless of priority
    assert_eq!(ready[0].id, first.id);
    assert_eq!(ready[1].id, second.id);
    assert_eq!(ready[2].id, third.id);
}

// ========== Blocked Issues Tests ==========

#[tokio::test]
async fn test_blocked_issues() {
    let mut storage = new_in_memory_storage("test".to_string());

    let issue1 = storage.create(create_test_issue("Blocker")).await.unwrap();
    let issue2 = storage.create(create_test_issue("Blocked")).await.unwrap();

    storage
        .add_dependency(&issue2.id, &issue1.id, DependencyType::Blocks)
        .await
        .unwrap();

    let blocked = storage.blocked_issues().await.unwrap();
    assert_eq!(blocked.len(), 1);
    assert_eq!(blocked[0].0.title, "Blocked");
    assert_eq!(blocked[0].1[0].title, "Blocker");
}

// ========== Filter Tests ==========

#[tokio::test]
async fn test_list_with_filter() {
    let mut storage = new_in_memory_storage("test".to_string());

    let mut issue1 = create_test_issue("Issue 1");
    issue1.priority = 1;
    storage.create(issue1).await.unwrap();

    let mut issue2 = create_test_issue("Issue 2");
    issue2.priority = 2;
    storage.create(issue2).await.unwrap();

    // Filter by priority
    let filter = IssueFilter {
        priority: Some(1),
        ..Default::default()
    };
    let results = storage.list(&filter).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].title, "Issue 1");
}

#[tokio::test]
async fn test_ready_to_work_with_assignee_filter() {
    let mut storage = new_in_memory_storage("test".to_string());

    let mut alice_issue = create_test_issue("Alice's Task");
    alice_issue.assignee = Some("alice".to_string());
    let alice = storage.create(alice_issue).await.unwrap();

    let mut bob_issue = create_test_issue("Bob's Task");
    bob_issue.assignee = Some("bob".to_string());
    let _bob = storage.create(bob_issue).await.unwrap();

    let filter = IssueFilter {
        assignee: Some("alice".to_string()),
        ..Default::default()
    };

    let ready = storage.ready_to_work(Some(&filter), None).await.unwrap();

    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].id, alice.id);
}

// ========== Import/Export Tests ==========

#[tokio::test]
async fn test_import_export() {
    let mut storage = new_in_memory_storage("test".to_string());

    let issue1 = storage.create(create_test_issue("Issue 1")).await.unwrap();
    let issue2 = storage.create(create_test_issue("Issue 2")).await.unwrap();

    // Export all issues
    let exported_issues = storage.export_all().await.unwrap();
    assert_eq!(exported_issues.len(), 2);

    // Create new storage and import
    let mut new_storage = new_in_memory_storage("test".to_string());
    new_storage.import_issues(exported_issues).await.unwrap();

    // Verify imported issues
    let retrieved1 = new_storage.get(&issue1.id).await.unwrap();
    let retrieved2 = new_storage.get(&issue2.id).await.unwrap();
    assert!(retrieved1.is_some());
    assert!(retrieved2.is_some());

    assert_eq!(retrieved1.unwrap().title, "Issue 1");
    assert_eq!(retrieved2.unwrap().title, "Issue 2");
}

// ========== JSONL Round Trip Tests ==========

#[tokio::test]
async fn test_jsonl_persistence_round_trip() {
    let mut storage = new_in_memory_storage("test".to_string());

    let issue1 = storage.create(create_test_issue("Issue 1")).await.unwrap();
    let issue2 = storage.create(create_test_issue("Issue 2")).await.unwrap();
    let issue3 = storage.create(create_test_issue("Issue 3")).await.unwrap();

    // Add dependencies
    storage
        .add_dependency(&issue2.id, &issue1.id, DependencyType::Blocks)
        .await
        .unwrap();
    storage
        .add_dependency(&issue3.id, &issue2.id, DependencyType::Related)
        .await
        .unwrap();

    // Save to JSONL
    let temp_dir = tempdir().unwrap();
    let file_path = temp_dir.path().join("test.jsonl");

    save_to_jsonl(storage.as_ref(), &file_path).await.unwrap();

    // Load from JSONL
    let (loaded_storage, warnings) = load_from_jsonl(&file_path, "test".to_string())
        .await
        .unwrap();

    // Verify no warnings
    assert!(
        warnings.is_empty(),
        "Expected no warnings, got: {:?}",
        warnings
    );

    // Verify all issues loaded
    let loaded_issues = loaded_storage.export_all().await.unwrap();
    assert_eq!(loaded_issues.len(), 3);

    // Verify dependencies were preserved
    let deps = loaded_storage.get_dependencies(&issue2.id).await.unwrap();
    assert_eq!(deps.len(), 1);
    assert_eq!(deps[0].depends_on_id, issue1.id);

    temp_dir.close().unwrap();
}

// ========== Edge Cases ==========

#[tokio::test]
async fn test_duplicate_dependency() {
    let mut storage = new_in_memory_storage("test".to_string());

    let issue1 = storage.create(create_test_issue("Issue 1")).await.unwrap();
    let issue2 = storage.create(create_test_issue("Issue 2")).await.unwrap();

    // Add dependency
    storage
        .add_dependency(&issue2.id, &issue1.id, DependencyType::Blocks)
        .await
        .unwrap();

    // Try to add same dependency again
    let result = storage
        .add_dependency(&issue2.id, &issue1.id, DependencyType::Blocks)
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_remove_nonexistent_dependency() {
    let mut storage = new_in_memory_storage("test".to_string());

    let issue1 = storage.create(create_test_issue("Issue 1")).await.unwrap();
    let issue2 = storage.create(create_test_issue("Issue 2")).await.unwrap();

    // Try to remove nonexistent dependency
    let result = storage.remove_dependency(&issue2.id, &issue1.id).await;

    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        Error::DependencyNotFound { .. }
    ));
}

#[tokio::test]
async fn test_dependency_on_nonexistent_issue() {
    let mut storage = new_in_memory_storage("test".to_string());

    let issue = storage.create(create_test_issue("Issue")).await.unwrap();

    let result = storage
        .add_dependency(
            &issue.id,
            &IssueId::new("nonexistent"),
            DependencyType::Blocks,
        )
        .await;

    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), Error::IssueNotFound(_)));
}

#[tokio::test]
async fn test_ready_to_work_empty_storage() {
    let storage = new_in_memory_storage("test".to_string());

    let ready = storage.ready_to_work(None, None).await.unwrap();
    assert!(
        ready.is_empty(),
        "Empty storage should return no ready issues"
    );
}

#[tokio::test]
async fn test_ready_to_work_all_closed() {
    let mut storage = new_in_memory_storage("test".to_string());

    let issue1 = storage.create(create_test_issue("Issue 1")).await.unwrap();
    let issue2 = storage.create(create_test_issue("Issue 2")).await.unwrap();

    // Close all issues
    storage
        .update(
            &issue1.id,
            IssueUpdate {
                status: Some(IssueStatus::Closed),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    storage
        .update(
            &issue2.id,
            IssueUpdate {
                status: Some(IssueStatus::Closed),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let ready = storage.ready_to_work(None, None).await.unwrap();
    assert!(
        ready.is_empty(),
        "All closed issues should return no ready issues"
    );
}

// ========== Graph-Vector Synchronization Tests ==========

/// Helper function to verify graph-vector synchronization for a specific issue.
async fn verify_sync_for_issue(storage: &dyn IssueStorage, issue_id: &IssueId) -> Option<String> {
    // Get dependencies from graph via get_dependencies()
    let graph_deps = match storage.get_dependencies(issue_id).await {
        Ok(deps) => deps,
        Err(e) => return Some(format!("Failed to get graph deps for {}: {}", issue_id, e)),
    };

    // Get issue to access vector dependencies
    let issue = match storage.get(issue_id).await {
        Ok(Some(issue)) => issue,
        Ok(None) => return Some(format!("Issue {} not found", issue_id)),
        Err(e) => return Some(format!("Failed to get issue {}: {}", issue_id, e)),
    };

    let vector_deps = &issue.dependencies;

    // Check count matches
    if graph_deps.len() != vector_deps.len() {
        return Some(format!(
            "Issue {}: graph has {} deps, vector has {} deps",
            issue_id,
            graph_deps.len(),
            vector_deps.len()
        ));
    }

    // Check each graph dependency exists in vector
    for graph_dep in &graph_deps {
        let found = vector_deps.iter().any(|v| {
            v.depends_on_id == graph_dep.depends_on_id && v.dep_type == graph_dep.dep_type
        });
        if !found {
            return Some(format!(
                "Issue {}: graph dep {:?} not found in vector",
                issue_id, graph_dep
            ));
        }
    }

    None
}

/// Helper function to verify synchronization for all issues in storage.
async fn verify_all_issues_synchronized(
    storage: &dyn IssueStorage,
) -> std::result::Result<(), String> {
    let all_issues = storage.export_all().await.map_err(|e| e.to_string())?;

    for issue in &all_issues {
        if let Some(err) = verify_sync_for_issue(storage, &issue.id).await {
            return Err(err);
        }
    }

    Ok(())
}

#[tokio::test]
async fn test_sync_after_add_dependency() {
    let mut storage = new_in_memory_storage("test".to_string());

    let issue1 = storage.create(create_test_issue("Issue 1")).await.unwrap();
    let issue2 = storage.create(create_test_issue("Issue 2")).await.unwrap();
    let issue3 = storage.create(create_test_issue("Issue 3")).await.unwrap();

    // Add multiple dependencies
    storage
        .add_dependency(&issue2.id, &issue1.id, DependencyType::Blocks)
        .await
        .unwrap();
    storage
        .add_dependency(&issue3.id, &issue1.id, DependencyType::Related)
        .await
        .unwrap();
    storage
        .add_dependency(&issue3.id, &issue2.id, DependencyType::ParentChild)
        .await
        .unwrap();

    // Verify synchronization for all issues
    verify_all_issues_synchronized(storage.as_ref())
        .await
        .expect("Graph and vector should be synchronized after add_dependency");
}

#[tokio::test]
async fn test_sync_after_remove_dependency() {
    let mut storage = new_in_memory_storage("test".to_string());

    let issue1 = storage.create(create_test_issue("Issue 1")).await.unwrap();
    let issue2 = storage.create(create_test_issue("Issue 2")).await.unwrap();

    storage
        .add_dependency(&issue2.id, &issue1.id, DependencyType::Blocks)
        .await
        .unwrap();

    // Remove the dependency
    storage
        .remove_dependency(&issue2.id, &issue1.id)
        .await
        .unwrap();

    // Verify sync after removal
    verify_all_issues_synchronized(storage.as_ref())
        .await
        .expect("Graph and vector should be synchronized after remove_dependency");
}

#[tokio::test]
async fn test_sync_after_jsonl_round_trip() {
    let mut storage = new_in_memory_storage("test".to_string());

    let issue1 = storage.create(create_test_issue("Issue 1")).await.unwrap();
    let issue2 = storage.create(create_test_issue("Issue 2")).await.unwrap();
    let issue3 = storage.create(create_test_issue("Issue 3")).await.unwrap();

    storage
        .add_dependency(&issue2.id, &issue1.id, DependencyType::Blocks)
        .await
        .unwrap();
    storage
        .add_dependency(&issue3.id, &issue2.id, DependencyType::Related)
        .await
        .unwrap();
    storage
        .add_dependency(&issue3.id, &issue1.id, DependencyType::ParentChild)
        .await
        .unwrap();

    // Save to JSONL
    let temp_dir = tempdir().unwrap();
    let file_path = temp_dir.path().join("sync_test.jsonl");
    save_to_jsonl(storage.as_ref(), &file_path).await.unwrap();

    // Load from JSONL
    let (loaded_storage, warnings) = load_from_jsonl(&file_path, "test".to_string())
        .await
        .unwrap();

    assert!(warnings.is_empty());

    // Verify synchronization in loaded storage
    verify_all_issues_synchronized(loaded_storage.as_ref())
        .await
        .expect("Graph and vector should be synchronized after JSONL round-trip");

    temp_dir.close().unwrap();
}
