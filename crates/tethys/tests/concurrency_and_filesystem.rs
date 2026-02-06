//! Integration tests for concurrency patterns and filesystem edge cases.
//!
//! Tests that Tethys handles real-world scenarios correctly:
//! - Concurrent reads after indexing
//! - Re-indexing consistency
//! - Deeply nested directories
//! - Many files in a single directory
//! - Symlinks, unreadable directories, special-character filenames

use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::thread;
use tempfile::TempDir;
use tethys::Tethys;

/// Create a temporary workspace with the given files.
fn workspace_with_files(files: &[(&str, &str)]) -> (TempDir, Tethys) {
    let dir = tempfile::tempdir().expect("should create temp dir");

    for (path, content) in files {
        let full_path = dir.path().join(path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).expect("should create parent dirs");
        }
        fs::write(&full_path, content).expect("should write file");
    }

    let tethys = Tethys::new(dir.path()).expect("should create Tethys");
    (dir, tethys)
}

// === Concurrency: reads after indexing ===

#[test]
fn concurrent_searches_after_indexing_return_consistent_results() {
    let (_dir, mut tethys) = workspace_with_files(&[
        (
            "src/auth.rs",
            "pub fn authenticate() {}\npub fn authorize() {}\n",
        ),
        (
            "src/db.rs",
            "pub fn connect() {}\npub fn query() {}\npub fn disconnect() {}\n",
        ),
        (
            "src/api.rs",
            "pub fn handle_request() {}\npub fn send_response() {}\n",
        ),
    ]);

    let stats = tethys.index().expect("index should succeed");
    assert_eq!(stats.files_indexed, 3);

    // Wrap in Arc for shared read access across threads
    // Tethys uses Mutex<Connection> internally, so concurrent reads are safe
    let tethys = Arc::new(tethys);

    let handles: Vec<_> = (0..4)
        .map(|_| {
            let tethys = Arc::clone(&tethys);
            thread::spawn(move || {
                let results = tethys
                    .search_symbols("auth")
                    .expect("search should succeed");
                assert!(
                    !results.is_empty(),
                    "concurrent search should find auth symbols"
                );
                results.len()
            })
        })
        .collect();

    let counts: Vec<usize> = handles.into_iter().map(|h| h.join().unwrap()).collect();

    // All threads should see the same number of results
    assert!(
        counts.windows(2).all(|w| w[0] == w[1]),
        "all concurrent searches should return same count, got: {counts:?}"
    );
}

#[test]
fn concurrent_symbol_and_file_queries_after_indexing() {
    let (_dir, mut tethys) = workspace_with_files(&[(
        "src/lib.rs",
        r"
pub struct Config {
    pub name: String,
}

pub fn load_config() -> Config {
    Config { name: String::new() }
}
",
    )]);

    tethys.index().expect("index should succeed");
    let tethys = Arc::new(tethys);

    // Spawn threads doing different query types concurrently
    let t1 = {
        let t = Arc::clone(&tethys);
        thread::spawn(move || t.search_symbols("Config").expect("search should succeed"))
    };

    let t2 = {
        let t = Arc::clone(&tethys);
        thread::spawn(move || {
            t.get_file(Path::new("src/lib.rs"))
                .expect("get_file should succeed")
        })
    };

    let t3 = {
        let t = Arc::clone(&tethys);
        thread::spawn(move || t.get_stats().expect("get_stats should succeed"))
    };

    let symbols = t1.join().expect("search thread should not panic");
    let file = t2.join().expect("get_file thread should not panic");
    let stats = t3.join().expect("get_stats thread should not panic");

    assert!(!symbols.is_empty(), "should find Config symbol");
    assert!(file.is_some(), "should find lib.rs");
    assert!(stats.file_count > 0, "should have indexed files");
}

// === Re-indexing consistency ===

#[test]
fn reindex_produces_same_symbol_count() {
    let (_dir, mut tethys) = workspace_with_files(&[
        (
            "src/lib.rs",
            "pub fn alpha() {}\npub fn beta() {}\npub fn gamma() {}\n",
        ),
        ("src/util.rs", "pub fn helper() {}\n"),
    ]);

    let stats1 = tethys.index().expect("first index should succeed");
    let stats2 = tethys.index().expect("second index should succeed");

    assert_eq!(
        stats1.files_indexed, stats2.files_indexed,
        "re-index should find same number of files"
    );
    assert_eq!(
        stats1.symbols_found, stats2.symbols_found,
        "re-index should find same number of symbols"
    );
}

#[test]
fn rebuild_clears_and_reindexes_cleanly() {
    let (_dir, mut tethys) =
        workspace_with_files(&[("src/lib.rs", "pub fn original() {}\npub struct Data {}\n")]);

    let stats1 = tethys.index().expect("initial index should succeed");
    let stats2 = tethys.rebuild().expect("rebuild should succeed");

    assert_eq!(
        stats1.symbols_found, stats2.symbols_found,
        "rebuild should produce same symbol count"
    );

    // Verify queries still work after rebuild
    let symbols = tethys
        .search_symbols("original")
        .expect("search after rebuild should succeed");
    assert!(!symbols.is_empty(), "should find symbols after rebuild");
}

// === Filesystem edge cases: deeply nested directories ===

#[test]
fn indexes_files_in_deeply_nested_directories() {
    let dir = tempfile::tempdir().expect("should create temp dir");

    // Create a path 20 levels deep
    let mut nested_path = String::from("src");
    for i in 0..20 {
        write!(nested_path, "/level{i}").unwrap();
    }
    let file_path = format!("{nested_path}/deep.rs");
    let full_path = dir.path().join(&file_path);
    fs::create_dir_all(full_path.parent().unwrap()).expect("should create deep dirs");
    fs::write(&full_path, "pub fn deep_function() {}\n").expect("should write deep file");

    let mut tethys = Tethys::new(dir.path()).expect("should create Tethys");
    let stats = tethys.index().expect("index should succeed");

    assert_eq!(
        stats.files_indexed, 1,
        "should index file in deeply nested dir"
    );
    assert_eq!(stats.symbols_found, 1, "should find function in deep file");

    let symbols = tethys
        .search_symbols("deep_function")
        .expect("search should succeed");
    assert_eq!(symbols.len(), 1, "should find the deeply nested function");
}

// === Filesystem edge cases: many files in one directory ===

#[test]
fn indexes_many_files_in_single_directory() {
    let dir = tempfile::tempdir().expect("should create temp dir");
    let src_dir = dir.path().join("src");
    fs::create_dir_all(&src_dir).expect("should create src dir");

    let file_count = 100;
    for i in 0..file_count {
        let content = format!("pub fn func_{i}() {{}}\n");
        fs::write(src_dir.join(format!("file_{i}.rs")), content).expect("should write file");
    }

    let mut tethys = Tethys::new(dir.path()).expect("should create Tethys");
    let stats = tethys.index().expect("index should succeed");

    assert_eq!(
        stats.files_indexed, file_count,
        "should index all {file_count} files"
    );
    assert_eq!(
        stats.symbols_found, file_count,
        "should find one symbol per file"
    );
    assert!(
        stats.errors.is_empty(),
        "should have no errors indexing {file_count} files"
    );
}

// === Filesystem edge cases: symlinks ===

#[cfg(unix)]
#[test]
fn follows_symlinked_files() {
    let dir = tempfile::tempdir().expect("should create temp dir");
    let src_dir = dir.path().join("src");
    fs::create_dir_all(&src_dir).expect("should create src dir");

    // Create a real file outside src/
    let real_file = dir.path().join("real_module.rs");
    fs::write(&real_file, "pub fn from_symlink() {}\n").expect("should write real file");

    // Create a symlink inside src/ pointing to the real file
    std::os::unix::fs::symlink(&real_file, src_dir.join("linked.rs"))
        .expect("should create symlink");

    let mut tethys = Tethys::new(dir.path()).expect("should create Tethys");
    let stats = tethys.index().expect("index should succeed");

    // The real file is at root level (not under src/), but the symlink is under src/
    // Both may or may not be indexed depending on walk logic; at minimum the symlink target
    // should be readable
    assert!(
        stats.files_indexed >= 1,
        "should index at least the symlinked file"
    );

    let symbols = tethys
        .search_symbols("from_symlink")
        .expect("search should succeed");
    assert!(
        !symbols.is_empty(),
        "should find symbol from symlinked file"
    );
}

#[cfg(unix)]
#[test]
fn follows_symlinked_directories() {
    let dir = tempfile::tempdir().expect("should create temp dir");

    // Create a real directory with a file
    let real_dir = dir.path().join("real_modules");
    fs::create_dir_all(&real_dir).expect("should create real dir");
    fs::write(real_dir.join("module.rs"), "pub fn in_linked_dir() {}\n")
        .expect("should write file");

    // Create src/ with a symlink to real_modules/
    let src_dir = dir.path().join("src");
    fs::create_dir_all(&src_dir).expect("should create src dir");
    std::os::unix::fs::symlink(&real_dir, src_dir.join("linked_modules"))
        .expect("should create dir symlink");

    let mut tethys = Tethys::new(dir.path()).expect("should create Tethys");
    let _stats = tethys.index().expect("index should succeed");

    // Should index the file through the symlinked directory
    let symbols = tethys
        .search_symbols("in_linked_dir")
        .expect("search should succeed");
    assert!(
        !symbols.is_empty(),
        "should find symbol from file in symlinked directory"
    );
}

// === Filesystem edge cases: unreadable directory ===

#[cfg(unix)]
#[test]
fn unreadable_directory_is_skipped_gracefully() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().expect("should create temp dir");
    let src_dir = dir.path().join("src");
    fs::create_dir_all(&src_dir).expect("should create src dir");

    // Create a readable file
    fs::write(src_dir.join("good.rs"), "pub fn accessible() {}\n").expect("should write good file");

    // Create an unreadable subdirectory
    let restricted = src_dir.join("restricted");
    fs::create_dir_all(&restricted).expect("should create restricted dir");
    fs::write(restricted.join("secret.rs"), "pub fn hidden() {}\n")
        .expect("should write secret file");

    // Remove read permission from the directory
    fs::set_permissions(&restricted, fs::Permissions::from_mode(0o000))
        .expect("should set permissions");

    let mut tethys = Tethys::new(dir.path()).expect("should create Tethys");
    let stats = tethys
        .index()
        .expect("index should succeed despite unreadable dir");

    // Restore permissions for cleanup
    fs::set_permissions(&restricted, fs::Permissions::from_mode(0o755))
        .expect("should restore permissions");

    // The good file should be indexed; the restricted directory should be skipped
    assert!(
        stats.files_indexed >= 1,
        "should index at least the accessible file"
    );

    let symbols = tethys
        .search_symbols("accessible")
        .expect("search should succeed");
    assert!(
        !symbols.is_empty(),
        "should find symbol from accessible file"
    );
}

// === Filesystem edge cases: special characters in filenames ===

#[test]
fn indexes_files_with_spaces_in_path() {
    let dir = tempfile::tempdir().expect("should create temp dir");
    let src_dir = dir.path().join("src/my module");
    fs::create_dir_all(&src_dir).expect("should create dir with spaces");

    fs::write(src_dir.join("my file.rs"), "pub fn spaced_function() {}\n")
        .expect("should write file with spaces");

    let mut tethys = Tethys::new(dir.path()).expect("should create Tethys");
    let stats = tethys.index().expect("index should succeed");

    assert_eq!(
        stats.files_indexed, 1,
        "should index file with spaces in path"
    );
    assert_eq!(stats.symbols_found, 1);
}

#[test]
fn indexes_files_with_unicode_in_path() {
    let dir = tempfile::tempdir().expect("should create temp dir");
    let src_dir = dir.path().join("src/\u{00e9}t\u{00e9}");
    fs::create_dir_all(&src_dir).expect("should create unicode dir");

    fs::write(src_dir.join("caf\u{00e9}.rs"), "pub fn unicode_name() {}\n")
        .expect("should write unicode file");

    let mut tethys = Tethys::new(dir.path()).expect("should create Tethys");
    let stats = tethys.index().expect("index should succeed");

    assert_eq!(
        stats.files_indexed, 1,
        "should index file with unicode path"
    );
    assert_eq!(stats.symbols_found, 1);
}

// === Filesystem edge cases: empty directories ===

#[test]
fn empty_subdirectories_do_not_cause_errors() {
    let dir = tempfile::tempdir().expect("should create temp dir");
    let src_dir = dir.path().join("src");
    fs::create_dir_all(&src_dir).expect("should create src dir");

    // Create several empty subdirectories
    for name in &["models", "services", "utils", "tests"] {
        fs::create_dir_all(src_dir.join(name)).expect("should create subdir");
    }

    // One file in src/ itself
    fs::write(src_dir.join("lib.rs"), "pub fn main_lib() {}\n").expect("should write lib file");

    let mut tethys = Tethys::new(dir.path()).expect("should create Tethys");
    let stats = tethys.index().expect("index should succeed");

    assert_eq!(stats.files_indexed, 1);
    assert!(stats.errors.is_empty());
}

// === Filesystem edge cases: large file ===

#[test]
fn indexes_large_file_with_many_symbols() {
    let dir = tempfile::tempdir().expect("should create temp dir");
    let src_dir = dir.path().join("src");
    fs::create_dir_all(&src_dir).expect("should create src dir");

    // Generate a file with 500 functions
    let mut content = String::new();
    let symbol_count = 500;
    for i in 0..symbol_count {
        writeln!(content, "pub fn function_{i}() -> i32 {{ {i} }}\n").unwrap();
    }

    fs::write(src_dir.join("big.rs"), &content).expect("should write large file");

    let mut tethys = Tethys::new(dir.path()).expect("should create Tethys");
    let stats = tethys.index().expect("index should succeed");

    assert_eq!(stats.files_indexed, 1);
    assert_eq!(
        stats.symbols_found, symbol_count,
        "should extract all {symbol_count} symbols from large file"
    );
}
