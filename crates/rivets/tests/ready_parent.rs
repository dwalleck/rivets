//! Shared Ready query contracts for direct Epic Parentage.

use rivets::domain::{
    BlockingDependency, Issue, IssueId, IssueKind, Label, NewIssue, Parentage, ParentageError,
    ReadyAssignmentFilter, ReadyFilter, SortPolicy,
};
use rivets::error::Error;
use rivets::storage::{IssueStorage, in_memory::new_in_memory_storage};

async fn create(
    storage: &mut dyn IssueStorage,
    title: &str,
    kind: IssueKind,
    priority: u8,
    assignee: Option<&str>,
) -> Issue {
    storage
        .create(NewIssue {
            title: title.into(),
            description: String::new(),
            priority,
            issue_kind: kind,
            assignee: assignee.map(str::to_owned),
            labels: vec![Label::new("focus").unwrap()],
            design: None,
            acceptance_criteria: None,
            initial_note: None,
            prerequisites: vec![],
        })
        .await
        .unwrap()
}

async fn ids(storage: &dyn IssueStorage, filter: &ReadyFilter) -> Vec<IssueId> {
    storage
        .ready_to_work(filter, Some(SortPolicy::Priority))
        .await
        .unwrap()
        .into_iter()
        .map(|issue| issue.id)
        .collect()
}

#[tokio::test]
async fn ready_parent_scope_contract() {
    let mut storage = new_in_memory_storage("test".into());
    let parent = create(storage.as_mut(), "Parent", IssueKind::Epic, 4, None).await;
    let other = create(storage.as_mut(), "Other", IssueKind::Epic, 4, None).await;
    let outside = create(storage.as_mut(), "Outside", IssueKind::Task, 0, None).await;
    let child = create(storage.as_mut(), "Child", IssueKind::Task, 1, None).await;
    let assigned = create(
        storage.as_mut(),
        "Assigned",
        IssueKind::Task,
        2,
        Some("alice"),
    )
    .await;
    let nested = create(storage.as_mut(), "Nested", IssueKind::Epic, 3, None).await;
    let grandchild = create(storage.as_mut(), "Grandchild", IssueKind::Task, 3, None).await;
    let blocked = create(storage.as_mut(), "Blocked", IssueKind::Task, 0, None).await;
    for direct in [&child, &assigned, &nested, &blocked] {
        storage
            .set_parent(Parentage::new(direct.id.clone(), parent.id.clone()).unwrap())
            .await
            .unwrap();
    }
    storage
        .set_parent(Parentage::new(grandchild.id.clone(), nested.id.clone()).unwrap())
        .await
        .unwrap();
    storage
        .set_parent(Parentage::new(outside.id.clone(), other.id.clone()).unwrap())
        .await
        .unwrap();
    for dependent in [&blocked, &parent] {
        storage
            .add_blocking_dependency(
                BlockingDependency::new(dependent.id.clone(), outside.id.clone()).unwrap(),
            )
            .await
            .unwrap();
    }
    let mut filter = ReadyFilter {
        parent_id: Some(parent.id.clone()),
        ..Default::default()
    };
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("issues.jsonl");
    rivets::storage::in_memory::save_to_jsonl(storage.as_ref(), &path)
        .await
        .unwrap();
    let before = std::fs::read(&path).unwrap();
    assert_eq!(
        ids(storage.as_ref(), &filter).await,
        vec![child.id.clone(), nested.id.clone()]
    );
    filter.issue_kind = Some(IssueKind::Task);
    filter.label = Some(Label::new("focus").unwrap());
    filter.limit = Some(1);
    assert_eq!(ids(storage.as_ref(), &filter).await, vec![child.id.clone()]);
    filter.parent_id = None;
    assert_eq!(
        ids(storage.as_ref(), &filter).await,
        vec![outside.id.clone()]
    );
    filter.limit = None;
    assert_eq!(
        ids(storage.as_ref(), &filter).await,
        vec![outside.id.clone(), child.id.clone(), grandchild.id.clone()]
    );
    filter.parent_id = Some(parent.id.clone());
    filter.assignment = ReadyAssignmentFilter::Assignee("alice".into());
    assert_eq!(
        ids(storage.as_ref(), &filter).await,
        vec![assigned.id.clone()]
    );
    filter.assignment = ReadyAssignmentFilter::All;
    assert_eq!(
        ids(storage.as_ref(), &filter).await,
        vec![child.id.clone(), assigned.id.clone()]
    );
    filter.priority = Some(2);
    assert_eq!(
        ids(storage.as_ref(), &filter).await,
        vec![assigned.id.clone()]
    );
    filter.label = Some(Label::new("other").unwrap());
    assert_eq!(ids(storage.as_ref(), &filter).await, Vec::<IssueId>::new());
    rivets::storage::in_memory::save_to_jsonl(storage.as_ref(), &path)
        .await
        .unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), before);
    filter = ReadyFilter {
        parent_id: Some(parent.id.clone()),
        issue_kind: Some(IssueKind::Task),
        ..Default::default()
    };
    storage
        .move_parent(Parentage::new(child.id.clone(), other.id.clone()).unwrap())
        .await
        .unwrap();
    assert_eq!(ids(storage.as_ref(), &filter).await, Vec::<IssueId>::new());
    filter.parent_id = Some(other.id.clone());
    assert_eq!(
        ids(storage.as_ref(), &filter).await,
        vec![outside.id.clone(), child.id.clone()]
    );
    storage.clear_parent(&child.id).await.unwrap();
    assert_eq!(
        ids(storage.as_ref(), &filter).await,
        vec![outside.id.clone()]
    );
    filter.parent_id = None;
    assert_eq!(
        ids(storage.as_ref(), &filter).await,
        vec![outside.id, child.id, grandchild.id]
    );
}

#[tokio::test]
async fn ready_parent_scope_errors() {
    let mut storage = new_in_memory_storage("test".into());
    let empty = create(storage.as_mut(), "Empty", IssueKind::Epic, 2, None).await;
    let task = create(storage.as_mut(), "Task", IssueKind::Task, 2, None).await;
    // The unscoped Workspace still has Ready Issues, so an empty scoped
    // result is attributable to the parent scope rather than to nothing being
    // Ready at all.
    assert!(
        ids(storage.as_ref(), &ReadyFilter::default())
            .await
            .contains(&task.id),
        "fixture must keep at least one Ready Issue outside the empty Epic"
    );
    assert_eq!(
        ids(
            storage.as_ref(),
            &ReadyFilter {
                parent_id: Some(empty.id.clone()),
                ..Default::default()
            }
        )
        .await,
        Vec::<IssueId>::new()
    );
    // Scope validation fires even when the limit admits no candidates.
    let filter = ReadyFilter {
        parent_id: Some(empty.id),
        limit: Some(0),
        ..Default::default()
    };
    let missing = IssueId::new("test-missing");
    assert!(
        matches!(storage.ready_to_work(&ReadyFilter { parent_id: Some(missing.clone()), ..filter.clone() }, None).await, Err(Error::IssueNotFound(id)) if id == missing)
    );
    assert!(
        matches!(storage.ready_to_work(&ReadyFilter { parent_id: Some(task.id.clone()), ..filter }, None).await, Err(Error::InvalidParentage(ParentageError::ParentNotEpic { parent_id, actual_kind: IssueKind::Task })) if parent_id == task.id)
    );
}

#[tokio::test]
async fn ready_parent_ignores_rejected_parentage_records() {
    let mut storage = new_in_memory_storage("test".into());
    let parent = create(storage.as_mut(), "Parent", IssueKind::Epic, 0, None).await;
    let child = create(storage.as_mut(), "Child", IssueKind::Task, 1, None).await;
    storage
        .set_parent(Parentage::new(child.id.clone(), parent.id.clone()).unwrap())
        .await
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("issues.jsonl");
    rivets::storage::in_memory::save_to_jsonl(storage.as_ref(), &path)
        .await
        .unwrap();
    let mut rows: Vec<serde_json::Value> = std::fs::read_to_string(&path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let parent_row = rows
        .iter_mut()
        .find(|row| row["id"].as_str() == Some(parent.id.as_str()))
        .unwrap();
    parent_row["dependencies"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "depends_on_id": parent.id.as_str(),
            "dep_type": "parent-child"
        }));
    let bytes = rows
        .iter()
        .map(|row| serde_json::to_string(row).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&path, bytes).unwrap();
    let (loaded, _) = rivets::storage::in_memory::load_from_jsonl(&path, "test".into())
        .await
        .unwrap();
    assert_eq!(loaded.parent_of(&parent.id).await.unwrap(), None);
    let filter = ReadyFilter {
        parent_id: Some(parent.id),
        ..Default::default()
    };
    assert_eq!(ids(loaded.as_ref(), &filter).await, vec![child.id]);
}
