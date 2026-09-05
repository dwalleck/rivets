use chrono::{DateTime, Utc};
use rivets::domain::{IssueFilter, IssueStatus, ListQuery, QueryError, StaleQuery};
use rivets::storage::{IssueStorage, in_memory::load_from_jsonl};
use std::io::Write;
use std::num::NonZeroUsize;
use tempfile::NamedTempFile;

fn instant(value: &str) -> DateTime<Utc> {
    value.parse().expect("fixed test instant should parse")
}

fn write_record(
    file: &mut NamedTempFile,
    id: &str,
    status: &str,
    priority: u8,
    created_at: &str,
    updated_at: &str,
) {
    let assignee = if status == "in_progress" {
        r#""alice""#
    } else {
        "null"
    };
    writeln!(
        file,
        r#"{{"id":"{id}","title":"{id}","description":"query fixture","status":"{status}","priority":{priority},"issue_kind":"task","assignee":{assignee},"labels":[],"design":null,"acceptance_criteria":null,"notes":[],"resources":[],"dependencies":[],"created_at":"{created_at}","updated_at":"{updated_at}","closed_at":null}}"#
    )
    .expect("fixture record should be written");
}

fn fixture() -> NamedTempFile {
    let mut file = NamedTempFile::new().expect("temporary JSONL fixture should be created");
    for (id, status, priority, created_at, updated_at) in [
        (
            "test-a",
            "open",
            4,
            "2026-01-01T00:00:00Z",
            "2026-01-06T00:00:00Z",
        ),
        (
            "test-b",
            "open",
            0,
            "2026-01-02T00:00:00Z",
            "2026-01-07T00:00:00Z",
        ),
        (
            "test-c",
            "closed",
            2,
            "2026-01-03T00:00:00Z",
            "2026-01-06T00:00:00Z",
        ),
        (
            "test-d",
            "in_progress",
            1,
            "2026-01-02T00:00:00Z",
            "2026-01-08T00:00:00Z",
        ),
        (
            "test-e",
            "open",
            3,
            "2026-01-04T00:00:00Z",
            "2026-01-06T00:00:00Z",
        ),
    ] {
        write_record(&mut file, id, status, priority, created_at, updated_at);
    }
    file
}

fn scale_fixture() -> NamedTempFile {
    let mut file = NamedTempFile::new().expect("temporary scale fixture should be created");
    for index in 0..10_000 {
        let id = format!("test-{index:05}");
        write_record(
            &mut file,
            &id,
            "open",
            (index % 5) as u8,
            "2026-01-01T00:00:00Z",
            "2026-01-01T00:00:00Z",
        );
    }
    file
}

async fn storage_from_fixture(file: &NamedTempFile) -> Box<dyn IssueStorage> {
    let (storage, warnings) = load_from_jsonl(file.path(), "test".to_string())
        .await
        .expect("fixture should load");
    assert!(
        warnings.is_empty(),
        "fixture should not emit warnings: {warnings:?}"
    );
    storage
}

fn ids(issues: Vec<rivets::domain::Issue>) -> Vec<String> {
    issues
        .into_iter()
        .map(|issue| issue.id.to_string())
        .collect()
}

#[tokio::test]
async fn list_selection_contract() {
    let file = fixture();
    let storage = storage_from_fixture(&file).await;
    let query = ListQuery::try_from(IssueFilter {
        limit: Some(3),
        ..Default::default()
    })
    .expect("bounded list query should be valid");

    let actual = storage
        .list_issues(&query)
        .await
        .expect("list query should succeed");
    assert_eq!(ids(actual), vec!["test-e", "test-c", "test-b"]);
}

#[tokio::test]
async fn stale_selection_contract() {
    let file = fixture();
    let storage = storage_from_fixture(&file).await;
    let now = instant("2026-01-10T00:00:00Z");
    let default_query =
        StaleQuery::new(NonZeroUsize::new(3).expect("positive limit"), None, 3, now)
            .expect("stale query should be valid");
    let explicit_closed = StaleQuery::new(
        NonZeroUsize::new(3).expect("positive limit"),
        Some(IssueStatus::Closed),
        3,
        now,
    )
    .expect("closed stale query should be valid");

    assert_eq!(
        ids(storage
            .stale_issues(&default_query)
            .await
            .expect("stale query should succeed")),
        vec!["test-a", "test-e"],
        "strict cutoff excludes the equal timestamp and default stale excludes Closed",
    );
    assert_eq!(
        ids(storage
            .stale_issues(&explicit_closed)
            .await
            .expect("closed stale query should succeed")),
        vec!["test-c"],
    );
}

#[tokio::test]
async fn generic_enumeration_is_unchanged() {
    let file = fixture();
    let storage = storage_from_fixture(&file).await;
    let actual = storage
        .list(&IssueFilter::default())
        .await
        .expect("generic enumeration should succeed");
    let mut actual = ids(actual);
    actual.sort();
    assert_eq!(
        actual,
        vec!["test-a", "test-b", "test-c", "test-d", "test-e"]
    );
}

#[test]
fn priority_range() {
    for priority in 0..=4 {
        ListQuery::try_from(IssueFilter {
            priority: Some(priority),
            limit: Some(1),
            ..Default::default()
        })
        .expect("priority 0..=4 should be accepted");
    }
    assert!(matches!(
        ListQuery::try_from(IssueFilter {
            priority: Some(5),
            limit: Some(1),
            ..Default::default()
        }),
        Err(QueryError::InvalidPriority(5))
    ));
}

#[test]
fn age_and_limit_boundaries() {
    assert!(matches!(
        ListQuery::try_from(IssueFilter::default()),
        Err(QueryError::MissingLimit)
    ));
    assert!(matches!(
        ListQuery::try_from(IssueFilter {
            limit: Some(0),
            ..Default::default()
        }),
        Err(QueryError::InvalidLimit(0))
    ));
    let now = instant("2026-01-10T00:00:00Z");
    let query = StaleQuery::new(NonZeroUsize::new(1).expect("positive limit"), None, 0, now)
        .expect("zero age is a valid checked cutoff");
    assert_eq!(query.cutoff(), now);
    for (days, instant) in [(u32::MAX, now), (1, DateTime::<Utc>::MIN_UTC)] {
        assert_eq!(
            StaleQuery::new(
                NonZeroUsize::new(1).expect("positive limit"),
                None,
                days,
                instant
            ),
            Err(QueryError::CutoffOverflow { days }),
        );
    }
}

#[tokio::test]
async fn maximum_limit_does_not_change_selection() {
    let file = fixture();
    let storage = storage_from_fixture(&file).await;
    let query = ListQuery::try_from(IssueFilter {
        limit: Some(usize::MAX),
        ..Default::default()
    })
    .expect("maximum usize limit should be representable");
    assert_eq!(
        ids(storage
            .list_issues(&query)
            .await
            .expect("query should succeed")),
        vec!["test-e", "test-c", "test-b", "test-d", "test-a"]
    );
}

#[tokio::test]
#[ignore = "production-scale query timing checkpoint"]
async fn query_scale_budget() {
    let file = scale_fixture();
    let storage = storage_from_fixture(&file).await;
    for limit in [1, 100] {
        let query = ListQuery::try_from(IssueFilter {
            limit: Some(limit),
            priority: Some(2),
            ..Default::default()
        })
        .expect("bounded list query should be valid");
        let stale = StaleQuery::new(
            NonZeroUsize::new(limit).expect("positive limit"),
            None,
            30,
            instant("2026-09-05T00:00:00Z"),
        )
        .expect("valid age");
        let expected_list: Vec<String> = (0..limit)
            .map(|i| format!("test-{:05}", 2 + 5 * i))
            .collect();
        let expected_stale: Vec<String> = (0..limit).map(|i| format!("test-{i:05}")).collect();
        let started = std::time::Instant::now();
        let actual = storage.list_issues(&query).await.expect("list query");
        let elapsed = started.elapsed();
        assert_eq!(ids(actual), expected_list);
        eprintln!("C8 list limit={limit}: {elapsed:?}");
        assert!(
            elapsed < std::time::Duration::from_millis(100),
            "C8 List exceeded 100ms: {elapsed:?}"
        );
        let started = std::time::Instant::now();
        let actual = storage.stale_issues(&stale).await.expect("stale query");
        let elapsed = started.elapsed();
        assert_eq!(ids(actual), expected_stale);
        eprintln!("C8 stale limit={limit}: {elapsed:?}");
        assert!(
            elapsed < std::time::Duration::from_millis(100),
            "C8 Stale exceeded 100ms: {elapsed:?}"
        );
    }
}
