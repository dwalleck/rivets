//! Regression fence for rivets-044i: qualified refs from import-less files.
//!
//! Stored `symbols.qualified_name` is module-stripped (free fns: `name`;
//! methods: `parent_name::name` — see `indexing.rs:627-630`), so the literal
//! `get_symbol_by_qualified_name` lookup in Pass 2's `fallback_symbol_search`
//! cannot match a ref like `helper::do_thing_q` whose text carries a module
//! prefix. The fix in `resolve.rs::qualified_module_fallback` interprets the
//! prefix as a module path via `resolver::resolve_module_path`, then looks up
//! the tail in the resulting file.
//!
//! These tests defend each input-shape branch of that fix.

use rusqlite::params;

mod common;

use common::{open_db, workspace_with_files};

/// Shape s-submod: `helper::do_thing_q()` from import-less `src/lib.rs`
/// resolves to `do_thing_q` in `src/helper.rs` via the implicit-crate
/// interpretation in `qualified_module_fallback`.
#[test]
fn submodule_qualified_call_resolves() {
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

/// Shape s-wscrate: workspace-crate-prefixed call from an import-less
/// integration test resolves through the as-written interpretation in
/// `qualified_module_fallback` (the `crate_a` prefix is a workspace crate
/// name, so `resolve_module_path` dispatches to its workspace-crate arm).
///
/// Layout: workspace with two members. `crate_a/src/lib.rs` defines
/// `Widget::make_widget_044i`. `crate_b/tests/it.rs` is import-less and
/// calls `crate_a::Widget::make_widget_044i()`.
#[test]
fn workspace_crate_prefixed_call_resolves() {
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

/// Adversarial fixture: when a name is BOTH a workspace-member crate AND a
/// submodule of the current crate, Rust scoping says the submodule wins.
/// The fix's implicit-crate-first interpretation order must honor that.
///
/// Layout: workspace with two members. `crate_a` declares `mod helper;`
/// pointing at `crate_a/src/helper.rs::pub fn local_thing`. A second member
/// crate is also named `helper`, defining `helper/src/lib.rs::pub fn
/// external_thing`. From import-less `crate_a/src/lib.rs`, the ref
/// `helper::local_thing()` MUST resolve to the submodule (not the extern
/// crate). If interpretation order is wrong, the resolver would walk into
/// `helper/src/lib.rs` and find `external_thing` there — but never
/// `local_thing`, so the ref would stay unresolved. The assertion checks
/// for resolution to the correct target by symbol name AND file path.
#[test]
fn submodule_shadows_workspace_crate() {
    let (_dir, mut tethys) = workspace_with_files(&[
        (
            "Cargo.toml",
            r#"
[workspace]
members = ["crate_a", "helper"]
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

[dependencies]
helper = { path = "../helper" }
"#,
        ),
        (
            "crate_a/src/lib.rs",
            r"
mod helper;

pub fn entry() {
    helper::local_thing_044i();
}
",
        ),
        (
            "crate_a/src/helper.rs",
            r"
pub fn local_thing_044i() {}
",
        ),
        (
            "helper/Cargo.toml",
            r#"
[package]
name = "helper"
version = "0.1.0"
edition = "2021"
"#,
        ),
        (
            "helper/src/lib.rs",
            r"
pub fn external_thing_044i() {}
",
        ),
    ]);

    tethys.index().expect("index should succeed");

    let conn = open_db(&tethys);

    // The ref must resolve to local_thing_044i in crate_a's helper.rs, NOT
    // to anything in helper/src/lib.rs.
    let resolved_to_local: i64 = conn
        .query_row(
            "SELECT COUNT(*)
             FROM refs r
             JOIN files f_caller ON f_caller.id = r.file_id
             JOIN symbols s ON s.id = r.symbol_id
             JOIN files f_target ON f_target.id = s.file_id
             WHERE f_caller.path = 'crate_a/src/lib.rs'
               AND s.name = 'local_thing_044i'
               AND f_target.path = 'crate_a/src/helper.rs'",
            params![],
            |row| row.get(0),
        )
        .expect("count");

    // Cross-check: the extern-crate `external_thing_044i` MUST NOT have been
    // erroneously resolved as the target of the ref from lib.rs.
    let resolved_to_external: i64 = conn
        .query_row(
            "SELECT COUNT(*)
             FROM refs r
             JOIN files f_caller ON f_caller.id = r.file_id
             JOIN symbols s ON s.id = r.symbol_id
             JOIN files f_target ON f_target.id = s.file_id
             WHERE f_caller.path = 'crate_a/src/lib.rs'
               AND f_target.path = 'helper/src/lib.rs'",
            params![],
            |row| row.get(0),
        )
        .expect("count");

    assert!(
        resolved_to_local >= 1,
        "ref `helper::local_thing_044i()` from crate_a/src/lib.rs must \
         resolve to the submodule, not to the extern crate. \
         Got resolved_to_local={resolved_to_local}"
    );
    assert_eq!(
        resolved_to_external, 0,
        "ref MUST NOT resolve to any symbol in extern crate `helper` — \
         submodule shadows extern crate per Rust scoping. \
         Got resolved_to_external={resolved_to_external}"
    );
}
