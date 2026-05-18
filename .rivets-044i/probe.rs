//! Probe for rivets-044i (prove-it-prototype).
//!
//! Question: For an import-less `src/lib.rs` containing
//! `mod helper; pub fn entry() { helper::do_thing_q(); }`, with `do_thing_q`
//! defined in `src/helper.rs`, does Tethys resolve the `helper::do_thing_q`
//! reference today (pre-fix)?
//!
//! Oracle: source inspection. There is exactly one definition of
//! `do_thing_q` in the workspace, exactly one call site, both files are
//! indexed, and they live in the same crate. Therefore the ref MUST resolve
//! if the resolver is correct.
//!
//! Expected probe result (pre-fix): ref stays unresolved
//! (`refs.symbol_id IS NULL` for the row at `src/lib.rs:4`).
//!
//! This test is meant to FAIL pre-fix; after the rivets-044i fix lands it
//! will pass and serve as the regression fence.

use rusqlite::params;

mod common;

use common::{open_db, workspace_with_files};

#[test]
fn probe_044i_qualified_ref_from_import_less_file() {
    let (_dir, mut tethys) = workspace_with_files(&[
        (
            "Cargo.toml",
            r#"
[package]
name = "crate_a"
version = "0.1.0"
edition = "2021"
"#,
        ),
        (
            "src/lib.rs",
            r"
mod helper;

pub fn entry() {
    helper::do_thing_q();
}
",
        ),
        (
            "src/helper.rs",
            r"
pub fn do_thing_q() {}
",
        ),
    ]);

    tethys.index().expect("index should succeed");

    let conn = open_db(&tethys);

    let total_refs: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM refs r JOIN files f ON f.id = r.file_id
             WHERE f.path = 'src/lib.rs'",
            params![],
            |row| row.get(0),
        )
        .expect("count refs");

    let resolved_refs: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM refs r JOIN files f ON f.id = r.file_id
             WHERE f.path = 'src/lib.rs' AND r.symbol_id IS NOT NULL",
            params![],
            |row| row.get(0),
        )
        .expect("count resolved refs");

    let resolved_to_target: i64 = conn
        .query_row(
            "SELECT COUNT(*)
             FROM refs r
             JOIN files f ON f.id = r.file_id
             JOIN symbols s ON s.id = r.symbol_id
             WHERE f.path = 'src/lib.rs' AND s.name = 'do_thing_q'",
            params![],
            |row| row.get(0),
        )
        .expect("count refs resolved to do_thing_q");

    let definition_exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM symbols s JOIN files f ON f.id = s.file_id
             WHERE s.name = 'do_thing_q' AND f.path = 'src/helper.rs'",
            params![],
            |row| row.get(0),
        )
        .expect("count definitions");

    eprintln!(
        "PROBE 044i state: total_refs={total_refs}, resolved_refs={resolved_refs}, \
         resolved_to_target={resolved_to_target}, definition_exists={definition_exists}"
    );

    // Sanity precondition from oracle: the definition is indexed exactly once.
    assert_eq!(
        definition_exists, 1,
        "oracle precondition: do_thing_q must be indexed in helper.rs"
    );

    // The bug claim: the qualified ref does NOT resolve to the target today.
    // Pre-fix this will hold (probe agrees with bug); post-fix this flips
    // (probe demonstrates fix is effective).
    assert!(
        resolved_to_target >= 1,
        "POST-FIX expectation: qualified call `helper::do_thing_q()` from \
         import-less src/lib.rs must resolve to its definition in src/helper.rs. \
         Pre-fix this assert FAILS, demonstrating rivets-044i."
    );
}

/// Probe shape #2: workspace-crate-prefix call from an import-less integration test.
///
/// Layout: workspace with two members. `crate_a/src/lib.rs` defines
/// `pub struct Widget; impl Widget { pub fn new() -> Self { Widget } }`.
/// `crate_b/tests/it.rs` is import-less and calls `crate_a::Widget::new()`.
///
/// Oracle: there is exactly one `Widget::new` symbol in the workspace.
/// The ref's first segment is the workspace-crate name `crate_a`, which
/// `resolver::resolve_module_path` already handles (resolver.rs:45-63).
/// Therefore a correct resolver should link the ref to `Widget::new` in
/// `crate_a/src/lib.rs`.
#[test]
fn probe_044i_workspace_crate_prefix_from_import_less_file() {
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
pub struct Widget;

impl Widget {
    pub fn make_widget_044i() -> Self {
        Widget
    }
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

[dependencies]
crate_a = { path = "../crate_a" }
"#,
        ),
        (
            "crate_b/tests/it.rs",
            r"
#[test]
fn smoke() {
    let _ = crate_a::Widget::make_widget_044i();
}
",
        ),
    ]);

    tethys.index().expect("index should succeed");

    let conn = open_db(&tethys);

    let resolved_to_target: i64 = conn
        .query_row(
            "SELECT COUNT(*)
             FROM refs r
             JOIN files f ON f.id = r.file_id
             JOIN symbols s ON s.id = r.symbol_id
             WHERE f.path = 'crate_b/tests/it.rs' AND s.name = 'make_widget_044i'",
            params![],
            |row| row.get(0),
        )
        .expect("count refs resolved to make_widget_044i");

    let definition_exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM symbols s JOIN files f ON f.id = s.file_id
             WHERE s.name = 'make_widget_044i' AND f.path = 'crate_a/src/lib.rs'",
            params![],
            |row| row.get(0),
        )
        .expect("count definitions");

    let unresolved_refs_in_test: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM refs r JOIN files f ON f.id = r.file_id
             WHERE f.path = 'crate_b/tests/it.rs' AND r.symbol_id IS NULL
             AND r.reference_name LIKE '%make_widget_044i%'",
            params![],
            |row| row.get(0),
        )
        .expect("count unresolved refs");

    eprintln!(
        "PROBE 044i shape #2 state: resolved_to_target={resolved_to_target}, \
         definition_exists={definition_exists}, unresolved_in_test={unresolved_refs_in_test}"
    );

    assert_eq!(
        definition_exists, 1,
        "oracle precondition: make_widget_044i must be indexed in crate_a/src/lib.rs"
    );

    assert!(
        resolved_to_target >= 1,
        "POST-FIX expectation: workspace-crate-prefix call \
         `crate_a::Widget::make_widget_044i()` from import-less crate_b/tests/it.rs \
         must resolve to its definition in crate_a/src/lib.rs."
    );
}
