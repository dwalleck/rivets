//! Integration tests for `get_callers_with_lsp` functionality.
//!
//! These tests verify that LSP-augmented caller detection works correctly:
//! - Graceful fallback when LSP is not available
//! - Proper merging of DB and LSP results (when LSP is available)
//!
//! Tests marked with `#[ignore]` require rust-analyzer to be installed.
//! Run with: `cargo test --test lsp_callers -- --ignored`

use std::fs;
use std::process::Command;
use tempfile::TempDir;
use tethys::Tethys;

/// Check if rust-analyzer is available in PATH.
fn rust_analyzer_available() -> bool {
    let check_cmd = if cfg!(windows) { "where" } else { "which" };
    Command::new(check_cmd)
        .arg("rust-analyzer")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Create a workspace with intra-file symbol references for testing.
///
/// Symbol graph:
/// ```text
///   process() -> validate() -> Helper::new()
///                            -> Helper::check()
/// ```
fn workspace_with_intra_file_calls() -> (TempDir, Tethys) {
    let dir = tempfile::tempdir().expect("failed to create temp dir");

    fs::create_dir_all(dir.path().join("src")).expect("failed to create src dir");

    fs::write(
        dir.path().join("src/lib.rs"),
        r"
pub struct Helper;

impl Helper {
    pub fn new() -> Helper {
        Helper
    }

    pub fn check(&self) -> bool {
        true
    }
}

pub fn validate() -> bool {
    let h = Helper::new();
    h.check()
}

pub fn process() -> bool {
    validate()
}
",
    )
    .expect("failed to write lib.rs");

    // Create Cargo.toml so rust-analyzer can analyze the project
    fs::write(
        dir.path().join("Cargo.toml"),
        r#"
[package]
name = "test_workspace"
version = "0.1.0"
edition = "2021"
"#,
    )
    .expect("failed to write Cargo.toml");

    let tethys = Tethys::new(dir.path()).expect("failed to create Tethys");
    (dir, tethys)
}

// ============================================================================
// Fallback Behavior Tests (run without LSP)
// ============================================================================

#[test]
fn get_callers_with_lsp_returns_db_callers_when_lsp_unavailable() {
    let (_dir, mut tethys) = workspace_with_intra_file_calls();
    tethys.index().expect("index failed");

    // Even if LSP fails, should return DB callers gracefully
    // validate is called by process
    let callers = tethys
        .get_callers_with_lsp("validate")
        .expect("get_callers_with_lsp should succeed even without LSP");

    // Should have at least the DB callers
    assert!(
        !callers.is_empty(),
        "should have callers from DB even if LSP fails"
    );
}

#[test]
fn get_callers_with_lsp_returns_error_for_nonexistent_symbol() {
    let (_dir, mut tethys) = workspace_with_intra_file_calls();
    tethys.index().expect("index failed");

    let result = tethys.get_callers_with_lsp("NonExistent");

    assert!(
        result.is_err(),
        "should return error for non-existent symbol"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("not found") || err.contains("Not found") || err.contains("NonExistent"),
        "error should indicate symbol not found, got: {err}"
    );
}

#[test]
fn get_callers_with_lsp_returns_empty_for_uncalled_symbol() {
    let (_dir, mut tethys) = workspace_with_intra_file_calls();
    tethys.index().expect("index failed");

    // process is never called
    let callers = tethys
        .get_callers_with_lsp("process")
        .expect("get_callers_with_lsp should succeed");

    assert!(
        callers.is_empty(),
        "process should have no callers, got: {callers:?}"
    );
}

#[test]
fn get_callers_with_lsp_matches_get_callers_baseline() {
    let (_dir, mut tethys) = workspace_with_intra_file_calls();
    tethys.index().expect("index failed");

    // get_callers_with_lsp should return at least what get_callers returns
    let db_callers = tethys.get_callers("validate").expect("get_callers failed");
    let lsp_callers = tethys
        .get_callers_with_lsp("validate")
        .expect("get_callers_with_lsp failed");

    // LSP version should have >= DB version (may have additional from LSP)
    assert!(
        lsp_callers.len() >= db_callers.len(),
        "LSP callers ({}) should be >= DB callers ({})",
        lsp_callers.len(),
        db_callers.len()
    );
}

// ============================================================================
// LSP Integration Tests (require rust-analyzer)
// ============================================================================

#[test]
#[ignore = "requires rust-analyzer installed"]
fn get_callers_with_lsp_merges_lsp_results() {
    if !rust_analyzer_available() {
        eprintln!("Skipping test: rust-analyzer not available");
        return;
    }

    let (_dir, mut tethys) = workspace_with_intra_file_calls();
    tethys.index().expect("index failed");

    // With LSP, we may find additional callers
    let callers = tethys
        .get_callers_with_lsp("validate")
        .expect("get_callers_with_lsp should succeed");

    // Should have at least one caller (process)
    assert!(
        !callers.is_empty(),
        "validate should have callers: {callers:?}"
    );
}

/// Test that cross-file references can be found via LSP that tree-sitter might miss.
///
/// This test creates a workspace where a function uses a type from another file,
/// which tree-sitter might not fully resolve but LSP can.
#[test]
#[ignore = "requires rust-analyzer installed"]
fn get_callers_with_lsp_finds_cross_file_callers() {
    if !rust_analyzer_available() {
        eprintln!("Skipping test: rust-analyzer not available");
        return;
    }

    let dir = tempfile::tempdir().expect("failed to create temp dir");
    fs::create_dir_all(dir.path().join("src")).expect("failed to create src dir");

    // Create a multi-file workspace
    fs::write(
        dir.path().join("src/lib.rs"),
        r"
mod helper;
mod caller;
",
    )
    .expect("failed to write lib.rs");

    fs::write(
        dir.path().join("src/helper.rs"),
        r"
pub fn do_work() -> i32 {
    42
}
",
    )
    .expect("failed to write helper.rs");

    fs::write(
        dir.path().join("src/caller.rs"),
        r"
use crate::helper::do_work;

pub fn call_helper() -> i32 {
    do_work()
}
",
    )
    .expect("failed to write caller.rs");

    fs::write(
        dir.path().join("Cargo.toml"),
        r#"
[package]
name = "test_workspace"
version = "0.1.0"
edition = "2021"
"#,
    )
    .expect("failed to write Cargo.toml");

    let mut tethys = Tethys::new(dir.path()).expect("failed to create Tethys");
    tethys.index().expect("index failed");

    // With LSP, should find that call_helper calls do_work
    let callers = tethys
        .get_callers_with_lsp("do_work")
        .expect("get_callers_with_lsp should succeed");

    // LSP should help find the cross-file caller
    assert!(
        !callers.is_empty(),
        "do_work should have callers (call_helper), got: {callers:?}"
    );
}

// ============================================================================
// Deduplication Tests
// ============================================================================

#[test]
fn get_callers_with_lsp_does_not_duplicate_callers() {
    let (_dir, mut tethys) = workspace_with_intra_file_calls();
    tethys.index().expect("index failed");

    let callers = tethys
        .get_callers_with_lsp("validate")
        .expect("get_callers_with_lsp should succeed");

    // Check for duplicates by qualified name
    let caller_names: Vec<_> = callers.iter().map(|c| &c.symbols_used).collect();
    let unique_names: std::collections::HashSet<_> = caller_names.iter().collect();

    assert_eq!(
        caller_names.len(),
        unique_names.len(),
        "should not have duplicate callers: {caller_names:?}"
    );
}
