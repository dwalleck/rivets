//! Module path resolution for Rust source files.
//!
//! Maps use statement paths to actual file paths within the workspace.
//! External crates return `None` since we can't analyze external code.

use std::path::{Path, PathBuf};

/// Resolve a module path to a file path within the workspace.
///
/// # Arguments
/// * `path` - Module path segments (e.g., `["crate", "auth"]`)
/// * `current_file` - Path to the file containing the use statement
/// * `crate_root` - Root of the crate (usually `src/` directory)
///
/// # Returns
/// * `Some(PathBuf)` - Resolved file path within the workspace
/// * `None` - External crate or unresolvable path
#[must_use]
pub fn resolve_module_path(
    path: &[String],
    current_file: &Path,
    crate_root: &Path,
) -> Option<PathBuf> {
    if path.is_empty() {
        return None;
    }

    match path[0].as_str() {
        "crate" => resolve_crate_path(&path[1..], crate_root),
        "self" => resolve_self_path(&path[1..], current_file),
        "super" => resolve_super_path(&path[1..], current_file),
        // External crate - cannot resolve
        _ => None,
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
        let result = resolve_module_path(&path, &current, &crate_root);

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
        let result = resolve_module_path(&path, &current, &crate_root);

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
        let result = resolve_module_path(&path, &current, &crate_root);

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
        let result = resolve_module_path(&path, &current, &crate_root);

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
        let result = resolve_module_path(&path, &current, &crate_root);

        assert!(result.is_some());
        let resolved = result.unwrap();
        assert!(resolved.ends_with("middleware.rs"));
    }

    #[test]
    fn empty_path_returns_none() {
        let dir = create_test_workspace();
        let crate_root = dir.path().join("src");
        let current = crate_root.join("lib.rs");

        let result = resolve_module_path(&[], &current, &crate_root);
        assert!(result.is_none());
    }
}
