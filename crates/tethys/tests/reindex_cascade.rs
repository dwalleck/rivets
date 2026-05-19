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
