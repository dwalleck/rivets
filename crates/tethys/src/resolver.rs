//! Module path resolution for Rust source files.
//!
//! Maps use statement paths to actual file paths within the workspace.
//! External crates return `None` since we can't analyze external code.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Maps symbol names to their origin (file path and full path segments).
///
/// Built from use statements, allows looking up where a symbol was imported from.
///
/// Note: This type is infrastructure for advanced resolution features.
/// Current implementation uses direct path resolution in `compute_dependencies`.
#[allow(dead_code)] // Phase 3+: advanced symbol resolution
#[derive(Debug, Default)]
pub struct SymbolOriginMap {
    /// Maps symbol name -> (`file_path`, `path_segments`)
    origins: HashMap<String, (PathBuf, Vec<String>)>,
}

#[allow(dead_code)] // Phase 3+: advanced symbol resolution
impl SymbolOriginMap {
    /// Create an empty symbol origin map.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a symbol origin map from use statements.
    ///
    /// # Arguments
    /// * `uses` - Use statements extracted from a file
    /// * `current_file` - Path to the file containing the use statements
    /// * `crate_root` - Root of the crate (usually `src/` or where `lib.rs`/`main.rs` lives)
    #[must_use]
    pub fn from_uses(
        uses: &[super::languages::rust::UseStatement],
        current_file: &Path,
        crate_root: &Path,
    ) -> Self {
        let mut map = Self::new();

        for use_stmt in uses {
            // Skip glob imports - they import everything, hard to track
            if use_stmt.is_glob {
                continue;
            }

            // Resolve the module path to a file
            let resolved_path = resolve_module_path(&use_stmt.path, current_file, crate_root);

            // For each imported name, record its origin
            for name in &use_stmt.imported_names {
                let key = use_stmt.alias.clone().unwrap_or_else(|| name.clone());
                let mut full_path = use_stmt.path.clone();
                full_path.push(name.clone());

                if let Some(ref file_path) = resolved_path {
                    map.origins.insert(key, (file_path.clone(), full_path));
                } else {
                    // External crate - store path without file
                    map.origins.insert(key, (PathBuf::new(), full_path));
                }
            }
        }

        map
    }

    /// Look up the origin of a symbol by name.
    ///
    /// Returns `Some((file_path, path_segments))` if the symbol was imported.
    /// - `file_path` is empty for external crate symbols
    /// - `path_segments` is the full path like `["crate", "auth", "Authenticator"]`
    #[must_use]
    pub fn lookup(&self, name: &str) -> Option<&(PathBuf, Vec<String>)> {
        self.origins.get(name)
    }

    /// Check if a symbol is from an external crate (empty file path).
    #[must_use]
    pub fn is_external(&self, name: &str) -> bool {
        self.lookup(name)
            .is_some_and(|(path, _)| path.as_os_str().is_empty())
    }
}

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
fn resolve_as_module(path: &Path) -> PathBuf {
    // Try as a .rs file first
    let rs_path = path.with_extension("rs");
    if rs_path.exists() {
        return rs_path;
    }

    // Try as a directory with mod.rs
    let mod_rs = path.join("mod.rs");
    if mod_rs.exists() {
        return mod_rs;
    }

    // Return .rs variant as best guess
    rs_path
}

/// Resolve a crate-relative path.
#[allow(clippy::unnecessary_wraps)] // Returns Option for API consistency with resolve_module_path
fn resolve_crate_path(path: &[String], crate_root: &Path) -> Option<PathBuf> {
    if path.is_empty() {
        return Some(crate_root.to_path_buf());
    }

    // Build the path from crate root
    let mut result = crate_root.to_path_buf();
    for segment in path {
        result.push(segment);
    }

    Some(resolve_as_module(&result))
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

    Some(resolve_as_module(&result))
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
        let parent_file = parent_dir.join(format!("{dir_name}.rs"));
        if parent_file.exists() {
            return Some(parent_file);
        }
        return None;
    }

    let mut result = parent_dir.to_path_buf();
    for segment in path {
        result.push(segment);
    }

    Some(resolve_as_module(&result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_workspace() -> TempDir {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        fs::create_dir_all(&src).unwrap();

        // Create typical Rust project structure
        fs::write(src.join("lib.rs"), "mod auth;\nmod config;").unwrap();
        fs::write(src.join("config.rs"), "pub struct Config {}").unwrap();

        // Create auth as a directory module
        let auth_dir = src.join("auth");
        fs::create_dir_all(&auth_dir).unwrap();
        fs::write(auth_dir.join("mod.rs"), "pub struct Authenticator {}").unwrap();
        fs::write(auth_dir.join("middleware.rs"), "pub fn check() {}").unwrap();

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
        fs::create_dir_all(&inner).unwrap();
        fs::write(inner.join("mod.rs"), "use super::middleware;").unwrap();

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
