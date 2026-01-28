//! Shared utilities for Tethys benchmarks.

// Benchmark utilities - pedantic lints not critical here
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::must_use_candidate)]

use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;
use tethys::Tethys;

/// A workspace ready for benchmarking with Tethys already indexed.
pub struct IndexedWorkspace {
    /// Temp directory - must be kept alive for the duration of the benchmark.
    /// Access the workspace path via `dir.path()`.
    pub dir: TempDir,
    /// Tethys instance with indexed workspace.
    pub tethys: Tethys,
}

/// Create a temporary workspace with the given files.
/// Returns the temp directory (must be kept alive) and the workspace path.
pub fn create_workspace(files: &[(&str, &str)]) -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("failed to create temp dir");

    for (path, content) in files {
        let full_path = dir.path().join(path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).expect("failed to create parent dirs");
        }
        fs::write(&full_path, content).expect("failed to write file");
    }

    let path = dir.path().to_path_buf();
    (dir, path)
}

/// Convert owned file list to borrowed references for `create_workspace`.
pub fn as_file_refs(files: &[(String, String)]) -> Vec<(&str, &str)> {
    files
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect()
}

/// Create a workspace, initialize Tethys, and run indexing.
/// Returns the indexed workspace ready for benchmarking queries.
pub fn create_indexed_workspace(files: &[(&str, &str)]) -> IndexedWorkspace {
    let (dir, path) = create_workspace(files);
    let mut tethys = Tethys::new(&path).expect("failed to create Tethys");
    tethys.index().expect("index failed");
    IndexedWorkspace { dir, tethys }
}
