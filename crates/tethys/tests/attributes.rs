//! Integration tests for attribute persistence.
//!
//! Verifies that attributes extracted by `languages::rust` are persisted to
//! the `attributes` table during indexing and can be retrieved by joining
//! against `symbols`.

#![allow(
    clippy::needless_raw_string_hashes,
    clippy::doc_markdown,
    clippy::uninlined_format_args
)]

mod common;

use common::{open_db, workspace_with_files};

#[test]
fn attributes_table_exists_after_indexing() {
    let (_dir, mut tethys) = workspace_with_files(&[("src/lib.rs", "pub fn noop() {}")]);
    tethys.index().expect("index failed");

    let conn = open_db(&tethys);
    let exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'attributes'",
            [],
            |r| r.get(0),
        )
        .expect("sqlite_master query should succeed");
    assert_eq!(exists, 1, "attributes table should exist after indexing");
}

#[test]
fn derive_attribute_persisted() {
    let (_dir, mut tethys) = workspace_with_files(&[(
        "src/lib.rs",
        r#"
#[derive(Clone, Debug)]
pub struct Foo { x: i32 }
"#,
    )]);
    tethys.index().expect("index failed");

    let conn = open_db(&tethys);
    let (name, args): (String, Option<String>) = conn
        .query_row(
            "SELECT a.name, a.args
             FROM attributes a
             JOIN symbols s ON s.id = a.symbol_id
             WHERE s.name = 'Foo'",
            [],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)),
        )
        .expect("derive attribute should be persisted on Foo");

    assert_eq!(name, "derive");
    assert_eq!(args.as_deref(), Some("Clone, Debug"));
}

#[test]
fn marker_attribute_persists_with_null_args() {
    let (_dir, mut tethys) = workspace_with_files(&[(
        "src/lib.rs",
        r"
#[non_exhaustive]
pub enum E { A, B }
",
    )]);
    tethys.index().expect("index failed");

    let conn = open_db(&tethys);
    let args: Option<String> = conn
        .query_row(
            "SELECT a.args
             FROM attributes a
             JOIN symbols s ON s.id = a.symbol_id
             WHERE s.name = 'E' AND a.name = 'non_exhaustive'",
            [],
            |r| r.get(0),
        )
        .expect("non_exhaustive attribute should be persisted on E");
    assert!(
        args.is_none(),
        "marker attributes have NULL args, got {:?}",
        args
    );
}

#[test]
fn symbol_without_attributes_has_no_rows() {
    let (_dir, mut tethys) = workspace_with_files(&[("src/lib.rs", "pub struct Plain { x: i32 }")]);
    tethys.index().expect("index failed");

    let conn = open_db(&tethys);
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*)
             FROM attributes a
             JOIN symbols s ON s.id = a.symbol_id
             WHERE s.name = 'Plain'",
            [],
            |r| r.get(0),
        )
        .expect("count query should succeed");
    assert_eq!(count, 0, "Plain has no attributes; expected zero rows");
}

#[test]
fn attribute_attaches_through_visibility_modifier_on_tuple_field() {
    // Regression for the data-loss case flagged on PR #58: a tuple-style
    // variant carrying both `#[source]` and an explicit `pub` modifier used
    // to drop the attribute because the previous-sibling walk terminated on
    // the `visibility_modifier` before it ever reached the `attribute_item`.
    let (_dir, mut tethys) = workspace_with_files(&[(
        "src/lib.rs",
        r"
pub enum AgentError {
    Failed(#[source] pub serde_json::Error),
}
",
    )]);
    tethys.index().expect("index failed");

    let conn = open_db(&tethys);
    let row_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM attributes a
             JOIN symbols s ON s.id = a.symbol_id
             WHERE s.kind = 'struct_field'
               AND s.signature LIKE '%serde_json::Error%'
               AND a.name = 'source'",
            [],
            |r| r.get(0),
        )
        .expect("count query should succeed");
    assert_eq!(
        row_count, 1,
        "exactly one #[source] attribute row expected on the pub tuple field",
    );
}

#[test]
fn attribute_attaches_across_intervening_comment() {
    // Regression for the brittle prev_sibling walk: a comment line between
    // an attribute and the item it annotates used to terminate the walk and
    // silently drop the attribute. Both orderings (comment-then-attribute
    // and attribute-then-comment) should now resolve correctly.
    let (_dir, mut tethys) = workspace_with_files(&[(
        "src/lib.rs",
        r"
#[derive(Clone)]
// hand-written note between the attribute and the struct
pub struct Sandwich;
",
    )]);
    tethys.index().expect("index failed");

    let conn = open_db(&tethys);
    let args: Option<String> = conn
        .query_row(
            "SELECT a.args FROM attributes a
             JOIN symbols s ON s.id = a.symbol_id
             WHERE s.name = 'Sandwich' AND a.name = 'derive'",
            [],
            |r| r.get(0),
        )
        .expect("derive attribute should attach across the intervening comment");
    assert_eq!(args.as_deref(), Some("Clone"));
}
