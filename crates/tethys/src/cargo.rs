//! Cargo.toml discovery and parsing.
//!
//! This module handles discovering Rust crate structure by parsing
//! Cargo.toml manifest files. It supports workspaces, single crates,
//! and virtual workspaces.

use std::path::{Path, PathBuf};

use cargo_toml::Manifest;
use tracing::{debug, warn};

use crate::CrateInfo;

/// Discover all crates in a workspace by parsing Cargo.toml files.
///
/// Handles three cases:
/// 1. Virtual workspace - `[workspace]` without `[package]`
/// 2. Workspace with root crate - Both `[workspace]` and `[package]`
/// 3. Single crate - Just `[package]`, no workspace
///
/// Returns empty vec if:
/// - No Cargo.toml found (non-Rust project)
/// - Cargo.toml cannot be parsed (malformed manifest)
///
/// Individual workspace members that fail to parse are skipped with a warning log.
pub fn discover_crates(workspace_root: &Path) -> Vec<CrateInfo> {
    let manifest_path = workspace_root.join("Cargo.toml");

    let manifest = match Manifest::from_path(&manifest_path) {
        Ok(m) => m,
        Err(e) => {
            debug!(
                path = %manifest_path.display(),
                error = %e,
                "No valid Cargo.toml found, treating as non-Rust project"
            );
            return Vec::new();
        }
    };

    let mut crates = Vec::new();

    if let Some(workspace) = &manifest.workspace {
        for member in &workspace.members {
            let member_path = workspace_root.join(member);

            if member.contains('*') {
                match glob_member(workspace_root, member) {
                    Ok(entries) => {
                        for entry in entries {
                            if let Some(info) = parse_crate(&entry) {
                                crates.push(info);
                            }
                        }
                    }
                    Err(e) => {
                        warn!(
                            workspace = %workspace_root.display(),
                            pattern = member,
                            error = %e,
                            "Failed to expand workspace member glob pattern"
                        );
                    }
                }
            } else if let Some(info) = parse_crate(&member_path) {
                crates.push(info);
            }
        }
    }

    if manifest.package.is_some() {
        if let Some(info) = parse_crate_from_manifest(workspace_root, &manifest) {
            crates.push(info);
        }
    }

    debug!(
        workspace = %workspace_root.display(),
        crate_count = crates.len(),
        "Discovered crates"
    );

    crates
}

/// Parse a single crate's Cargo.toml.
///
/// Returns `None` if:
/// - Cargo.toml doesn't exist
/// - Cargo.toml cannot be parsed
/// - Manifest has no `[package]` section
///
/// Errors are logged before returning `None`.
fn parse_crate(crate_path: &Path) -> Option<CrateInfo> {
    let manifest_path = crate_path.join("Cargo.toml");
    let manifest = match Manifest::from_path(&manifest_path) {
        Ok(m) => m,
        Err(e) => {
            warn!(
                path = %manifest_path.display(),
                error = %e,
                "Failed to parse crate manifest, skipping"
            );
            return None;
        }
    };
    parse_crate_from_manifest(crate_path, &manifest)
}

/// Extract `CrateInfo` from a parsed manifest.
fn parse_crate_from_manifest(crate_path: &Path, manifest: &Manifest) -> Option<CrateInfo> {
    let Some(package) = manifest.package.as_ref() else {
        debug!(
            path = %crate_path.display(),
            "Manifest has no [package] section, skipping (likely virtual workspace root)"
        );
        return None;
    };

    // Canonicalize crate path for consistent matching in get_crate_for_file()
    let crate_path = crate_path.canonicalize().unwrap_or_else(|e| {
        debug!(
            path = %crate_path.display(),
            error = %e,
            "Failed to canonicalize crate path, using original"
        );
        crate_path.to_path_buf()
    });

    let lib_path = if let Some(lib) = &manifest.lib {
        lib.path.as_ref().map(PathBuf::from)
    } else {
        let default_lib = crate_path.join("src/lib.rs");
        if default_lib.exists() {
            Some(PathBuf::from("src/lib.rs"))
        } else {
            None
        }
    };

    let mut bin_paths = Vec::new();

    // Explicit [[bin]] entries. When path is unset, uses Cargo's convention: src/bin/{name}.rs.
    // When name is also unset, defaults to the package name.
    for bin in &manifest.bin {
        let name = bin.name.clone().unwrap_or_else(|| package.name.clone());
        let path = if let Some(explicit_path) = bin.path.as_ref() {
            PathBuf::from(explicit_path)
        } else {
            let inferred = PathBuf::from(format!("src/bin/{name}.rs"));
            let full_path = crate_path.join(&inferred);
            if !full_path.exists() {
                debug!(
                    crate_path = %crate_path.display(),
                    bin_name = %name,
                    inferred_path = %inferred.display(),
                    "[[bin]] entry has no path and inferred location doesn't exist"
                );
            }
            inferred
        };
        bin_paths.push((name, path));
    }

    if bin_paths.is_empty() {
        let default_main = crate_path.join("src/main.rs");
        if default_main.exists() {
            bin_paths.push((package.name.clone(), PathBuf::from("src/main.rs")));
        }
    }

    Some(CrateInfo {
        name: package.name.clone(),
        path: crate_path,
        lib_path,
        bin_paths,
    })
}

/// Compute the Rust module path for a file within a crate.
///
/// Given a file path and crate info, computes the module path that would be used
/// to reference items in that file (e.g., `crate::db::query` for `src/db/query.rs`).
///
/// Returns `None` if the file is not within the crate's module tree
/// (e.g., examples, benches, tests, or files outside the src directory).
///
/// # Module Path Rules
///
/// - Library files use `crate` as the root prefix
/// - Binary files use the binary name (with hyphens converted to underscores)
/// - `lib.rs` and `main.rs` map to just the prefix (no additional segments)
/// - `mod.rs` files use the parent directory name as their module
/// - Regular `.rs` files use their file stem as the module name
///
/// # Examples
///
/// | File Path | Module Path |
/// |-----------|-------------|
/// | `src/lib.rs` | `crate` |
/// | `src/main.rs` | `{crate_name}` |
/// | `src/db.rs` | `crate::db` |
/// | `src/db/mod.rs` | `crate::db` |
/// | `src/db/query.rs` | `crate::db::query` |
/// | `src/bin/cli/main.rs` | `cli` |
/// | `examples/demo.rs` | `None` |
#[must_use]
pub fn compute_module_path(file_path: &Path, crate_info: &CrateInfo) -> Option<String> {
    let (entry_dir, prefix) = determine_entry_point(file_path, crate_info)?;
    let relative = file_path.strip_prefix(&entry_dir).ok()?;

    let mut segments = vec![prefix];

    if let Some(parent) = relative.parent() {
        for component in parent.components() {
            if let std::path::Component::Normal(name) = component {
                let Some(name_str) = name.to_str() else {
                    debug!(
                        file = %file_path.display(),
                        component = ?name,
                        "Path component is not valid UTF-8, cannot compute module path"
                    );
                    return None;
                };
                segments.push(name_str.to_string());
            }
        }
    }

    let Some(file_stem) = file_path.file_stem() else {
        debug!(file = %file_path.display(), "File has no stem");
        return None;
    };
    let Some(file_stem_str) = file_stem.to_str() else {
        debug!(
            file = %file_path.display(),
            stem = ?file_stem,
            "File stem is not valid UTF-8"
        );
        return None;
    };

    match file_stem_str {
        "mod" | "lib" | "main" => {}
        _ => segments.push(file_stem_str.to_string()),
    }

    Some(segments.join("::"))
}

/// Determine the entry point directory and module prefix for a file.
///
/// Returns `(entry_directory, prefix)` where:
/// - `entry_directory` is the parent of lib.rs/main.rs (typically `src/`)
/// - `prefix` is `"crate"` for libraries or the binary name for binaries
///
/// Returns `None` if the file is not under any recognized entry point.
///
/// ## Priority Logic
///
/// 1. If file IS a binary entry point (exact match), use that binary
/// 2. If file is under a binary's unique directory (deeper than lib's src/), use that binary
/// 3. If file is under the library's src/ directory, use the library (`crate` prefix)
/// 4. Fall back to any binary whose entry directory contains the file
///
/// This means for a crate with both `src/lib.rs` and `src/main.rs`, files in `src/`
/// (other than `main.rs` itself) will use the library's `crate::` prefix.
fn determine_entry_point(file_path: &Path, crate_info: &CrateInfo) -> Option<(PathBuf, String)> {
    let lib_entry_dir = crate_info.lib_path.as_ref().and_then(|lib_path| {
        let lib_full = crate_info.path.join(lib_path);
        lib_full.parent().map(Path::to_path_buf)
    });

    for (bin_name, bin_path) in &crate_info.bin_paths {
        let bin_full = crate_info.path.join(bin_path);
        let Some(bin_entry_dir) = bin_full.parent() else {
            debug!(
                bin_name = %bin_name,
                bin_path = %bin_path.display(),
                "Binary path has no parent directory, skipping"
            );
            continue;
        };

        if file_path == bin_full {
            let prefix = bin_name.replace('-', "_");
            return Some((bin_entry_dir.to_path_buf(), prefix));
        }

        if file_path.starts_with(bin_entry_dir) {
            let bin_is_more_specific = lib_entry_dir.as_ref().is_none_or(|lib_dir| {
                bin_entry_dir.components().count() > lib_dir.components().count()
            });

            if bin_is_more_specific {
                let prefix = bin_name.replace('-', "_");
                return Some((bin_entry_dir.to_path_buf(), prefix));
            }
        }
    }

    if let Some(lib_dir) = lib_entry_dir {
        if file_path.starts_with(&lib_dir) {
            return Some((lib_dir, "crate".to_string()));
        }
    }

    for (bin_name, bin_path) in &crate_info.bin_paths {
        let bin_full = crate_info.path.join(bin_path);
        let Some(bin_entry_dir) = bin_full.parent() else {
            continue;
        };
        if file_path.starts_with(bin_entry_dir) {
            let prefix = bin_name.replace('-', "_");
            return Some((bin_entry_dir.to_path_buf(), prefix));
        }
    }

    None
}

/// Find which crate a file belongs to.
///
/// Returns a reference to the `CrateInfo` whose path is the longest prefix of the file path.
/// This handles overlapping crate paths correctly (e.g., `/workspace/crate` vs `/workspace/crate-utils`).
/// Returns `None` if the file is not within any known crate.
#[must_use]
pub fn get_crate_for_file<'a>(file_path: &Path, crates: &'a [CrateInfo]) -> Option<&'a CrateInfo> {
    crates
        .iter()
        .filter(|c| file_path.starts_with(&c.path))
        .max_by_key(|c| c.path.components().count())
}

/// Expand a simple glob pattern to matching directories.
///
/// Only supports `prefix/*` patterns (e.g., `"crates/*"`). Full glob support
/// (via the `glob` crate) could be added if needed, but workspace member
/// patterns in practice are typically simple.
///
/// # Errors
///
/// Returns an error if:
/// - The pattern is not in `prefix/*` format (unsupported pattern)
/// - Directory enumeration fails (I/O error)
fn glob_member(workspace_root: &Path, pattern: &str) -> std::io::Result<Vec<PathBuf>> {
    let mut results = Vec::new();

    let prefix = pattern.strip_suffix("/*").ok_or_else(|| {
        warn!(
            pattern = pattern,
            "Unsupported glob pattern, only 'prefix/*' supported"
        );
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("Unsupported glob pattern: {pattern}"),
        )
    })?;

    let search_dir = workspace_root.join(prefix);
    if search_dir.is_dir() {
        for entry in std::fs::read_dir(&search_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() && path.join("Cargo.toml").exists() {
                results.push(path);
            }
        }
    } else {
        debug!(
            search_dir = %search_dir.display(),
            pattern = pattern,
            "Glob pattern search directory does not exist"
        );
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[test]
    fn glob_member_expands_simple_pattern() {
        // This test uses the actual rivets workspace structure
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("CARGO_MANIFEST_DIR should have parent directory")
            .parent()
            .expect("tethys crate should be nested under workspace");

        let results = glob_member(workspace, "crates/*").expect("glob should work");

        // Should find at least tethys
        assert!(!results.is_empty());
        assert!(results.iter().any(|p| p.ends_with("tethys")));
    }

    // Helper to create CrateInfo for tests
    fn make_crate_info(name: &str, path: PathBuf, lib_path: Option<&str>) -> CrateInfo {
        CrateInfo {
            name: name.to_string(),
            path,
            lib_path: lib_path.map(PathBuf::from),
            bin_paths: Vec::new(),
        }
    }

    fn make_crate_info_with_bins(
        name: &str,
        path: PathBuf,
        lib_path: Option<&str>,
        bin_paths: Vec<(&str, &str)>,
    ) -> CrateInfo {
        CrateInfo {
            name: name.to_string(),
            path,
            lib_path: lib_path.map(PathBuf::from),
            bin_paths: bin_paths
                .into_iter()
                .map(|(n, p)| (n.to_string(), PathBuf::from(p)))
                .collect(),
        }
    }

    #[rstest]
    #[case::lib_rs("/workspace/my-crate/src/lib.rs", "crate")]
    #[case::main_rs("/workspace/my-crate/src/main.rs", "my_crate")]
    #[case::nested_module("/workspace/my-crate/src/db/query.rs", "crate::db::query")]
    #[case::mod_rs_style("/workspace/my-crate/src/db/mod.rs", "crate::db")]
    #[case::deeply_nested("/workspace/my-crate/src/a/b/c/d.rs", "crate::a::b::c::d")]
    fn compute_module_path_lib_crate(#[case] file_path: &str, #[case] expected: &str) {
        let crate_info = make_crate_info_with_bins(
            "my-crate",
            PathBuf::from("/workspace/my-crate"),
            Some("src/lib.rs"),
            vec![("my-crate", "src/main.rs")],
        );

        let result = compute_module_path(Path::new(file_path), &crate_info);
        assert_eq!(result, Some(expected.to_string()));
    }

    #[rstest]
    #[case::bin_main("/workspace/my-crate/src/bin/cli/main.rs", "cli", "cli")]
    #[case::bin_submodule("/workspace/my-crate/src/bin/cli/commands.rs", "cli", "cli::commands")]
    #[case::bin_nested("/workspace/my-crate/src/bin/cli/cmd/list.rs", "cli", "cli::cmd::list")]
    fn compute_module_path_binary(
        #[case] file_path: &str,
        #[case] bin_name: &str,
        #[case] expected: &str,
    ) {
        let crate_info = make_crate_info_with_bins(
            "my-crate",
            PathBuf::from("/workspace/my-crate"),
            Some("src/lib.rs"),
            vec![(bin_name, &format!("src/bin/{bin_name}/main.rs"))],
        );

        let result = compute_module_path(Path::new(file_path), &crate_info);
        assert_eq!(result, Some(expected.to_string()));
    }

    #[test]
    fn compute_module_path_hyphenated_crate_name() {
        // Hyphens in crate names become underscores in module paths
        let crate_info = make_crate_info_with_bins(
            "my-cool-crate",
            PathBuf::from("/workspace/my-cool-crate"),
            None,
            vec![("my-cool-crate", "src/main.rs")],
        );

        let result = compute_module_path(
            Path::new("/workspace/my-cool-crate/src/main.rs"),
            &crate_info,
        );
        assert_eq!(result, Some("my_cool_crate".to_string()));
    }

    #[rstest]
    #[case::examples("/workspace/my-crate/examples/demo.rs")]
    #[case::benches("/workspace/my-crate/benches/perf.rs")]
    #[case::tests("/workspace/my-crate/tests/integration.rs")]
    #[case::outside_src("/workspace/my-crate/build.rs")]
    fn compute_module_path_returns_none_outside_module_tree(#[case] file_path: &str) {
        let crate_info = make_crate_info(
            "my-crate",
            PathBuf::from("/workspace/my-crate"),
            Some("src/lib.rs"),
        );

        let result = compute_module_path(Path::new(file_path), &crate_info);
        assert_eq!(result, None);
    }

    #[test]
    fn get_crate_for_file_finds_matching_crate() {
        let crates = vec![
            CrateInfo {
                name: "crate_a".to_string(),
                path: PathBuf::from("/workspace/crates/a"),
                lib_path: Some(PathBuf::from("src/lib.rs")),
                bin_paths: vec![],
            },
            CrateInfo {
                name: "crate_b".to_string(),
                path: PathBuf::from("/workspace/crates/b"),
                lib_path: Some(PathBuf::from("src/lib.rs")),
                bin_paths: vec![],
            },
        ];

        let result = get_crate_for_file(Path::new("/workspace/crates/b/src/lib.rs"), &crates);
        assert!(result.is_some(), "should find matching crate");
        assert_eq!(result.expect("already checked").name, "crate_b");
    }

    #[test]
    fn get_crate_for_file_returns_none_for_no_match() {
        let crates = vec![CrateInfo {
            name: "my_crate".to_string(),
            path: PathBuf::from("/workspace/my_crate"),
            lib_path: Some(PathBuf::from("src/lib.rs")),
            bin_paths: vec![],
        }];

        let result = get_crate_for_file(Path::new("/other/path/file.rs"), &crates);
        assert!(result.is_none(), "should not find crate for unrelated path");
    }

    #[test]
    fn get_crate_for_file_prefers_longest_prefix_match() {
        let crates = vec![
            CrateInfo {
                name: "crate".to_string(),
                path: PathBuf::from("/workspace/crate"),
                lib_path: Some(PathBuf::from("src/lib.rs")),
                bin_paths: vec![],
            },
            CrateInfo {
                name: "crate-utils".to_string(),
                path: PathBuf::from("/workspace/crate-utils"),
                lib_path: Some(PathBuf::from("src/lib.rs")),
                bin_paths: vec![],
            },
        ];

        // File in crate-utils should match crate-utils, not crate
        let result = get_crate_for_file(Path::new("/workspace/crate-utils/src/lib.rs"), &crates);
        assert!(result.is_some(), "should find matching crate");
        assert_eq!(
            result.expect("already checked").name,
            "crate-utils",
            "should prefer longer path prefix"
        );

        // File in crate should still match crate
        let result = get_crate_for_file(Path::new("/workspace/crate/src/lib.rs"), &crates);
        assert!(result.is_some(), "should find matching crate");
        assert_eq!(
            result.expect("already checked").name,
            "crate",
            "should match exact prefix"
        );
    }

    #[rstest]
    #[case::shared_lib_rs("/workspace/my-crate/src/lib.rs", "crate")]
    #[case::shared_main_rs("/workspace/my-crate/src/main.rs", "my_crate")]
    #[case::shared_nested_from_lib("/workspace/my-crate/src/db.rs", "crate::db")]
    #[case::shared_deeply_nested("/workspace/my-crate/src/db/query.rs", "crate::db::query")]
    fn compute_module_path_shared_src_directory(#[case] file_path: &str, #[case] expected: &str) {
        // Crate with both lib.rs and main.rs in src/ - lib takes priority for shared files
        let crate_info = make_crate_info_with_bins(
            "my-crate",
            PathBuf::from("/workspace/my-crate"),
            Some("src/lib.rs"),
            vec![("my-crate", "src/main.rs")],
        );

        let result = compute_module_path(Path::new(file_path), &crate_info);
        assert_eq!(
            result,
            Some(expected.to_string()),
            "lib.rs should take priority for shared src/ directory files"
        );
    }

    #[rstest]
    #[case::bin_only_main("/workspace/my-bin/src/main.rs", "my_bin")]
    #[case::bin_only_nested("/workspace/my-bin/src/commands.rs", "my_bin::commands")]
    #[case::bin_only_deeply_nested("/workspace/my-bin/src/cli/args.rs", "my_bin::cli::args")]
    #[case::bin_only_mod_rs("/workspace/my-bin/src/cli/mod.rs", "my_bin::cli")]
    fn compute_module_path_binary_only_crate(#[case] file_path: &str, #[case] expected: &str) {
        // Binary-only crate (no lib.rs)
        let crate_info = make_crate_info_with_bins(
            "my-bin",
            PathBuf::from("/workspace/my-bin"),
            None,
            vec![("my-bin", "src/main.rs")],
        );

        let result = compute_module_path(Path::new(file_path), &crate_info);
        assert_eq!(
            result,
            Some(expected.to_string()),
            "binary-only crate should use binary name as module root"
        );
    }
}
