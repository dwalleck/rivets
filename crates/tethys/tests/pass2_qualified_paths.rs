//! Regression fence for rivets-044i: qualified refs from import-less files.
//!
//! Stored `symbols.qualified_name` is module-stripped — the free-fn arm of
//! `indexing.rs::store_references` writes `name` only; the method arm writes
//! `parent_name::name`. So the literal `get_symbol_by_qualified_name` lookup
//! in Pass 2's `fallback_symbol_search` cannot match a ref like
//! `helper::do_thing_q` whose text carries a module prefix. The fix in
//! `resolve.rs::qualified_module_fallback` interprets the prefix as a module
//! path via `resolver::resolve_module_path`, then looks up the tail in the
//! resulting file.
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

    // Sanity precondition from oracle: the definition is indexed exactly once.
    assert_eq!(
        definition_exists, 1,
        "oracle precondition: do_thing_q must be indexed in helper.rs"
    );
    assert!(
        total_refs >= 1,
        "oracle precondition: at least one ref must be recorded in src/lib.rs \
         (got total_refs={total_refs}, resolved_refs={resolved_refs})"
    );

    assert!(
        resolved_to_target >= 1,
        "qualified call `helper::do_thing_q()` from import-less src/lib.rs \
         must resolve to its definition in src/helper.rs (regression fence \
         for rivets-044i)."
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

    // No-phantoms fence: any cross-file ref in the test file that binds to
    // a `make_widget_044i` or `Widget` symbol must target crate_a/src/lib.rs,
    // since the fixture has exactly one definition of each there. Catches
    // the bug class where Pass 2 phantom-resolves to a same-named symbol
    // outside the expected target file — which the prior round-1 disjunctive
    // `(unresolved == 0) || (resolved >= 1)` was structurally incapable of
    // catching.
    let phantom_resolved: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM refs r
             JOIN files f_caller ON f_caller.id = r.file_id
             JOIN symbols s ON s.id = r.symbol_id
             JOIN files f_target ON f_target.id = s.file_id
             WHERE f_caller.path = 'crate_b/tests/it.rs'
               AND s.name IN ('make_widget_044i', 'Widget')
               AND f_target.path != 'crate_a/src/lib.rs'",
            params![],
            |row| row.get(0),
        )
        .expect("count phantoms");

    assert_eq!(
        definition_exists, 1,
        "oracle precondition: make_widget_044i must be indexed in crate_a/src/lib.rs"
    );
    assert_eq!(
        phantom_resolved, 0,
        "no ref in crate_b/tests/it.rs that resolves to `make_widget_044i` or \
         `Widget` should bind outside crate_a/src/lib.rs. Got phantom_resolved={phantom_resolved}"
    );

    assert!(
        resolved_to_target >= 1,
        "workspace-crate-prefix call `crate_a::Widget::make_widget_044i()` from \
         import-less crate_b/tests/it.rs must resolve to its definition in \
         crate_a/src/lib.rs (regression fence for rivets-044i)."
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

/// Shape s-extern: refs prefixed with an external-crate name
/// (`std::collections::HashMap`) MUST NOT be phantom-resolved by the new
/// fallback. The implicit-crate retry must not stumble onto a same-named
/// submodule and corrupt the answer. To defeat that bug class, the
/// fixture intentionally adds a `std_helper` submodule with a similarly
/// std-prefixed name — only the qualified call into the local helper
/// should resolve.
#[test]
fn qualified_external_crate_stays_unresolved() {
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
mod std_helper;

pub fn entry() {
    let _ = std::collections::HashMap::<u32, u32>::new();
    std_helper::do_local_044i();
}
",
        ),
        (
            "src/std_helper.rs",
            r"
pub fn do_local_044i() {}
",
        ),
    ]);

    tethys.index().expect("index should succeed");

    let conn = open_db(&tethys);

    // `resolve_reference` clears `reference_name` to NULL atomically with
    // setting `symbol_id`, so filtering resolved rows by `reference_name LIKE
    // 'std::%'` would be vacuous (the predicate is never true after
    // resolution). Instead, target the tail-symbol names that a phantom
    // resolution would have to bind to: any workspace symbol named `HashMap`
    // or `new` reached from src/lib.rs would be a phantom for `std::*`.
    // The fixture introduces no such workspace symbols, so the count must be
    // zero — and if it isn't, we know the new fallback phantom-resolved.
    let std_phantom_resolved: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM refs r
             JOIN files f ON f.id = r.file_id
             JOIN symbols s ON s.id = r.symbol_id
             WHERE f.path = 'src/lib.rs'
               AND s.name IN ('HashMap', 'new')",
            params![],
            |row| row.get(0),
        )
        .expect("count");

    assert_eq!(
        std_phantom_resolved, 0,
        "external-crate-prefixed ref `std::collections::HashMap::<u32, u32>::new()` \
         MUST stay unresolved (no workspace symbol to bind to). Got \
         std_phantom_resolved={std_phantom_resolved}"
    );

    // Sanity: the same-shape local call still resolves. If this regresses
    // the test no longer tells us anything useful about the std::-stays-unresolved
    // claim.
    let local_resolved: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM refs r JOIN files f ON f.id = r.file_id
             JOIN symbols s ON s.id = r.symbol_id
             WHERE f.path = 'src/lib.rs' AND s.name = 'do_local_044i'",
            params![],
            |row| row.get(0),
        )
        .expect("count");

    assert!(
        local_resolved >= 1,
        "sanity precondition: `std_helper::do_local_044i()` must resolve \
         locally (otherwise this test isn't pinning std::-stays-unresolved)"
    );
}

/// Shape s-crate: explicit `crate::sub::fn()` from an import-less file
/// resolves via the `crate::` arm of `resolve_module_path`. The plausible
/// bug class this defeats is the implicit-crate-prepend retry inadvertently
/// producing `crate::crate::sub::fn` and shadowing the correct path. The
/// `qualified_module_fallback` code branches on `prefix[0] == "crate"` to
/// skip the implicit-prepend specifically to avoid this.
///
/// The fixture lays a phantom-resolution trap to fence the gate: a literal
/// `src/crate/sub_044i.rs` (directory named "crate" on disk — possible at
/// the filesystem level even though `mod crate;` is not legal Rust) carries
/// a `do_crate_thing_044i` free fn with the same name as the real target in
/// `src/sub_044i.rs`. With the gate working, Interpretation A is skipped
/// and Interpretation B resolves `["crate","sub_044i"]` to the real file.
/// With the gate broken (e.g., dropping `"crate"` from the `matches!` set),
/// Interpretation A builds `["crate","crate","sub_044i"]`, which
/// `resolve_module_path` walks as a filesystem path and finds the trap
/// `src/crate/sub_044i.rs`. The tail then phantom-resolves to the trap. The
/// negative assertion below catches that path.
///
/// The call shape `crate::sub_044i::do_crate_thing_044i()` (free-fn call)
/// is chosen so tree-sitter records the qualified `reference_name`. A
/// type-position path like `let _: crate::sub_044i::ThingFour;` is recorded
/// by the rust extractor as just `ThingFour` (the leaf), which would
/// resolve via the unqualified workspace-wide search instead — bypassing
/// `qualified_module_fallback` entirely and giving false confidence.
#[test]
fn qualified_crate_prefix_resolves() {
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
mod sub_044i;

pub fn entry() {
    crate::sub_044i::do_crate_thing_044i();
}
",
        ),
        (
            "src/sub_044i.rs",
            r"
pub fn do_crate_thing_044i() {}
",
        ),
        // Phantom-resolution trap. Directory literally named "crate" is
        // possible on disk; `qualified_module_fallback`'s implicit-crate
        // retry would walk into this file under a broken gate. Same-named
        // function ensures the tail lookup would succeed and bind here.
        (
            "src/crate/sub_044i.rs",
            r"
pub fn do_crate_thing_044i() {}
",
        ),
    ]);

    tethys.index().expect("index should succeed");

    let conn = open_db(&tethys);

    let resolved_to_real_target: i64 = conn
        .query_row(
            "SELECT COUNT(*)
             FROM refs r
             JOIN files f_caller ON f_caller.id = r.file_id
             JOIN symbols s ON s.id = r.symbol_id
             JOIN files f_target ON f_target.id = s.file_id
             WHERE f_caller.path = 'src/lib.rs'
               AND s.name = 'do_crate_thing_044i'
               AND f_target.path = 'src/sub_044i.rs'",
            params![],
            |row| row.get(0),
        )
        .expect("count");

    let resolved_to_trap: i64 = conn
        .query_row(
            "SELECT COUNT(*)
             FROM refs r
             JOIN files f_caller ON f_caller.id = r.file_id
             JOIN symbols s ON s.id = r.symbol_id
             JOIN files f_target ON f_target.id = s.file_id
             WHERE f_caller.path = 'src/lib.rs'
               AND s.name = 'do_crate_thing_044i'
               AND f_target.path = 'src/crate/sub_044i.rs'",
            params![],
            |row| row.get(0),
        )
        .expect("count");

    assert!(
        resolved_to_real_target >= 1,
        "ref `crate::sub_044i::do_crate_thing_044i()` MUST resolve via the \
         as-written `crate::` arm of resolve_module_path to the free fn in \
         src/sub_044i.rs. Got resolved_to_real_target={resolved_to_real_target}"
    );

    assert_eq!(
        resolved_to_trap, 0,
        "ref MUST NOT phantom-resolve to the free fn in the literal-`crate` \
         trap directory — the `prefix[0]==\"crate\"` gate in \
         qualified_module_fallback must skip the implicit-crate prepend. \
         Got resolved_to_trap={resolved_to_trap}"
    );
}

/// Pin the longest-prefix-first iteration order in `qualified_module_fallback`.
///
/// For ref `outer::inner::deep_thing_044i`, both `outer::inner` (deeper) and
/// `outer` (shallower) resolve to module files that contain a tail-matching
/// symbol — by design:
///
/// - `src/outer/inner.rs::deep_thing_044i` (free fn, stored `qualified_name`
///   = `deep_thing_044i`). Reached by split=2 with tail `deep_thing_044i`.
/// - `src/outer.rs::inner::deep_thing_044i` (method on `impl inner`, stored
///   `qualified_name` = `inner::deep_thing_044i` per the method arm of
///   `indexing.rs::store_references`). Reached by split=1 with tail
///   `inner::deep_thing_044i`.
///
/// With the working loop `(1..segments.len()).rev()` (longest first), split=2
/// fires first, finds the free fn, and returns. The ref binds to the file at
/// `src/outer/inner.rs`. If the loop direction inverted to `1..segments.len()`
/// (shortest first), split=1 would phantom-resolve to the method on `inner`
/// in `src/outer.rs`. The two assertions distinguish those cases.
#[test]
fn longest_prefix_wins_over_shorter() {
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
mod outer;

pub fn entry() {
    outer::inner::deep_thing_044i();
}
",
        ),
        // Shallow-prefix target: an `impl inner` whose method stores
        // qualified_name = `inner::deep_thing_044i`. If the fallback's loop
        // tried split=1 first, it would resolve here.
        (
            "src/outer.rs",
            r"
#[allow(non_camel_case_types)]
pub struct inner;

impl inner {
    pub fn deep_thing_044i() {}
}
",
        ),
        // Deep-prefix target: a free fn whose qualified_name is just
        // `deep_thing_044i`. The fallback's longest-first loop must reach
        // here first.
        (
            "src/outer/inner.rs",
            r"
pub fn deep_thing_044i() {}
",
        ),
    ]);

    tethys.index().expect("index should succeed");

    let conn = open_db(&tethys);

    let resolved_to_deeper: i64 = conn
        .query_row(
            "SELECT COUNT(*)
             FROM refs r
             JOIN files f_caller ON f_caller.id = r.file_id
             JOIN symbols s ON s.id = r.symbol_id
             JOIN files f_target ON f_target.id = s.file_id
             WHERE f_caller.path = 'src/lib.rs'
               AND s.name = 'deep_thing_044i'
               AND f_target.path = 'src/outer/inner.rs'",
            params![],
            |row| row.get(0),
        )
        .expect("count");

    let resolved_to_shallower: i64 = conn
        .query_row(
            "SELECT COUNT(*)
             FROM refs r
             JOIN files f_caller ON f_caller.id = r.file_id
             JOIN symbols s ON s.id = r.symbol_id
             JOIN files f_target ON f_target.id = s.file_id
             WHERE f_caller.path = 'src/lib.rs'
               AND s.name = 'deep_thing_044i'
               AND f_target.path = 'src/outer.rs'",
            params![],
            |row| row.get(0),
        )
        .expect("count");

    assert!(
        resolved_to_deeper >= 1,
        "longest-prefix-first iteration must resolve `outer::inner::deep_thing_044i` \
         to the free fn in src/outer/inner.rs. Got resolved_to_deeper={resolved_to_deeper}"
    );
    assert_eq!(
        resolved_to_shallower, 0,
        "loop direction is wrong: the shorter prefix `outer` resolved first and \
         phantom-resolved the ref to the method on `impl inner` in src/outer.rs. \
         Got resolved_to_shallower={resolved_to_shallower}"
    );
}

/// Pin that a successfully-resolved prefix whose tail symbol does not exist
/// in the resolved file leaves the ref unresolved instead of phantom-binding
/// to something else.
///
/// The fixture provides `mod helper;` with a single fn `other_thing_044i` —
/// but the caller references `helper::nonexistent_044i`. The fallback
/// resolves the prefix to `src/helper.rs`, calls
/// `search_symbol_by_qualified_name_in_file("nonexistent_044i", helper_id)`,
/// gets None, and the loop continues to shorter splits (none of which
/// resolve), exiting at the final `Ok(None)`. The ref must remain unresolved.
#[test]
fn prefix_resolves_but_tail_missing_stays_unresolved() {
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
    helper::nonexistent_044i();
}
",
        ),
        (
            "src/helper.rs",
            r"
pub fn other_thing_044i() {}
",
        ),
    ]);

    tethys.index().expect("index should succeed");

    let conn = open_db(&tethys);

    let phantom_resolves: i64 = conn
        .query_row(
            "SELECT COUNT(*)
             FROM refs r
             JOIN files f ON f.id = r.file_id
             JOIN symbols s ON s.id = r.symbol_id
             WHERE f.path = 'src/lib.rs'
               AND s.name IN ('nonexistent_044i', 'other_thing_044i')",
            params![],
            |row| row.get(0),
        )
        .expect("count");

    // Cross-check: `other_thing_044i` is indexed (so a phantom binding to it
    // would not be silently impossible).
    let other_exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM symbols s JOIN files f ON f.id = s.file_id
             WHERE s.name = 'other_thing_044i' AND f.path = 'src/helper.rs'",
            params![],
            |row| row.get(0),
        )
        .expect("count");

    assert_eq!(
        other_exists, 1,
        "oracle precondition: other_thing_044i must be indexed in helper.rs \
         (otherwise a phantom binding to it would be impossible by absence, \
         not by the fallback's correctness)"
    );
    assert_eq!(
        phantom_resolves, 0,
        "ref `helper::nonexistent_044i()` must remain unresolved — prefix \
         resolved but tail absent. Got phantom_resolves={phantom_resolves}"
    );
}

/// Pin that `self::*` and `super::*` paths route through the as-written
/// arm of `qualified_module_fallback` (the `matches!(prefix[0], "crate" |
/// "self" | "super")` gate must include both).
///
/// Note tethys's filesystem-walk semantics, which differ from Rust:
/// `resolve_self_path` joins segments to the caller's directory, so
/// `self::sibling` from `src/parent/child.rs` reaches `src/parent/sibling.rs`.
/// `resolve_super_path` joins to the caller's grandparent directory, so
/// `super::cousin` from `src/parent/child.rs` reaches `src/cousin.rs` (NOT
/// `src/parent/cousin.rs` — Rust spec would say the latter). The fixture
/// lays files where each interpretation expects them. The divergence is
/// preexisting tethys behavior, not introduced by rivets-044i, and is
/// tracked separately as rivets-nkjd; when that issue closes, this test
/// fixture (and likely this docstring) will need adjusting.
#[test]
fn self_and_super_paths_resolve_via_as_written() {
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
mod parent;
mod cousin;
",
        ),
        (
            "src/parent.rs",
            r"
mod child;
pub mod sibling;
",
        ),
        (
            "src/parent/child.rs",
            r"
pub fn entry() {
    self::sibling::do_self_kid_044i();
    super::cousin::do_super_kid_044i();
}
",
        ),
        (
            "src/parent/sibling.rs",
            r"
pub fn do_self_kid_044i() {}
",
        ),
        (
            "src/cousin.rs",
            r"
pub fn do_super_kid_044i() {}
",
        ),
    ]);

    tethys.index().expect("index should succeed");

    let conn = open_db(&tethys);

    let self_resolved: i64 = conn
        .query_row(
            "SELECT COUNT(*)
             FROM refs r
             JOIN files f_caller ON f_caller.id = r.file_id
             JOIN symbols s ON s.id = r.symbol_id
             JOIN files f_target ON f_target.id = s.file_id
             WHERE f_caller.path = 'src/parent/child.rs'
               AND s.name = 'do_self_kid_044i'
               AND f_target.path = 'src/parent/sibling.rs'",
            params![],
            |row| row.get(0),
        )
        .expect("count");

    let super_resolved: i64 = conn
        .query_row(
            "SELECT COUNT(*)
             FROM refs r
             JOIN files f_caller ON f_caller.id = r.file_id
             JOIN symbols s ON s.id = r.symbol_id
             JOIN files f_target ON f_target.id = s.file_id
             WHERE f_caller.path = 'src/parent/child.rs'
               AND s.name = 'do_super_kid_044i'
               AND f_target.path = 'src/cousin.rs'",
            params![],
            |row| row.get(0),
        )
        .expect("count");

    assert!(
        self_resolved >= 1,
        "`self::sibling::do_self_kid_044i()` must resolve via the as-written \
         arm of qualified_module_fallback. Got self_resolved={self_resolved}"
    );
    assert!(
        super_resolved >= 1,
        "`super::cousin::do_super_kid_044i()` must resolve via the as-written \
         arm of qualified_module_fallback. Got super_resolved={super_resolved}"
    );
}
