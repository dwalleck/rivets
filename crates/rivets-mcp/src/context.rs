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
}

impl Context {
    /// Create a new empty context.
    #[must_use]
    pub fn new() -> Self {
        Self {
            current_workspace: None,
            storage_cache: HashMap::new(),
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
            .map_err(|_| Error::WorkspaceNotFound(workspace_root.display().to_string()))?;

        // Verify .rivets directory exists
        let rivets_dir = canonical.join(".rivets");
        if !rivets_dir.exists() {
            return Err(Error::NoRivetsDirectory(canonical.display().to_string()));
        }

        // Find the database file
        let db_path = find_database(&rivets_dir)?;

        self.current_workspace = Some(canonical.clone());

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
    /// Returns an error if the workspace doesn't exist or no context is set.
    pub fn storage_for(
        &self,
        workspace_root: Option<&Path>,
    ) -> Result<Arc<RwLock<Box<dyn IssueStorage>>>> {
        let workspace = match workspace_root {
            Some(path) => path
                .canonicalize()
                .map_err(|_| Error::WorkspaceNotFound(path.display().to_string()))?,
            None => self.current_workspace.clone().ok_or(Error::NoContext)?,
        };

        self.storage_cache
            .get(&workspace)
            .cloned()
            .ok_or(Error::NoContext)
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
fn find_database(rivets_dir: &Path) -> Result<PathBuf> {
    // Look for .jsonl files (primary) or .db files
    for entry in std::fs::read_dir(rivets_dir)? {
        let entry = entry?;
        let path = entry.path();
        if let Some(ext) = path.extension() {
            if ext == "jsonl" || ext == "db" {
                return Ok(path);
            }
        }
    }

    // Default to issues.jsonl if no database found
    Ok(rivets_dir.join("issues.jsonl"))
}

/// Discover a rivets workspace by walking up from the given directory.
///
/// Returns the workspace root (directory containing `.rivets/`).
///
/// # Errors
///
/// Returns `Error::NoRivetsDirectory` if no `.rivets/` directory is found.
pub fn discover_workspace(start: &Path) -> Result<PathBuf> {
    let mut current = start.to_path_buf();

    loop {
        let rivets_dir = current.join(".rivets");
        if rivets_dir.exists() && rivets_dir.is_dir() {
            return Ok(current);
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
        assert_eq!(result.unwrap(), temp.path());
    }

    #[test]
    fn test_discover_workspace_not_found() {
        let temp = TempDir::new().unwrap();
        let result = discover_workspace(temp.path());
        assert!(result.is_err());
    }
}
