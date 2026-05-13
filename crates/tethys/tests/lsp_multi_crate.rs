//! Integration regression tests for `tethys index --lsp` on multi-crate
//! Cargo workspaces.
//!
//! Asserts two invariants of LSP URI construction in `format_uri`:
//!
//! 1. End-to-end indexing of a multi-crate Windows workspace emits zero
//!    `url is not a file` / `-32603` errors. A regression that re-introduces
//!    `\\?\` prefix leakage, drops percent-encoding, or otherwise produces
//!    URIs rust-analyzer rejects would fail this test.
//!
//! 2. After indexing the 2-crate fixture, the DB contains at least one
//!    resolved cross-file reference. Distinguishes "errors silently swallowed"
//!    (1 alone is satisfied vacuously) from "Pass-3 LSP is actually working."
//!
//! Tests are `#[ignore]`d by default because they shell out to the `tethys`
//! binary and require `rust-analyzer` on PATH; opt in with `--ignored`.

#![cfg(windows)]

use std::fs;
use std::process::Command;
use tempfile::TempDir;

/// Panic with an actionable message if `rust-analyzer` is not on PATH.
///
/// Returning silently here would let the test report PASS while verifying
/// nothing — `#[ignore]` already gates these tests, so by the time this
/// runs the caller has explicitly opted in with `--ignored` and expects
/// the test to actually execute. A silent-skip would turn the regression
/// fence into a no-op on any environment missing the binary.
fn require_rust_analyzer() {
    let available = Command::new("where")
        .arg("rust-analyzer")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    assert!(
        available,
        "rust-analyzer not found on PATH — install it or filter this test \
         out with `--skip lsp_multi_crate` instead of letting it false-pass"
    );
}

/// Build a 2-crate Cargo workspace where `crate_caller` depends on
/// `crate_target` and calls a method on a `crate_target::Widget` — an
/// in-workspace cross-file reference that Pass-3 LSP must resolve.
///
/// The intra-workspace edge is what makes the resolved-ref assertion
/// observable in the DB; an external-only dependency (e.g. `std::HashMap`)
/// wouldn't produce a row whose `file_id` differs from the symbol's
/// defining file.
fn build_2_crate_fixture(dir: &TempDir) {
    let files = [
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
        let p = dir.path().join(rel);
        fs::create_dir_all(p.parent().expect("relative path has parent")).expect("create dir");
        fs::write(&p, content).expect("write file");
    }
}

/// `tethys index --lsp` on a multi-crate Windows workspace emits zero
/// `url is not a file` / `-32603` errors.
///
/// Runs tethys as a subprocess against the 2-crate fixture, captures
/// stdout+stderr, and asserts no line matches the error pattern. A
/// regression that re-introduces `\\?\` URI malformation or drops
/// percent-encoding would fail this because rust-analyzer would reject
/// the URIs.
#[test]
#[ignore = "requires rust-analyzer installed; subprocess invokes tethys binary"]
fn lsp_multi_crate_emits_no_url_errors() {
    require_rust_analyzer();

    let dir = tempfile::tempdir().expect("create tempdir");
    build_2_crate_fixture(&dir);

    let tethys_exe = env!("CARGO_BIN_EXE_tethys");
    let output = Command::new(tethys_exe)
        .args(["index", "--rebuild", "--lsp", "-w"])
        .arg(dir.path())
        .output()
        .expect("run tethys --lsp");

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Assert success before grepping. Without this, a future regression
    // that crashes tethys early (before any LSP request is made) would
    // pass vacuously: zero error lines emitted because zero requests
    // attempted.
    assert!(
        output.status.success(),
        "tethys index --lsp exited {:?}\nstdout: {stdout}\nstderr: {stderr}",
        output.status
    );

    let combined = format!("{stdout}\n{stderr}");
    let url_errors: Vec<&str> = combined
        .lines()
        .filter(|line| line.contains("url is not a file") || line.contains("LSP error -32603"))
        .collect();

    assert!(
        url_errors.is_empty(),
        "expected zero 'url is not a file' errors; got {}:\n{}",
        url_errors.len(),
        url_errors.join("\n")
    );
}

/// Indexing the 2-crate fixture with `--lsp` resolves at least one
/// cross-file reference in the DB.
///
/// The threshold of ≥1 distinguishes "URI errors silently swallowed" (the
/// no-errors test above is satisfied vacuously) from "Pass-3 LSP actually
/// works." Queried out of the DB directly rather than via tethys's
/// in-process API, so the assertion holds independent of any code path
/// the production binary might take.
#[test]
#[ignore = "requires rust-analyzer installed; subprocess invokes tethys binary"]
fn lsp_multi_crate_resolves_at_least_one_cross_file_ref() {
    require_rust_analyzer();

    let dir = tempfile::tempdir().expect("create tempdir");
    build_2_crate_fixture(&dir);

    let tethys_exe = env!("CARGO_BIN_EXE_tethys");
    let status = Command::new(tethys_exe)
        .args(["index", "--rebuild", "--lsp", "-w"])
        .arg(dir.path())
        .status()
        .expect("run tethys --lsp");
    assert!(
        status.success(),
        "tethys index --lsp should exit successfully"
    );

    let db_path = dir.path().join(".rivets").join("index").join("tethys.db");
    // Read-only flags: if db_path is wrong (e.g., tethys changes its DB
    // location), open fails immediately with a clear error rather than
    // silently creating an empty DB and returning 0 cross-file refs (which
    // would falsely look like a regression while hiding the root cause).
    let conn =
        rusqlite::Connection::open_with_flags(&db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("open tethys.db (read-only)");

    // Any resolved ref where the caller's file differs from the symbol's
    // defining file. Doesn't filter on path-form (workspace-relative vs
    // absolute) so the assertion stays correct regardless of tethys's
    // path-storage decisions.
    let resolved_cross_file: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM refs r
             JOIN symbols s ON s.id = r.symbol_id
             WHERE r.symbol_id IS NOT NULL
               AND r.file_id != s.file_id",
            [],
            |row| row.get(0),
        )
        .expect("query resolved cross-file refs");

    assert!(
        resolved_cross_file >= 1,
        "expected ≥1 resolved cross-file reference on the 2-crate fixture; \
         got {resolved_cross_file}. Even Pass-2 fallback can produce \
         cross-file edges, so this floor is conservative."
    );
}
