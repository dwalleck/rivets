//! Durable, nonblocking ownership for one Workspace mutation transaction.

use crate::error::{Error, Result};
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};

const GITIGNORE_FILE_NAME: &str = ".gitignore";

fn ensure_workspace_lock_ignored(workspace_root: &Path) -> Result<()> {
    let gitignore_path = workspace_root
        .join(RIVETS_DIR_NAME)
        .join(GITIGNORE_FILE_NAME);
    let contents = match fs::read_to_string(&gitignore_path) {
        Ok(contents) => contents,
        Err(source) if source.kind() == ErrorKind::NotFound => String::new(),
        Err(source) => {
            return Err(Error::WorkspaceLock {
                lock_path: gitignore_path,
                source,
            });
        }
    };

    let entry_count = contents
        .split_inclusive('\n')
        .filter(|segment| {
            let line = segment.strip_suffix('\n').unwrap_or(segment);
            let line = line.strip_suffix('\r').unwrap_or(line);
            line == WORKSPACE_LOCK_FILE_NAME
        })
        .count();
    if entry_count == 1 {
        return Ok(());
    }

    let updated_contents = if entry_count == 0 {
        let mut updated = contents;
        if !updated.is_empty() && !updated.ends_with('\n') {
            updated.push('\n');
        }
        updated.push_str(WORKSPACE_LOCK_FILE_NAME);
        updated.push('\n');
        updated
    } else {
        let mut updated = String::with_capacity(contents.len());
        let mut retained = false;
        for segment in contents.split_inclusive('\n') {
            let line = segment.strip_suffix('\n').unwrap_or(segment);
            let line = line.strip_suffix('\r').unwrap_or(line);
            if line == WORKSPACE_LOCK_FILE_NAME {
                if retained {
                    continue;
                }
                retained = true;
            }
            updated.push_str(segment);
        }
        updated
    };

    let temp_path = gitignore_path.with_file_name(format!(
        ".{GITIGNORE_FILE_NAME}.rivets-{}.tmp",
        std::process::id()
    ));
    let mut temp_file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&temp_path)
        .map_err(|source| Error::WorkspaceLock {
            lock_path: temp_path.clone(),
            source,
        })?;
    temp_file
        .write_all(updated_contents.as_bytes())
        .map_err(|source| Error::WorkspaceLock {
            lock_path: temp_path.clone(),
            source,
        })?;
    temp_file
        .sync_all()
        .map_err(|source| Error::WorkspaceLock {
            lock_path: temp_path.clone(),
            source,
        })?;
    drop(temp_file);

    fs::rename(&temp_path, &gitignore_path).map_err(|source| Error::WorkspaceLock {
        lock_path: gitignore_path,
        source,
    })
}

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

#[allow(clippy::io_other_error)]
fn join_error_to_io(source: tokio::task::JoinError) -> io::Error {
    io::Error::new(ErrorKind::Other, source)
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
            Ok(()) => {
                ensure_workspace_lock_ignored(&workspace_root)?;
                Ok(Self {
                    workspace_root,
                    lock_path,
                    _file: file,
                })
            }
            Err(TryLockError::WouldBlock) => Err(Error::WorkspaceBusy { workspace_root }),
            Err(TryLockError::Error(source)) => Err(Error::WorkspaceLock { lock_path, source }),
        }
    }

    /// Try to acquire exclusive ownership on Tokio's blocking thread pool.
    ///
    /// This keeps canonicalization, sidecar creation, lock acquisition, and
    /// metadata upgrades off the async runtime thread.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`Self::try_acquire`]. A task join failure is
    /// reported as [`Error::WorkspaceLock`].
    pub async fn try_acquire_async(workspace_root: PathBuf) -> Result<Self> {
        let unresolved_lock_path = workspace_root
            .join(RIVETS_DIR_NAME)
            .join(WORKSPACE_LOCK_FILE_NAME);
        tokio::task::spawn_blocking(move || Self::try_acquire(&workspace_root))
            .await
            .map_err(|source| Error::WorkspaceLock {
                lock_path: unresolved_lock_path,
                source: join_error_to_io(source),
            })?
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
