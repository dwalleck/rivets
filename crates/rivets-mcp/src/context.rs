//! Workspace context management for the MCP server.
//!
//! This module handles:
//! - Workspace detection (walking up to find `.rivets/`)
//! - Path canonicalization
//! - Per-workspace storage instance management

use crate::error::{Error, Result};
use rivets::storage::{create_storage, IssueStorage, StorageBackend};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Global context state for the MCP server.
///
/// Manages workspace contexts and storage instances for multi-workspace support.
pub struct Context {
    /// The current active workspace root.
    current_workspace: Option<PathBuf>,

    /// Per-workspace storage instances.
    storage_cache: HashMap<PathBuf, Arc<RwLock<Box<dyn IssueStorage>>>>,

    /// Per-workspace database paths (discovered dynamically).
    database_paths: HashMap<PathBuf, PathBuf>,
}

impl Context {
    /// Create a new empty context.
    #[must_use]
    pub fn new() -> Self {
        Self {
            current_workspace: None,
            storage_cache: HashMap::new(),
            database_paths: HashMap::new(),
        }
    }

    /// Set the current workspace root.
    ///
    /// This will:
    /// 1. Canonicalize the path
    /// 2. Verify a `.rivets/` directory exists
    /// 3. Create or retrieve a storage instance
    ///
    /// # Errors
    ///
    /// Returns an error if the workspace path doesn't exist, has no `.rivets/` directory,
    /// or if storage creation fails.
    pub async fn set_workspace(&mut self, workspace_root: &Path) -> Result<WorkspaceInfo> {
        let canonical = workspace_root
            .canonicalize()
            .map_err(|e| Error::WorkspaceNotFound {
                path: workspace_root.display().to_string(),
                source: Some(e),
            })?;

        // Verify .rivets directory exists
        let rivets_dir = canonical.join(".rivets");
        if !rivets_dir.exists() {
            return Err(Error::NoRivetsDirectory(canonical.display().to_string()));
        }

        // Find the database file
        let db_path = find_database(&rivets_dir)?;

        self.current_workspace = Some(canonical.clone());

        // Store database path
        self.database_paths
            .insert(canonical.clone(), db_path.clone());

        // Create storage if not cached
        if !self.storage_cache.contains_key(&canonical) {
            let storage = create_storage(StorageBackend::Jsonl(db_path.clone())).await?;
            self.storage_cache
                .insert(canonical.clone(), Arc::new(RwLock::new(storage)));
        }

        Ok(WorkspaceInfo {
            workspace_root: canonical,
            database_path: db_path,
        })
    }

    /// Get the current workspace root.
    #[must_use]
    pub fn current_workspace(&self) -> Option<&PathBuf> {
        self.current_workspace.as_ref()
    }

    /// Get the database path for the current workspace.
    #[must_use]
    pub fn current_database_path(&self) -> Option<&PathBuf> {
        self.current_workspace
            .as_ref()
            .and_then(|ws| self.database_paths.get(ws))
    }

    /// Get storage for the current workspace.
    ///
    /// # Errors
    ///
    /// Returns `Error::NoContext` if no workspace has been set.
    pub fn storage(&self) -> Result<Arc<RwLock<Box<dyn IssueStorage>>>> {
        let workspace = self.current_workspace.as_ref().ok_or(Error::NoContext)?;

        self.storage_cache
            .get(workspace)
            .cloned()
            .ok_or(Error::NoContext)
    }

    /// Get storage for a specific workspace, or the current one if not specified.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - No context is set and no workspace path is provided
    /// - The workspace path doesn't exist (with IO error context)
    /// - The workspace exists but wasn't initialized via `set_workspace()`
    pub fn storage_for(
        &self,
        workspace_root: Option<&Path>,
    ) -> Result<Arc<RwLock<Box<dyn IssueStorage>>>> {
        let workspace = match workspace_root {
            Some(path) => path.canonicalize().map_err(|e| Error::WorkspaceNotFound {
                path: path.display().to_string(),
                source: Some(e),
            })?,
            None => self.current_workspace.clone().ok_or(Error::NoContext)?,
        };

        self.storage_cache
            .get(&workspace)
            .cloned()
            .ok_or_else(|| Error::WorkspaceNotInitialized(workspace.display().to_string()))
    }

    /// Discover and set the workspace by walking up from the given directory.
    ///
    /// This is a convenience method that combines `discover_workspace()` and `set_workspace()`.
    /// It walks up from the starting directory to find a `.rivets/` directory, then
    /// initializes storage for that workspace.
    ///
    /// # Errors
    ///
    /// Returns an error if no `.rivets/` directory is found in the path hierarchy,
    /// or if storage creation fails.
    pub async fn discover_and_set_workspace(&mut self, start: &Path) -> Result<WorkspaceInfo> {
        let workspace_root = discover_workspace(start)?;
        self.set_workspace(&workspace_root).await
    }
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}

/// Information about a workspace.
#[derive(Debug, Clone)]
pub struct WorkspaceInfo {
    /// The canonical path to the workspace root.
    pub workspace_root: PathBuf,

    /// The path to the database file.
    pub database_path: PathBuf,
}

/// Find the database file in a `.rivets/` directory.
///
/// Prefers the standard `issues.jsonl` location first, then falls back to
/// searching for any `.jsonl` or `.db` file. Returns an error if multiple
/// database files are found to avoid ambiguity.
fn find_database(rivets_dir: &Path) -> Result<PathBuf> {
    // Prefer the standard location first for deterministic behavior
    let standard = rivets_dir.join("issues.jsonl");
    if standard.exists() {
        return Ok(standard);
    }

    // Fall back to searching for any .jsonl or .db file
    let mut found = Vec::new();
    for entry in std::fs::read_dir(rivets_dir)? {
        let entry = entry?;
        let path = entry.path();
        if let Some(ext) = path.extension() {
            if ext == "jsonl" || ext == "db" {
                found.push(path);
            }
        }
    }

    match found.len() {
        0 => Ok(rivets_dir.join("issues.jsonl")), // Default for new workspaces
        1 => Ok(found.into_iter().next().expect("checked len")),
        _ => Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "Multiple database files found in {}. Remove duplicates or use issues.jsonl.",
                rivets_dir.display()
            ),
        ))),
    }
}

/// Discover a rivets workspace by walking up from the given directory.
///
/// Returns the canonicalized workspace root (directory containing `.rivets/`).
///
/// # Errors
///
/// Returns `Error::NoRivetsDirectory` if no `.rivets/` directory is found,
/// or `Error::WorkspaceNotFound` if the path cannot be canonicalized.
pub fn discover_workspace(start: &Path) -> Result<PathBuf> {
    let mut current = start.to_path_buf();

    loop {
        let rivets_dir = current.join(".rivets");
        if rivets_dir.exists() && rivets_dir.is_dir() {
            // Canonicalize to resolve symlinks (e.g., /var -> /private/var on macOS)
            return current
                .canonicalize()
                .map_err(|e| Error::WorkspaceNotFound {
                    path: current.display().to_string(),
                    source: Some(e),
                });
        }

        if !current.pop() {
            break;
        }
    }

    Err(Error::NoRivetsDirectory(start.display().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_discover_workspace() {
        let temp = TempDir::new().unwrap();
        let rivets_dir = temp.path().join(".rivets");
        std::fs::create_dir(&rivets_dir).unwrap();

        let result = discover_workspace(temp.path());
        assert!(result.is_ok());
        // Compare canonicalized paths to handle symlinks (e.g., /var -> /private/var on macOS)
        assert_eq!(result.unwrap(), temp.path().canonicalize().unwrap());
    }

    #[test]
    fn test_discover_workspace_not_found() {
        let temp = TempDir::new().unwrap();
        let result = discover_workspace(temp.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_discover_workspace_from_nested_dir() {
        let temp = TempDir::new().unwrap();
        let rivets_dir = temp.path().join(".rivets");
        std::fs::create_dir(&rivets_dir).unwrap();

        // Create a deeply nested subdirectory
        let subdir = temp.path().join("src").join("nested").join("deep");
        std::fs::create_dir_all(&subdir).unwrap();

        // Discovery should walk up and find the .rivets directory
        let result = discover_workspace(&subdir);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), temp.path().canonicalize().unwrap());
    }

    // Note: Full discover_and_set_workspace() integration test requires
    // working storage backend. This will be added in rivets-d06 (integration tests).

    #[test]
    fn test_storage_for_uninitialized_workspace() {
        let temp = TempDir::new().unwrap();
        let rivets_dir = temp.path().join(".rivets");
        std::fs::create_dir(&rivets_dir).unwrap();

        let context = Context::new();
        // Path exists but wasn't initialized via set_workspace
        let result = context.storage_for(Some(temp.path()));

        match result {
            Err(Error::WorkspaceNotInitialized(_)) => {} // Expected
            Err(e) => panic!("Expected WorkspaceNotInitialized, got {e:?}"),
            Ok(_) => panic!("Expected error, got Ok"),
        }
    }

    #[test]
    fn test_storage_for_nonexistent_path() {
        let context = Context::new();
        let result = context.storage_for(Some(Path::new("/nonexistent/path/to/workspace")));

        match result {
            Err(Error::WorkspaceNotFound { .. }) => {} // Expected
            Err(e) => panic!("Expected WorkspaceNotFound, got {e:?}"),
            Ok(_) => panic!("Expected error, got Ok"),
        }
    }
}
