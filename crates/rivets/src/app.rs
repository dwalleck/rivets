//! Application context for CLI command execution.
//!
//! This module provides the `App` struct that manages storage lifecycle
//! and provides a context for executing CLI commands.
//!
//! # Example
//!
//! ```no_run
//! use rivets::app::App;
//! use std::path::Path;
//!
//! #[tokio::main(flavor = "current_thread")]
//! async fn main() -> anyhow::Result<()> {
//!     let app = App::from_directory(Path::new(".")).await?;
//!     // Execute commands using app...
//!     Ok(())
//! }
//! ```

use crate::commands::init::{CONFIG_FILE_NAME, RivetsConfig, find_rivets_root};
use crate::error::{ConfigError, Result};
use crate::reporting::WorkspaceInformation;
use crate::storage::{IssueStorage, create_storage};
use crate::workspace_lock::{RIVETS_DIR_NAME, WorkspaceMutationLock};
use std::path::{Path, PathBuf};

/// Application context for CLI operations.
///
/// Manages storage initialization, lifecycle, and provides the execution
/// context for CLI commands. Storage is automatically loaded from the
/// rivets directory on creation.
pub struct App {
    /// The storage backend (trait object for polymorphism)
    storage: Box<dyn IssueStorage>,

    /// Immutable configuration snapshot used to initialize `storage`.
    information: WorkspaceInformation,

    /// Path to the rivets directory (.rivets)
    rivets_dir: PathBuf,

    /// Durable mutation ownership retained through save and error recovery.
    _mutation_lock: Option<WorkspaceMutationLock>,
}

impl std::fmt::Debug for App {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("App")
            .field("information", &self.information)
            .field("rivets_dir", &self.rivets_dir)
            .field("storage", &"<dyn IssueStorage>")
            .field("mutation_locked", &self._mutation_lock.is_some())
            .finish()
    }
}

impl App {
    /// Create an App instance from the given working directory.
    ///
    /// Searches up the directory tree to find a `.rivets/` directory,
    /// loads configuration, and initializes storage.
    ///
    /// # Errors
    ///
    /// Returns an error if no Workspace is found, configuration cannot be loaded
    /// or resolved, or storage initialization fails.
    pub async fn from_directory(working_dir: &Path) -> Result<Self> {
        let root_dir = find_rivets_root(working_dir).ok_or(ConfigError::NotInitialized)?;
        Self::from_root(root_dir, None).await
    }

    /// Create an App that exclusively owns one existing-Workspace mutation.
    ///
    /// The durable lock is acquired before configuration or storage is loaded
    /// and remains owned until the returned App is dropped.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::Error::WorkspaceBusy`] on contention, or the
    /// configuration and storage errors described by [`Self::from_directory`].
    pub async fn from_directory_for_mutation(working_dir: &Path) -> Result<Self> {
        let root_dir = find_rivets_root(working_dir).ok_or(ConfigError::NotInitialized)?;
        let mutation_lock = WorkspaceMutationLock::try_acquire(&root_dir)?;
        let canonical_root = mutation_lock.workspace_root().to_path_buf();
        Self::from_root(canonical_root, Some(mutation_lock)).await
    }

    async fn from_root(
        root_dir: PathBuf,
        mutation_lock: Option<WorkspaceMutationLock>,
    ) -> Result<Self> {
        let root_dir = root_dir.canonicalize()?;
        let rivets_dir = root_dir.join(RIVETS_DIR_NAME);
        let config_path = rivets_dir.join(CONFIG_FILE_NAME);
        let config = RivetsConfig::load(&config_path).await?;
        let backend = config.storage.to_backend(&root_dir)?;
        let information = WorkspaceInformation::from_config(&root_dir, &config, &backend)?;
        let storage = create_storage(backend, config.issue_prefix.clone()).await?;

        Ok(Self {
            storage,
            information,
            rivets_dir,
            _mutation_lock: mutation_lock,
        })
    }

    /// Get a mutable reference to the storage.
    pub fn storage_mut(&mut self) -> &mut dyn IssueStorage {
        self.storage.as_mut()
    }

    /// Get an immutable reference to the storage.
    pub fn storage(&self) -> &dyn IssueStorage {
        self.storage.as_ref()
    }

    /// Get the immutable Workspace configuration snapshot used by this App.
    pub fn information(&self) -> &WorkspaceInformation {
        &self.information
    }

    /// Get the issue ID prefix.
    pub fn prefix(&self) -> &str {
        self.information.issue_prefix()
    }

    /// Get the path to the rivets directory.
    pub fn rivets_dir(&self) -> &Path {
        &self.rivets_dir
    }

    /// Save storage state to persistent storage.
    ///
    /// This should be called after any mutating operations.
    pub async fn save(&self) -> Result<()> {
        self.storage.save().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::init;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_app_from_initialized_directory() {
        let temp_dir = TempDir::new().unwrap();
        init::init(temp_dir.path(), Some("test")).await.unwrap();

        let app = App::from_directory(temp_dir.path()).await.unwrap();

        assert_eq!(app.prefix(), "test");
        assert!(app.rivets_dir().ends_with(".rivets"));
        assert_eq!(app.information().issue_prefix(), "test");
        assert_eq!(app.information().storage_backend(), "jsonl");
    }

    #[tokio::test]
    async fn test_app_from_subdirectory() {
        let temp_dir = TempDir::new().unwrap();
        init::init(temp_dir.path(), Some("proj")).await.unwrap();

        let sub_dir = temp_dir.path().join("src").join("lib");
        std::fs::create_dir_all(&sub_dir).unwrap();

        let app = App::from_directory(&sub_dir).await.unwrap();
        assert_eq!(app.prefix(), "proj");
    }

    #[tokio::test]
    async fn test_app_from_uninitialized_directory() {
        let temp_dir = TempDir::new().unwrap();

        let result = App::from_directory(temp_dir.path()).await;
        assert!(result.is_err());

        let err = result.unwrap_err().to_string();
        assert!(err.contains("Not a rivets repository"));
    }

    #[tokio::test]
    async fn mutation_app_owns_lock_until_drop() {
        let temp_dir = TempDir::new().expect("mutation test temporary directory should be created");
        init::init(temp_dir.path(), Some("test"))
            .await
            .expect("mutation test workspace should initialize");
        let app = App::from_directory_for_mutation(temp_dir.path())
            .await
            .expect("mutation App should acquire");
        assert!(matches!(
            WorkspaceMutationLock::try_acquire(temp_dir.path()),
            Err(crate::error::Error::WorkspaceBusy { .. })
        ));
        drop(app);
        let reacquired = WorkspaceMutationLock::try_acquire(temp_dir.path())
            .expect("dropping App should release");
        drop(reacquired);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn app_canonicalizes_symlink_root() {
        let temp_dir = TempDir::new().unwrap();
        init::init(temp_dir.path(), Some("symlink")).await.unwrap();
        let alias_parent = TempDir::new().unwrap();
        let alias = alias_parent.path().join("workspace-link");
        std::os::unix::fs::symlink(temp_dir.path(), &alias).unwrap();

        let app = App::from_directory(&alias).await.unwrap();
        assert_eq!(
            app.information().workspace_root(),
            temp_dir.path().canonicalize().unwrap()
        );
        assert_eq!(
            app.information().database_path(),
            temp_dir
                .path()
                .canonicalize()
                .unwrap()
                .join(".rivets/issues.jsonl")
        );
        std::fs::remove_file(alias).unwrap();
    }
}
