//! Integration tests for the architecture-analysis phase end-to-end.
//!
//! Builds a three-crate workspace where `crate_a` → `crate_b`, `crate_a` → `crate_c`,
//! and `crate_b` → `crate_c` (chain plus shortcut), then verifies coupling math.

use std::fs;
use tempfile::TempDir;
use tethys::{CouplingSort, Tethys};

/// Builds the canonical three-crate fixture in a temp dir, indexes it,
/// and returns (dir, tethys). The dir must be kept alive.
fn three_crate_workspace() -> (TempDir, Tethys) {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    fs::write(
        root.join("Cargo.toml"),
        r#"[workspace]
members = ["crate_a", "crate_b", "crate_c"]
resolver = "2"
"#,
    )
    .expect("workspace toml");

    fs::create_dir_all(root.join("crate_c/src")).expect("mkdir c");
    fs::write(
        root.join("crate_c/Cargo.toml"),
        r#"[package]
name = "crate_c"
version = "0.1.0"
edition = "2021"
"#,
    )
    .expect("c toml");
    fs::write(
        root.join("crate_c/src/lib.rs"),
        "pub fn leaf() -> u32 { 0 }\n",
    )
    .expect("c lib");

    fs::create_dir_all(root.join("crate_b/src")).expect("mkdir b");
    fs::write(
        root.join("crate_b/Cargo.toml"),
        r#"[package]
name = "crate_b"
version = "0.1.0"
edition = "2021"

[dependencies]
crate_c = { path = "../crate_c" }
"#,
    )
    .expect("b toml");
    fs::write(
        root.join("crate_b/src/lib.rs"),
        "use crate_c::leaf;\npub fn middle() -> u32 { leaf() + 1 }\n",
    )
    .expect("b lib");

    fs::create_dir_all(root.join("crate_a/src")).expect("mkdir a");
    fs::write(
        root.join("crate_a/Cargo.toml"),
        r#"[package]
name = "crate_a"
version = "0.1.0"
edition = "2021"

[dependencies]
crate_b = { path = "../crate_b" }
crate_c = { path = "../crate_c" }
"#,
    )
    .expect("a toml");
    fs::write(
        root.join("crate_a/src/lib.rs"),
        "use crate_b::middle;\nuse crate_c::leaf;\npub fn root() -> u32 { middle() + leaf() }\n",
    )
    .expect("a lib");

    let mut tethys = Tethys::new(root).expect("Tethys::new");
    tethys.index().expect("index");
    (dir, tethys)
}

#[test]
fn coupling_metrics_match_expected_values() {
    let (_dir, tethys) = three_crate_workspace();
    let rows = tethys
        .get_coupling_metrics(CouplingSort::Name)
        .expect("get_coupling_metrics");

    assert_eq!(rows.len(), 3, "three crates expected");

    let by_name = |n: &str| {
        rows.iter()
            .find(|m| m.package.name == n)
            .expect("crate present")
    };

    let a = by_name("crate_a");
    assert_eq!((a.afferent, a.efferent), (0, 2), "crate_a Ca=0, Ce=2");
    assert!((a.instability - 1.0).abs() < 1e-9);

    let b = by_name("crate_b");
    assert_eq!((b.afferent, b.efferent), (1, 1), "crate_b Ca=1, Ce=1");
    assert!((b.instability - 0.5).abs() < 1e-9);

    let c = by_name("crate_c");
    assert_eq!((c.afferent, c.efferent), (2, 0), "crate_c Ca=2, Ce=0");
    assert!((c.instability - 0.0).abs() < 1e-9);
}

#[test]
fn coupling_sort_orders_match_spec() {
    let (_dir, tethys) = three_crate_workspace();

    let by_instability = tethys
        .get_coupling_metrics(CouplingSort::Instability)
        .expect("by I");
    let names_i: Vec<_> = by_instability
        .iter()
        .map(|m| m.package.name.as_str())
        .collect();
    assert_eq!(names_i, ["crate_a", "crate_b", "crate_c"]);

    let by_name = tethys
        .get_coupling_metrics(CouplingSort::Name)
        .expect("by name");
    let names_n: Vec<_> = by_name.iter().map(|m| m.package.name.as_str()).collect();
    assert_eq!(names_n, ["crate_a", "crate_b", "crate_c"]);
}

#[test]
fn package_coupling_drilldown_for_middle_crate() {
    let (_dir, tethys) = three_crate_workspace();
    let detail = tethys
        .get_package_coupling("crate_b")
        .expect("query")
        .expect("found");

    let in_names: Vec<_> = detail
        .incoming
        .iter()
        .map(|d| d.package.name.as_str())
        .collect();
    let out_names: Vec<_> = detail
        .outgoing
        .iter()
        .map(|d| d.package.name.as_str())
        .collect();

    assert_eq!(in_names, ["crate_a"]);
    assert_eq!(out_names, ["crate_c"]);
}

#[test]
fn re_indexing_yields_identical_metrics() {
    let (_dir, mut tethys) = three_crate_workspace();
    let first = tethys
        .get_coupling_metrics(CouplingSort::Name)
        .expect("first");
    tethys.index().expect("re-index");
    let second = tethys
        .get_coupling_metrics(CouplingSort::Name)
        .expect("second");
    assert_eq!(first, second);
}

#[test]
fn empty_workspace_returns_empty_metrics() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut tethys = Tethys::new(dir.path()).expect("Tethys::new");
    tethys.index().expect("index");
    assert!(tethys.get_packages().expect("packages").is_empty());
    assert!(
        tethys
            .get_coupling_metrics(CouplingSort::default())
            .expect("metrics")
            .is_empty()
    );
}
