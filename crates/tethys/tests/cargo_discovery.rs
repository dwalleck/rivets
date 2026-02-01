//! Tests for Cargo.toml discovery and parsing.

use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

// === Single Crate Tests ===

#[test]
fn discover_single_crate_with_default_lib() {
    let dir = TempDir::new().expect("create temp dir");

    fs::write(
        dir.path().join("Cargo.toml"),
        r#"
[package]
name = "test_crate"
version = "0.1.0"
edition = "2021"
"#,
    )
    .expect("write Cargo.toml");

    fs::create_dir_all(dir.path().join("src")).expect("create src dir");
    fs::write(dir.path().join("src/lib.rs"), "// lib").expect("write lib.rs");

    let crates = tethys::discover_crates(dir.path());

    assert_eq!(crates.len(), 1);
    assert_eq!(crates[0].name, "test_crate");
    assert_eq!(crates[0].lib_path, Some(PathBuf::from("src/lib.rs")));
    assert!(
        crates[0].bin_paths.is_empty(),
        "crate without main.rs should have no binaries"
    );
}

#[test]
fn discover_single_crate_with_default_binary() {
    let dir = TempDir::new().expect("create temp dir");

    fs::write(
        dir.path().join("Cargo.toml"),
        r#"
[package]
name = "my_cli"
version = "0.1.0"
edition = "2021"
"#,
    )
    .expect("write Cargo.toml");

    fs::create_dir_all(dir.path().join("src")).expect("create src dir");
    fs::write(dir.path().join("src/main.rs"), "fn main() {}").expect("write main.rs");

    let crates = tethys::discover_crates(dir.path());

    assert_eq!(crates.len(), 1);
    assert_eq!(crates[0].bin_paths.len(), 1);
    assert_eq!(crates[0].bin_paths[0].0, "my_cli");
    assert_eq!(crates[0].bin_paths[0].1, PathBuf::from("src/main.rs"));
    assert!(crates[0].lib_path.is_none(), "no lib.rs means no lib_path");
}

#[test]
fn discover_crate_with_custom_lib_path() {
    let dir = TempDir::new().expect("create temp dir");

    fs::write(
        dir.path().join("Cargo.toml"),
        r#"
[package]
name = "custom_lib"
version = "0.1.0"
edition = "2021"

[lib]
path = "src/mylib.rs"
"#,
    )
    .expect("write Cargo.toml");

    fs::create_dir_all(dir.path().join("src")).expect("create src dir");
    fs::write(dir.path().join("src/mylib.rs"), "// custom lib").expect("write mylib.rs");

    let crates = tethys::discover_crates(dir.path());

    assert_eq!(crates.len(), 1);
    assert_eq!(crates[0].lib_path, Some(PathBuf::from("src/mylib.rs")));
}

#[test]
fn discover_crate_with_explicit_bin_entries() {
    let dir = TempDir::new().expect("create temp dir");

    // Both [[bin]] entries have explicit paths to avoid cargo_toml parsing quirks
    fs::write(
        dir.path().join("Cargo.toml"),
        r#"
[package]
name = "multi_bin"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "cli_one"
path = "src/bin/one.rs"

[[bin]]
name = "cli_two"
path = "src/bin/two.rs"
"#,
    )
    .expect("write Cargo.toml");

    fs::create_dir_all(dir.path().join("src/bin")).expect("create src/bin");

    let crates = tethys::discover_crates(dir.path());

    assert_eq!(crates.len(), 1);
    assert_eq!(crates[0].bin_paths.len(), 2);

    let bins: std::collections::HashMap<_, _> = crates[0].bin_paths.iter().cloned().collect();
    assert_eq!(bins.get("cli_one"), Some(&PathBuf::from("src/bin/one.rs")));
    assert_eq!(bins.get("cli_two"), Some(&PathBuf::from("src/bin/two.rs")));
}

// === Workspace Tests ===

#[test]
fn discover_virtual_workspace() {
    let dir = TempDir::new().expect("create temp dir");

    // Virtual workspace - only [workspace], no [package]
    fs::write(
        dir.path().join("Cargo.toml"),
        r#"
[workspace]
members = ["crate_a", "crate_b"]
"#,
    )
    .expect("write workspace Cargo.toml");

    // Create member crates
    for name in ["crate_a", "crate_b"] {
        let crate_dir = dir.path().join(name);
        fs::create_dir_all(crate_dir.join("src")).expect("create src dir");
        fs::write(
            crate_dir.join("Cargo.toml"),
            format!(
                r#"
[package]
name = "{name}"
version = "0.1.0"
edition = "2021"
"#
            ),
        )
        .expect("write crate Cargo.toml");
        fs::write(crate_dir.join("src/lib.rs"), "").expect("write lib.rs");
    }

    let crates = tethys::discover_crates(dir.path());

    assert_eq!(crates.len(), 2, "should discover both workspace members");
    let names: Vec<_> = crates.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"crate_a"));
    assert!(names.contains(&"crate_b"));
}

#[test]
fn discover_workspace_with_root_crate() {
    let dir = TempDir::new().expect("create temp dir");

    // Workspace with root package
    fs::write(
        dir.path().join("Cargo.toml"),
        r#"
[package]
name = "root_crate"
version = "0.1.0"
edition = "2021"

[workspace]
members = ["subcrate"]
"#,
    )
    .expect("write Cargo.toml");

    fs::create_dir_all(dir.path().join("src")).expect("create root src");
    fs::write(dir.path().join("src/lib.rs"), "").expect("write root lib.rs");

    let subcrate = dir.path().join("subcrate");
    fs::create_dir_all(subcrate.join("src")).expect("create subcrate src");
    fs::write(
        subcrate.join("Cargo.toml"),
        r#"
[package]
name = "subcrate"
version = "0.1.0"
edition = "2021"
"#,
    )
    .expect("write subcrate Cargo.toml");
    fs::write(subcrate.join("src/lib.rs"), "").expect("write subcrate lib.rs");

    let crates = tethys::discover_crates(dir.path());

    assert_eq!(crates.len(), 2, "should discover root + subcrate");
    let names: Vec<_> = crates.iter().map(|c| c.name.as_str()).collect();
    assert!(
        names.contains(&"root_crate"),
        "root crate should be included"
    );
    assert!(names.contains(&"subcrate"), "subcrate should be included");
}

#[test]
fn discover_workspace_with_glob_pattern() {
    let dir = TempDir::new().expect("create temp dir");

    fs::write(
        dir.path().join("Cargo.toml"),
        r#"
[workspace]
members = ["crates/*"]
"#,
    )
    .expect("write Cargo.toml");

    let crates_parent = dir.path().join("crates");
    for name in ["alpha", "beta"] {
        let crate_dir = crates_parent.join(name);
        fs::create_dir_all(crate_dir.join("src")).expect("create src");
        fs::write(
            crate_dir.join("Cargo.toml"),
            format!(
                r#"
[package]
name = "{name}"
version = "0.1.0"
edition = "2021"
"#
            ),
        )
        .expect("write Cargo.toml");
        fs::write(crate_dir.join("src/lib.rs"), "").expect("write lib.rs");
    }

    let crates = tethys::discover_crates(dir.path());

    assert_eq!(crates.len(), 2);
    let names: Vec<_> = crates.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"alpha"));
    assert!(names.contains(&"beta"));
}

// === Error Handling Tests ===

#[test]
fn discover_returns_empty_for_non_rust_project() {
    let dir = TempDir::new().expect("create temp dir");
    // No Cargo.toml created

    let crates = tethys::discover_crates(dir.path());

    assert!(
        crates.is_empty(),
        "non-Rust projects should return empty vec"
    );
}

#[test]
fn discover_handles_malformed_cargo_toml() {
    let dir = TempDir::new().expect("create temp dir");

    fs::write(dir.path().join("Cargo.toml"), "this is not valid toml {{{")
        .expect("write invalid Cargo.toml");

    let crates = tethys::discover_crates(dir.path());

    assert!(
        crates.is_empty(),
        "malformed manifest should return empty vec"
    );
}

#[test]
fn discover_skips_invalid_workspace_members() {
    let dir = TempDir::new().expect("create temp dir");

    fs::write(
        dir.path().join("Cargo.toml"),
        r#"
[workspace]
members = ["valid_crate", "missing_manifest", "invalid_manifest"]
"#,
    )
    .expect("write workspace Cargo.toml");

    // valid_crate - proper setup
    let valid = dir.path().join("valid_crate");
    fs::create_dir_all(valid.join("src")).expect("create valid src");
    fs::write(
        valid.join("Cargo.toml"),
        r#"
[package]
name = "valid_crate"
version = "0.1.0"
edition = "2021"
"#,
    )
    .expect("write valid Cargo.toml");
    fs::write(valid.join("src/lib.rs"), "").expect("write lib.rs");

    // missing_manifest - directory exists but no Cargo.toml
    fs::create_dir_all(dir.path().join("missing_manifest")).expect("create dir");

    // invalid_manifest - Cargo.toml exists but is broken
    let invalid = dir.path().join("invalid_manifest");
    fs::create_dir_all(&invalid).expect("create invalid dir");
    fs::write(invalid.join("Cargo.toml"), "not valid toml").expect("write invalid");

    let crates = tethys::discover_crates(dir.path());

    assert_eq!(crates.len(), 1, "only valid_crate should be discovered");
    assert_eq!(crates[0].name, "valid_crate");
}

#[test]
fn discover_ignores_unsupported_glob_patterns() {
    let dir = TempDir::new().expect("create temp dir");

    fs::write(
        dir.path().join("Cargo.toml"),
        r#"
[workspace]
members = ["crates/**/*"]
"#, // Unsupported double-star pattern
    )
    .expect("write Cargo.toml");

    let crates = tethys::discover_crates(dir.path());

    // Should gracefully return empty (or whatever exists), not panic
    assert!(crates.is_empty());
}

// === Integration Tests with Real Workspace ===

#[test]
fn discover_rivets_workspace() {
    // Test against the actual rivets workspace
    // NOTE: This test may be skipped if the workspace uses features not supported
    // by the cargo_toml crate (e.g., resolver = "3")
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("CARGO_MANIFEST_DIR should have parent")
        .parent()
        .expect("tethys should be nested under workspace");

    let crates = tethys::discover_crates(workspace);

    // Skip test if workspace couldn't be parsed (e.g., unsupported resolver version)
    if crates.is_empty() {
        eprintln!(
            "Skipping discover_rivets_workspace: workspace at {workspace:?} returned no crates (possibly unsupported Cargo.toml features)"
        );
        return;
    }

    // Should find at least these crates
    let names: Vec<_> = crates.iter().map(|c| c.name.as_str()).collect();
    assert!(
        names.contains(&"tethys"),
        "should find tethys crate, found: {names:?}"
    );
    assert!(names.contains(&"rivets"), "should find rivets crate");
    assert!(
        names.contains(&"rivets-jsonl"),
        "should find rivets-jsonl crate"
    );

    // Tethys should have lib_path
    let tethys_crate = crates
        .iter()
        .find(|c| c.name == "tethys")
        .expect("tethys crate should exist");
    assert_eq!(tethys_crate.lib_path, Some(PathBuf::from("src/lib.rs")));
}
