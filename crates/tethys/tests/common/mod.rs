//! Shared helpers for tethys integration tests.
//!
//! Lives at `tests/common/mod.rs` so cargo treats it as a sub-module of each
//! integration test binary that does `mod common;`, rather than building it
//! as its own integration test target.

#![allow(dead_code)]

use std::fs;

use rusqlite::Connection;
use tempfile::TempDir;
use tethys::Tethys;

/// Create a temporary workspace, write the given files into it, and return a
/// fresh `Tethys` rooted at that workspace.
///
/// The `TempDir` is returned alongside so the caller can hold it for the
/// lifetime of the test — dropping it would delete the on-disk fixture out
/// from under the indexer.
pub fn workspace_with_files(files: &[(&str, &str)]) -> (TempDir, Tethys) {
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

/// Open a read-only connection to a `Tethys` workspace's `tethys.db` for
/// direct SQL inspection in assertions.
pub fn open_db(tethys: &Tethys) -> Connection {
    Connection::open(tethys.db_path()).expect("opening tethys.db should succeed")
}
