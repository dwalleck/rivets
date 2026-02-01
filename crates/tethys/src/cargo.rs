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
}
