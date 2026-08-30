//! Integration tests for in-memory storage.
//!
//! These tests verify the full functionality of the in-memory storage backend,
//! including CRUD operations, dependency management, cycle detection, blocking
//! semantics, and sort policies.

use rivets::domain::{
    BlockingDependency, Dependency, DependencyType, DiscoveryOrigin, IssueFilter, IssueId,
    IssueKind, IssueStatus, IssueUpdate, MAX_PRIORITY, NewIssue, NewResource, NoteContent,
    RelatedAssociation, ResourceId, ResourceLabel, ResourceRole, ResourceTarget, ResourceUpdate,
    SortPolicy, WebUrl, WorkspacePath,
};
use rivets::error::Error;
use rivets::storage::IssueStorage;
use rivets::storage::in_memory::{load_from_jsonl, new_in_memory_storage, save_to_jsonl};
use std::collections::HashSet;
use std::time::{Duration, Instant};
use tempfile::tempdir;

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
        prerequisites: vec![],
    }
}

fn create_test_issue_with_priority(title: &str, priority: u8) -> NewIssue {
    NewIssue {
        title: title.to_string(),
        description: "Test description".to_string(),
        priority,
        issue_kind: IssueKind::Task,
        assignee: None,
        labels: vec![],
        design: None,
        acceptance_criteria: None,
        initial_note: None,
        prerequisites: vec![],
    }
}
fn literal_path_exists<'a>(edges: &[(&'a str, &'a str)], start: &'a str, target: &'a str) -> bool {
    let mut stack = vec![start];
    let mut visited = HashSet::new();
    while let Some(node) = stack.pop() {
        if node == target {
            return true;
        }
        if visited.insert(node) {
            stack.extend(
                edges
                    .iter()
                    .filter_map(|(from, to)| (*from == node).then_some(*to)),
            );
        }
    }
    false
}

async fn seed_legacy_relationship(
    storage: &mut Box<dyn IssueStorage>,
    dependent_id: &IssueId,
    prerequisite_id: &IssueId,
    dep_type: DependencyType,
) {
    let mut issues = storage.export_all().await.unwrap();
    issues
        .iter_mut()
        .find(|issue| issue.id == *dependent_id)
        .unwrap()
        .dependencies
        .push(Dependency {
            depends_on_id: prerequisite_id.clone(),
            dep_type,
        });
    let mut rebuilt = new_in_memory_storage("test".to_string());
    rebuilt.import_issues(issues).await.unwrap();
    *storage = rebuilt;
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
async fn create_with_prerequisites_is_atomic() {
    let mut storage = new_in_memory_storage("test".to_string());
    let prerequisite_a = storage.create(create_test_issue("A")).await.unwrap();
    let prerequisite_b = storage.create(create_test_issue("B")).await.unwrap();
    let mut valid = create_test_issue("Valid dependent");
    valid.prerequisites = vec![prerequisite_b.id.clone(), prerequisite_a.id.clone()];
    let dependent = storage.create(valid).await.unwrap();
    let relationships = storage.blocking_prerequisites(&dependent.id).await.unwrap();
    assert_eq!(relationships.len(), 2);
    let mut expected_prerequisites = vec![&prerequisite_a.id, &prerequisite_b.id];
    expected_prerequisites.sort_unstable();
    assert_eq!(
        relationships
            .iter()
            .map(|dependency| dependency.prerequisite_id())
            .collect::<Vec<_>>(),
        expected_prerequisites
    );

    let count_before_failures = storage.export_all().await.unwrap().len();
    let mut duplicate = create_test_issue("Duplicate prerequisites");
    duplicate.prerequisites = vec![prerequisite_a.id.clone(), prerequisite_a.id.clone()];
    assert!(storage.create(duplicate).await.is_err());
    assert_eq!(
        storage.export_all().await.unwrap().len(),
        count_before_failures
    );

    let mut missing = create_test_issue("Missing prerequisite");
    missing.prerequisites = vec![IssueId::new("test-missing")];
    assert!(storage.create(missing).await.is_err());
    assert_eq!(
        storage.export_all().await.unwrap().len(),
        count_before_failures
    );
}

#[tokio::test]
async fn create_prerequisite_validation_stays_within_budget() {
    const PREREQUISITE_COUNT: usize = 1_000;
    let mut storage = new_in_memory_storage("test".to_string());
    let mut prerequisite_ids = Vec::with_capacity(PREREQUISITE_COUNT);
    for index in 0..PREREQUISITE_COUNT {
        prerequisite_ids.push(
            storage
                .create(create_test_issue(&format!("Prerequisite {index}")))
                .await
                .unwrap()
                .id,
        );
    }
    let mut candidate = create_test_issue("Stress dependent");
    candidate.prerequisites = prerequisite_ids;

    let started = Instant::now();
    let created = storage.create(candidate).await.unwrap();
    let elapsed = started.elapsed();
    assert_eq!(
        storage
            .blocking_prerequisites(&created.id)
            .await
            .unwrap()
            .len(),
        PREREQUISITE_COUNT
    );
    assert!(
        elapsed <= Duration::from_millis(20),
        "1,000-prerequisite create took {elapsed:?}"
    );
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
async fn test_update_rejects_invalid_priority() {
    let mut storage = new_in_memory_storage("test".to_string());

    let new_issue = create_test_issue("Test Issue");
    let created = storage.create(new_issue).await.unwrap();

    let result = storage
        .update(
            &created.id,
            IssueUpdate {
                priority: Some(MAX_PRIORITY + 1),
                ..Default::default()
            },
        )
        .await;

    assert!(matches!(result, Err(Error::InvalidPriority(_))));
}

#[tokio::test]
async fn rejected_update_does_not_append_note_or_mutate_issue() {
    let mut storage = new_in_memory_storage("test".to_string());
    let created = storage
        .create(create_test_issue("Original Title"))
        .await
        .unwrap();

    let result = storage
        .update(
            &created.id,
            IssueUpdate {
                title: Some(" ".to_string()),
                note: Some(NoteContent::new("Must not persist").unwrap()),
                ..Default::default()
            },
        )
        .await;
    assert!(result.is_err());

    let unchanged = storage.get(&created.id).await.unwrap().unwrap();
    assert_eq!(unchanged.title, "Original Title");
    assert!(unchanged.notes().is_empty());
    assert_eq!(unchanged.updated_at, created.updated_at);
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
        .add_blocking_dependency(
            BlockingDependency::new(issue2.id.clone(), issue1.id.clone()).unwrap(),
        )
        .await
        .unwrap();

    // Deleting issue1 should fail because issue2 depends on it
    let result = storage.delete(&issue1.id).await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), Error::HasDependents { .. }));
}

// ========== Dependency Tests ==========
#[tokio::test]
async fn blocking_dependency_coexists_with_legacy_kind() {
    let mut storage = new_in_memory_storage("test".to_string());
    let prerequisite = storage
        .create(create_test_issue("Prerequisite"))
        .await
        .unwrap();
    let dependent = storage
        .create(create_test_issue("Dependent"))
        .await
        .unwrap();
    seed_legacy_relationship(
        &mut storage,
        &dependent.id,
        &prerequisite.id,
        DependencyType::Related,
    )
    .await;
    let blocking = BlockingDependency::new(dependent.id.clone(), prerequisite.id.clone()).unwrap();

    storage
        .add_blocking_dependency(blocking.clone())
        .await
        .unwrap();
    assert_eq!(
        storage.blocking_prerequisites(&dependent.id).await.unwrap(),
        vec![blocking.clone()]
    );
    assert_eq!(
        storage.blocking_dependents(&prerequisite.id).await.unwrap(),
        vec![blocking.clone()]
    );
    assert_eq!(
        storage
            .get(&dependent.id)
            .await
            .unwrap()
            .unwrap()
            .dependencies
            .len(),
        2
    );

    let directory = tempdir().unwrap();
    let path = directory.path().join("issues.jsonl");
    save_to_jsonl(storage.as_ref(), &path).await.unwrap();
    let (mut reloaded, warnings) = load_from_jsonl(&path, "test".to_string()).await.unwrap();
    assert!(warnings.is_empty());
    assert_eq!(
        reloaded
            .blocking_prerequisites(&dependent.id)
            .await
            .unwrap(),
        vec![blocking.clone()]
    );

    reloaded
        .remove_blocking_dependency(&blocking)
        .await
        .unwrap();
    let remaining = reloaded.get(&dependent.id).await.unwrap().unwrap();
    assert_eq!(remaining.dependencies.len(), 1);
    assert_eq!(remaining.dependencies[0].dep_type, DependencyType::Related);
    assert_eq!(remaining.dependencies[0].depends_on_id, prerequisite.id);
}

#[tokio::test]
async fn blocking_dependency_duplicate_missing_and_absent_errors_do_not_mutate() {
    let mut storage = new_in_memory_storage("test".to_string());
    let prerequisite = storage
        .create(create_test_issue("Prerequisite"))
        .await
        .unwrap();
    let dependent = storage
        .create(create_test_issue("Dependent"))
        .await
        .unwrap();
    let blocking = BlockingDependency::new(dependent.id.clone(), prerequisite.id.clone()).unwrap();
    storage
        .add_blocking_dependency(blocking.clone())
        .await
        .unwrap();

    assert!(
        storage
            .add_blocking_dependency(blocking.clone())
            .await
            .is_err()
    );
    let missing = BlockingDependency::new(
        dependent.id.clone(),
        IssueId::new("test-missing-prerequisite"),
    )
    .unwrap();
    assert!(storage.add_blocking_dependency(missing).await.is_err());
    assert_eq!(
        storage.blocking_prerequisites(&dependent.id).await.unwrap(),
        vec![blocking.clone()]
    );

    storage.remove_blocking_dependency(&blocking).await.unwrap();
    assert!(storage.remove_blocking_dependency(&blocking).await.is_err());
    assert!(
        storage
            .blocking_prerequisites(&dependent.id)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn blocking_cycles_ignore_other_relationship_kinds() {
    let mut storage = new_in_memory_storage("test".to_string());
    let issue_a = storage.create(create_test_issue("A")).await.unwrap();
    let issue_b = storage.create(create_test_issue("B")).await.unwrap();
    let issue_c = storage.create(create_test_issue("C")).await.unwrap();

    seed_legacy_relationship(
        &mut storage,
        &issue_b.id,
        &issue_a.id,
        DependencyType::Related,
    )
    .await;
    storage
        .add_blocking_dependency(
            BlockingDependency::new(issue_a.id.clone(), issue_b.id.clone()).unwrap(),
        )
        .await
        .expect("a non-blocking reverse path must not create a Blocking cycle");
    storage
        .add_blocking_dependency(
            BlockingDependency::new(issue_b.id.clone(), issue_c.id.clone()).unwrap(),
        )
        .await
        .unwrap();

    let cycle = BlockingDependency::new(issue_c.id.clone(), issue_a.id.clone()).unwrap();
    assert!(
        literal_path_exists(
            &[
                (issue_a.id.as_str(), issue_b.id.as_str()),
                (issue_b.id.as_str(), issue_c.id.as_str()),
            ],
            issue_a.id.as_str(),
            issue_c.id.as_str(),
        ),
        "the independent Blocking-only oracle should identify the cycle-closing path"
    );

    assert!(storage.add_blocking_dependency(cycle).await.is_err());
    assert!(
        storage
            .blocking_prerequisites(&issue_c.id)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn blocking_tree_preserves_direction_and_depth() {
    let mut storage = new_in_memory_storage("test".to_string());
    let issue_a = storage.create(create_test_issue("A")).await.unwrap();
    let issue_b = storage.create(create_test_issue("B")).await.unwrap();
    let issue_c = storage.create(create_test_issue("C")).await.unwrap();
    let issue_d = storage.create(create_test_issue("D")).await.unwrap();
    let issue_e = storage.create(create_test_issue("E")).await.unwrap();

    for dependency in [
        BlockingDependency::new(issue_a.id.clone(), issue_c.id.clone()).unwrap(),
        BlockingDependency::new(issue_a.id.clone(), issue_b.id.clone()).unwrap(),
        BlockingDependency::new(issue_b.id.clone(), issue_d.id.clone()).unwrap(),
    ] {
        storage.add_blocking_dependency(dependency).await.unwrap();
    }
    seed_legacy_relationship(
        &mut storage,
        &issue_a.id,
        &issue_e.id,
        DependencyType::Related,
    )
    .await;

    let tree = storage
        .blocking_dependency_tree(&issue_a.id, None)
        .await
        .unwrap();
    let actual = tree
        .iter()
        .map(|(dependency, depth)| {
            (
                dependency.dependent_id().as_str(),
                dependency.prerequisite_id().as_str(),
                *depth,
            )
        })
        .collect::<Vec<_>>();
    let mut direct_prerequisites = [issue_b.id.as_str(), issue_c.id.as_str()];
    direct_prerequisites.sort_unstable();
    assert_eq!(
        actual,
        vec![
            (issue_a.id.as_str(), direct_prerequisites[0], 1),
            (issue_a.id.as_str(), direct_prerequisites[1], 1),
            (issue_b.id.as_str(), issue_d.id.as_str(), 2),
        ]
    );
    assert_eq!(
        storage
            .blocking_dependency_tree(&issue_a.id, Some(1))
            .await
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn closed_prerequisite_stays_recorded_without_blocking() {
    let mut storage = new_in_memory_storage("test".to_string());
    let prerequisite = storage
        .create(create_test_issue("Prerequisite"))
        .await
        .unwrap();
    let dependent = storage
        .create(create_test_issue("Dependent"))
        .await
        .unwrap();
    let blocking = BlockingDependency::new(dependent.id.clone(), prerequisite.id.clone()).unwrap();
    storage
        .add_blocking_dependency(blocking.clone())
        .await
        .unwrap();
    let prerequisite_state = storage.get(&prerequisite.id).await.unwrap().unwrap();
    assert_eq!(prerequisite_state.status, IssueStatus::Open);
    let blocked = storage.blocked_issues().await.unwrap();
    assert_eq!(blocked.len(), 1);
    assert_eq!(blocked[0].0.id, dependent.id);
    assert!(
        !storage
            .ready_to_work(None, None)
            .await
            .unwrap()
            .iter()
            .any(|issue| issue.id == dependent.id)
    );

    storage
        .update(
            &prerequisite.id,
            IssueUpdate {
                status: Some(IssueStatus::Closed),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let prerequisite_state = storage.get(&prerequisite.id).await.unwrap().unwrap();
    assert_eq!(prerequisite_state.status, IssueStatus::Closed);
    assert!(storage.blocked_issues().await.unwrap().is_empty());
    assert!(
        storage
            .ready_to_work(None, None)
            .await
            .unwrap()
            .iter()
            .any(|issue| issue.id == dependent.id)
    );
    assert_eq!(
        storage.blocking_prerequisites(&dependent.id).await.unwrap(),
        vec![blocking]
    );
}

#[tokio::test]
async fn legacy_parentage_propagation_remains_fenced_until_readiness_cutover() {
    let mut storage = new_in_memory_storage("test".to_string());
    let blocker = storage.create(create_test_issue("Blocker")).await.unwrap();
    let parent = storage.create(create_test_issue("Parent")).await.unwrap();
    let child = storage.create(create_test_issue("Child")).await.unwrap();
    storage
        .add_blocking_dependency(
            BlockingDependency::new(parent.id.clone(), blocker.id.clone()).unwrap(),
        )
        .await
        .unwrap();
    seed_legacy_relationship(
        &mut storage,
        &child.id,
        &parent.id,
        DependencyType::ParentChild,
    )
    .await;

    let ready = storage.ready_to_work(None, None).await.unwrap();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].id, blocker.id);
    let blocked = storage.blocked_issues().await.unwrap();
    assert_eq!(blocked.len(), 1);
    assert_eq!(blocked[0].0.id, parent.id);
}

#[tokio::test]
async fn related_association_is_symmetric_idempotent_and_removable_from_either_side() {
    let mut storage = new_in_memory_storage("test".to_string());
    let issue_a = storage.create(create_test_issue("A")).await.unwrap();
    let issue_b = storage.create(create_test_issue("B")).await.unwrap();
    let issue_c = storage.create(create_test_issue("C")).await.unwrap();
    let forward = RelatedAssociation::new(issue_a.id.clone(), issue_b.id.clone()).unwrap();
    let reverse = RelatedAssociation::new(issue_b.id.clone(), issue_a.id.clone()).unwrap();
    assert_eq!(forward, reverse);

    storage
        .add_related_association(reverse.clone())
        .await
        .unwrap();
    storage
        .add_related_association(forward.clone())
        .await
        .unwrap();
    assert_eq!(
        storage.related_associations(&issue_a.id).await.unwrap(),
        vec![forward.clone()]
    );
    assert_eq!(
        storage.related_associations(&issue_b.id).await.unwrap(),
        vec![forward.clone()]
    );
    assert!(
        storage
            .related_associations(&issue_c.id)
            .await
            .unwrap()
            .is_empty()
    );

    let exported = storage.export_all().await.unwrap();
    let related_records = exported
        .iter()
        .flat_map(|issue| {
            issue
                .dependencies
                .iter()
                .filter(|dependency| dependency.dep_type == DependencyType::Related)
                .map(move |dependency| (&issue.id, &dependency.depends_on_id))
        })
        .collect::<Vec<_>>();
    assert_eq!(
        related_records,
        vec![(forward.left_issue_id(), forward.right_issue_id())]
    );

    seed_legacy_relationship(
        &mut storage,
        forward.right_issue_id(),
        forward.left_issue_id(),
        DependencyType::Related,
    )
    .await;
    assert_eq!(
        storage.related_associations(&issue_a.id).await.unwrap(),
        vec![forward.clone()],
        "reciprocal legacy records should remain one logical Association"
    );

    storage.remove_related_association(&reverse).await.unwrap();
    assert!(
        storage
            .related_associations(&issue_a.id)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        storage
            .related_associations(&issue_b.id)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(storage.remove_related_association(&forward).await.is_err());

    let missing =
        RelatedAssociation::new(issue_a.id.clone(), IssueId::new("test-missing")).unwrap();
    assert!(storage.add_related_association(missing).await.is_err());
}

#[tokio::test]
async fn discovery_origin_is_directed_multi_source_and_acyclic() {
    let mut storage = new_in_memory_storage("test".to_string());
    let issue_a = storage.create(create_test_issue("A")).await.unwrap();
    let issue_b = storage.create(create_test_issue("B")).await.unwrap();
    let issue_c = storage.create(create_test_issue("C")).await.unwrap();
    let issue_d = storage.create(create_test_issue("D")).await.unwrap();
    let origin_ab = DiscoveryOrigin::new(issue_a.id.clone(), issue_b.id.clone()).unwrap();
    let origin_ad = DiscoveryOrigin::new(issue_a.id.clone(), issue_d.id.clone()).unwrap();

    storage
        .add_discovery_origin(origin_ab.clone())
        .await
        .unwrap();
    storage
        .add_discovery_origin(origin_ad.clone())
        .await
        .unwrap();
    let mut expected = vec![origin_ab.clone(), origin_ad.clone()];
    expected.sort();
    assert_eq!(
        storage.discovery_origins(&issue_a.id).await.unwrap(),
        expected
    );
    assert!(
        storage
            .add_discovery_origin(origin_ab.clone())
            .await
            .is_err()
    );

    seed_legacy_relationship(
        &mut storage,
        &issue_b.id,
        &issue_c.id,
        DependencyType::Related,
    )
    .await;
    let origin_ca = DiscoveryOrigin::new(issue_c.id.clone(), issue_a.id.clone()).unwrap();
    storage
        .add_discovery_origin(origin_ca.clone())
        .await
        .expect("a non-Discovery path must not create a provenance cycle");
    let cycle = DiscoveryOrigin::new(issue_b.id.clone(), issue_c.id.clone()).unwrap();
    assert!(literal_path_exists(
        &[
            (issue_c.id.as_str(), issue_a.id.as_str()),
            (issue_a.id.as_str(), issue_b.id.as_str()),
        ],
        issue_c.id.as_str(),
        issue_b.id.as_str(),
    ));
    assert!(storage.add_discovery_origin(cycle).await.is_err());
    assert!(
        storage
            .discovery_origins(&issue_b.id)
            .await
            .unwrap()
            .is_empty()
    );

    let reversed = DiscoveryOrigin::new(issue_b.id.clone(), issue_a.id.clone()).unwrap();
    assert!(storage.remove_discovery_origin(&reversed).await.is_err());
    assert!(
        storage
            .discovery_origins(&issue_a.id)
            .await
            .unwrap()
            .contains(&origin_ab)
    );
    seed_legacy_relationship(
        &mut storage,
        &issue_a.id,
        &issue_b.id,
        DependencyType::DiscoveredFrom,
    )
    .await;
    storage.remove_discovery_origin(&origin_ab).await.unwrap();
    assert!(
        !storage
            .discovery_origins(&issue_a.id)
            .await
            .unwrap()
            .contains(&origin_ab),
        "removal should clear duplicate compatibility records and graph edges"
    );
    storage.remove_discovery_origin(&origin_ca).await.unwrap();

    let missing = DiscoveryOrigin::new(issue_a.id.clone(), IssueId::new("test-missing")).unwrap();
    assert!(storage.add_discovery_origin(missing).await.is_err());
}

#[tokio::test]
async fn nonblocking_relationships_do_not_change_ready_or_blocked_and_coexist() {
    let mut storage = new_in_memory_storage("test".to_string());
    let issue_a = storage.create(create_test_issue("A")).await.unwrap();
    let issue_b = storage.create(create_test_issue("B")).await.unwrap();
    let blocking = BlockingDependency::new(issue_a.id.clone(), issue_b.id.clone()).unwrap();
    storage
        .add_blocking_dependency(blocking.clone())
        .await
        .unwrap();
    seed_legacy_relationship(
        &mut storage,
        &issue_a.id,
        &issue_b.id,
        DependencyType::ParentChild,
    )
    .await;

    let ready_before = storage
        .ready_to_work(None, None)
        .await
        .unwrap()
        .into_iter()
        .map(|issue| issue.id)
        .collect::<HashSet<_>>();
    let blocked_before = storage
        .blocked_issues()
        .await
        .unwrap()
        .into_iter()
        .map(|(issue, _)| issue.id)
        .collect::<HashSet<_>>();

    let related = RelatedAssociation::new(issue_a.id.clone(), issue_b.id.clone()).unwrap();
    let discovery = DiscoveryOrigin::new(issue_a.id.clone(), issue_b.id.clone()).unwrap();
    storage
        .add_related_association(related.clone())
        .await
        .unwrap();
    storage
        .add_discovery_origin(discovery.clone())
        .await
        .unwrap();

    let ready_after = storage
        .ready_to_work(None, None)
        .await
        .unwrap()
        .into_iter()
        .map(|issue| issue.id)
        .collect::<HashSet<_>>();
    let blocked_after = storage
        .blocked_issues()
        .await
        .unwrap()
        .into_iter()
        .map(|(issue, _)| issue.id)
        .collect::<HashSet<_>>();
    assert_eq!(ready_after, ready_before);
    assert_eq!(blocked_after, blocked_before);
    assert_eq!(
        storage.blocking_prerequisites(&issue_a.id).await.unwrap(),
        vec![blocking]
    );
    assert_eq!(
        storage.related_associations(&issue_b.id).await.unwrap(),
        vec![related]
    );
    assert_eq!(
        storage.discovery_origins(&issue_a.id).await.unwrap(),
        vec![discovery]
    );

    let mut kinds = storage
        .export_all()
        .await
        .unwrap()
        .into_iter()
        .flat_map(|issue| issue.dependencies)
        .map(|dependency| dependency.dep_type)
        .collect::<Vec<_>>();
    kinds.sort_unstable();
    assert_eq!(
        kinds,
        vec![
            DependencyType::Blocks,
            DependencyType::Related,
            DependencyType::ParentChild,
            DependencyType::DiscoveredFrom,
        ]
    );
}

#[tokio::test]
async fn nonblocking_relationship_operations_stay_within_scale_budget() {
    const ISSUE_COUNT: usize = 300;
    const RELATED_OFFSETS: [usize; 3] = [1, 2, 3];
    const DISCOVERED_COUNT: usize = 100;
    const SOURCES_PER_DISCOVERY: usize = 4;
    const OPERATION_BUDGET: Duration = Duration::from_secs(2);

    let mut storage = new_in_memory_storage("stress".to_string());
    let mut issues = Vec::with_capacity(ISSUE_COUNT);
    for index in 0..ISSUE_COUNT {
        issues.push(
            storage
                .create(create_test_issue(&format!("Stress {index}")))
                .await
                .unwrap(),
        );
    }

    for index in 0..ISSUE_COUNT {
        for offset in RELATED_OFFSETS {
            let related = (index + offset) % ISSUE_COUNT;
            storage
                .add_related_association(
                    RelatedAssociation::new(issues[index].id.clone(), issues[related].id.clone())
                        .unwrap(),
                )
                .await
                .unwrap();
        }
    }
    for discovered in 0..DISCOVERED_COUNT {
        for source_offset in 0..SOURCES_PER_DISCOVERY {
            let source = DISCOVERED_COUNT
                + (discovered * SOURCES_PER_DISCOVERY + source_offset)
                    % (ISSUE_COUNT - DISCOVERED_COUNT);
            storage
                .add_discovery_origin(
                    DiscoveryOrigin::new(issues[discovered].id.clone(), issues[source].id.clone())
                        .unwrap(),
                )
                .await
                .unwrap();
        }
    }
    for index in 0..DISCOVERED_COUNT {
        storage
            .add_blocking_dependency(
                BlockingDependency::new(
                    issues[DISCOVERED_COUNT + index].id.clone(),
                    issues[2 * DISCOVERED_COUNT + index].id.clone(),
                )
                .unwrap(),
            )
            .await
            .unwrap();
    }

    let duplicate = RelatedAssociation::new(issues[1].id.clone(), issues[0].id.clone()).unwrap();
    let started = Instant::now();
    storage.add_related_association(duplicate).await.unwrap();
    assert!(started.elapsed() <= OPERATION_BUDGET);

    let started = Instant::now();
    let associations = storage.related_associations(&issues[0].id).await.unwrap();
    assert_eq!(associations.len(), RELATED_OFFSETS.len() * 2);
    assert!(started.elapsed() <= OPERATION_BUDGET);

    let cycle =
        DiscoveryOrigin::new(issues[DISCOVERED_COUNT].id.clone(), issues[0].id.clone()).unwrap();
    let started = Instant::now();
    assert!(storage.add_discovery_origin(cycle).await.is_err());
    assert!(started.elapsed() <= OPERATION_BUDGET);
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
        .add_blocking_dependency(
            BlockingDependency::new(issue2.id.clone(), issue1.id.clone()).unwrap(),
        )
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
        .add_blocking_dependency(
            BlockingDependency::new(blocked.id.clone(), blocker.id.clone()).unwrap(),
        )
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
        .add_blocking_dependency(
            BlockingDependency::new(issue2.id.clone(), issue1.id.clone()).unwrap(),
        )
        .await
        .unwrap();
    seed_legacy_relationship(
        &mut storage,
        &issue3.id,
        &issue2.id,
        DependencyType::Related,
    )
    .await;

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
    assert_eq!(
        loaded_storage
            .blocking_prerequisites(&issue2.id)
            .await
            .unwrap(),
        vec![BlockingDependency::new(issue2.id.clone(), issue1.id.clone()).unwrap()]
    );

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
        .add_blocking_dependency(
            BlockingDependency::new(issue2.id.clone(), issue1.id.clone()).unwrap(),
        )
        .await
        .unwrap();

    // Try to add same dependency again
    let result = storage
        .add_blocking_dependency(
            BlockingDependency::new(issue2.id.clone(), issue1.id.clone()).unwrap(),
        )
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_remove_nonexistent_dependency() {
    let mut storage = new_in_memory_storage("test".to_string());

    let issue1 = storage.create(create_test_issue("Issue 1")).await.unwrap();
    let issue2 = storage.create(create_test_issue("Issue 2")).await.unwrap();

    // Try to remove nonexistent dependency
    let result = storage
        .remove_blocking_dependency(
            &BlockingDependency::new(issue2.id.clone(), issue1.id.clone()).unwrap(),
        )
        .await;

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
        .add_blocking_dependency(
            BlockingDependency::new(issue.id.clone(), IssueId::new("nonexistent")).unwrap(),
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

// ---------------------------------------------------------------------------
// Deterministic serialization
//
// `.rivets/issues.jsonl` is committed to git, so byte-stable output is a
// correctness property, not cosmetics. `export_all` collects from a `HashMap`,
// whose iteration order differs per instance, so without an explicit sort every
// save reshuffles the whole file. Observed in a downstream repo: a single
// `create` moved 131 of 132 issues to new lines, which made routine merges
// collide across the entire file instead of on the one changed line.
// ---------------------------------------------------------------------------

/// Read a JSONL file and return the `id` of each line, in file order.
fn ids_in_file_order(path: &std::path::Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let v: serde_json::Value = serde_json::from_str(l).expect("each line is valid JSON");
            v["id"]
                .as_str()
                .expect("every line carries an id")
                .to_string()
        })
        .collect()
}

#[tokio::test]
async fn save_to_jsonl_orders_lines_by_id() {
    let mut storage = new_in_memory_storage("test".to_string());
    for n in 0..25 {
        storage
            .create(create_test_issue(&format!("Issue {n}")))
            .await
            .unwrap();
    }

    let temp_dir = tempdir().unwrap();
    let file_path = temp_dir.path().join("sorted.jsonl");
    save_to_jsonl(storage.as_ref(), &file_path).await.unwrap();

    let ids = ids_in_file_order(&file_path);
    let mut expected = ids.clone();
    expected.sort();
    assert_eq!(ids, expected, "JSONL lines must be written in id order");
    assert_eq!(ids.len(), 25, "every issue should be written exactly once");

    temp_dir.close().unwrap();
}

#[tokio::test]
async fn save_to_jsonl_is_byte_stable_across_reloads() {
    let mut storage = new_in_memory_storage("test".to_string());
    for n in 0..25 {
        storage
            .create(create_test_issue(&format!("Issue {n}")))
            .await
            .unwrap();
    }

    let temp_dir = tempdir().unwrap();

    // Each `load_from_jsonl` builds a fresh `HashMap` with its own iteration
    // order, so an unsorted writer diverges on the very first reload.
    let first = temp_dir.path().join("round0.jsonl");
    save_to_jsonl(storage.as_ref(), &first).await.unwrap();
    let baseline = std::fs::read_to_string(&first).unwrap();
    assert!(!baseline.is_empty(), "baseline must not be empty");

    let mut previous = first;
    for round in 1..=3 {
        let (loaded, warnings) = load_from_jsonl(&previous, "test".to_string())
            .await
            .unwrap();
        assert!(warnings.is_empty(), "round {round} produced warnings");

        let next = temp_dir.path().join(format!("round{round}.jsonl"));
        save_to_jsonl(loaded.as_ref(), &next).await.unwrap();
        assert_eq!(
            std::fs::read_to_string(&next).unwrap(),
            baseline,
            "round {round}: save output must be byte-identical to the baseline"
        );
        previous = next;
    }

    temp_dir.close().unwrap();
}

// ========== Associated Resource Update/Remove Round-Trip ==========

#[tokio::test]
async fn resource_update_and_remove_round_trip_through_jsonl() {
    let temp_dir = tempdir().expect("temp dir should be created");
    let jsonl_path = temp_dir.path().join("issues.jsonl");

    let mut storage = new_in_memory_storage("test".to_string());
    let issue = storage
        .create(create_test_issue("Resource owner"))
        .await
        .expect("issue should be created");
    let issue_id = issue.id.clone();
    let created_updated_at = issue.updated_at;

    for (target, role) in [
        (
            ResourceTarget::web(WebUrl::new("https://a.example.com").expect("valid test URL")),
            ResourceRole::Implementation,
        ),
        (
            ResourceTarget::web(WebUrl::new("https://b.example.com").expect("valid test URL")),
            ResourceRole::Evidence,
        ),
        (
            ResourceTarget::path(WorkspacePath::new("docs/adr/0003.md").expect("valid test path")),
            ResourceRole::Reference,
        ),
    ] {
        storage
            .add_resource(
                &issue_id,
                NewResource {
                    target,
                    role,
                    label: None,
                },
            )
            .await
            .expect("resource should be added");
    }

    // Update the middle resource's role and label; bump updated_at.
    let updated = storage
        .update_resource(
            &issue_id,
            &ResourceId::new("r2").expect("valid resource id"),
            ResourceUpdate {
                target: None,
                role: Some(ResourceRole::Documentation),
                label: Some(Some(
                    ResourceLabel::new("updated label").expect("valid test label"),
                )),
            },
        )
        .await
        .expect("update should succeed");
    assert!(
        updated.updated_at > created_updated_at,
        "update must bump updated_at"
    );

    // Remove the first resource; remaining keep ids/positions.
    let after_remove = storage
        .remove_resource(
            &issue_id,
            &ResourceId::new("r1").expect("valid resource id"),
        )
        .await
        .expect("remove should succeed");
    let ids: Vec<_> = after_remove
        .resources()
        .iter()
        .map(|r| r.id().as_str())
        .collect();
    assert_eq!(ids, ["r2", "r3"]);
    assert_eq!(
        after_remove.resources()[0].role(),
        ResourceRole::Documentation
    );
    assert_eq!(
        after_remove.resources()[0].label().map(|l| l.as_str()),
        Some("updated label")
    );

    // Persist and reload from disk; state must survive a fresh storage.
    save_to_jsonl(storage.as_ref(), &jsonl_path)
        .await
        .expect("save should succeed");
    let (mut reloaded, warnings) = load_from_jsonl(&jsonl_path, "test".to_string())
        .await
        .expect("reload should succeed");
    assert!(warnings.is_empty(), "clean round-trip must not warn");
    let reloaded_issue = reloaded
        .get(&issue_id)
        .await
        .expect("get should succeed")
        .expect("issue should exist after reload");
    let ids: Vec<_> = reloaded_issue
        .resources()
        .iter()
        .map(|r| r.id().as_str())
        .collect();
    assert_eq!(ids, ["r2", "r3"]);
    assert_eq!(
        reloaded_issue.resources()[0].role(),
        ResourceRole::Documentation
    );
    assert_eq!(
        reloaded_issue.resources()[1].target().to_string(),
        "docs/adr/0003.md"
    );

    // The sequence never reuses the removed r1.
    let with_new = reloaded
        .add_resource(
            &issue_id,
            NewResource {
                target: ResourceTarget::web(
                    WebUrl::new("https://c.example.com").expect("valid test URL"),
                ),
                role: ResourceRole::Successor,
                label: None,
            },
        )
        .await
        .expect("add after reload should succeed");
    assert_eq!(
        with_new
            .resources()
            .last()
            .expect("resources should be non-empty")
            .id()
            .as_str(),
        "r4"
    );

    // Duplicate detection flows through storage as a typed resource error:
    // r3 updated to r2's target+role collides on the post-update state.
    let duplicate = reloaded
        .update_resource(
            &issue_id,
            &ResourceId::new("r3").expect("valid resource id"),
            ResourceUpdate {
                target: Some(ResourceTarget::web(
                    WebUrl::new("https://b.example.com").expect("valid test URL"),
                )),
                role: Some(ResourceRole::Documentation),
                label: None,
            },
        )
        .await;
    assert!(matches!(
        duplicate,
        Err(Error::Storage(rivets::error::StorageError::Resource(
            rivets::domain::ResourceError::DuplicateTargetRole { .. }
        )))
    ));
    temp_dir.close().expect("temp dir should close cleanly");
}
