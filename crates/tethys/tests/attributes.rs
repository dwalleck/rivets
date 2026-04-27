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

use std::fs;

use rusqlite::Connection;
use tempfile::TempDir;
use tethys::Tethys;

fn workspace_with_files(files: &[(&str, &str)]) -> (TempDir, Tethys) {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    for (path, content) in files {
        let full_path = dir.path().join(path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).expect("failed to create parent dirs");
        }
        fs::write(&full_path, content).expect("failed to write file");
    }
    let tethys = Tethys::new(dir.path()).expect("failed to create Tethys");
    (dir, tethys)
}

fn open_db(tethys: &Tethys) -> Connection {
    Connection::open(tethys.db_path()).expect("opening tethys.db should succeed")
}

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
