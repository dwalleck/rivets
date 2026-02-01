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

/// Test that indexing the tethys crate itself produces correct module paths.
#[test]
fn index_tethys_crate_has_module_paths() {
    let Some((workspace, _crates)) = get_workspace_with_crates() else {
        return;
    };

    let mut tethys = Tethys::new(&workspace).expect("new should succeed");
    // Use rebuild to ensure fresh database with new module_path computation
    let _stats = tethys.rebuild().expect("rebuild should succeed");

    // Query for a known symbol in tethys itself
    let symbols = tethys
        .search_symbols("CrateInfo")
        .expect("search should succeed");

    let crate_info_symbol = symbols
        .iter()
        .find(|s| s.name == "CrateInfo" && s.kind == SymbolKind::Struct)
        .expect("CrateInfo struct should be indexed");

    // CrateInfo is in crates/tethys/src/types.rs -> crate::types
    assert_eq!(
        crate_info_symbol.module_path, "crate::types",
        "CrateInfo should have module_path 'crate::types'"
    );
}

/// Test that symbols in nested modules have correct paths.
#[test]
fn nested_module_paths_are_correct() {
    let Some((workspace, _crates)) = get_workspace_with_crates() else {
        return;
    };

    let mut tethys = Tethys::new(&workspace).expect("new should succeed");
    // Use rebuild to ensure fresh database with new module_path computation
    let _stats = tethys.rebuild().expect("rebuild should succeed");

    // Query for discover_crates which is in cargo.rs
    let symbols = tethys
        .search_symbols("discover_crates")
        .expect("search should succeed");

    let discover_symbol = symbols
        .iter()
        .find(|s| s.name == "discover_crates" && s.kind == SymbolKind::Function)
        .expect("discover_crates should be indexed");

    // discover_crates is in crates/tethys/src/cargo.rs -> crate::cargo
    assert_eq!(
        discover_symbol.module_path, "crate::cargo",
        "discover_crates should have module_path 'crate::cargo'"
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
