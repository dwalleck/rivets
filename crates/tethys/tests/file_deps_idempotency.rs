//! Integration tests for `file_deps` inter-run idempotency (rivets-lcb6).
//!
//! Pre-fix: `file_deps` was UPSERT-only with no `DELETE` between index runs.
//! Two `tethys index` invocations on the same workspace would accumulate
//! stale rows; removing a `use` statement and re-indexing would leave the
//! old edge in the table.
//!
//! Post-fix: `index_with_options` calls `clear_all_file_deps` before per-file
//! dependency computation, so re-indexing always reflects the current source
//! state.
//!
//! Both batch (`IndexOptions::default()`) and streaming (`with_streaming()`)
//! modes are exercised because they take different code paths to populate
//! `file_deps`: batch computes per-file inside the parse loop, streaming
//! aggregates after parse via `compute_all_dependencies`. The clear runs
//! before both branches, so both should be idempotent.

use rstest::rstest;
use std::fs;
use tempfile::TempDir;
use tethys::{IndexOptions, Tethys};

/// Create a 2-crate workspace with a cross-crate `use` statement.
/// `crate_caller::main` imports `crate_target::Widget`, producing one
/// expected cross-file edge after indexing.
fn build_two_crate_workspace(dir: &TempDir) {
    let files: [(&str, &str); 5] = [
        (
            "Cargo.toml",
            "[workspace]\nmembers = [\"crate_caller\", \"crate_target\"]\nresolver = \"2\"\n",
        ),
        (
            "crate_caller/Cargo.toml",
            "[package]\nname = \"crate_caller\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\
             [dependencies]\ncrate_target = { path = \"../crate_target\" }\n",
        ),
        (
            "crate_caller/src/lib.rs",
            "use crate_target::Widget;\n\
             pub fn make_and_ping() {\n\
                 let w = Widget;\n\
                 w.ping();\n\
             }\n",
        ),
        (
            "crate_target/Cargo.toml",
            "[package]\nname = \"crate_target\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        ),
        (
            "crate_target/src/lib.rs",
            "pub struct Widget;\nimpl Widget { pub fn ping(&self) {} }\n",
        ),
    ];
    for (rel, content) in files {
        let full = dir.path().join(rel);
        fs::create_dir_all(full.parent().expect("relative path has parent"))
            .expect("create parent");
        fs::write(&full, content).expect("write file");
    }
}

/// Snapshot of `file_deps` table state. Row count alone doesn't catch the
/// inter-run UPSERT bug — keys are preserved across runs, so only
/// `ref_count` grows when stale rows accumulate. Both fields together
/// pin both the row-membership invariant and the per-row count invariant.
struct FileDepsSnapshot {
    row_count: i64,
    ref_count_sum: i64,
}

fn file_deps_snapshot(db_path: &std::path::Path) -> FileDepsSnapshot {
    let conn =
        rusqlite::Connection::open_with_flags(db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("open tethys.db read-only");
    conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(ref_count), 0) FROM file_deps",
        [],
        |row| {
            Ok(FileDepsSnapshot {
                row_count: row.get(0)?,
                ref_count_sum: row.get(1)?,
            })
        },
    )
    .expect("snapshot file_deps")
}

/// Indexing the same workspace twice produces an identical `file_deps`
/// state — same rows AND same `ref_count` per row.
///
/// Row count alone is preserved pre-fix because the UPSERT has unique
/// `(from_file_id, to_file_id)` keys. The fence is in `ref_count_sum`:
/// pre-fix, each re-index of an unchanged dep increments `ref_count`,
/// so the sum grows. Post-fix, `clear_all_file_deps` wipes the table
/// before re-population, so the sum is stable.
///
/// Parameterized across batch and streaming indexing modes because they
/// take different paths to populate `file_deps` (per-file inside the
/// parse loop vs. post-parse aggregation via `compute_all_dependencies`).
/// The clear runs before both branches; both should be idempotent.
#[rstest]
#[case::batch(IndexOptions::default)]
#[case::streaming(IndexOptions::with_streaming)]
fn file_deps_stable_across_repeated_indexing(#[case] options_factory: fn() -> IndexOptions) {
    let dir = tempfile::tempdir().expect("create tempdir");
    build_two_crate_workspace(&dir);
    let mut tethys = Tethys::new(dir.path()).expect("Tethys::new");

    tethys
        .index_with_options(options_factory())
        .expect("first index");
    let db_path = tethys.db_path().to_path_buf();
    let first = file_deps_snapshot(&db_path);

    // Touch the source file to force the indexer past its mtime/size short-
    // circuit on the second run. Without this the second `index_with_options`
    // would skip the file entirely and `insert_file_dependency` would never
    // be invoked — masking the bug class.
    let caller = dir.path().join("crate_caller/src/lib.rs");
    let content = fs::read_to_string(&caller).expect("read caller");
    fs::write(&caller, format!("{content}\n// touch")).expect("touch caller");

    tethys
        .index_with_options(options_factory())
        .expect("second index");
    let second = file_deps_snapshot(&db_path);

    assert_eq!(
        first.row_count, second.row_count,
        "file_deps row count drifted; first={} second={}",
        first.row_count, second.row_count
    );
    assert_eq!(
        first.ref_count_sum, second.ref_count_sum,
        "file_deps ref_count_sum drifted; first={} second={}. \
         Pre-fix the UPSERT incremented ref_count on every re-index, so \
         this sum grew without source changes.",
        first.ref_count_sum, second.ref_count_sum
    );
}

/// Removing a `use` statement and re-indexing removes the corresponding
/// edge from `file_deps`. Pre-fix the stale edge would persist because
/// the table was never cleared.
///
/// Parameterized across batch and streaming modes for the same reason
/// as `file_deps_stable_across_repeated_indexing`.
#[rstest]
#[case::batch(IndexOptions::default)]
#[case::streaming(IndexOptions::with_streaming)]
fn file_deps_removed_when_use_statement_deleted(#[case] options_factory: fn() -> IndexOptions) {
    let dir = tempfile::tempdir().expect("create tempdir");
    build_two_crate_workspace(&dir);
    let mut tethys = Tethys::new(dir.path()).expect("Tethys::new");

    tethys
        .index_with_options(options_factory())
        .expect("first index");
    let db_path = tethys.db_path().to_path_buf();
    let before = file_deps_snapshot(&db_path);
    assert!(
        before.row_count >= 1,
        "expected ≥1 file_deps edge after indexing the cross-crate fixture; \
         got {}. Without the initial edge the rest of the test is vacuous, \
         so this floor failing means the fixture didn't trigger dependency \
         computation at all.",
        before.row_count
    );

    // Rewrite the caller to remove the cross-crate `use`. The edge from
    // crate_caller/src/lib.rs -> crate_target/src/lib.rs should disappear
    // on the next index run.
    let caller = dir.path().join("crate_caller/src/lib.rs");
    fs::write(&caller, "pub fn make_and_ping() {}\n").expect("rewrite caller");

    tethys
        .index_with_options(options_factory())
        .expect("second index");
    let after = file_deps_snapshot(&db_path);

    assert_eq!(
        after.row_count,
        before.row_count - 1,
        "expected exactly one file_deps edge removed after deleting the \
         cross-crate `use`; got before={} after={}. Pre-fix the stale edge \
         would persist (after == before); a count drop greater than 1 would \
         indicate unrelated edges were also removed.",
        before.row_count,
        after.row_count
    );
}
