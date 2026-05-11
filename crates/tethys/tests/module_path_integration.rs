//! Integration tests for module path computation during indexing.

use std::path::PathBuf;

use tethys::cargo::{compute_module_path, discover_crates, get_crate_for_file};
use tethys::{CrateInfo, SymbolKind, Tethys};

/// Helper to get workspace root and skip if crates cannot be discovered.
///
/// Returns `(workspace_path, discovered_crates)` if successful, or prints skip message.
fn get_workspace_with_crates() -> Option<(PathBuf, Vec<CrateInfo>)> {
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("tethys should be in crates/")
        .parent()
        .expect("crates/ should be in workspace root")
        .to_path_buf();

    let crates = discover_crates(&workspace);

    // Skip if workspace couldn't be parsed (e.g., unsupported resolver version)
    if crates.is_empty() {
        eprintln!(
            "Skipping test: workspace at {} returned no crates \
            (possibly unsupported Cargo.toml features like resolver = \"3\")",
            workspace.display()
        );
        return None;
    }

    Some((workspace, crates))
}

/// Assert a single symbol in `tethys` has the expected module path.
fn assert_symbol_has_module_path(
    tethys: &Tethys,
    name: &str,
    kind: SymbolKind,
    expected_module_path: &str,
) {
    let symbols = tethys
        .search_symbols(name)
        .expect("search should succeed");

    let symbol = symbols
        .iter()
        .find(|s| s.name == name && s.kind == kind)
        .unwrap_or_else(|| panic!("{name} ({kind:?}) should be indexed"));

    assert_eq!(
        symbol.module_path, expected_module_path,
        "{name} should have module_path '{expected_module_path}'"
    );
}

/// Index the rivets workspace and verify that known symbols are
/// assigned the correct module path.
///
/// This is intentionally one test (rather than one-test-per-symbol). All
/// assertions share a single `tethys.index()` call. Splitting into multiple
/// `#[test]` functions causes nextest to spawn separate processes that race
/// on the same workspace `SQLite` DB — historically that surfaced as
/// `DatabaseBusy` and FK-constraint failures on Linux and macOS CI runners.
#[test]
fn indexing_rivets_workspace_assigns_correct_module_paths() {
    let Some((workspace, _crates)) = get_workspace_with_crates() else {
        return;
    };

    let mut tethys = Tethys::new(&workspace).expect("new should succeed");
    let _stats = tethys.index().expect("index should succeed");

    // `CrateInfo` is in `crates/tethys/src/types.rs` → `crate::types`.
    assert_symbol_has_module_path(&tethys, "CrateInfo", SymbolKind::Struct, "crate::types");

    // `discover_crates` is in `crates/tethys/src/cargo.rs` → `crate::cargo`.
    assert_symbol_has_module_path(
        &tethys,
        "discover_crates",
        SymbolKind::Function,
        "crate::cargo",
    );
}

/// Test crate discovery and module path computation using direct API.
#[test]
fn crate_discovery_and_module_path_direct() {
    let Some((workspace, crates)) = get_workspace_with_crates() else {
        return;
    };

    // Test file path for cargo.rs
    let cargo_rs = workspace.join("crates/tethys/src/cargo.rs");
    let canonical = cargo_rs
        .canonicalize()
        .expect("cargo.rs should exist and be canonicalizable");

    let crate_info =
        get_crate_for_file(&canonical, &crates).expect("cargo.rs should be found in a crate");

    assert_eq!(
        crate_info.name, "tethys",
        "cargo.rs should be in tethys crate"
    );

    let module_path = compute_module_path(&canonical, crate_info)
        .expect("compute_module_path should succeed for cargo.rs");

    assert_eq!(
        module_path, "crate::cargo",
        "cargo.rs should have module_path 'crate::cargo'"
    );
}

/// Test that files outside crates have empty module paths.
#[test]
fn files_outside_module_tree_have_no_module_path() {
    use std::path::Path;

    let crate_info = CrateInfo {
        name: "test".to_string(),
        path: PathBuf::from("/workspace/crates/test"),
        lib_path: Some(PathBuf::from("src/lib.rs")),
        bin_paths: vec![],
    };

    // File in examples/ is outside module tree
    let result = compute_module_path(
        Path::new("/workspace/crates/test/examples/demo.rs"),
        &crate_info,
    );
    assert_eq!(result, None, "examples should not have module paths");

    // File in tests/ is outside module tree
    let result = compute_module_path(
        Path::new("/workspace/crates/test/tests/integration.rs"),
        &crate_info,
    );
    assert_eq!(result, None, "tests should not have module paths");

    // File in benches/ is outside module tree
    let result = compute_module_path(
        Path::new("/workspace/crates/test/benches/bench.rs"),
        &crate_info,
    );
    assert_eq!(result, None, "benches should not have module paths");
}
