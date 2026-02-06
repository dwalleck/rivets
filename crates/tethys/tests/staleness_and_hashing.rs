//! Integration tests for staleness detection, content hashing, and rebuild.
//!
//! Covers `get_stale_files()`, `needs_update()`, `compute_content_hash()` (via
//! indexing), `rebuild()`, and depth-limited impact analysis.

use std::fs;
use std::path::Path;
use std::thread;
use std::time::Duration;
use tempfile::TempDir;
use tethys::Tethys;

/// Create a temporary workspace with the given files.
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

// -- Staleness detection tests --

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
}

#[test]
fn get_stale_files_detects_modified_files() {
    let (dir, mut tethys) = workspace_with_files(&[("src/lib.rs", "fn hello() {}")]);
    tethys.index().expect("index failed");

    // Wait briefly then modify the file so mtime changes
    thread::sleep(Duration::from_millis(50));
    fs::write(
        dir.path().join("src/lib.rs"),
        "fn hello() { println!(\"hi\"); }",
    )
    .unwrap();

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

    // Add a new file
    fs::write(dir.path().join("src/new.rs"), "fn new_fn() {}").unwrap();

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

    // Delete one file
    fs::remove_file(dir.path().join("src/helper.rs")).unwrap();

    let report = tethys.get_stale_files().expect("staleness check failed");

    assert!(report.is_stale());
    assert!(report.modified.is_empty());
    assert!(report.added.is_empty());
    assert_eq!(report.deleted.len(), 1, "should detect one deleted file");
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
    fs::write(dir.path().join("src/lib.rs"), "fn changed() {}").unwrap();

    assert!(tethys.needs_update().expect("needs_update failed"));
}

// -- Content hashing tests --

#[test]
fn content_hash_stored_after_indexing() {
    let (_dir, mut tethys) = workspace_with_files(&[("src/lib.rs", "fn hello() {}")]);
    tethys.index().expect("index failed");

    let file = tethys
        .get_file(Path::new("src/lib.rs"))
        .expect("get_file failed")
        .expect("file should exist after indexing");

    assert!(
        file.content_hash.is_some(),
        "content_hash should be set after indexing for {}",
        file.path.display()
    );
}

#[test]
fn content_hash_changes_when_file_modified() {
    let (dir, mut tethys) = workspace_with_files(&[("src/lib.rs", "fn hello() {}")]);
    tethys.index().expect("first index failed");

    let file_before = tethys
        .get_file(Path::new("src/lib.rs"))
        .expect("get_file failed")
        .expect("file should exist after first index");
    let hash_before = file_before.content_hash;

    // Modify content and re-index
    thread::sleep(Duration::from_millis(50));
    fs::write(dir.path().join("src/lib.rs"), "fn goodbye() {}").unwrap();
    tethys.index().expect("re-index failed");

    let file_after = tethys
        .get_file(Path::new("src/lib.rs"))
        .expect("get_file failed")
        .expect("file should exist after re-index");
    let hash_after = file_after.content_hash;

    assert_ne!(
        hash_before, hash_after,
        "content hash should change when file content changes"
    );
}

// -- Rebuild tests --

#[test]
fn rebuild_recreates_schema_and_reindexes() {
    let (_dir, mut tethys) = workspace_with_files(&[
        ("src/lib.rs", "fn hello() {}"),
        ("src/other.rs", "fn other() {}"),
    ]);
    let initial_stats = tethys.index().expect("initial index failed");
    assert_eq!(initial_stats.files_indexed, 2);

    // Rebuild should recreate everything
    let rebuild_stats = tethys.rebuild().expect("rebuild failed");
    assert_eq!(
        rebuild_stats.files_indexed, 2,
        "rebuild should re-index all files"
    );

    // Verify data is still accessible after rebuild
    let lib_file = tethys
        .get_file(Path::new("src/lib.rs"))
        .expect("get_file after rebuild failed");
    assert!(lib_file.is_some(), "src/lib.rs should exist after rebuild");

    let other_file = tethys
        .get_file(Path::new("src/other.rs"))
        .expect("get_file after rebuild failed");
    assert!(
        other_file.is_some(),
        "src/other.rs should exist after rebuild"
    );

    let symbols = tethys
        .search_symbols("hello")
        .expect("search after rebuild failed");
    assert!(!symbols.is_empty(), "should find symbols after rebuild");
}

// -- Depth-limited impact analysis tests --

#[test]
fn depth_limits_forward_dependency_traversal() {
    // Create a chain: a -> b -> c (via function calls)
    let (_dir, mut tethys) = workspace_with_files(&[
        ("src/a.rs", "use crate::b;\npub fn a_fn() { b::b_fn(); }"),
        ("src/b.rs", "use crate::c;\npub fn b_fn() { c::c_fn(); }"),
        ("src/c.rs", "pub fn c_fn() {}"),
    ]);
    tethys.index().expect("index failed");

    // depth=1 from a.rs should only show direct dependencies
    let impact_d1 = tethys
        .get_impact(std::path::Path::new("src/a.rs"), Some(1))
        .expect("get_impact depth=1 failed");

    // depth=50 (default) should show transitive dependencies
    let impact_full = tethys
        .get_impact(std::path::Path::new("src/a.rs"), None)
        .expect("get_impact default depth failed");

    // The full traversal should find at least as many (likely more) reachable files
    assert!(
        impact_full.transitive_dependents.len() >= impact_d1.transitive_dependents.len(),
        "full depth should find >= depth-1 results"
    );
}
