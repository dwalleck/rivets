//! Durable, nonblocking ownership for one Workspace mutation transaction.

use crate::error::{Error, Result};
use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

/// Name of the metadata directory that identifies a Rivets Workspace.
pub const RIVETS_DIR_NAME: &str = ".rivets";

/// Name of the persistent mutation-lock sidecar inside `.rivets`.
pub const WORKSPACE_LOCK_FILE_NAME: &str = "workspace.lock";

/// Exclusive ownership of one canonical Workspace's mutation transaction.
///
/// The lock is released when this guard is dropped. The sidecar intentionally
/// remains on disk: deleting a locked file can split lock identity between
/// processes that opened the old and new files.
#[derive(Debug)]
#[must_use = "dropping the guard releases the Workspace mutation lock"]
pub struct WorkspaceMutationLock {
    workspace_root: PathBuf,
    lock_path: PathBuf,
    _file: File,
}

impl WorkspaceMutationLock {
    /// Try to acquire exclusive ownership without waiting for another writer.
    ///
    /// The Workspace root is canonicalized before deriving the sidecar path so
    /// relative and symlink aliases contend on the same file.
    ///
    /// # Errors
    ///
    /// Returns [`Error::WorkspaceBusy`] when another handle owns the lock.
    /// Returns [`Error::WorkspaceLock`] when canonicalization, opening, or lock
    /// acquisition fails for another reason.
    pub fn try_acquire(workspace_root: &Path) -> Result<Self> {
        let unresolved_lock_path = workspace_root
            .join(RIVETS_DIR_NAME)
            .join(WORKSPACE_LOCK_FILE_NAME);
        let workspace_root =
            workspace_root
                .canonicalize()
                .map_err(|source| Error::WorkspaceLock {
                    lock_path: unresolved_lock_path,
                    source,
                })?;
        let lock_path = workspace_root
            .join(RIVETS_DIR_NAME)
            .join(WORKSPACE_LOCK_FILE_NAME);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|source| Error::WorkspaceLock {
                lock_path: lock_path.clone(),
                source,
            })?;
        match file.try_lock() {
            Ok(()) => Ok(Self {
                workspace_root,
                lock_path,
                _file: file,
            }),
            Err(TryLockError::WouldBlock) => Err(Error::WorkspaceBusy { workspace_root }),
            Err(TryLockError::Error(source)) => Err(Error::WorkspaceLock { lock_path, source }),
        }
    }

    /// Canonical root whose mutation transaction this guard owns.
    #[must_use]
    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    /// Persistent sidecar carrying the OS lock.
    #[must_use]
    pub fn lock_path(&self) -> &Path {
        &self.lock_path
    }
}
