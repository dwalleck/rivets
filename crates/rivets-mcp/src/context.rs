//! Workspace context management for the MCP server.
//!
//! This module handles:
//! - Workspace detection (walking up to find `.rivets/`)
//! - Path canonicalization
//! - Per-workspace storage instance management
//!
//! # Lock Ordering
//!
//! When using `Context` with `Tools`, locks must be acquired in this order:
//! 1. `Context` read/write lock (via `Arc<RwLock<Context>>`)
//! 2. Storage read/write lock (via `Arc<RwLock<Box<dyn IssueStorage>>>`)
//!
//! Never attempt to acquire a context lock while holding a storage lock.
//! This prevents potential deadlocks in concurrent scenarios.

use crate::error::{Error, Result};
use rivets::commands::init::RivetsConfig;
use rivets::storage::{create_storage, IssueStorage};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::debug;

/// Maximum number of cached workspaces to prevent resource exhaustion.
///
/// When this limit is reached, the oldest workspace is evicted from cache.
const MAX_CACHED_WORKSPACES: usize = 32;

/// Global context state for the MCP server.
///
/// Manages workspace contexts and storage instances for multi-workspace support.
pub struct Context {
    /// The current active workspace root.
    current_workspace: Option<PathBuf>,

    /// Per-workspace storage instances (limited to [`MAX_CACHED_WORKSPACES`]).
    storage_cache: HashMap<PathBuf, Arc<RwLock<Box<dyn IssueStorage>>>>,

    /// Per-workspace database paths (discovered dynamically).
    database_paths: HashMap<PathBuf, PathBuf>,

    /// Insertion order for FIFO cache eviction.
    cache_order: VecDeque<PathBuf>,
}

impl Context {
    /// Create a new empty context.
    #[must_use]
    pub fn new() -> Self {
        Self {
            current_workspace: None,
            storage_cache: HashMap::new(),
            database_paths: HashMap::new(),
            cache_order: VecDeque::new(),
        }
    }

    /// Set the current workspace root.
    ///
    /// This will:
    /// 1. Canonicalize the path (resolves `..`, symlinks, validates existence)
    /// 2. Validate the path is safe (no null bytes, is absolute)
    /// 3. Verify a `.rivets/` directory exists
    /// 4. Create or retrieve a storage instance
    ///
    /// # Security
    ///
    /// - Path canonicalization prevents directory traversal attacks
    /// - Cache size is limited to prevent resource exhaustion
    ///
    /// # Errors
    ///
    /// Returns an error if the workspace path doesn't exist, has no `.rivets/` directory,
    /// or if storage creation fails.
    pub async fn set_workspace(&mut self, workspace_root: &Path) -> Result<WorkspaceInfo> {
        debug!(path = %workspace_root.display(), "Setting workspace");

        // Canonicalize to resolve symlinks and `..` (prevents path traversal)
        let canonical = workspace_root
            .canonicalize()
            .map_err(|e| Error::WorkspaceNotFound {
                path: workspace_root.display().to_string(),
                source: Some(e),
            })?;

        // Validate path is safe
        validate_path(&canonical)?;

        // Verify .rivets directory exists
        let rivets_dir = canonical.join(".rivets");
        if !rivets_dir.exists() {
            debug!(path = %rivets_dir.display(), "No .rivets directory found");
            return Err(Error::NoRivetsDirectory(canonical.display().to_string()));
        }

        // Load config to get storage settings
        let config_path = rivets_dir.join("config.yaml");
        let config = RivetsConfig::load(&config_path)
            .await
            .map_err(|e| Error::ConfigLoad {
                path: config_path.display().to_string(),
                reason: e.to_string(),
            })?;
        debug!(prefix = %config.issue_prefix, backend = %config.storage.backend, "Loaded config");

        // Create backend configuration (this resolves the data path)
        let backend = config.storage.to_backend(&canonical)?;
        let db_path = backend.data_path().map_or_else(
            || canonical.join(&config.storage.data_file),
            Path::to_path_buf,
        );
        debug!(db_path = %db_path.display(), "Database path from backend");

        self.current_workspace = Some(canonical.clone());

        // Store database path
        self.database_paths
            .insert(canonical.clone(), db_path.clone());

        // Create storage if not cached
        if self.storage_cache.contains_key(&canonical) {
            debug!("Using cached storage instance");
        } else {
            debug!("Creating new storage instance");
            // Evict oldest workspace if cache is full
            while self.storage_cache.len() >= MAX_CACHED_WORKSPACES {
                self.evict_oldest();
            }

            let storage = create_storage(backend.clone(), config.issue_prefix).await?;
            self.storage_cache
                .insert(canonical.clone(), Arc::new(RwLock::new(storage)));
            self.cache_order.push_back(canonical.clone());
        }

        Ok(WorkspaceInfo {
            workspace_root: canonical,
            database_path: db_path,
        })
    }

    /// Evict the oldest cached workspace to make room for new entries.
    fn evict_oldest(&mut self) {
        if let Some(oldest) = self.cache_order.pop_front() {
            self.storage_cache.remove(&oldest);
            self.database_paths.remove(&oldest);
            tracing::debug!(workspace = %oldest.display(), "Evicted workspace from cache");
        }
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
    /// Returns `Error::NoContext` if no workspace has been set, or
    /// `Error::WorkspaceNotInitialized` if the workspace wasn't initialized via `set_workspace()`.
    pub fn storage(&self) -> Result<Arc<RwLock<Box<dyn IssueStorage>>>> {
        let workspace = self.current_workspace.as_ref().ok_or(Error::NoContext)?;

        self.storage_cache
            .get(workspace)
            .cloned()
            .ok_or_else(|| Error::WorkspaceNotInitialized(workspace.display().to_string()))
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

    /// Set up a workspace with injected storage for testing.
    ///
    /// This bypasses the normal storage creation flow and cache eviction,
    /// allowing tests to inject mock or in-memory storage without requiring
    /// a real `.rivets/` directory.
    #[cfg(test)]
    pub fn set_test_workspace(&mut self, workspace_root: PathBuf, storage: Box<dyn IssueStorage>) {
        self.current_workspace = Some(workspace_root.clone());
        self.database_paths
            .insert(workspace_root.clone(), PathBuf::from("test://memory"));
        self.storage_cache
            .insert(workspace_root.clone(), Arc::new(RwLock::new(storage)));
        self.cache_order.push_back(workspace_root);
    }

    /// Get the number of cached workspaces (for testing).
    #[cfg(test)]
    #[must_use]
    pub fn cache_size(&self) -> usize {
        self.storage_cache.len()
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

/// Validate that a path is safe to use as a workspace.
///
/// # Security Checks
///
/// - Path must be absolute (canonicalization ensures this)
/// - Path must not contain null bytes
/// - Path components must not contain path traversal attempts after canonicalization
fn validate_path(path: &Path) -> Result<()> {
    // Canonicalized paths should always be absolute
    if !path.is_absolute() {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Workspace path must be absolute",
        )));
    }

    // Check for null bytes in path (could be used for injection)
    let path_str = path.to_string_lossy();
    if path_str.contains('\0') {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Workspace path contains invalid characters",
        )));
    }

    // After canonicalization, there should be no `..` components
    // This is a defense-in-depth check
    for component in path.components() {
        if let std::path::Component::ParentDir = component {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Workspace path contains parent directory references",
            )));
        }
    }

    Ok(())
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

    #[test]
    fn test_validate_path_rejects_relative() {
        let result = validate_path(Path::new("relative/path"));
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_path_accepts_absolute() {
        // Use temp_dir() which is absolute on all platforms
        let result = validate_path(&std::env::temp_dir());
        assert!(result.is_ok());
    }

    #[test]
    fn test_cache_eviction() {
        use rivets::storage::in_memory::new_in_memory_storage;

        let mut context = Context::new();

        // Add workspaces up to the limit
        for i in 0..super::MAX_CACHED_WORKSPACES {
            let path = PathBuf::from(format!("/test/workspace{i}"));
            let storage = new_in_memory_storage("test".to_string());
            context.set_test_workspace(path, storage);
        }

        assert_eq!(context.cache_size(), super::MAX_CACHED_WORKSPACES);

        // Adding one more should maintain the limit (via eviction when set_workspace is called)
        // For set_test_workspace, we manually manage the cache, so this tests the structure
        let path = PathBuf::from("/test/workspace_extra");
        let storage = new_in_memory_storage("test".to_string());
        context.set_test_workspace(path, storage);

        // set_test_workspace doesn't evict, so cache grows
        // This verifies the cache_size() method works
        assert_eq!(context.cache_size(), super::MAX_CACHED_WORKSPACES + 1);
    }

    #[test]
    fn test_evict_oldest() {
        use rivets::storage::in_memory::new_in_memory_storage;

        let mut context = Context::new();

        // Add a few workspaces
        for i in 0..3 {
            let path = PathBuf::from(format!("/test/workspace{i}"));
            let storage = new_in_memory_storage("test".to_string());
            context.set_test_workspace(path, storage);
        }

        assert_eq!(context.cache_size(), 3);
        assert_eq!(context.cache_order.len(), 3);

        // Evict oldest
        context.evict_oldest();
        assert_eq!(context.cache_size(), 2);
        assert_eq!(context.cache_order.len(), 2);

        // Evict again
        context.evict_oldest();
        assert_eq!(context.cache_size(), 1);

        // Evict last
        context.evict_oldest();
        assert_eq!(context.cache_size(), 0);

        // Evicting from empty cache is a no-op
        context.evict_oldest();
        assert_eq!(context.cache_size(), 0);
    }
}
