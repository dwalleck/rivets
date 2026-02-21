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

use crate::commands::init::{find_rivets_root, RivetsConfig, CONFIG_FILE_NAME, RIVETS_DIR_NAME};
use crate::error::{ConfigError, Result};
use crate::storage::{create_storage, IssueStorage};
use std::path::{Path, PathBuf};

/// Application context for CLI operations.
///
/// Manages storage initialization, lifecycle, and provides the execution
/// context for CLI commands. Storage is automatically loaded from the
/// rivets directory on creation.
pub struct App {
    /// The storage backend (trait object for polymorphism)
    storage: Box<dyn IssueStorage>,

    /// Path to the rivets directory (.rivets)
    rivets_dir: PathBuf,

    /// Issue ID prefix from configuration
    prefix: String,
}

impl std::fmt::Debug for App {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("App")
            .field("rivets_dir", &self.rivets_dir)
            .field("prefix", &self.prefix)
            .field("storage", &"<dyn IssueStorage>")
            .finish()
    }
}

impl App {
    /// Create an App instance from the given working directory.
    ///
    /// Searches up the directory tree to find a `.rivets/` directory,
    /// loads configuration, and initializes storage.
    ///
    /// # Arguments
    ///
    /// * `working_dir` - The directory to start searching from
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - No rivets repository is found in the directory tree
    /// - Configuration cannot be loaded
    /// - Storage initialization fails
    pub async fn from_directory(working_dir: &Path) -> Result<Self> {
        // Find rivets root directory
        let root_dir = find_rivets_root(working_dir).ok_or(ConfigError::NotInitialized)?;

        let rivets_dir = root_dir.join(RIVETS_DIR_NAME);
        let config_path = rivets_dir.join(CONFIG_FILE_NAME);

        // Load configuration
        let config = RivetsConfig::load(&config_path).await?;

        // Create storage based on configuration
        let backend = config.storage.to_backend(&root_dir)?;
        let storage = create_storage(backend, config.issue_prefix.clone()).await?;

        Ok(Self {
            storage,
            rivets_dir,
            prefix: config.issue_prefix,
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

    /// Get the issue ID prefix.
    pub fn prefix(&self) -> &str {
        &self.prefix
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

        // Initialize rivets first
        init::init(temp_dir.path(), Some("test")).await.unwrap();

        // Create app from that directory
        let app = App::from_directory(temp_dir.path()).await.unwrap();

        assert_eq!(app.prefix(), "test");
        assert!(app.rivets_dir().ends_with(".rivets"));
    }

    #[tokio::test]
    async fn test_app_from_subdirectory() {
        let temp_dir = TempDir::new().unwrap();

        // Initialize rivets in root
        init::init(temp_dir.path(), Some("proj")).await.unwrap();

        // Create a subdirectory
        let sub_dir = temp_dir.path().join("src").join("lib");
        std::fs::create_dir_all(&sub_dir).unwrap();

        // App should find rivets from subdirectory
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
}
