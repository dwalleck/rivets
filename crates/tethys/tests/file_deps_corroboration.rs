//! Integration regression fence for the rivets-3d0s K-hybrid filter
//! (`crates/tethys/src/db/call_edges.rs::populate_file_deps_from_call_edges`).
//!
//! A cross-crate method call whose name collides with a workspace symbol
//! in a NON-imported crate must not produce a cross-crate `file_deps`
//! edge. The filter at `populate_file_deps_from_call_edges` ensures this
//! by requiring import corroboration for every cross-crate aggregation.
//!
//! This file pairs with the unit tests in `db::call_edges::k_hybrid_filter_tests`:
//! those exercise the filter against an in-memory DB with hand-inserted
//! rows; this one exercises the full `tethys index` pipeline against a
//! synthetic Cargo workspace that exhibits the exact rivets-3d0s phantom
//! shape (workspace-wide method-name collision with no corroborating
//! import in the caller).

use rusqlite::params;
use tempfile::TempDir;
use tethys::Tethys;

mod common;

use common::{open_db, workspace_with_files};

/// Three-crate Cargo workspace fixture for the rivets-3d0s K-hybrid filter.
///
/// `crate_caller` depends on `crate_target` only; `crate_caller`'s
/// `caller_fn` uses `Helper` (legitimate cross-crate use) AND calls
/// `some_input.len()` (a stdlib slice method that, pre-K-hybrid, resolves
/// to `crate_collider::Phantom::len` because it's the unique workspace
/// `len` method — the rivets-3d0s phantom shape).
fn build_collider_workspace() -> (TempDir, Tethys) {
    workspace_with_files(&[
        (
            "Cargo.toml",
            r#"
[workspace]
members = ["crate_caller", "crate_target", "crate_collider"]
resolver = "2"
"#,
        ),
        (
            "crate_caller/Cargo.toml",
            r#"
[package]
name = "crate_caller"
version = "0.1.0"
edition = "2021"

[dependencies]
crate_target = { path = "../crate_target" }
"#,
        ),
        (
            "crate_caller/src/lib.rs",
            r"
use crate_target::Helper;

pub fn caller_fn(some_input: &[i32]) -> usize {
    let h = Helper::new();
    h.do_work();
    some_input.len()
}
",
        ),
        (
            "crate_target/Cargo.toml",
            r#"
[package]
name = "crate_target"
version = "0.1.0"
edition = "2021"
"#,
        ),
        (
            "crate_target/src/lib.rs",
            r"
pub struct Helper;

impl Helper {
    pub fn new() -> Self {
        Helper
    }
    pub fn do_work(&self) {}
}
",
        ),
        (
            "crate_collider/Cargo.toml",
            r#"
[package]
name = "crate_collider"
version = "0.1.0"
edition = "2021"
"#,
        ),
        (
            "crate_collider/src/lib.rs",
            r"
pub struct Phantom;

impl Phantom {
    pub fn len(&self) -> usize {
        0
    }
}
",
        ),
    ])
}

/// Three-crate Cargo workspace exhibiting the rivets-3d0s phantom shape:
///
/// - `crate_caller` depends on `crate_target` (legitimate cross-crate use).
/// - `crate_caller` does NOT depend on `crate_collider` (no Cargo dep, no
///   `use` statement).
/// - `crate_collider` defines a struct with a `len()` method.
/// - `crate_caller` calls `some_input.len()` — a stdlib slice method.
///   Tethys's unscoped resolver collapses this to `crate_collider::Phantom::len`
///   because it's the unique workspace `len` method. Pre-K-hybrid, this
///   produced a `crate_caller -> crate_collider` `file_deps` edge — a phantom.
///
/// **Post-K-hybrid:** the phantom edge is suppressed because `crate_caller`
/// has no import into `crate_collider`. The legitimate `crate_caller ->
/// crate_target` edge survives because the source has `use crate_target::Helper`.
///
/// Falsifiability check (manual): reverting the K-hybrid filter (replacing
/// `populate_file_deps_from_call_edges`'s filtered logic with the original
/// unfiltered SQL aggregation) causes this test to FAIL with a non-zero
/// phantom edge count. The K-hybrid filter is what makes it pass.
#[test]
fn k_hybrid_drops_cross_crate_call_without_import_corroboration() {
    let (_dir, mut tethys) = build_collider_workspace();
    tethys.index().expect("index should succeed");

    let conn = open_db(&tethys);

    // C7: no cross-crate file_deps edge from crate_caller into crate_collider
    // (the source file lacks `use crate_collider::*` — phantom-eligible call
    // edges must not aggregate into file_deps).
    let phantom_edges: Vec<(String, String)> = conn
        .prepare(
            "SELECT f1.path, f2.path
             FROM file_deps d
             JOIN files f1 ON f1.id = d.from_file_id
             JOIN files f2 ON f2.id = d.to_file_id
             WHERE f1.path LIKE 'crate_caller/%' AND f2.path LIKE 'crate_collider/%'",
        )
        .expect("prepare phantom query")
        .query_map(params![], |row| Ok((row.get(0)?, row.get(1)?)))
        .expect("query phantom")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect phantom");
    assert!(
        phantom_edges.is_empty(),
        "K-hybrid filter must drop cross-crate file_deps edges where source lacks import \
         to target's crate; got phantom edges: {phantom_edges:?}"
    );

    // C8: legitimate cross-crate edge from crate_caller into crate_target
    // IS preserved (the source has `use crate_target::Helper` — imports-derived
    // file_deps captures the dependency, and the call-derived edge passes
    // corroboration).
    let legitimate_edges: Vec<(String, String)> = conn
        .prepare(
            "SELECT f1.path, f2.path
             FROM file_deps d
             JOIN files f1 ON f1.id = d.from_file_id
             JOIN files f2 ON f2.id = d.to_file_id
             WHERE f1.path LIKE 'crate_caller/%' AND f2.path LIKE 'crate_target/%'",
        )
        .expect("prepare legit query")
        .query_map(params![], |row| Ok((row.get(0)?, row.get(1)?)))
        .expect("query legit")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect legit");
    assert!(
        !legitimate_edges.is_empty(),
        "K-hybrid filter must preserve cross-crate edges with corroborating import; \
         expected at least one crate_caller/* -> crate_target/* edge, got: {legitimate_edges:?}"
    );
}
