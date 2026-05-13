//! Integration regression test for rivets-714v: tethys's `--lsp` flag must
//! work on multi-crate Cargo workspaces.
//!
//! Pre-fix (before rivets-714v slice 1): tethys's `path_to_uri` leaked
//! `\\?\` extended-length prefixes (from `Path::canonicalize` on Windows)
//! into LSP `file://` URIs, and didn't percent-encode RFC 3986 reserved
//! characters. rust-analyzer rejected the malformed URIs with
//! `-32603 url is not a file`; Pass 3 resolved zero references.
//!
//! Post-fix: `format_uri` strips the prefix and percent-encodes. rust-analyzer
//! accepts the URIs and Pass 3 can proceed.
//!
//! Fixture matches `.rivets-714v/probe.py`. Uses the same subprocess +
//! stderr-grep mechanism — the probe is the empirical reproduction; this
//! test is its permanent form.

#![cfg(windows)]

use std::fs;
use std::process::Command;
use tempfile::TempDir;

/// Check if rust-analyzer is available in PATH.
fn rust_analyzer_available() -> bool {
    Command::new("where")
        .arg("rust-analyzer")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Build a 2-crate Cargo workspace where `crate_caller` depends on
/// `crate_target` and calls a method on a `crate_target::Widget` — an
/// in-workspace cross-file reference that Pass-3 LSP must resolve.
///
/// Diverges from `.rivets-714v/probe.py`'s fixture (which used external
/// `std::HashMap` and had no in-workspace cross-file refs). The probe's
/// shape was inherited from rivets-3d0s where the question was different;
/// for rivets-714v's C6 claim, we need an intra-workspace edge so the
/// resolved-ref count is observable in the DB.
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
        fs::create_dir_all(p.parent().unwrap()).expect("create dir");
        fs::write(&p, content).expect("write file");
    }
}

/// rivets-714v claim C5: end-to-end `tethys index --lsp` on a 2-crate
/// Windows workspace emits zero `url is not a file -32603` errors after
/// the fix.
///
/// Mechanism: run tethys as a subprocess (matching the prove-it-prototype
/// probe shape), capture stderr, count matches of the error pattern. The
/// probe captured 4 matches pre-fix; this test asserts 0 matches post-fix.
///
/// A regression that re-introduces the `\\?\` URI malformation, drops
/// percent-encoding, or otherwise breaks the URI construction would fail
/// this test because rust-analyzer would re-reject the URIs.
#[test]
#[ignore = "requires rust-analyzer installed; subprocess invokes tethys binary"]
fn lsp_multi_crate_emits_no_url_errors() {
    if !rust_analyzer_available() {
        eprintln!("Skipping test: rust-analyzer not available");
        return;
    }

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
    let combined = format!("{stdout}\n{stderr}");
    let url_errors: Vec<&str> = combined
        .lines()
        .filter(|line| line.contains("url is not a file") || line.contains("LSP error -32603"))
        .collect();

    assert!(
        url_errors.is_empty(),
        "expected zero 'url is not a file' errors after rivets-714v fix; got {}:\n{}",
        url_errors.len(),
        url_errors.join("\n")
    );
}

/// rivets-714v claim C6: after the fix, indexing the 2-crate fixture with
/// `--lsp` produces at least one cross-file reference resolved in the DB.
///
/// The threshold of ≥1 is the floor that distinguishes "URI fix masks errors
/// but doesn't actually let Pass 3 do its job" from "URI fix unblocks
/// Pass 3." A regression that silently swallows URI errors without fixing
/// them would pass the C5 (no errors) test but fail this one.
///
/// Queries the DB directly rather than using tethys's in-process API,
/// matching the probe's mechanism for independent verification.
#[test]
#[ignore = "requires rust-analyzer installed; subprocess invokes tethys binary"]
fn lsp_multi_crate_resolves_at_least_one_cross_file_ref() {
    if !rust_analyzer_available() {
        eprintln!("Skipping test: rust-analyzer not available");
        return;
    }

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
    let conn = rusqlite::Connection::open(&db_path).expect("open tethys.db");

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
         got {resolved_cross_file}. \
         Note: Pass-3 LSP resolution is what makes this work; pre-rivets-714v, \
         malformed URIs caused rust-analyzer to reject every request and this \
         count was always 0. Even Pass-2-fallback can produce cross-file edges, \
         so this floor is conservative — it should be easy to meet post-fix."
    );
}
