//! Integration tests for Phase 3 graph operations.
//!
//! These tests verify the graph analysis pipeline through the public Tethys API:
//! - File impact analysis (direct and transitive dependents)
//! - Dependency chain finding (shortest path between files)
//! - Cycle detection

use std::fs;
use tempfile::TempDir;
use tethys::Tethys;

/// Create a workspace with a known dependency structure for testing.
///
/// Dependency graph:
/// ```text
///     main.rs
///      /    \
///     v      v
/// auth.rs  cache.rs
///      \    /
///       v  v
///      db.rs (leaf)
/// ```
fn workspace_with_call_graph() -> (TempDir, Tethys) {
    let dir = tempfile::tempdir().expect("failed to create temp dir");

    // Create src directory
    fs::create_dir_all(dir.path().join("src")).unwrap();

    // main.rs uses auth::User and cache::Cache
    fs::write(
        dir.path().join("src/main.rs"),
        r"
use crate::auth::User;
use crate::cache::Cache;

fn main() {
    let _user = User;
    let _cache = Cache;
}
",
    )
    .unwrap();

    // auth.rs uses db::Connection
    fs::write(
        dir.path().join("src/auth.rs"),
        r"
use crate::db::Connection;

pub struct User;

impl User {
    pub fn connect() -> Connection {
        Connection
    }
}
",
    )
    .unwrap();

    // cache.rs uses db::Connection
    fs::write(
        dir.path().join("src/cache.rs"),
        r"
use crate::db::Connection;

pub struct Cache;

impl Cache {
    pub fn get_conn() -> Connection {
        Connection
    }
}
",
    )
    .unwrap();

    // db.rs is the leaf - exports Connection
    fs::write(
        dir.path().join("src/db.rs"),
        r"
pub struct Connection;
",
    )
    .unwrap();

    // lib.rs declares all modules
    fs::write(
        dir.path().join("src/lib.rs"),
        r"
mod auth;
mod cache;
mod db;
",
    )
    .unwrap();

    let tethys = Tethys::new(dir.path()).expect("failed to create Tethys");
    (dir, tethys)
}

// ============================================================================
// Impact Analysis Tests
// ============================================================================

#[test]
fn get_impact_returns_file_dependents() {
    let (_dir, mut tethys) = workspace_with_call_graph();
    tethys.index().expect("index failed");

    let impact = tethys
        .get_impact(std::path::Path::new("src/db.rs"))
        .expect("get_impact failed");

    // db.rs should have auth.rs and cache.rs as direct dependents
    assert!(
        !impact.direct_dependents.is_empty(),
        "db.rs should have dependents"
    );
}

#[test]
fn get_impact_returns_transitive_dependents() {
    let (_dir, mut tethys) = workspace_with_call_graph();
    tethys.index().expect("index failed");

    let impact = tethys
        .get_impact(std::path::Path::new("src/db.rs"))
        .expect("get_impact failed");

    // db.rs's transitive dependents should include files that depend on auth.rs and cache.rs
    // (i.e., main.rs depends on auth.rs and cache.rs which depend on db.rs)
    let total_dependents = impact.direct_dependents.len() + impact.transitive_dependents.len();
    assert!(
        total_dependents >= 2,
        "db.rs should have at least 2 total dependents (auth, cache, possibly main), got: {total_dependents}"
    );
}

#[test]
fn get_impact_returns_empty_for_leaf_with_no_dependents() {
    let (_dir, mut tethys) = workspace_with_call_graph();
    tethys.index().expect("index failed");

    // main.rs is at the top of the dependency tree - nothing depends on it
    let impact = tethys
        .get_impact(std::path::Path::new("src/main.rs"))
        .expect("get_impact failed");

    assert!(
        impact.direct_dependents.is_empty(),
        "main.rs should have no direct dependents, got: {:?}",
        impact.direct_dependents
    );
}

// ============================================================================
// Dependency Chain Tests
// ============================================================================

#[test]
fn get_dependency_chain_finds_path() {
    let (_dir, mut tethys) = workspace_with_call_graph();
    tethys.index().expect("index failed");

    let chain = tethys
        .get_dependency_chain(
            std::path::Path::new("src/auth.rs"),
            std::path::Path::new("src/db.rs"),
        )
        .expect("get_dependency_chain failed");

    assert!(chain.is_some(), "should find path from auth.rs to db.rs");
    let chain = chain.unwrap();
    assert!(chain.len() >= 2, "path should have at least 2 files");
}

#[test]
fn get_dependency_chain_returns_none_for_unconnected() {
    let (_dir, mut tethys) = workspace_with_call_graph();
    tethys.index().expect("index failed");

    // db.rs doesn't depend on main.rs (reverse direction)
    let chain = tethys
        .get_dependency_chain(
            std::path::Path::new("src/db.rs"),
            std::path::Path::new("src/main.rs"),
        )
        .expect("get_dependency_chain failed");

    assert!(chain.is_none(), "should not find path in reverse direction");
}

#[test]
fn get_dependency_chain_returns_none_for_same_file() {
    let (_dir, mut tethys) = workspace_with_call_graph();
    tethys.index().expect("index failed");

    // A file to itself might return None or a single-element path
    let chain = tethys
        .get_dependency_chain(
            std::path::Path::new("src/db.rs"),
            std::path::Path::new("src/db.rs"),
        )
        .expect("get_dependency_chain failed");

    // Either None or a trivial path is acceptable
    if let Some(path) = chain {
        assert!(
            path.len() <= 1,
            "same-file path should be trivial, got: {path:?}"
        );
    }
}

#[test]
fn get_dependency_chain_finds_shortest_path() {
    let (_dir, mut tethys) = workspace_with_call_graph();
    tethys.index().expect("index failed");

    // auth.rs -> db.rs should be direct (2 nodes)
    let chain = tethys
        .get_dependency_chain(
            std::path::Path::new("src/auth.rs"),
            std::path::Path::new("src/db.rs"),
        )
        .expect("get_dependency_chain failed");

    if let Some(path) = chain {
        assert_eq!(
            path.len(),
            2,
            "direct dependency should have 2 files in path, got: {path:?}"
        );
    }
}

// ============================================================================
// Cycle Detection Tests
// ============================================================================

#[test]
fn detect_cycles_returns_not_implemented_error() {
    let (_dir, mut tethys) = workspace_with_call_graph();
    tethys.index().expect("index failed");

    let result = tethys.detect_cycles();

    // Cycle detection is not yet implemented and returns an error
    assert!(
        result.is_err(),
        "detect_cycles should return error (not yet implemented)"
    );
    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("not yet implemented"),
        "error should indicate not implemented, got: {err}"
    );
}

/// NOTE: Cycle detection is not yet implemented. This test verifies that the
/// cyclic dependencies ARE recorded in the `file_deps` table, even though
/// cycle detection returns an error.
#[test]
fn cyclic_dependencies_are_recorded_in_file_deps() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    fs::create_dir_all(dir.path().join("src")).unwrap();

    // Create a simple A -> B -> A cycle
    fs::write(
        dir.path().join("src/lib.rs"),
        r"
mod a;
mod b;
",
    )
    .unwrap();

    fs::write(
        dir.path().join("src/a.rs"),
        r"
use crate::b::B;

pub struct A;

impl A {
    pub fn get_b() -> B { B }
}
",
    )
    .unwrap();

    fs::write(
        dir.path().join("src/b.rs"),
        r"
use crate::a::A;

pub struct B;

impl B {
    pub fn get_a() -> A { A }
}
",
    )
    .unwrap();

    let mut tethys = Tethys::new(dir.path()).expect("failed to create Tethys");
    tethys.index().expect("index failed");

    // Cycle detection returns error (not yet implemented)
    let result = tethys.detect_cycles();
    assert!(result.is_err(), "detect_cycles should return error");

    // Verify the cyclic dependencies ARE recorded in the file_deps table
    // (cycle detection is not implemented, but dependencies are tracked)
    let deps_a = tethys
        .get_dependencies(std::path::Path::new("src/a.rs"))
        .expect("get_dependencies failed");
    let deps_b = tethys
        .get_dependencies(std::path::Path::new("src/b.rs"))
        .expect("get_dependencies failed");

    assert!(
        deps_a.iter().any(|p| p.to_string_lossy().contains("b.rs")),
        "a.rs should depend on b.rs"
    );
    assert!(
        deps_b.iter().any(|p| p.to_string_lossy().contains("a.rs")),
        "b.rs should depend on a.rs"
    );
}

/// This test verifies that three-file cyclic dependencies are recorded in `file_deps`.
#[test]
fn three_file_cycle_dependencies_are_recorded() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    fs::create_dir_all(dir.path().join("src")).unwrap();

    // Create A -> B -> C -> A cycle
    fs::write(dir.path().join("src/lib.rs"), "mod a;\nmod b;\nmod c;").unwrap();

    fs::write(
        dir.path().join("src/a.rs"),
        r"
use crate::b::B;

pub struct A;

impl A {
    pub fn get() -> B { B }
}
",
    )
    .unwrap();

    fs::write(
        dir.path().join("src/b.rs"),
        r"
use crate::c::C;

pub struct B;

impl B {
    pub fn get() -> C { C }
}
",
    )
    .unwrap();

    fs::write(
        dir.path().join("src/c.rs"),
        r"
use crate::a::A;

pub struct C;

impl C {
    pub fn get() -> A { A }
}
",
    )
    .unwrap();

    let mut tethys = Tethys::new(dir.path()).expect("failed to create Tethys");
    tethys.index().expect("index failed");

    // Verify all cycle edges are recorded in file_deps
    let deps_a = tethys
        .get_dependencies(std::path::Path::new("src/a.rs"))
        .expect("get_dependencies failed");
    let deps_b = tethys
        .get_dependencies(std::path::Path::new("src/b.rs"))
        .expect("get_dependencies failed");
    let deps_c = tethys
        .get_dependencies(std::path::Path::new("src/c.rs"))
        .expect("get_dependencies failed");

    assert!(
        deps_a.iter().any(|p| p.to_string_lossy().contains("b.rs")),
        "a.rs should depend on b.rs, got: {deps_a:?}"
    );
    assert!(
        deps_b.iter().any(|p| p.to_string_lossy().contains("c.rs")),
        "b.rs should depend on c.rs, got: {deps_b:?}"
    );
    assert!(
        deps_c.iter().any(|p| p.to_string_lossy().contains("a.rs")),
        "c.rs should depend on a.rs, got: {deps_c:?}"
    );

    // Cycle detection is not yet implemented
    let result = tethys.detect_cycles();
    assert!(
        result.is_err(),
        "detect_cycles should return error (not yet implemented)"
    );
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[test]
fn get_impact_returns_error_for_nonexistent_file() {
    let (_dir, mut tethys) = workspace_with_call_graph();
    tethys.index().expect("index failed");

    let result = tethys.get_impact(std::path::Path::new("src/nonexistent.rs"));

    assert!(
        result.is_err(),
        "should return error for non-indexed file, got: {result:?}"
    );
}

#[test]
fn get_dependency_chain_returns_error_for_nonexistent_from() {
    let (_dir, mut tethys) = workspace_with_call_graph();
    tethys.index().expect("index failed");

    let result = tethys.get_dependency_chain(
        std::path::Path::new("src/nonexistent.rs"),
        std::path::Path::new("src/db.rs"),
    );

    assert!(
        result.is_err(),
        "should return error when 'from' file doesn't exist"
    );
}

#[test]
fn get_dependency_chain_returns_error_for_nonexistent_to() {
    let (_dir, mut tethys) = workspace_with_call_graph();
    tethys.index().expect("index failed");

    let result = tethys.get_dependency_chain(
        std::path::Path::new("src/db.rs"),
        std::path::Path::new("src/nonexistent.rs"),
    );

    assert!(
        result.is_err(),
        "should return error when 'to' file doesn't exist"
    );
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn graph_operations_work_after_reindex() {
    let (_dir, mut tethys) = workspace_with_call_graph();

    // Index twice
    tethys.index().expect("first index failed");
    tethys.rebuild().expect("rebuild failed");

    // Graph operations should still work
    let impact = tethys
        .get_impact(std::path::Path::new("src/db.rs"))
        .expect("get_impact failed after reindex");

    assert!(
        !impact.direct_dependents.is_empty(),
        "impact analysis should work after reindex"
    );

    // Cycle detection consistently returns error (not yet implemented)
    let result = tethys.detect_cycles();
    assert!(
        result.is_err(),
        "detect_cycles should return error (not yet implemented)"
    );
}

#[test]
fn empty_workspace_detect_cycles_returns_error() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let mut tethys = Tethys::new(dir.path()).expect("failed to create Tethys");

    tethys.index().expect("index failed");

    // Cycle detection is not yet implemented
    let result = tethys.detect_cycles();
    assert!(
        result.is_err(),
        "detect_cycles should return error (not yet implemented)"
    );
}

#[test]
fn single_file_workspace_detect_cycles_returns_error() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/lib.rs"), "pub fn hello() {}").unwrap();

    let mut tethys = Tethys::new(dir.path()).expect("failed to create Tethys");
    tethys.index().expect("index failed");

    // Cycle detection is not yet implemented
    let result = tethys.detect_cycles();
    assert!(
        result.is_err(),
        "detect_cycles should return error (not yet implemented)"
    );
}

// ============================================================================
// Symbol-Level Graph Analysis Tests: get_callers
// ============================================================================

/// Create a workspace with intra-file symbol references for symbol graph testing.
///
/// This workspace has symbols that call other symbols within the same file,
/// which is required for the symbol graph since cross-file reference resolution
/// is not yet implemented.
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

    let tethys = Tethys::new(dir.path()).expect("failed to create Tethys");
    (dir, tethys)
}

#[test]
fn get_callers_returns_error_for_nonexistent_symbol() {
    let (_dir, mut tethys) = workspace_with_call_graph();
    tethys.index().expect("index failed");

    let result = tethys.get_callers("NonExistent");

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
fn get_callers_returns_empty_for_uncalled_symbol() {
    let (_dir, mut tethys) = workspace_with_intra_file_calls();
    tethys.index().expect("index failed");

    // process is the top-level function - nothing calls it
    let callers = tethys
        .get_callers("process")
        .expect("get_callers for process should succeed");

    assert!(
        callers.is_empty(),
        "process should have no callers, got: {callers:?}"
    );
}

#[test]
fn get_callers_finds_intra_file_callers() {
    let (_dir, mut tethys) = workspace_with_intra_file_calls();
    tethys.index().expect("index failed");

    // validate is called by process
    let callers = tethys
        .get_callers("validate")
        .expect("get_callers for validate should succeed");

    assert!(
        !callers.is_empty(),
        "validate should have at least one caller (process)"
    );
}

#[test]
fn get_callers_cross_file_refs_not_resolved() {
    let (_dir, mut tethys) = workspace_with_call_graph();
    tethys.index().expect("index failed");

    // Connection is referenced from other files via `use crate::db::Connection`,
    // but cross-file symbol resolution is not yet implemented, so callers should be empty.
    let callers = tethys
        .get_callers("Connection")
        .expect("get_callers for Connection should succeed");

    assert!(
        callers.is_empty(),
        "cross-file callers should not be resolved yet, got: {callers:?}"
    );
}

// ============================================================================
// Symbol-Level Graph Analysis Tests: get_symbol_dependencies
// ============================================================================

#[test]
fn get_symbol_dependencies_returns_error_for_nonexistent_symbol() {
    let (_dir, mut tethys) = workspace_with_call_graph();
    tethys.index().expect("index failed");

    let result = tethys.get_symbol_dependencies("DoesNotExist");

    assert!(
        result.is_err(),
        "should return error for non-existent symbol"
    );
}

#[test]
fn get_symbol_dependencies_returns_empty_for_leaf_symbol() {
    let (_dir, mut tethys) = workspace_with_intra_file_calls();
    tethys.index().expect("index failed");

    // Helper is a leaf struct with no outgoing calls
    let deps = tethys
        .get_symbol_dependencies("Helper")
        .expect("get_symbol_dependencies for Helper should succeed");

    assert!(
        deps.is_empty(),
        "Helper (leaf struct) should have no dependencies, got: {deps:?}"
    );
}

#[test]
fn get_symbol_dependencies_finds_callees() {
    let (_dir, mut tethys) = workspace_with_intra_file_calls();
    tethys.index().expect("index failed");

    // validate calls Helper::new and Helper::check
    let deps = tethys
        .get_symbol_dependencies("validate")
        .expect("get_symbol_dependencies for validate should succeed");

    assert!(!deps.is_empty(), "validate should have dependencies");
}

#[test]
fn get_symbol_dependencies_cross_file_not_resolved() {
    let (_dir, mut tethys) = workspace_with_call_graph();
    tethys.index().expect("index failed");

    // main references User and Cache from other files, but cross-file
    // resolution is not implemented, so dependencies should be empty.
    let deps = tethys
        .get_symbol_dependencies("main")
        .expect("get_symbol_dependencies for main should succeed");

    assert!(
        deps.is_empty(),
        "cross-file dependencies should not be resolved yet, got: {deps:?}"
    );
}

// ============================================================================
// Symbol-Level Graph Analysis Tests: get_symbol_impact
// ============================================================================

#[test]
fn get_symbol_impact_returns_error_for_nonexistent_symbol() {
    let (_dir, mut tethys) = workspace_with_call_graph();
    tethys.index().expect("index failed");

    let result = tethys.get_symbol_impact("NoSuchSymbol");

    assert!(
        result.is_err(),
        "should return error for non-existent symbol"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("not found") || err.contains("Not found") || err.contains("NoSuchSymbol"),
        "error should indicate symbol not found, got: {err}"
    );
}

#[test]
fn get_symbol_impact_returns_empty_for_uncalled_symbol() {
    let (_dir, mut tethys) = workspace_with_intra_file_calls();
    tethys.index().expect("index failed");

    // process is never called by other symbols
    let impact = tethys
        .get_symbol_impact("process")
        .expect("get_symbol_impact for process should succeed");

    assert!(
        impact.direct_dependents.is_empty(),
        "process should have no direct dependents, got: {:?}",
        impact.direct_dependents
    );
    assert!(
        impact.transitive_dependents.is_empty(),
        "process should have no transitive dependents, got: {:?}",
        impact.transitive_dependents
    );
}

#[test]
fn get_symbol_impact_finds_direct_dependents() {
    let (_dir, mut tethys) = workspace_with_intra_file_calls();
    tethys.index().expect("index failed");

    // validate is called by process directly
    let impact = tethys
        .get_symbol_impact("validate")
        .expect("get_symbol_impact for validate should succeed");

    assert!(
        !impact.direct_dependents.is_empty(),
        "validate should have direct dependents (process)"
    );
}

#[test]
fn get_symbol_impact_target_points_to_correct_file() {
    let (_dir, mut tethys) = workspace_with_intra_file_calls();
    tethys.index().expect("index failed");

    let impact = tethys
        .get_symbol_impact("validate")
        .expect("get_symbol_impact for validate should succeed");

    assert!(
        impact.target.to_string_lossy().contains("lib.rs"),
        "validate impact target should be lib.rs, got: {:?}",
        impact.target
    );
}

#[test]
fn get_symbol_impact_cross_file_returns_empty() {
    let (_dir, mut tethys) = workspace_with_call_graph();
    tethys.index().expect("index failed");

    // Connection's callers are all cross-file, so impact should be empty
    let impact = tethys
        .get_symbol_impact("Connection")
        .expect("get_symbol_impact for Connection should succeed");

    assert!(
        impact.direct_dependents.is_empty(),
        "cross-file dependents should not be resolved yet, got: {:?}",
        impact.direct_dependents
    );
}
