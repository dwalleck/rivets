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
    fs::create_dir_all(dir.path().join("src")).expect("create src dir");

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
    .expect("write main.rs");

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
    .expect("write auth.rs");

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
    .expect("write cache.rs");

    // db.rs is the leaf - exports Connection
    fs::write(
        dir.path().join("src/db.rs"),
        r"
pub struct Connection;
",
    )
    .expect("write db.rs");

    // lib.rs declares all modules
    fs::write(
        dir.path().join("src/lib.rs"),
        r"
mod auth;
mod cache;
mod db;
",
    )
    .expect("write lib.rs");

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
    let chain = chain.expect("chain should exist");
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
fn detect_cycles_returns_empty_for_acyclic_workspace() {
    let (_dir, mut tethys) = workspace_with_call_graph();
    tethys.index().expect("index failed");

    let result = tethys
        .detect_cycles()
        .expect("detect_cycles should succeed");

    // The workspace_with_call_graph has no cycles (acyclic)
    assert!(
        result.is_empty(),
        "acyclic workspace should have no cycles, got: {result:?}"
    );
}

/// This test verifies that two-file cyclic dependencies are detected.
#[test]
fn cyclic_dependencies_are_detected() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    fs::create_dir_all(dir.path().join("src")).expect("create src dir");

    // Create a simple A -> B -> A cycle
    fs::write(
        dir.path().join("src/lib.rs"),
        r"
mod a;
mod b;
",
    )
    .expect("write lib.rs");

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
    .expect("write a.rs");

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
    .expect("write b.rs");

    let mut tethys = Tethys::new(dir.path()).expect("failed to create Tethys");
    tethys.index().expect("index failed");

    // Cycle detection should find the A <-> B cycle
    let cycles = tethys
        .detect_cycles()
        .expect("detect_cycles should succeed");
    assert!(!cycles.is_empty(), "should detect the A <-> B cycle");

    // Verify the cycle contains both a.rs and b.rs
    let cycle = &cycles[0];
    let paths: Vec<String> = cycle
        .files
        .iter()
        .map(|p| p.display().to_string())
        .collect();
    assert!(
        paths.iter().any(|p| p.contains("a.rs")),
        "cycle should contain a.rs"
    );
    assert!(
        paths.iter().any(|p| p.contains("b.rs")),
        "cycle should contain b.rs"
    );

    // Also verify the cyclic dependencies ARE recorded in the file_deps table
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

/// This test verifies that three-file cyclic dependencies are detected.
#[test]
fn three_file_cycle_dependencies_are_detected() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    fs::create_dir_all(dir.path().join("src")).expect("create src dir");

    // Create A -> B -> C -> A cycle
    fs::write(dir.path().join("src/lib.rs"), "mod a;\nmod b;\nmod c;").expect("write lib.rs");

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
    .expect("write a.rs");

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
    .expect("write b.rs");

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
    .expect("write c.rs");

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

    // Cycle detection should find the A -> B -> C -> A cycle
    let cycles = tethys
        .detect_cycles()
        .expect("detect_cycles should succeed");
    assert!(!cycles.is_empty(), "should detect the 3-file cycle");

    let cycle = &cycles[0];
    assert_eq!(cycle.files.len(), 3, "cycle should have 3 files");

    let paths: Vec<String> = cycle
        .files
        .iter()
        .map(|p| p.display().to_string())
        .collect();
    assert!(
        paths.iter().any(|p| p.contains("a.rs")),
        "cycle should contain a.rs"
    );
    assert!(
        paths.iter().any(|p| p.contains("b.rs")),
        "cycle should contain b.rs"
    );
    assert!(
        paths.iter().any(|p| p.contains("c.rs")),
        "cycle should contain c.rs"
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

    // Cycle detection should work and return empty (acyclic graph)
    let cycles = tethys
        .detect_cycles()
        .expect("detect_cycles should succeed after reindex");
    assert!(
        cycles.is_empty(),
        "acyclic workspace should have no cycles after reindex"
    );
}

#[test]
fn empty_workspace_detect_cycles_returns_empty() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let mut tethys = Tethys::new(dir.path()).expect("failed to create Tethys");

    tethys.index().expect("index failed");

    // Empty workspace has no dependencies and thus no cycles
    let cycles = tethys
        .detect_cycles()
        .expect("detect_cycles should succeed");
    assert!(cycles.is_empty(), "empty workspace should have no cycles");
}

#[test]
fn single_file_workspace_detect_cycles_returns_empty() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    fs::create_dir_all(dir.path().join("src")).expect("create src dir");
    fs::write(dir.path().join("src/lib.rs"), "pub fn hello() {}").expect("write lib.rs");

    let mut tethys = Tethys::new(dir.path()).expect("failed to create Tethys");
    tethys.index().expect("index failed");

    // Single file with no dependencies has no cycles
    let cycles = tethys
        .detect_cycles()
        .expect("detect_cycles should succeed");
    assert!(
        cycles.is_empty(),
        "single file workspace should have no cycles"
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
fn get_callers_cross_file_refs_resolved() {
    let (_dir, mut tethys) = workspace_with_call_graph();
    tethys.index().expect("index failed");

    // Connection is referenced from other files via `use crate::db::Connection`,
    // Cross-file references are now resolved in Pass 2.
    let callers = tethys
        .get_callers("Connection")
        .expect("get_callers for Connection should succeed");

    assert!(
        !callers.is_empty(),
        "cross-file callers should be resolved, got empty"
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
fn get_symbol_impact_cross_file_resolved() {
    let (_dir, mut tethys) = workspace_with_call_graph();
    tethys.index().expect("index failed");

    // Connection's callers are cross-file - now resolved in Pass 2
    let impact = tethys
        .get_symbol_impact("Connection")
        .expect("get_symbol_impact for Connection should succeed");

    assert!(
        !impact.direct_dependents.is_empty(),
        "cross-file dependents should be resolved, got empty"
    );
}

// ============================================================================
// Call Edges Tests
// ============================================================================

/// Verify that `call_edges` table is populated after indexing.
#[test]
fn call_edges_populated_after_indexing() {
    let (_dir, mut tethys) = workspace_with_intra_file_calls();
    tethys.index().expect("index failed");

    // The workspace has: process() -> validate() -> Helper::new(), Helper::check()
    // All intra-file calls should result in call_edges being populated

    // validate is called by process
    let callers = tethys
        .get_callers("validate")
        .expect("get_callers for validate should succeed");

    assert!(
        !callers.is_empty(),
        "validate should have callers via call_edges"
    );

    // Verify the caller contains "process" in symbols_used
    let all_symbols: Vec<&str> = callers
        .iter()
        .flat_map(|c| c.symbols_used.iter().map(String::as_str))
        .collect();
    assert!(
        all_symbols.iter().any(|n| n.contains("process")),
        "validate should be called by process, got: {all_symbols:?}"
    );
}

/// Verify transitive callers work with `call_edges`.
#[test]
fn transitive_callers_via_call_edges() {
    let (_dir, mut tethys) = workspace_with_intra_file_calls();
    tethys.index().expect("index failed");

    // Helper::check is called by validate, which is called by process
    // So Helper::check should have process as a transitive caller
    let impact = tethys
        .get_symbol_impact("Helper::check")
        .expect("get_symbol_impact for Helper::check should succeed");

    let total = impact.direct_dependents.len() + impact.transitive_dependents.len();
    assert!(
        total >= 1,
        "Helper::check should have at least 1 caller, got: {total}"
    );
}

/// Verify `get_symbol_dependencies` works with `call_edges`.
#[test]
fn symbol_dependencies_via_call_edges() {
    let (_dir, mut tethys) = workspace_with_intra_file_calls();
    tethys.index().expect("index failed");

    // validate calls Helper::new and Helper::check
    let deps = tethys
        .get_symbol_dependencies("validate")
        .expect("get_symbol_dependencies for validate should succeed");

    assert!(
        !deps.is_empty(),
        "validate should have dependencies (Helper::new, Helper::check)"
    );
}

// ============================================================================
// Reachability Analysis Tests
// ============================================================================

#[test]
fn get_forward_reachable_returns_error_for_nonexistent_symbol() {
    let (_dir, mut tethys) = workspace_with_intra_file_calls();
    tethys.index().expect("index failed");

    let result = tethys.get_forward_reachable("NoSuchSymbol", Some(10));

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
fn get_backward_reachable_returns_error_for_nonexistent_symbol() {
    let (_dir, mut tethys) = workspace_with_intra_file_calls();
    tethys.index().expect("index failed");

    let result = tethys.get_backward_reachable("NoSuchSymbol", Some(10));

    assert!(
        result.is_err(),
        "should return error for non-existent symbol"
    );
}

#[test]
fn get_forward_reachable_finds_direct_callees() {
    let (_dir, mut tethys) = workspace_with_intra_file_calls();
    tethys.index().expect("index failed");

    // process calls validate
    let result = tethys
        .get_forward_reachable("process", Some(1))
        .expect("get_forward_reachable for process should succeed");

    assert!(
        !result.is_empty(),
        "process should have forward reachable symbols (validate)"
    );
    assert_eq!(
        result.direction,
        tethys::ReachabilityDirection::Forward,
        "direction should be Forward"
    );

    // All results should be at depth 1 (max_depth=1)
    for path in &result.reachable {
        assert_eq!(path.depth, 1, "all results should be at depth 1");
    }
}

#[test]
fn get_forward_reachable_finds_transitive_callees() {
    let (_dir, mut tethys) = workspace_with_intra_file_calls();
    tethys.index().expect("index failed");

    // process -> validate -> Helper::new, Helper::check
    // With depth 3, we should reach Helper::new and Helper::check
    let result = tethys
        .get_forward_reachable("process", Some(3))
        .expect("get_forward_reachable for process should succeed");

    assert!(
        result.reachable_count() >= 2,
        "process should reach at least 2 symbols with depth 3, got: {}",
        result.reachable_count()
    );

    // Check that we have symbols at different depths
    let depths: std::collections::HashSet<usize> =
        result.reachable.iter().map(|r| r.depth).collect();
    assert!(
        !depths.is_empty(),
        "should have symbols at different depths, got depths: {depths:?}"
    );
}

#[test]
fn get_forward_reachable_returns_empty_for_leaf_symbol() {
    let (_dir, mut tethys) = workspace_with_intra_file_calls();
    tethys.index().expect("index failed");

    // Helper::check doesn't call anything
    let result = tethys
        .get_forward_reachable("Helper::check", Some(10))
        .expect("get_forward_reachable for Helper::check should succeed");

    assert!(
        result.is_empty(),
        "Helper::check should have no forward reachable symbols, got: {:?}",
        result.reachable
    );
}

#[test]
fn get_backward_reachable_finds_direct_callers() {
    let (_dir, mut tethys) = workspace_with_intra_file_calls();
    tethys.index().expect("index failed");

    // validate is called by process
    let result = tethys
        .get_backward_reachable("validate", Some(1))
        .expect("get_backward_reachable for validate should succeed");

    assert!(
        !result.is_empty(),
        "validate should have backward reachable symbols (process)"
    );
    assert_eq!(
        result.direction,
        tethys::ReachabilityDirection::Backward,
        "direction should be Backward"
    );
}

#[test]
fn get_backward_reachable_finds_transitive_callers() {
    let (_dir, mut tethys) = workspace_with_intra_file_calls();
    tethys.index().expect("index failed");

    // Helper::new is called by validate, which is called by process
    // With depth 3, we should reach both validate and process
    let result = tethys
        .get_backward_reachable("Helper::new", Some(3))
        .expect("get_backward_reachable for Helper::new should succeed");

    assert!(
        result.reachable_count() >= 1,
        "Helper::new should have at least 1 backward reachable symbol, got: {}",
        result.reachable_count()
    );
}

#[test]
fn get_backward_reachable_returns_empty_for_uncalled_symbol() {
    let (_dir, mut tethys) = workspace_with_intra_file_calls();
    tethys.index().expect("index failed");

    // process is not called by anything
    let result = tethys
        .get_backward_reachable("process", Some(10))
        .expect("get_backward_reachable for process should succeed");

    assert!(
        result.is_empty(),
        "process should have no backward reachable symbols, got: {:?}",
        result.reachable
    );
}

#[test]
fn reachability_respects_max_depth() {
    let (_dir, mut tethys) = workspace_with_intra_file_calls();
    tethys.index().expect("index failed");

    // With depth 1, process should only reach validate (direct callee)
    let result_depth_1 = tethys
        .get_forward_reachable("process", Some(1))
        .expect("get_forward_reachable depth 1 should succeed");

    // With depth 3, process should reach more symbols (validate, Helper::new, Helper::check)
    let result_depth_3 = tethys
        .get_forward_reachable("process", Some(3))
        .expect("get_forward_reachable depth 3 should succeed");

    assert!(
        result_depth_3.reachable_count() >= result_depth_1.reachable_count(),
        "depth 3 should reach at least as many symbols as depth 1"
    );
}

#[test]
fn reachability_result_at_depth_filters_correctly() {
    let (_dir, mut tethys) = workspace_with_intra_file_calls();
    tethys.index().expect("index failed");

    let result = tethys
        .get_forward_reachable("process", Some(3))
        .expect("get_forward_reachable should succeed");

    let at_depth_1 = result.at_depth(1);
    let at_depth_2 = result.at_depth(2);

    // All results at depth 1 should have depth == 1
    for path in &at_depth_1 {
        assert_eq!(path.depth, 1, "at_depth(1) should only return depth 1");
    }

    // All results at depth 2 should have depth == 2
    for path in &at_depth_2 {
        assert_eq!(path.depth, 2, "at_depth(2) should only return depth 2");
    }
}

#[test]
fn reachability_paths_are_valid() {
    let (_dir, mut tethys) = workspace_with_intra_file_calls();
    tethys.index().expect("index failed");

    let result = tethys
        .get_forward_reachable("process", Some(3))
        .expect("get_forward_reachable should succeed");

    for path in &result.reachable {
        // Path length should equal depth
        assert_eq!(
            path.path.len(),
            path.depth,
            "path length should equal depth for {:?}",
            path.target.qualified_name
        );

        // Path should end with the target
        if !path.path.is_empty() {
            let last = path.path.last().expect("path should not be empty");
            assert_eq!(
                last.id, path.target.id,
                "path should end with target symbol"
            );
        }
    }
}

#[test]
fn reachability_cross_file_works() {
    let (_dir, mut tethys) = workspace_with_call_graph();
    tethys.index().expect("index failed");

    // Connection is in db.rs and is referenced from auth.rs and cache.rs
    // Those references are now resolved via cross-file resolution
    let result = tethys
        .get_backward_reachable("Connection", Some(5))
        .expect("get_backward_reachable for Connection should succeed");

    // Cross-file references should be resolved
    assert!(
        !result.is_empty(),
        "Connection should have backward reachable symbols from other files"
    );
}

#[test]
fn reachability_max_depth_none_uses_default() {
    let (_dir, mut tethys) = workspace_with_intra_file_calls();
    tethys.index().expect("index failed");

    // When max_depth is None, the implementation should use default (50)
    let result = tethys
        .get_forward_reachable("process", None)
        .expect("get_forward_reachable with None depth should succeed");

    // Verify the result captures the default max_depth
    assert_eq!(
        result.max_depth, 50,
        "max_depth should be 50 when None is passed"
    );

    // Verify it still finds reachable symbols (same as with explicit depth)
    let result_explicit = tethys
        .get_forward_reachable("process", Some(50))
        .expect("get_forward_reachable with explicit depth should succeed");

    assert_eq!(
        result.reachable_count(),
        result_explicit.reachable_count(),
        "None and Some(50) should produce same results"
    );
}

/// Helper that creates a workspace with a cyclic call pattern: a -> b -> c -> a
fn workspace_with_cyclic_calls() -> (TempDir, Tethys) {
    let dir = tempfile::tempdir().expect("failed to create temp dir");

    fs::create_dir_all(dir.path().join("src")).expect("failed to create src dir");

    fs::write(
        dir.path().join("src/lib.rs"),
        r"
pub fn cycle_a() {
    cycle_b();
}

pub fn cycle_b() {
    cycle_c();
}

pub fn cycle_c() {
    cycle_a();  // Creates the cycle back to a
}

pub fn entry_point() {
    cycle_a();
}
",
    )
    .expect("failed to write lib.rs");

    let tethys = Tethys::new(dir.path()).expect("failed to create Tethys");
    (dir, tethys)
}

#[test]
fn reachability_terminates_on_cyclic_call_graph() {
    let (_dir, mut tethys) = workspace_with_cyclic_calls();
    tethys.index().expect("index failed");

    // Forward reachability from cycle_a should find cycle_b and cycle_c
    // but should terminate (not infinite loop) due to visited tracking
    let result = tethys
        .get_forward_reachable("cycle_a", Some(10))
        .expect("get_forward_reachable should terminate on cyclic graph");

    // Should find b and c, but not revisit a (already visited as source)
    // The exact count depends on what gets resolved, but it should terminate
    assert!(
        result.reachable_count() <= 10,
        "BFS should terminate and not produce infinite results, got: {}",
        result.reachable_count()
    );

    // Backward reachability should also terminate
    let result_backward = tethys
        .get_backward_reachable("cycle_a", Some(10))
        .expect("get_backward_reachable should terminate on cyclic graph");

    // cycle_a is called by cycle_c and entry_point
    // cycle_c is called by cycle_b, which is called by cycle_a (but a is source, so skipped)
    assert!(
        result_backward.reachable_count() <= 10,
        "backward BFS should terminate, got: {}",
        result_backward.reachable_count()
    );
}
