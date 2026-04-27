//! Integration tests for sub-symbol extraction (variants, fields).
//!
//! Verifies that variants and fields land in the `symbols` table with the
//! expected `kind`, `parent_name`, and `signature` populated, and that the
//! Gate 4-shape SQL query (find external-error types behind `#[source]` on a
//! pub variant field) returns the expected hits against an indexed fixture.

#![allow(
    clippy::needless_raw_string_hashes,
    clippy::doc_markdown,
    clippy::uninlined_format_args
)]

mod common;

use common::{open_db, workspace_with_files};

#[test]
fn enum_variants_persist_with_parent_name() {
    let (_dir, mut tethys) = workspace_with_files(&[(
        "src/lib.rs",
        r"
pub enum Status {
    Active,
    Pending(String),
    Failed { reason: String },
}
",
    )]);
    tethys.index().expect("index failed");

    let conn = open_db(&tethys);
    let mut stmt = conn
        .prepare(
            "SELECT name FROM symbols
             WHERE kind = 'enum_variant'
             ORDER BY line",
        )
        .expect("prepare should succeed");
    let names: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .expect("query_map should succeed")
        .collect::<Result<_, _>>()
        .expect("collect should succeed");

    assert_eq!(names, vec!["Active", "Pending", "Failed"]);
}

#[test]
fn struct_field_signatures_persist() {
    let (_dir, mut tethys) = workspace_with_files(&[(
        "src/lib.rs",
        r"
pub struct User {
    pub id: u64,
    name: String,
}
",
    )]);
    tethys.index().expect("index failed");

    let conn = open_db(&tethys);
    let mut stmt = conn
        .prepare(
            "SELECT name, signature FROM symbols
             WHERE kind = 'struct_field'
             ORDER BY line",
        )
        .expect("prepare should succeed");
    let rows: Vec<(String, Option<String>)> = stmt
        .query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        })
        .expect("query_map should succeed")
        .collect::<Result<_, _>>()
        .expect("collect should succeed");

    assert_eq!(
        rows,
        vec![
            ("id".to_string(), Some("u64".to_string())),
            ("name".to_string(), Some("String".to_string())),
        ]
    );
}

#[test]
fn tuple_struct_fields_use_positional_names() {
    let (_dir, mut tethys) = workspace_with_files(&[("src/lib.rs", "pub struct GitRef(String);")]);
    tethys.index().expect("index failed");

    let conn = open_db(&tethys);
    let (name, signature): (String, Option<String>) = conn
        .query_row(
            "SELECT s.name, s.signature
             FROM symbols s
             JOIN symbols parent ON parent.name = 'GitRef' AND parent.kind = 'struct'
             WHERE s.kind = 'struct_field'",
            [],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)),
        )
        .expect("tuple field should be persisted");
    assert_eq!(name, "0");
    assert_eq!(signature.as_deref(), Some("String"));
}

#[test]
fn tuple_field_comments_are_not_emitted_as_fake_fields() {
    // Regression for the catch-all match arm in extract_tuple_fields:
    // line/block comments inside an ordered_field_declaration_list used to
    // fall through to the type-extraction branch and emit fake struct_field
    // rows whose `signature` was the comment text. Comments should be
    // skipped entirely without resetting pending_visibility.
    let (_dir, mut tethys) = workspace_with_files(&[(
        "src/lib.rs",
        r"
pub struct Coords(
    // x coordinate in pixels
    pub i32,
    /* y coordinate in pixels */
    pub i32,
);
",
    )]);
    tethys.index().expect("index failed");

    let conn = open_db(&tethys);
    let mut stmt = conn
        .prepare(
            "SELECT s.name, s.signature
             FROM symbols s
             JOIN symbols parent ON parent.name = 'Coords' AND parent.kind = 'struct'
             WHERE s.kind = 'struct_field'
             ORDER BY s.line",
        )
        .expect("prepare should succeed");
    let rows: Vec<(String, Option<String>)> = stmt
        .query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        })
        .expect("query_map should succeed")
        .collect::<Result<_, _>>()
        .expect("collect should succeed");

    assert_eq!(
        rows,
        vec![
            ("0".to_string(), Some("i32".to_string())),
            ("1".to_string(), Some("i32".to_string())),
        ],
        "tuple-field walk should skip comments, not emit them as positional fields",
    );
}

#[test]
fn gate_4_external_error_query_matches_violation() {
    // The canonical Gate 4 violation pattern from PR #64: a pub enum variant
    // carrying an external crate's error via #[source].
    let (_dir, mut tethys) = workspace_with_files(&[(
        "src/lib.rs",
        r"
pub enum AgentError {
    NativeManifestParseFailed {
        path: String,
        #[source]
        source: serde_json::Error,
    },
    Ok,
}
",
    )]);
    tethys.index().expect("index failed");

    let conn = open_db(&tethys);
    // Gate 4: a `#[source]` attribute attached to a struct_field whose
    // signature mentions an external crate's error path, parent symbol is
    // a variant of a public enum.
    let mut stmt = conn
        .prepare(
            "SELECT s.name, s.parent_name_legacy, s.signature
             FROM (
                 SELECT s.name AS name,
                        (SELECT s2.name FROM symbols s2 WHERE s2.id = s.parent_symbol_id) AS parent_name_legacy,
                        s.signature AS signature,
                        s.id AS id
                 FROM symbols s
                 WHERE s.kind = 'struct_field'
                   AND s.signature LIKE '%serde_json::%'
             ) s
             JOIN attributes a ON a.symbol_id = s.id
             WHERE a.name = 'source'",
        )
        .expect("prepare should succeed");
    let rows: Vec<(String, Option<String>, Option<String>)> = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })
        .expect("query_map should succeed")
        .collect::<Result<_, _>>()
        .expect("collect should succeed");

    assert_eq!(rows.len(), 1, "Gate 4 query should find one violation");
    let (name, parent_name_legacy, signature) = &rows[0];
    assert_eq!(name, "source");
    // parent_symbol_id resolution isn't implemented yet (a pre-existing gap
    // tracked separately), so the parent_name_legacy subquery resolves to
    // NULL in the indexed database. Asserting that explicitly here documents
    // the current limitation in code — if a future change starts populating
    // parent_symbol_id, this assertion will fail and prompt updating Gate 4.
    assert!(
        parent_name_legacy.is_none(),
        "parent_name_legacy expected to be NULL until parent_symbol_id resolution lands; got {:?}",
        parent_name_legacy
    );
    assert!(
        signature
            .as_deref()
            .is_some_and(|s| s.contains("serde_json::Error")),
        "signature should include serde_json::Error; got {:?}",
        signature
    );
}

// ─── Schema-population contract tests ────────────────────────────────────
//
// These lock the per-kind promises about which columns are non-NULL after
// indexing. The motivating failure: `signature` is declared on every symbols
// row but is only *populated* for some kinds. The pre-existing baseline left
// `signature` NULL on every `struct` and `enum` row despite the schema
// suggesting otherwise — exactly the silent-zero-results failure mode the
// broken Gate 4 grep had.
//
// If you add a new SymbolKind that promises to populate `signature`, add it
// to `signature_non_null_for_promised_kinds`. If the new kind is NULL by
// design, document the reason in code and don't add it here.

#[test]
fn signature_non_null_for_promised_kinds() {
    let (_dir, mut tethys) = workspace_with_files(&[(
        "src/lib.rs",
        r"
pub fn free_fn() -> i32 { 0 }

pub struct S {
    pub field: u64,
}

impl S {
    pub fn method(&self) -> u64 { self.field }
}

pub enum E {
    Tuple(String),
    Record { x: i32 },
}
",
    )]);
    tethys.index().expect("index failed");

    let conn = open_db(&tethys);
    for kind in ["function", "method", "struct_field"] {
        let null_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM symbols
                 WHERE kind = ?1 AND signature IS NULL",
                [kind],
                |r| r.get(0),
            )
            .expect("count query should succeed");
        assert_eq!(
            null_count, 0,
            "kind={} promises non-NULL signature; found {} rows with NULL",
            kind, null_count,
        );
    }

    // Non-unit enum variants must have populated signature.
    let null_non_unit: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM symbols
             WHERE kind = 'enum_variant'
               AND name IN ('Tuple', 'Record')
               AND signature IS NULL",
            [],
            |r| r.get(0),
        )
        .expect("count query should succeed");
    assert_eq!(
        null_non_unit, 0,
        "non-unit enum variants must have signature populated",
    );
}

#[test]
fn unit_variant_signature_is_explicitly_null() {
    let (_dir, mut tethys) = workspace_with_files(&[(
        "src/lib.rs",
        r"
pub enum Status { Active, Inactive }
",
    )]);
    tethys.index().expect("index failed");

    let conn = open_db(&tethys);
    let signatures: Vec<(String, Option<String>)> = conn
        .prepare(
            "SELECT name, signature FROM symbols
             WHERE kind = 'enum_variant'
             ORDER BY line",
        )
        .expect("prepare should succeed")
        .query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        })
        .expect("query_map should succeed")
        .collect::<Result<_, _>>()
        .expect("collect should succeed");

    assert_eq!(
        signatures,
        vec![("Active".to_string(), None), ("Inactive".to_string(), None),],
        "unit variants must have NULL signature (not empty string, not '()')",
    );
}

#[test]
fn derive_attribute_landing_on_struct_is_locked() {
    // Locks the contract that `#[derive(...)]` attached to a struct lands
    // as one row in the attributes table with name='derive' and the comma-
    // separated arg list. Drift here is the same shape as the original
    // signature drift that motivated this whole sub-symbol extraction.
    let (_dir, mut tethys) = workspace_with_files(&[(
        "src/lib.rs",
        r"
#[derive(Clone, Debug, PartialEq)]
pub struct Locked { x: u8 }
",
    )]);
    tethys.index().expect("index failed");

    let conn = open_db(&tethys);
    let row_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM attributes a
             JOIN symbols s ON s.id = a.symbol_id
             WHERE s.name = 'Locked' AND s.kind = 'struct' AND a.name = 'derive'",
            [],
            |r| r.get(0),
        )
        .expect("count query should succeed");
    assert_eq!(
        row_count, 1,
        "exactly one derive attribute row expected for struct Locked",
    );

    let args: Option<String> = conn
        .query_row(
            "SELECT a.args FROM attributes a
             JOIN symbols s ON s.id = a.symbol_id
             WHERE s.name = 'Locked' AND a.name = 'derive'",
            [],
            |r| r.get(0),
        )
        .expect("args query should succeed");
    assert_eq!(args.as_deref(), Some("Clone, Debug, PartialEq"));
}
