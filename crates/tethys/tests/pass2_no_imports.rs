//! Regression fence for rivets-dn35.
//!
//! Pass 2 of the resolver previously short-circuited when a file had no
//! `use` statements (`crates/tethys/src/resolve.rs::resolve_refs_for_file`
//! returning early on `imports.is_empty()`). That bypass also disabled the
//! import-agnostic resolution paths — most notably `fallback_symbol_search`'s
//! same-crate prefix search — leaving legitimate workspace-internal
//! references unresolved.
//!
//! The test below exercises an unqualified call from an import-less file
//! against a synthetic single-crate workspace. The entry file deliberately
//! has no `use` statements (only a `mod` declaration, which the Rust
//! extractor in `languages/rust.rs::extract_imports` does NOT record as
//! an import). Pre-dn35-fix the call stays unresolved
//! (`refs.symbol_id IS NULL`); post-fix it resolves to the sibling-module
//! definition via same-crate prefix search.
//!
//! Scope note: a separately-filed follow-up issue covers qualified-path
//! refs (`helper::do_thing_q()` or `crate_name::Type` from an import-less
//! file). Those need additional work in `try_resolve_reference` —
//! specifically a fallback that invokes `resolver::resolve_module_path` for
//! qualified refs when no import matches the first segment. The dn35 fix
//! is the necessary precondition (the short-circuit was blocking even the
//! existing qualified path) but it is not sufficient on its own.

use rusqlite::params;

mod common;

use common::{open_db, workspace_with_files};

/// Unqualified call from an import-less file resolves via `fallback_symbol_search`'s
/// same-crate prefix path.
#[test]
fn unqualified_call_in_import_less_file_resolves_via_fallback() {
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
    do_unique_dn35_thing();
}
",
        ),
        (
            "src/helper.rs",
            r"
pub fn do_unique_dn35_thing() {}
",
        ),
    ]);

    tethys.index().expect("index should succeed");

    let conn = open_db(&tethys);
    // Post-resolve, Pass 2 clears `reference_name` to NULL on the resolved row
    // (db::references::resolve_reference — see CLAUDE.md "Provenance gotcha").
    // Identify the ref by its target symbol name instead.
    let resolved_to_target: i64 = conn
        .query_row(
            "SELECT COUNT(*)
             FROM refs r
             JOIN files f ON f.id = r.file_id
             JOIN symbols s ON s.id = r.symbol_id
             WHERE f.path = 'src/lib.rs' AND s.name = 'do_unique_dn35_thing'",
            params![],
            |row| row.get(0),
        )
        .expect("count query should succeed");

    assert!(
        resolved_to_target >= 1,
        "unqualified call `do_unique_dn35_thing()` in src/lib.rs (zero use statements) \
         must resolve to its definition in src/helper.rs via fallback same-crate prefix \
         search. Pre-fix this stayed unresolved because Pass 2's imports.is_empty() \
         short-circuit bypassed fallback."
    );
}
