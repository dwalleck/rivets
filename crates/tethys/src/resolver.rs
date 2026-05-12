//! Module path resolution for Rust source files.
//!
//! Maps use statement paths to actual file paths within the workspace.
//! Handles `crate::` / `self::` / `super::` prefixes plus paths starting
//! with a known workspace-crate name (Rust 2018+ idiom). Paths starting
//! with an external crate name return `None` since we can't analyze
//! external code.

use std::path::{Path, PathBuf};

use crate::types::CrateInfo;

/// Resolve a module path to a file path within the workspace.
///
/// # Arguments
/// * `path` - Module path segments (e.g., `["crate", "auth"]` or
///   `["rivets", "storage", "in_memory"]`)
/// * `current_file` - Path to the file containing the use statement
/// * `crate_root` - Root of the *current* file's crate (usually `src/` directory)
/// * `workspace_crates` - All discovered crates in the workspace. When `path[0]`
///   matches a `CrateInfo::name` (with `-` → `_` normalization to convert Cargo
///   manifest names to Rust module names): a single-segment path resolves to
///   the target crate's entry-point file (`lib_path` or first bin; `None` if
///   neither is set); a multi-segment path recurses into that crate's `src/`
///   as the new `crate_root`.
///
/// # Returns
/// * `Some(PathBuf)` - Resolved file path within the workspace
/// * `None` - External crate or unresolvable path
#[must_use]
pub fn resolve_module_path(
    path: &[String],
    current_file: &Path,
    crate_root: &Path,
    workspace_crates: &[CrateInfo],
) -> Option<PathBuf> {
    if path.is_empty() {
        return None;
    }

    match path[0].as_str() {
        "crate" => resolve_crate_path(&path[1..], crate_root),
        "self" => resolve_self_path(&path[1..], current_file),
        "super" => resolve_super_path(&path[1..], current_file),
        head => {
            // Rust 2018+: workspace-crate prefix routes into that crate's src/.
            // External crates aren't in the list, so `?` returns None for them.
            let target = workspace_crates
                .iter()
                .find(|c| c.name.replace('-', "_") == head)?;

            // Single-segment path (e.g. `use rivets;`) refers to the crate
            // itself, which on disk is the entry-point file — not the src/ dir.
            // Filter on `.exists()` to mirror `resolve_as_module`'s guarantee
            // that returned paths exist on disk.
            if path.len() == 1 {
                return target
                    .lib_path
                    .as_ref()
                    .or_else(|| target.bin_paths.first().map(|(_, p)| p))
                    .map(|p| target.path.join(p))
                    .filter(|p| p.exists());
            }

            let other_src = target.src_root();
            resolve_crate_path(&path[1..], &other_src)
        }
    }
}

/// Try to resolve a path as a .rs file or directory with mod.rs.
///
/// Returns `None` if neither variant exists on disk, avoiding phantom dependencies.
fn resolve_as_module(path: &Path) -> Option<PathBuf> {
    // Try as a .rs file first
    let rs_path = path.with_extension("rs");
    if rs_path.exists() {
        return Some(rs_path);
    }

    // Try as a directory with mod.rs
    let mod_rs = path.join("mod.rs");
    if mod_rs.exists() {
        return Some(mod_rs);
    }

    // Neither variant exists - return None instead of a phantom path
    None
}

/// Resolve a crate-relative path.
fn resolve_crate_path(path: &[String], crate_root: &Path) -> Option<PathBuf> {
    if path.is_empty() {
        return Some(crate_root.to_path_buf());
    }

    // Build the path from crate root
    let mut result = crate_root.to_path_buf();
    for segment in path {
        result.push(segment);
    }

    resolve_as_module(&result)
}

/// Resolve a self-relative path (sibling module).
fn resolve_self_path(path: &[String], current_file: &Path) -> Option<PathBuf> {
    let current_dir = current_file.parent()?;

    if path.is_empty() {
        return Some(current_file.to_path_buf());
    }

    let mut result = current_dir.to_path_buf();
    for segment in path {
        result.push(segment);
    }

    resolve_as_module(&result)
}

/// Resolve a super-relative path (parent module).
fn resolve_super_path(path: &[String], current_file: &Path) -> Option<PathBuf> {
    let current_dir = current_file.parent()?;
    let parent_dir = current_dir.parent()?;

    if path.is_empty() {
        // super refers to the parent module's file
        // Could be parent_dir/mod.rs or the parent file itself
        let mod_rs = current_dir.join("mod.rs");
        if mod_rs.exists() && mod_rs != current_file {
            return Some(mod_rs);
        }
        // Look for parent's mod.rs or *.rs file
        let parent_mod = parent_dir.join("mod.rs");
        if parent_mod.exists() {
            return Some(parent_mod);
        }
        // The parent directory name as a .rs file
        let dir_name = current_dir.file_name()?.to_str()?;
        let parent_file = parent_dir.join(dir_name).with_extension("rs");
        if parent_file.exists() {
            return Some(parent_file);
        }
        return None;
    }

    let mut result = parent_dir.to_path_buf();
    for segment in path {
        result.push(segment);
    }

    resolve_as_module(&result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_workspace() -> TempDir {
        let dir = tempfile::tempdir().expect("should create temp directory");
        let src = dir.path().join("src");
        fs::create_dir_all(&src).expect("should create src directory");

        // Create typical Rust project structure
        fs::write(src.join("lib.rs"), "mod auth;\nmod config;").expect("should write lib.rs");
        fs::write(src.join("config.rs"), "pub struct Config {}").expect("should write config.rs");

        // Create auth as a directory module
        let auth_dir = src.join("auth");
        fs::create_dir_all(&auth_dir).expect("should create auth directory");
        fs::write(auth_dir.join("mod.rs"), "pub struct Authenticator {}")
            .expect("should write auth/mod.rs");
        fs::write(auth_dir.join("middleware.rs"), "pub fn check() {}")
            .expect("should write auth/middleware.rs");

        dir
    }

    #[test]
    fn resolves_crate_path_to_file() {
        let dir = create_test_workspace();
        let crate_root = dir.path().join("src");
        let current = crate_root.join("lib.rs");

        let path = vec!["crate".to_string(), "config".to_string()];
        let result = resolve_module_path(&path, &current, &crate_root, &[]);

        assert!(result.is_some());
        let resolved = result.unwrap();
        assert!(resolved.ends_with("config.rs"));
        assert!(resolved.exists());
    }

    #[test]
    fn resolves_crate_path_to_mod_rs() {
        let dir = create_test_workspace();
        let crate_root = dir.path().join("src");
        let current = crate_root.join("lib.rs");

        let path = vec!["crate".to_string(), "auth".to_string()];
        let result = resolve_module_path(&path, &current, &crate_root, &[]);

        assert!(result.is_some());
        let resolved = result.unwrap();
        assert!(resolved.ends_with("mod.rs"));
        assert!(resolved.exists());
    }

    #[test]
    fn resolves_self_path() {
        let dir = create_test_workspace();
        let crate_root = dir.path().join("src");
        let current = crate_root.join("auth").join("mod.rs");

        let path = vec!["self".to_string(), "middleware".to_string()];
        let result = resolve_module_path(&path, &current, &crate_root, &[]);

        assert!(result.is_some());
        let resolved = result.unwrap();
        assert!(resolved.ends_with("middleware.rs"));
        assert!(resolved.exists());
    }

    #[test]
    fn returns_none_for_external_crate() {
        let dir = create_test_workspace();
        let crate_root = dir.path().join("src");
        let current = crate_root.join("lib.rs");

        let path = vec!["serde".to_string(), "Serialize".to_string()];
        let result = resolve_module_path(&path, &current, &crate_root, &[]);

        assert!(result.is_none());
    }

    #[test]
    fn resolves_super_path() {
        let dir = create_test_workspace();
        let crate_root = dir.path().join("src");

        // Create nested module structure
        let inner = crate_root.join("auth").join("inner");
        fs::create_dir_all(&inner).expect("should create inner directory");
        fs::write(inner.join("mod.rs"), "use super::middleware;")
            .expect("should write inner/mod.rs");

        let current = inner.join("mod.rs");
        let path = vec!["super".to_string(), "middleware".to_string()];
        let result = resolve_module_path(&path, &current, &crate_root, &[]);

        assert!(result.is_some());
        let resolved = result.unwrap();
        assert!(resolved.ends_with("middleware.rs"));
    }

    #[test]
    fn empty_path_returns_none() {
        let dir = create_test_workspace();
        let crate_root = dir.path().join("src");
        let current = crate_root.join("lib.rs");

        let result = resolve_module_path(&[], &current, &crate_root, &[]);
        assert!(result.is_none());
    }

    /// A two-crate workspace where `caller_crate` imports from `target_crate`
    /// via the workspace-crate arm: `path[0]="target_crate"` must match the
    /// `CrateInfo` list and recurse into `target_crate/src/` as the new
    /// `crate_root`. Catches the "arm doesn't fire for a matching head" bug.
    #[test]
    fn resolves_workspace_crate_via_new_arm() {
        use crate::types::CrateInfo;

        let dir = tempfile::tempdir().expect("temp dir");

        // caller_crate only needs a src/lib.rs to exist.
        let caller_crate = dir.path().join("caller_crate");
        let caller_src = caller_crate.join("src");
        fs::create_dir_all(&caller_src).expect("caller src");
        fs::write(caller_src.join("lib.rs"), "").expect("caller lib.rs");

        // target_crate is what the import points at. `use target_crate::storage`
        // must resolve to target_crate/src/storage.rs.
        let target_crate = dir.path().join("target_crate");
        let target_src = target_crate.join("src");
        fs::create_dir_all(&target_src).expect("target src");
        fs::write(target_src.join("lib.rs"), "").expect("target lib.rs");
        fs::write(target_src.join("storage.rs"), "pub fn helper() {}").expect("target storage.rs");

        let crates = vec![
            CrateInfo {
                name: "caller_crate".to_string(),
                path: caller_crate.clone(),
                lib_path: Some(PathBuf::from("src/lib.rs")),
                bin_paths: vec![],
            },
            CrateInfo {
                name: "target_crate".to_string(),
                path: target_crate.clone(),
                lib_path: Some(PathBuf::from("src/lib.rs")),
                bin_paths: vec![],
            },
        ];

        let current = caller_src.join("lib.rs");
        let path = vec!["target_crate".to_string(), "storage".to_string()];
        let result = resolve_module_path(&path, &current, &caller_src, &crates);

        let resolved =
            result.expect("target_crate::storage should resolve to target_crate/src/storage.rs");
        assert!(
            resolved.ends_with("target_crate/src/storage.rs")
                || resolved.ends_with("target_crate\\src\\storage.rs"),
            "expected target_crate/src/storage.rs, got {resolved:?}"
        );
        assert!(resolved.exists(), "resolved path must exist on disk");
    }

    /// Build a multi-crate workspace tempdir with the given
    /// `(crate_name, extra_files)` pairs. Each `extra_files` entry is a
    /// path relative to that crate's `src/` directory.
    fn workspace_with_crates(
        crates: &[(&str, &[&str])],
    ) -> (TempDir, Vec<crate::types::CrateInfo>) {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut infos = Vec::new();
        for (name, extras) in crates {
            let crate_path = dir.path().join(name);
            let src = crate_path.join("src");
            fs::create_dir_all(&src).expect("crate src");
            fs::write(src.join("lib.rs"), "").expect("crate lib.rs");
            for relative in *extras {
                let full = src.join(relative);
                if let Some(parent) = full.parent() {
                    fs::create_dir_all(parent).expect("nested dir");
                }
                fs::write(&full, "").expect("nested file");
            }
            infos.push(crate::types::CrateInfo {
                name: (*name).to_string(),
                path: crate_path,
                lib_path: Some(PathBuf::from("src/lib.rs")),
                bin_paths: vec![],
            });
        }
        (dir, infos)
    }

    /// Multi-segment path through a workspace crate resolves to a deeply-nested
    /// file (not just the crate root's `lib.rs`). Catches the bug where the new
    /// arm hands off to `resolve_crate_path` but the latter can't reach files
    /// in subdirectories.
    #[test]
    fn workspace_crate_path_traverses_to_nested_file() {
        let (dir, crates) =
            workspace_with_crates(&[("caller", &[]), ("target", &["nested/deep/thing.rs"])]);
        let current = dir.path().join("caller/src/lib.rs");
        let path = vec![
            "target".to_string(),
            "nested".to_string(),
            "deep".to_string(),
            "thing".to_string(),
        ];
        let result = resolve_module_path(&path, &current, &dir.path().join("caller/src"), &crates);
        let resolved = result.expect("multi-segment workspace-crate path must resolve");
        assert!(
            resolved.ends_with("target/src/nested/deep/thing.rs")
                || resolved.ends_with("target\\src\\nested\\deep\\thing.rs"),
            "expected target/src/nested/deep/thing.rs, got {resolved:?}"
        );
    }

    /// With a non-empty `workspace_crates` list, an EXTERNAL crate head
    /// (`serde`) must still return `None`. Catches the bug where the new arm
    /// matches too eagerly (e.g., partial-name match, or always-`Some`).
    #[test]
    fn external_crate_returns_none_even_with_workspace_list() {
        let (dir, crates) = workspace_with_crates(&[("caller", &[]), ("target", &[])]);
        let current = dir.path().join("caller/src/lib.rs");
        let path = vec!["serde".to_string(), "Serialize".to_string()];
        let result = resolve_module_path(&path, &current, &dir.path().join("caller/src"), &crates);
        assert!(
            result.is_none(),
            "serde is not in workspace; new arm must not match it, got {result:?}"
        );
    }

    /// Single-segment path through the workspace-crate arm resolves to the
    /// entry-point file, not the `src/` directory. Without this, a `use foo;`
    /// import would feed a directory path into the dep-graph file table.
    #[test]
    fn single_segment_workspace_crate_resolves_to_entry_point_file() {
        let (dir, crates) = workspace_with_crates(&[("caller", &[]), ("target", &[])]);
        let current = dir.path().join("caller/src/lib.rs");
        let path = vec!["target".to_string()];
        let result = resolve_module_path(&path, &current, &dir.path().join("caller/src"), &crates);
        let resolved = result.expect("single-segment workspace-crate must resolve to entry point");
        assert!(
            resolved.ends_with("target/src/lib.rs") || resolved.ends_with("target\\src\\lib.rs"),
            "expected target/src/lib.rs, got {resolved:?}"
        );
    }

    /// Bin-only crate: when `lib_path` is `None`, a single-segment path must
    /// fall back to the first `bin_paths` entry. Locks down the `or_else`
    /// branch in the single-segment case.
    #[test]
    fn single_segment_falls_back_to_bin_when_lib_path_absent() {
        use crate::types::CrateInfo;

        let dir = tempfile::tempdir().expect("temp dir");
        let caller = dir.path().join("caller");
        fs::create_dir_all(caller.join("src")).expect("caller src");
        fs::write(caller.join("src/lib.rs"), "").expect("caller lib.rs");

        let bin_only = dir.path().join("bin_only");
        fs::create_dir_all(bin_only.join("src")).expect("bin_only src");
        fs::write(bin_only.join("src/main.rs"), "fn main() {}").expect("bin_only main.rs");

        let crates = vec![
            CrateInfo {
                name: "caller".to_string(),
                path: caller.clone(),
                lib_path: Some(PathBuf::from("src/lib.rs")),
                bin_paths: vec![],
            },
            CrateInfo {
                name: "bin_only".to_string(),
                path: bin_only.clone(),
                lib_path: None,
                bin_paths: vec![("bin_only".to_string(), PathBuf::from("src/main.rs"))],
            },
        ];

        let current = caller.join("src/lib.rs");
        let result = resolve_module_path(
            &["bin_only".to_string()],
            &current,
            &caller.join("src"),
            &crates,
        );
        let resolved = result.expect("bin-only single-segment must resolve to first bin path");
        assert!(
            resolved.ends_with("bin_only/src/main.rs")
                || resolved.ends_with("bin_only\\src\\main.rs"),
            "expected bin_only/src/main.rs, got {resolved:?}"
        );
    }

    /// Empty entry point: when both `lib_path` is `None` and `bin_paths` is
    /// empty, a single-segment path must return `None`. Locks down the
    /// invariant documented in `resolve_module_path`'s doc comment.
    #[test]
    fn single_segment_returns_none_when_no_entry_point() {
        use crate::types::CrateInfo;

        let dir = tempfile::tempdir().expect("temp dir");
        let caller = dir.path().join("caller");
        fs::create_dir_all(caller.join("src")).expect("caller src");
        fs::write(caller.join("src/lib.rs"), "").expect("caller lib.rs");

        let ghost = dir.path().join("ghost");
        fs::create_dir_all(&ghost).expect("ghost dir");

        let crates = vec![
            CrateInfo {
                name: "caller".to_string(),
                path: caller.clone(),
                lib_path: Some(PathBuf::from("src/lib.rs")),
                bin_paths: vec![],
            },
            CrateInfo {
                name: "ghost".to_string(),
                path: ghost,
                lib_path: None,
                bin_paths: vec![],
            },
        ];

        let current = caller.join("src/lib.rs");
        let result = resolve_module_path(
            &["ghost".to_string()],
            &current,
            &caller.join("src"),
            &crates,
        );
        assert!(
            result.is_none(),
            "single-segment must return None when neither lib_path nor bin_paths is set, got {result:?}"
        );
    }

    /// Self-reference: a file using its OWN crate's name in an import path
    /// (e.g. `use rivets::Foo` from inside `rivets`) must resolve identically
    /// to the `crate::Foo` form. The new arm should find the caller's own
    /// `CrateInfo` and recurse into the same `src/` it would have used for
    /// `crate::`.
    #[test]
    fn workspace_crate_self_reference_matches_crate_form() {
        let (dir, crates) = workspace_with_crates(&[("solo", &["storage.rs"])]);
        let current = dir.path().join("solo/src/lib.rs");
        let solo_src = dir.path().join("solo/src");

        let via_workspace_arm = resolve_module_path(
            &["solo".to_string(), "storage".to_string()],
            &current,
            &solo_src,
            &crates,
        )
        .expect("self-reference via workspace arm must resolve");
        let via_crate_arm = resolve_module_path(
            &["crate".to_string(), "storage".to_string()],
            &current,
            &solo_src,
            &crates,
        )
        .expect("crate::storage must resolve");

        assert_eq!(
            via_workspace_arm, via_crate_arm,
            "self-import via crate-name and `crate::` form must produce the same file path"
        );
    }

    /// Cargo manifest names allow hyphens; Rust module names use underscores.
    /// `use rivets_jsonl::Foo` (`path[0]="rivets_jsonl"`) must match a
    /// `CrateInfo` with name `"rivets-jsonl"`. Catches the bug where the new
    /// arm compares raw strings without normalization.
    #[test]
    fn hyphenated_crate_name_matches_underscore_path_head() {
        let (dir, crates) = workspace_with_crates(&[("caller", &[]), ("my-crate", &["thing.rs"])]);
        let current = dir.path().join("caller/src/lib.rs");
        let path = vec!["my_crate".to_string(), "thing".to_string()];
        let result = resolve_module_path(&path, &current, &dir.path().join("caller/src"), &crates);
        let resolved = result.expect("hyphenated my-crate should match my_crate path head");
        assert!(
            resolved.ends_with("my-crate/src/thing.rs")
                || resolved.ends_with("my-crate\\src\\thing.rs"),
            "expected my-crate/src/thing.rs, got {resolved:?}"
        );
    }

    /// Stress fixture for slice 4: a workspace crate with a non-standard
    /// `lib_path` must have its source modules resolved under the derived
    /// `src_root()`, NOT under a hardcoded `<crate>/src`. A pre-fix impl
    /// using `target.path.join("src")` would look for `<crate>/src/module.rs`
    /// (which doesn't exist) and return None; the post-fix `target.src_root()`
    /// derives `<crate>/custom/path` from `lib_path.parent()` and finds the
    /// actual file.
    #[test]
    fn workspace_crate_arm_uses_src_root_not_hardcoded_src() {
        use crate::types::CrateInfo;

        let dir = tempfile::tempdir().expect("temp dir");
        let target_crate = dir.path().join("target");
        let custom_dir = target_crate.join("custom/path");
        fs::create_dir_all(&custom_dir).expect("custom dir");
        fs::write(custom_dir.join("lib.rs"), "").expect("lib.rs");
        fs::write(custom_dir.join("module.rs"), "pub fn x() {}").expect("module.rs");

        let caller_crate = dir.path().join("caller");
        fs::create_dir_all(caller_crate.join("src")).expect("caller src");
        fs::write(caller_crate.join("src/lib.rs"), "").expect("caller lib.rs");

        let crates = vec![
            CrateInfo {
                name: "caller".into(),
                path: caller_crate.clone(),
                lib_path: Some(PathBuf::from("src/lib.rs")),
                bin_paths: vec![],
            },
            CrateInfo {
                name: "target".into(),
                path: target_crate.clone(),
                lib_path: Some(PathBuf::from("custom/path/lib.rs")),
                bin_paths: vec![],
            },
        ];

        let result = resolve_module_path(
            &["target".to_string(), "module".to_string()],
            &caller_crate.join("src/lib.rs"),
            &caller_crate.join("src"),
            &crates,
        );
        let resolved = result
            .expect("non-standard lib_path target should resolve via src_root, not hardcoded src/");
        assert!(
            resolved.ends_with("custom/path/module.rs")
                || resolved.ends_with("custom\\path\\module.rs"),
            "expected target/custom/path/module.rs (via lib_path.parent()), got {resolved:?}"
        );
    }
}
