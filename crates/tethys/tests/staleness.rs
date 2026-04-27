//! Integration tests for staleness detection (`get_stale_files`, `needs_update`).

use std::fs;
use std::thread;
use std::time::Duration;
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

    // Wait briefly so mtime resolution distinguishes the two writes
    thread::sleep(Duration::from_millis(50));
    fs::write(
        dir.path().join("src/lib.rs"),
        "fn hello() { println!(\"hi\"); }",
    )
    .expect("failed to overwrite file");

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

    thread::sleep(Duration::from_millis(50));
    fs::write(dir.path().join("src/lib.rs"), "fn hello() { let _x = 1; }")
        .expect("failed to overwrite file");
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

    thread::sleep(Duration::from_millis(50));
    fs::write(dir.path().join("src/lib.rs"), "fn changed() {}").expect("failed to overwrite file");

    assert!(tethys.needs_update().expect("needs_update failed"));
}
