//! Integration tests for staleness detection (`get_stale_files`, `needs_update`).

use std::fs;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};
use tempfile::TempDir;
use tethys::Tethys;

fn workspace_with_files(files: &[(&str, &str)]) -> (TempDir, Tethys) {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    for (path, content) in files {
        let full_path = dir.path().join(path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).expect("failed to create parent dirs");
        }
        fs::write(&full_path, content).expect("failed to write file");
    }
    let tethys = Tethys::new(dir.path()).expect("failed to create Tethys");
    (dir, tethys)
}

/// Overwrite a file and wait until the OS reports a fresh mtime, retrying as needed.
///
/// Filesystem mtime resolution varies (1ns on ext4, 100ns on NTFS, up to 2s on FAT32),
/// so a fixed sleep can be flaky on coarse filesystems or under CI load. This helper
/// rewrites until the OS observably advances mtime, or panics with a clear diagnostic.
fn write_and_advance_mtime(path: &Path, content: &str) {
    let before = fs::metadata(path)
        .expect("path should exist before mtime advance")
        .modified()
        .expect("filesystem should expose mtime");

    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        fs::write(path, content).expect("failed to overwrite file");
        let after = fs::metadata(path)
            .expect("path should exist while waiting on mtime")
            .modified()
            .expect("filesystem should expose mtime");
        if after != before {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!(
        "mtime did not advance within 2s for {} (filesystem may not track modification time)",
        path.display()
    );
}

#[test]
fn empty_workspace_is_not_stale() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let mut tethys = Tethys::new(dir.path()).expect("failed to create Tethys");
    tethys.index().expect("index failed");

    assert!(
        !tethys.needs_update().expect("needs_update failed"),
        "empty workspace should not need update"
    );
    let report = tethys.get_stale_files().expect("staleness check failed");
    assert!(
        !report.is_stale(),
        "empty workspace should produce empty report, got {report:?}"
    );
    assert_eq!(report.total(), 0);
}

#[test]
fn get_stale_files_empty_after_fresh_index() {
    let (_dir, mut tethys) = workspace_with_files(&[("src/lib.rs", "fn hello() {}")]);
    tethys.index().expect("index failed");

    let report = tethys.get_stale_files().expect("staleness check failed");

    assert!(
        !report.is_stale(),
        "freshly indexed workspace should not be stale"
    );
    assert!(report.modified.is_empty());
    assert!(report.added.is_empty());
    assert!(report.deleted.is_empty());
    assert_eq!(report.total(), 0);
}

#[test]
fn get_stale_files_detects_modified_files() {
    let (dir, mut tethys) = workspace_with_files(&[("src/lib.rs", "fn hello() {}")]);
    tethys.index().expect("index failed");

    write_and_advance_mtime(
        &dir.path().join("src/lib.rs"),
        "fn hello() { println!(\"hi\"); }",
    );

    let report = tethys.get_stale_files().expect("staleness check failed");

    assert!(report.is_stale());
    assert_eq!(report.modified.len(), 1, "should detect one modified file");
    assert!(report.added.is_empty());
    assert!(report.deleted.is_empty());
}

#[test]
fn get_stale_files_detects_added_files() {
    let (dir, mut tethys) = workspace_with_files(&[("src/lib.rs", "fn hello() {}")]);
    tethys.index().expect("index failed");

    fs::write(dir.path().join("src/new.rs"), "fn new_fn() {}").expect("failed to write new file");

    let report = tethys.get_stale_files().expect("staleness check failed");

    assert!(report.is_stale());
    assert!(report.modified.is_empty());
    assert_eq!(report.added.len(), 1, "should detect one added file");
    assert!(report.deleted.is_empty());
}

#[test]
fn get_stale_files_detects_deleted_files() {
    let (dir, mut tethys) = workspace_with_files(&[
        ("src/lib.rs", "fn hello() {}"),
        ("src/helper.rs", "fn help() {}"),
    ]);
    tethys.index().expect("index failed");

    fs::remove_file(dir.path().join("src/helper.rs")).expect("failed to delete file");

    let report = tethys.get_stale_files().expect("staleness check failed");

    assert!(report.is_stale());
    assert!(report.modified.is_empty());
    assert!(report.added.is_empty());
    assert_eq!(report.deleted.len(), 1, "should detect one deleted file");
}

#[test]
fn get_stale_files_reports_combined_changes() {
    let (dir, mut tethys) = workspace_with_files(&[
        ("src/lib.rs", "fn hello() {}"),
        ("src/helper.rs", "fn help() {}"),
    ]);
    tethys.index().expect("index failed");

    write_and_advance_mtime(&dir.path().join("src/lib.rs"), "fn hello() { let _x = 1; }");
    fs::remove_file(dir.path().join("src/helper.rs")).expect("failed to delete file");
    fs::write(dir.path().join("src/new.rs"), "fn new_fn() {}").expect("failed to write new file");

    let report = tethys.get_stale_files().expect("staleness check failed");

    assert_eq!(report.modified.len(), 1);
    assert_eq!(report.added.len(), 1);
    assert_eq!(report.deleted.len(), 1);
    assert_eq!(report.total(), 3);
    assert!(report.is_stale());
}

#[test]
fn needs_update_returns_false_after_fresh_index() {
    let (_dir, mut tethys) = workspace_with_files(&[("src/lib.rs", "fn hello() {}")]);
    tethys.index().expect("index failed");

    assert!(!tethys.needs_update().expect("needs_update failed"));
}

#[test]
fn needs_update_returns_true_after_modification() {
    let (dir, mut tethys) = workspace_with_files(&[("src/lib.rs", "fn hello() {}")]);
    tethys.index().expect("index failed");

    write_and_advance_mtime(&dir.path().join("src/lib.rs"), "fn changed() {}");

    assert!(tethys.needs_update().expect("needs_update failed"));
}

#[test]
fn needs_update_returns_true_after_addition() {
    let (dir, mut tethys) = workspace_with_files(&[("src/lib.rs", "fn hello() {}")]);
    tethys.index().expect("index failed");

    fs::write(dir.path().join("src/added.rs"), "fn added() {}")
        .expect("failed to write added file");

    assert!(tethys.needs_update().expect("needs_update failed"));
}

#[test]
fn detects_changes_in_nested_subdirectories() {
    let (dir, mut tethys) = workspace_with_files(&[
        ("src/lib.rs", "fn root() {}"),
        ("src/sub/mod.rs", "fn submod() {}"),
        ("src/sub/deeper/leaf.rs", "fn leaf() {}"),
    ]);
    tethys.index().expect("index failed");

    write_and_advance_mtime(
        &dir.path().join("src/sub/deeper/leaf.rs"),
        "fn leaf_renamed() {}",
    );
    fs::write(dir.path().join("src/sub/added.rs"), "fn added() {}")
        .expect("failed to write nested added file");

    let report = tethys.get_stale_files().expect("staleness check failed");

    assert!(
        report
            .modified
            .iter()
            .any(|p| p.ends_with("sub/deeper/leaf.rs")),
        "expected sub/deeper/leaf.rs in modified, got {:?}",
        report.modified
    );
    assert!(
        report.added.iter().any(|p| p.ends_with("sub/added.rs")),
        "expected sub/added.rs in added, got {:?}",
        report.added
    );
    assert_eq!(
        report.modified.len(),
        1,
        "expected exactly 1 modified file, got {}: {:?}",
        report.modified.len(),
        report.modified
    );
    assert_eq!(
        report.added.len(),
        1,
        "expected exactly 1 added file, got {}: {:?}",
        report.added.len(),
        report.added
    );
    assert!(
        report.deleted.is_empty(),
        "expected no deleted files, got {:?}",
        report.deleted
    );
    assert!(
        tethys.needs_update().expect("needs_update failed"),
        "needs_update should agree with the report's nested-change detection"
    );

    // Re-index, then confirm the nested changes have been absorbed and the
    // workspace looks clean again. Locks in the full detect → reindex → clean
    // cycle at depth, mirroring how callers will actually use this API.
    tethys
        .index()
        .expect("re-index after nested changes failed");
    let post = tethys
        .get_stale_files()
        .expect("post-reindex staleness check failed");
    assert!(
        post.is_empty(),
        "post-reindex report should be empty, got {post:?}"
    );
}

#[test]
fn needs_update_returns_true_after_deletion() {
    let (dir, mut tethys) = workspace_with_files(&[
        ("src/lib.rs", "fn hello() {}"),
        ("src/helper.rs", "fn help() {}"),
    ]);
    tethys.index().expect("index failed");

    fs::remove_file(dir.path().join("src/helper.rs")).expect("failed to delete file");

    assert!(tethys.needs_update().expect("needs_update failed"));
}

#[test]
fn get_stale_files_ignores_non_source_files() {
    let (dir, mut tethys) = workspace_with_files(&[("src/lib.rs", "fn hello() {}")]);
    tethys.index().expect("index failed");

    // Drop a handful of non-source files alongside the indexed one.
    fs::write(dir.path().join("README.md"), "# project").expect("failed to write README");
    fs::write(dir.path().join("Cargo.toml"), "[package]\nname=\"x\"\n")
        .expect("failed to write Cargo.toml");
    fs::write(dir.path().join("src/data.json"), "{}").expect("failed to write JSON");

    let report = tethys.get_stale_files().expect("staleness check failed");

    assert!(
        !report.is_stale(),
        "non-source files should not appear in any staleness bucket, got {report:?}"
    );
}

#[test]
fn needs_update_matches_get_stale_files_for_unchanged_workspace() {
    let (_dir, mut tethys) = workspace_with_files(&[("src/lib.rs", "fn hello() {}")]);
    tethys.index().expect("index failed");

    let report_says_stale = tethys
        .get_stale_files()
        .expect("staleness check failed")
        .is_stale();
    let needs_update = tethys.needs_update().expect("needs_update failed");

    assert_eq!(
        needs_update, report_says_stale,
        "needs_update must agree with get_stale_files().is_stale()"
    );
}

#[test]
fn needs_update_matches_get_stale_files_after_changes() {
    let (dir, mut tethys) = workspace_with_files(&[
        ("src/lib.rs", "fn hello() {}"),
        ("src/other.rs", "fn other() {}"),
    ]);
    tethys.index().expect("index failed");

    write_and_advance_mtime(&dir.path().join("src/lib.rs"), "fn hello() { let _x = 1; }");
    fs::remove_file(dir.path().join("src/other.rs")).expect("failed to delete file");
    fs::write(dir.path().join("src/new.rs"), "fn new_fn() {}").expect("failed to write new file");

    let report_says_stale = tethys
        .get_stale_files()
        .expect("staleness check failed")
        .is_stale();
    let needs_update = tethys.needs_update().expect("needs_update failed");

    assert!(report_says_stale, "report should report changes");
    assert_eq!(
        needs_update, report_says_stale,
        "needs_update must agree with get_stale_files().is_stale()"
    );
}
