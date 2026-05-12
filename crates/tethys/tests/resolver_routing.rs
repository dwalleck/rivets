//! Integration tests for resolver routing in `fallback_symbol_search`.
//!
//! These tests are the CI regression gate for the rivets-0gom bug class
//! (workspace-wide simple-name resolution producing phantom cross-crate
//! `file_deps` edges). The `.rivets-0gom/probe.py` oracle is a manual
//! verification tool; this file is the automated counterpart.

use rusqlite::params;

mod common;

use common::{open_db, workspace_with_files};

/// Two crates each define a function named `shared_helper`. `crate_a/src/lib.rs`
/// calls `shared_helper(...)` without an import or path qualifier — tree-sitter
/// extracts it as a fallback-eligible reference. The resolver must route it to
/// `crate_a/src/target_module.rs::shared_helper`, not
/// `crate_b/src/lib.rs::shared_helper`.
///
/// Asserts both legs of the rivets-0gom bug class:
/// 1. **Negative:** no `crate_a/src/lib.rs -> crate_b/*` `file_deps` edge for
///    this call. A regression that drops the ambiguity refusal in
///    `search_unique_symbol_by_name` (re-enabling arbitrary workspace-wide
///    picks) fails this assertion.
/// 2. **Positive:** there *is* a `crate_a/src/lib.rs -> crate_a/src/target_module.rs`
///    edge. A regression that drops the same-crate scoping branch in
///    `fallback_symbol_search` fails this assertion. The separation between
///    `imports_module.rs` (import-derived edge target) and
///    `target_module.rs` (fallback-derived edge target) is what makes this
///    leg load-bearing: an import-only resolver would produce the former but
///    not the latter.
///
/// The Pass-2 resolver in `resolve_refs_for_file` short-circuits when a file
/// has no imports, so the fixture deliberately includes an unrelated
/// `use crate::imports_module::imported_fn` to ensure fallback runs.
#[test]
fn fallback_routes_unqualified_ref_to_same_crate_not_cross_crate() {
    let (_dir, mut tethys) = workspace_with_files(&[
        (
            "Cargo.toml",
            r#"
[workspace]
members = ["crate_a", "crate_b"]
resolver = "2"
"#,
        ),
        (
            "crate_a/Cargo.toml",
            r#"
[package]
name = "crate_a"
version = "0.1.0"
edition = "2021"
"#,
        ),
        (
            "crate_a/src/lib.rs",
            r"
mod imports_module;
mod target_module;

use crate::imports_module::imported_fn;

pub fn entry() -> u32 {
    imported_fn();
    shared_helper(42)
}
",
        ),
        (
            "crate_a/src/imports_module.rs",
            r"
pub fn imported_fn() {}
",
        ),
        (
            "crate_a/src/target_module.rs",
            r"
pub fn shared_helper(x: u32) -> u32 {
    x + 1
}
",
        ),
        (
            "crate_b/Cargo.toml",
            r#"
[package]
name = "crate_b"
version = "0.1.0"
edition = "2021"
"#,
        ),
        (
            "crate_b/src/lib.rs",
            r"
pub fn shared_helper(x: u32) -> u32 {
    x * 2
}
",
        ),
    ]);

    tethys.index().expect("index should succeed");

    let conn = open_db(&tethys);
    let edges: Vec<(String, String)> = conn
        .prepare(
            "SELECT f1.path, f2.path
             FROM file_deps d
             JOIN files f1 ON f1.id = d.from_file_id
             JOIN files f2 ON f2.id = d.to_file_id",
        )
        .expect("prepare file_deps query")
        .query_map(params![], |row| Ok((row.get(0)?, row.get(1)?)))
        .expect("query")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect");

    // Negative leg: no phantom cross-crate edge from crate_a -> crate_b.
    // This is the rivets-0gom bug class: workspace-wide simple-name resolution
    // could route `shared_helper` to crate_b's copy.
    let phantom: Vec<&(String, String)> = edges
        .iter()
        .filter(|(from, to)| from.starts_with("crate_a/") && to.starts_with("crate_b/"))
        .collect();
    assert!(
        phantom.is_empty(),
        "phantom cross-crate edge(s) found - same-crate scoping should prevent this: \
         {phantom:?}\nAll edges: {edges:?}"
    );

    // Positive leg: same-crate edge from crate_a/src/lib.rs -> crate_a/src/target_module.rs
    // must exist. The import only produces the lib.rs -> imports_module.rs edge,
    // so the target_module.rs edge can only come from fallback resolution of
    // the unqualified shared_helper(42) call. A regression that drops the
    // same-crate branch in fallback_symbol_search fails this assertion.
    let same_crate_edge_present = edges
        .iter()
        .any(|(from, to)| from == "crate_a/src/lib.rs" && to == "crate_a/src/target_module.rs");
    assert!(
        same_crate_edge_present,
        "expected same-crate edge crate_a/src/lib.rs -> crate_a/src/target_module.rs from the \
         unqualified shared_helper(...) call; got: {edges:?}"
    );
}
