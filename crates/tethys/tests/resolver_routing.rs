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
/// The fixture includes an unrelated `use crate::imports_module::imported_fn`
/// to give the imports table a non-trivial row for `crate_a/src/lib.rs` —
/// useful as a positive control alongside the fallback path. (Pre-rivets-dn35
/// this was load-bearing because `resolve_refs_for_file` short-circuited on
/// `imports.is_empty()`; the short-circuit is now gone but the import-having
/// fixture still tests the realistic shape.)
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

/// Companion to the test above, exercising the *fallthrough* branch of
/// `fallback_symbol_search`. When `shared_helper` exists only in `crate_b`
/// (no same-crate candidate), the resolver must drop through to the
/// unscoped `search_unique_symbol_by_name` and resolve the ref to
/// `crate_b`'s symbol. A regression that broke the fallthrough (e.g.,
/// returning `None` after the same-crate miss instead of continuing)
/// would orphan this reference (`refs.symbol_id IS NULL`) and fail the
/// assertion below.
///
/// The assertion targets `refs` (resolver output), not `file_deps`
/// (aggregation output), because under the rivets-3d0s K-hybrid filter the
/// cross-crate edge is intentionally dropped from `file_deps` when the
/// caller lacks an import into the target's crate — even though the ref
/// itself resolves. The resolver's job (resolve the symbol) and the
/// aggregator's job (produce trustworthy cross-crate file deps) are now
/// separately observable. This test pins the resolver leg; the
/// K-hybrid `file_deps` filter is fenced by
/// `tests/file_deps_corroboration.rs` (rivets-3d0s slice 2).
///
/// Pair with `fallback_routes_unqualified_ref_to_same_crate_not_cross_crate`:
/// that test pins the *priority* leg (same-crate wins); this test pins the
/// *fallthrough* leg (no same-crate -> use workspace-wide if unique).
#[test]
fn fallback_resolves_via_unscoped_when_no_same_crate_candidate() {
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
    x + 1
}
",
        ),
    ]);

    tethys.index().expect("index should succeed");

    let conn = open_db(&tethys);
    // Query refs joined with symbols + files: find a resolved reference
    // originating in crate_a/src/lib.rs whose target symbol's file is in
    // crate_b/src/lib.rs. This proves the resolver fell through to the
    // unscoped path and resolved the cross-crate target — independent of
    // whether the K-hybrid filter chose to aggregate it into file_deps.
    let cross_crate_resolved_refs: Vec<(String, String, String)> = conn
        .prepare(
            "SELECT s.name, f_caller.path, f_target.path
             FROM refs r
             JOIN symbols s     ON s.id = r.symbol_id
             JOIN files f_caller ON f_caller.id = r.file_id
             JOIN files f_target ON f_target.id = s.file_id
             WHERE r.symbol_id IS NOT NULL
               AND f_caller.path = 'crate_a/src/lib.rs'
               AND f_target.path = 'crate_b/src/lib.rs'",
        )
        .expect("prepare refs query")
        .query_map(params![], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .expect("query")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect");

    let resolved_to_shared_helper = cross_crate_resolved_refs
        .iter()
        .any(|(name, _, _)| name == "shared_helper");
    assert!(
        resolved_to_shared_helper,
        "expected the unqualified shared_helper(42) call in crate_a/src/lib.rs to fall \
         through to unscoped resolution and find crate_b's shared_helper; got resolved \
         cross-crate refs: {cross_crate_resolved_refs:?}"
    );
}

/// Fetch all `(from_path, to_path)` edges from the indexed workspace's
/// `file_deps` table. Used by the multi-crate-resolution tests to assert
/// per-file `crate_root` routing produces the expected sub-crate edges.
fn file_deps_edges(tethys: &tethys::Tethys) -> Vec<(String, String)> {
    open_db(tethys)
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
        .expect("collect")
}

/// Two-crate workspace where each crate has `use crate::module;` resolving
/// to its OWN crate's `src/module.rs`. The resolver must compute
/// `crate_root` per file: `crate_a/src/lib.rs`'s `use crate::widget` resolves
/// under `crate_a/src/`, and `crate_b/src/lib.rs`'s `use crate::gadget`
/// resolves under `crate_b/src/`. An impl that hardcodes `crate_root` to a
/// single workspace-wide directory (e.g., `workspace_root/src`) would fail
/// for both crates in a workspace whose root has no `src/`.
#[test]
fn pass2_imports_resolve_per_crate_in_multi_crate_workspace() {
    let (_dir, mut tethys) = workspace_with_files(&[
        (
            "Cargo.toml",
            "[workspace]\nmembers = [\"crate_a\", \"crate_b\"]\nresolver = \"2\"\n",
        ),
        (
            "crate_a/Cargo.toml",
            "[package]\nname = \"crate_a\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        ),
        (
            "crate_a/src/lib.rs",
            "mod widget;\nuse crate::widget::Widget;\npub fn make() -> Widget { Widget::new() }\n",
        ),
        (
            "crate_a/src/widget.rs",
            "pub struct Widget;\nimpl Widget { pub fn new() -> Self { Self } }\n",
        ),
        (
            "crate_b/Cargo.toml",
            "[package]\nname = \"crate_b\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        ),
        (
            "crate_b/src/lib.rs",
            "mod gadget;\nuse crate::gadget::Gadget;\npub fn build() -> Gadget { Gadget::new() }\n",
        ),
        (
            "crate_b/src/gadget.rs",
            "pub struct Gadget;\nimpl Gadget { pub fn new() -> Self { Self } }\n",
        ),
    ]);

    tethys.index().expect("index should succeed");
    let edges = file_deps_edges(&tethys);

    let crate_a_to_widget = edges
        .iter()
        .any(|(from, to)| from == "crate_a/src/lib.rs" && to == "crate_a/src/widget.rs");
    let crate_b_to_gadget = edges
        .iter()
        .any(|(from, to)| from == "crate_b/src/lib.rs" && to == "crate_b/src/gadget.rs");

    assert!(
        crate_a_to_widget,
        "expected crate_a/src/lib.rs -> crate_a/src/widget.rs from `use crate::widget`; \
         got: {edges:?}"
    );
    assert!(
        crate_b_to_gadget,
        "expected crate_b/src/lib.rs -> crate_b/src/gadget.rs from `use crate::gadget`; \
         got: {edges:?}"
    );

    // Negative leg: no cross-crate phantom edges from these `crate::` imports.
    let phantom: Vec<&(String, String)> = edges
        .iter()
        .filter(|(from, to)| {
            (from.starts_with("crate_a/") && to.starts_with("crate_b/"))
                || (from.starts_with("crate_b/") && to.starts_with("crate_a/"))
        })
        .collect();
    assert!(
        phantom.is_empty(),
        "no cross-crate edges expected from intra-crate `crate::` imports; got: {phantom:?}"
    );
}

/// Regression gate: a 3-crate workspace where each crate has multiple
/// intra-crate `use crate::*` imports. After indexing, the resolved-ref count
/// must meet a minimum floor — proves that per-file `crate_root` lookup
/// (rivets-6aoc) is wired through Pass-2-imports + dep-graph computation.
///
/// A regression that re-introduces a hardcoded `workspace_root.join("src")`
/// `crate_root` would fail this test because the fixture's workspace root has
/// no `src/` directory, so the hardcoded path can't resolve any of the
/// intra-crate imports.
///
/// Runs the same fixture through both default and streaming indexing modes
/// (the streaming path goes through `compute_dependencies_from_stored`).
#[test]
fn multi_crate_intra_crate_imports_meet_resolved_ref_floor() {
    fn run_with_options(options: tethys::IndexOptions, mode_label: &str) {
        let (_dir, mut tethys) = workspace_with_files(&[
            (
                "Cargo.toml",
                "[workspace]\nmembers = [\"crate_a\", \"crate_b\", \"crate_c\"]\nresolver = \"2\"\n",
            ),
            (
                "crate_a/Cargo.toml",
                "[package]\nname = \"crate_a\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
            ),
            (
                "crate_a/src/lib.rs",
                "mod widget;\nmod alpha;\nmod beta;\n\
                 use crate::widget::Widget;\n\
                 use crate::alpha::Alpha;\n\
                 use crate::beta::Beta;\n\
                 pub fn make_a() -> (Widget, Alpha, Beta) { (Widget, Alpha, Beta) }\n",
            ),
            ("crate_a/src/widget.rs", "pub struct Widget;\n"),
            ("crate_a/src/alpha.rs", "pub struct Alpha;\n"),
            ("crate_a/src/beta.rs", "pub struct Beta;\n"),
            (
                "crate_b/Cargo.toml",
                "[package]\nname = \"crate_b\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
            ),
            (
                "crate_b/src/lib.rs",
                "mod gadget;\nmod gamma;\n\
                 use crate::gadget::Gadget;\n\
                 use crate::gamma::Gamma;\n\
                 pub fn make_b() -> (Gadget, Gamma) { (Gadget, Gamma) }\n",
            ),
            ("crate_b/src/gadget.rs", "pub struct Gadget;\n"),
            ("crate_b/src/gamma.rs", "pub struct Gamma;\n"),
            (
                "crate_c/Cargo.toml",
                "[package]\nname = \"crate_c\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
            ),
            (
                "crate_c/src/lib.rs",
                "mod gizmo;\nmod delta;\n\
                 use crate::gizmo::Gizmo;\n\
                 use crate::delta::Delta;\n\
                 pub fn make_c() -> (Gizmo, Delta) { (Gizmo, Delta) }\n",
            ),
            ("crate_c/src/gizmo.rs", "pub struct Gizmo;\n"),
            ("crate_c/src/delta.rs", "pub struct Delta;\n"),
        ]);

        tethys
            .index_with_options(options)
            .unwrap_or_else(|e| panic!("[{mode_label}] index should succeed: {e}"));

        let conn = open_db(&tethys);

        // Floor 1: resolved cross-file references. The 7 use-statements
        // (3 in crate_a + 2 in crate_b + 2 in crate_c) each generate at
        // least one resolved reference to the imported struct. With the
        // per-file `crate_root` fix, all 7 resolve via Pass-2-imports. A
        // regression that re-hardcodes `workspace_root.join("src")` would
        // find none of them (workspace root has no `src/`).
        let resolved_cross_file_refs: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM refs r
                 JOIN symbols s ON s.id = r.symbol_id
                 WHERE r.symbol_id IS NOT NULL AND s.file_id != r.file_id",
                params![],
                |row| row.get(0),
            )
            .expect("count resolved cross-file refs");
        assert!(
            resolved_cross_file_refs >= 7,
            "[{mode_label}] expected ≥7 resolved cross-file refs (one per `use crate::X` import), \
             got {resolved_cross_file_refs}. Likely a regression re-introducing a hardcoded \
             `workspace_root.join(\"src\")` crate_root."
        );

        // Floor 2: intra-crate dep-graph edges. Each `use crate::X` should
        // produce a `file_deps` edge from the importer's lib.rs to its
        // crate's X.rs. 7 imports → 7 edges.
        let intra_crate_edges: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM file_deps d
                 JOIN files f1 ON f1.id = d.from_file_id
                 JOIN files f2 ON f2.id = d.to_file_id
                 WHERE substr(f1.path, 1, instr(f1.path, '/')) =
                       substr(f2.path, 1, instr(f2.path, '/'))",
                params![],
                |row| row.get(0),
            )
            .expect("count intra-crate dep edges");
        assert!(
            intra_crate_edges >= 7,
            "[{mode_label}] expected ≥7 intra-crate file_deps edges (one per `use crate::X`), \
             got {intra_crate_edges}. Likely a regression in `compute_dependencies` or \
             `compute_dependencies_from_stored`."
        );
    }

    run_with_options(tethys::IndexOptions::default(), "default");
    run_with_options(tethys::IndexOptions::with_streaming(), "streaming");
}
