//! Regression fences for rivets-wsix: cascade-correctness across re-index runs.
//!
//! The wsix audit (`.rivets-wsix/what-i-learned.md`) found that re-index
//! correctness for `refs`, `attributes`, and `symbols` relies on the schema's
//! `ON DELETE CASCADE` chain rooted at `symbols(id)`, not the `clear_all_X`
//! pattern lcb6 established for `file_deps` and `call_edges`. These tests
//! lock that cascade behavior in so a future schema change (e.g., a cascade
//! FK silently relaxed to `SET NULL`) is caught in CI.

use rusqlite::Connection;

mod common;
use common::{open_db, workspace_with_files};

/// Count refs in `src/lib.rs` whose resolved symbol name matches `s.name`
/// against the provided list of names. Returns total across all names.
fn count_lib_refs_by_target_names(conn: &Connection, names: &[&str]) -> i64 {
    let placeholders = names.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT COUNT(*) FROM refs r
         JOIN files f ON f.id = r.file_id
         JOIN symbols s ON s.id = r.symbol_id
         WHERE f.path = 'src/lib.rs' AND s.name IN ({placeholders})"
    );
    let params_vec: Vec<&dyn rusqlite::ToSql> =
        names.iter().map(|n| n as &dyn rusqlite::ToSql).collect();
    conn.query_row(&sql, params_vec.as_slice(), |row| row.get(0))
        .expect("count refs by target names")
}

/// Pin claim C1: removing a function-body call from a file's source produces
/// exactly the expected row removals in `refs` after re-index, via the
/// `refs.in_symbol_id REFERENCES symbols(id) ON DELETE CASCADE` chain
/// triggered by the per-file `DELETE FROM symbols WHERE file_id = ?1` in
/// `files.rs::upsert_file`.
///
/// Stress shape (middle-removal): starting `entry()` calls `helper::a()`,
/// `helper::b()`, and `helper::c()`. After mutation we remove the MIDDLE
/// call. This defeats a hypothetical head-only or tail-only cascade bug:
/// the assertion is not just "count decreased" but "the specific b-ref is
/// gone and a/c-refs remain."
#[test]
fn refs_cascade_on_call_removal() {
    let (dir, mut tethys) = workspace_with_files(&[
        (
            "Cargo.toml",
            r#"
[package]
name = "wsix_refs"
version = "0.0.0"
edition = "2021"
"#,
        ),
        (
            "src/lib.rs",
            r"
mod helper;

pub fn entry() {
    helper::a();
    helper::b();
    helper::c();
}
",
        ),
        (
            "src/helper.rs",
            r"
pub fn a() {}
pub fn b() {}
pub fn c() {}
",
        ),
    ]);

    tethys.index().expect("initial index should succeed");

    let refs_pre = count_lib_refs_by_target_names(&open_db(&tethys), &["a", "b", "c"]);
    assert_eq!(
        refs_pre, 3,
        "fixture should produce 3 cross-file call refs (a, b, c)"
    );

    // Mutate source: remove the MIDDLE call. New content changes the file's
    // content_hash, which forces tethys to re-process this file.
    std::fs::write(
        dir.path().join("src/lib.rs"),
        r"
mod helper;

pub fn entry() {
    helper::a();
    helper::c();
}
",
    )
    .expect("rewrite src/lib.rs");

    tethys.index().expect("re-index should succeed");

    let conn = open_db(&tethys);
    let refs_post = count_lib_refs_by_target_names(&conn, &["a", "b", "c"]);
    let b_refs = count_lib_refs_by_target_names(&conn, &["b"]);
    let a_refs = count_lib_refs_by_target_names(&conn, &["a"]);
    let c_refs = count_lib_refs_by_target_names(&conn, &["c"]);

    assert_eq!(
        refs_post, 2,
        "expected 2 refs (a, c) after removing helper::b() — got {refs_post}"
    );
    assert_eq!(
        b_refs, 0,
        "ref to helper::b() must be cascade-deleted after source removal — got {b_refs}"
    );
    assert_eq!(
        a_refs, 1,
        "ref to helper::a() must survive cascade (it's still in source) — got {a_refs}"
    );
    // Explicit symmetric coverage of the third surviving ref. `c_refs` is
    // derivable from the prior three assertions today, but spelling it out
    // defends against future mutations to `count_lib_refs_by_target_names`'s
    // IN-clause that would silently break that arithmetic.
    assert_eq!(
        c_refs, 1,
        "ref to helper::c() must survive cascade (it's still in source) — got {c_refs}"
    );
}

/// Count attribute rows whose owning symbol has the given name.
fn count_attrs_for_symbol(conn: &Connection, symbol_name: &str) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM attributes a
         JOIN symbols s ON s.id = a.symbol_id
         WHERE s.name = ?1",
        [symbol_name],
        |row| row.get(0),
    )
    .expect("count attrs by symbol name")
}

/// Count symbol rows by name.
fn count_symbols_by_name(conn: &Connection, symbol_name: &str) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM symbols WHERE name = ?1",
        [symbol_name],
        |row| row.get(0),
    )
    .expect("count symbols by name")
}

/// Pin claim C2: removing an attributed symbol from source cascade-deletes
/// the symbol AND its `attributes` rows via
/// `attributes.symbol_id REFERENCES symbols(id) ON DELETE CASCADE`.
///
/// Stress shape (two attributed symbols, remove one): defeats a "cascade
/// too aggressive" bug class — if the cascade clobbered all of the file's
/// attributes instead of just the removed symbol's, the `keep` assertions
/// would catch it.
#[test]
fn attributes_cascade_on_symbol_removal() {
    let (dir, mut tethys) = workspace_with_files(&[
        (
            "Cargo.toml",
            r#"
[package]
name = "wsix_attrs"
version = "0.0.0"
edition = "2021"
"#,
        ),
        (
            "src/lib.rs",
            r"
#[allow(dead_code)]
pub fn target() {}

#[allow(dead_code)]
pub fn keep() {}
",
        ),
    ]);

    tethys.index().expect("initial index should succeed");

    let conn = open_db(&tethys);
    let target_attrs_pre = count_attrs_for_symbol(&conn, "target");
    let keep_attrs_pre = count_attrs_for_symbol(&conn, "keep");
    assert!(
        target_attrs_pre >= 1,
        "fixture should index target's #[allow] attribute — got {target_attrs_pre}"
    );
    assert!(
        keep_attrs_pre >= 1,
        "fixture should index keep's #[allow] attribute — got {keep_attrs_pre}"
    );
    drop(conn);

    // Remove the `target` fn from source.
    std::fs::write(
        dir.path().join("src/lib.rs"),
        r"
#[allow(dead_code)]
pub fn keep() {}
",
    )
    .expect("rewrite src/lib.rs");

    tethys.index().expect("re-index should succeed");

    let conn = open_db(&tethys);
    let target_sym_post = count_symbols_by_name(&conn, "target");
    let target_attrs_post = count_attrs_for_symbol(&conn, "target");
    let keep_attrs_post = count_attrs_for_symbol(&conn, "keep");

    assert_eq!(
        target_sym_post, 0,
        "target symbol must be gone after source removal — got {target_sym_post}"
    );
    assert_eq!(
        target_attrs_post, 0,
        "target's attributes must cascade-delete with the symbol — got {target_attrs_post}"
    );
    assert_eq!(
        keep_attrs_post, keep_attrs_pre,
        "keep's attributes MUST NOT cascade-delete (cascade was too aggressive) — pre={keep_attrs_pre} post={keep_attrs_post}"
    );
}

/// Snapshot of the row counts that should be invariant under unchanged-source
/// re-index when the `clear_all_X` discipline is in place.
struct ClearAllSnapshot {
    call_edges: i64,
    file_deps: i64,
    file_deps_ref_count_sum: i64,
}

fn snapshot_clear_all_tables(conn: &Connection) -> ClearAllSnapshot {
    let call_edges = conn
        .query_row("SELECT COUNT(*) FROM call_edges", [], |row| row.get(0))
        .expect("count call_edges");
    let file_deps = conn
        .query_row("SELECT COUNT(*) FROM file_deps", [], |row| row.get(0))
        .expect("count file_deps");
    let file_deps_ref_count_sum = conn
        .query_row(
            "SELECT COALESCE(SUM(ref_count), 0) FROM file_deps",
            [],
            |row| row.get(0),
        )
        .expect("sum file_deps.ref_count");
    ClearAllSnapshot {
        call_edges,
        file_deps,
        file_deps_ref_count_sum,
    }
}

/// Pin claim C3: re-indexing an unchanged workspace produces stable counts in
/// `call_edges` and `file_deps`, and a stable `SUM(file_deps.ref_count)`.
/// Catches regression of the `clear_all_X` discipline (rivets-lcb6's fix
/// for `file_deps`, plus the parallel `call_edges` path).
///
/// The `SUM(ref_count)` check defeats a specific UPSERT-aggregate-growth
/// bug class: if `clear_all_file_deps` were removed, `file_deps`'s row
/// count would not grow (the same dep is detected each run), but the
/// `ref_count` aggregate would increment via the `ON CONFLICT DO UPDATE
/// SET ref_count = ref_count + 1` clause and silently double on each
/// re-index. The row-count assertion alone would miss that.
#[test]
fn clear_all_tables_stable_under_reindex() {
    let (_dir, mut tethys) = workspace_with_files(&[
        (
            "Cargo.toml",
            r#"
[package]
name = "wsix_clear_all"
version = "0.0.0"
edition = "2021"
"#,
        ),
        (
            "src/lib.rs",
            r"
mod helper;

pub fn entry() {
    helper::do_thing();
}
",
        ),
        (
            "src/helper.rs",
            r"
pub fn do_thing() {}
",
        ),
    ]);

    tethys.index().expect("first index should succeed");

    let snap1 = snapshot_clear_all_tables(&open_db(&tethys));
    assert!(
        snap1.call_edges >= 1,
        "fixture should produce at least one call_edge — got {}",
        snap1.call_edges
    );
    assert!(
        snap1.file_deps >= 1,
        "fixture should produce at least one file_dep — got {}",
        snap1.file_deps
    );

    // Re-index with no source change.
    tethys.index().expect("second index should succeed");

    let snap2 = snapshot_clear_all_tables(&open_db(&tethys));

    assert_eq!(
        snap1.call_edges, snap2.call_edges,
        "call_edges count must not grow across unchanged-source re-index"
    );
    assert_eq!(
        snap1.file_deps, snap2.file_deps,
        "file_deps count must not grow across unchanged-source re-index"
    );
    assert_eq!(
        snap1.file_deps_ref_count_sum, snap2.file_deps_ref_count_sum,
        "SUM(file_deps.ref_count) must not grow (UPSERT-aggregate fence: lcb6)"
    );
}
