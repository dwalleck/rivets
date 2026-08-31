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

    async fn acquire_on_blocking_pool<F>(unresolved_lock_path: PathBuf, acquire: F) -> Result<Self>
    where
        F: FnOnce() -> Result<Self> + Send + 'static,
    {
        tokio::task::spawn_blocking(acquire)
            .await
            .map_err(|source| Error::WorkspaceLock {
                lock_path: unresolved_lock_path,
                source: join_error_to_io(source),
            })?
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
        Self::acquire_on_blocking_pool(unresolved_lock_path, move || {
            Self::try_acquire(&workspace_root)
        })
        .await
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};
    use tempfile::TempDir;
    use tokio::sync::oneshot;

    #[tokio::test(flavor = "current_thread")]
    async fn blocking_acquisition_leaves_current_thread_runtime_responsive() {
        let workspace = TempDir::new().expect("temporary Workspace should be created");
        std::fs::create_dir(workspace.path().join(RIVETS_DIR_NAME))
            .expect("Rivets metadata directory should be created");
        let root = workspace
            .path()
            .canonicalize()
            .expect("Workspace root should canonicalize");
        let lock_path = root.join(RIVETS_DIR_NAME).join(WORKSPACE_LOCK_FILE_NAME);
        let (started_tx, started_rx) = oneshot::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let watchdog_tx = release_tx.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(1));
            let _ = watchdog_tx.send(());
        });

        let operation_root = root.clone();
        let started_at = Instant::now();
        let acquisition = tokio::spawn(WorkspaceMutationLock::acquire_on_blocking_pool(
            lock_path,
            move || {
                let _ = started_tx.send(());
                release_rx
                    .recv()
                    .expect("test should release the blocking operation");
                WorkspaceMutationLock::try_acquire(&operation_root)
            },
        ));

        started_rx
            .await
            .expect("blocking operation should publish readiness");
        assert!(
            started_at.elapsed() < Duration::from_millis(500),
            "blocking operation ran on the current-thread async runtime"
        );
        tokio::time::timeout(Duration::from_millis(100), tokio::task::yield_now())
            .await
            .expect("current-thread runtime should remain responsive");
        release_tx
            .send(())
            .expect("blocking operation should still await release");
        drop(
            acquisition
                .await
                .expect("acquisition task should join")
                .expect("Workspace lock should be acquired"),
        );
    }
}
