//! Shared, immutable Workspace reporting values.
//!
//! The report values are constructed by core initialization/storage seams and
//! consumed by adapters as a single serialization contract. Their fields stay
//! private so adapters cannot assemble a competing representation.

use crate::commands::init::{CONFIG_FILE_NAME, RivetsConfig};
use crate::error::{ConfigError, Result};
use crate::storage::StorageBackend;
use crate::workspace_lock::RIVETS_DIR_NAME;
use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};
use std::path::{Path, PathBuf};

pub(crate) mod statistics;

/// Immutable information about the loaded Workspace configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkspaceInformation {
    workspace_root: PathBuf,
    database_path: PathBuf,
    config_path: PathBuf,
    storage_backend: String,
    issue_prefix: String,
}

impl WorkspaceInformation {
    /// Project the already-loaded configuration and resolved storage backend.
    ///
    /// `workspace_root` and the configuration path are canonical absolute
    /// identities. The database path preserves the configured backend location;
    /// it need not exist yet. The backend must be resolved from this configuration
    /// using the canonical Workspace root.
    pub fn from_config(
        workspace_root: &Path,
        config: &RivetsConfig,
        backend: &StorageBackend,
    ) -> Result<Self> {
        let workspace_root = workspace_root.canonicalize()?;
        let config_path = workspace_root
            .join(RIVETS_DIR_NAME)
            .join(CONFIG_FILE_NAME)
            .canonicalize()?;

        let database_path = backend
            .data_path()
            .ok_or_else(|| ConfigError::UnsupportedBackend(config.storage.backend.clone()))?;
        let database_path = workspace_root.join(database_path);

        Ok(Self {
            workspace_root,
            database_path,
            config_path,
            storage_backend: config.storage.backend.clone(),
            issue_prefix: config.issue_prefix.clone(),
        })
    }

    /// Canonical absolute Workspace root.
    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    /// Absolute configured storage path; unlike the root/config paths, symlinks are not canonicalized.
    pub fn database_path(&self) -> &Path {
        &self.database_path
    }

    /// Canonical absolute configuration path.
    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    /// Configured storage backend name.
    pub fn storage_backend(&self) -> &str {
        &self.storage_backend
    }

    /// Issue ID prefix from the loaded configuration.
    pub fn issue_prefix(&self) -> &str {
        &self.issue_prefix
    }
}

/// Shared Workspace-wide statistics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct WorkspaceStatistics {
    total: usize,
    by_status: StatusCounts,
    ready: usize,
    blocked_by_dependencies: usize,
    #[serde(serialize_with = "serialize_priority_counts")]
    by_priority: [usize; 5],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
struct StatusCounts {
    open: usize,
    in_progress: usize,
    closed: usize,
}

impl WorkspaceStatistics {
    /// Number of Issues in the Workspace.
    pub fn total(&self) -> usize {
        self.total
    }

    /// Number of Open Issues.
    pub fn open(&self) -> usize {
        self.by_status.open
    }

    /// Number of In Progress Issues.
    pub fn in_progress(&self) -> usize {
        self.by_status.in_progress
    }

    /// Number of Closed Issues.
    pub fn closed(&self) -> usize {
        self.by_status.closed
    }

    /// Number of intrinsically Ready Issues across all Assignments.
    pub fn ready(&self) -> usize {
        self.ready
    }

    /// Number of non-Closed Issues with at least one unresolved direct
    /// Blocking Dependency.
    pub fn blocked_by_dependencies(&self) -> usize {
        self.blocked_by_dependencies
    }

    /// Counts for priorities P0 through P4.
    pub fn by_priority(&self) -> &[usize; 5] {
        &self.by_priority
    }
}

fn serialize_priority_counts<S>(
    counts: &[usize; 5],
    serializer: S,
) -> std::result::Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut map = serializer.serialize_map(Some(5))?;
    map.serialize_entry("p0_critical", &counts[0])?;
    map.serialize_entry("p1_high", &counts[1])?;
    map.serialize_entry("p2_medium", &counts[2])?;
    map.serialize_entry("p3_low", &counts[3])?;
    map.serialize_entry("p4_backlog", &counts[4])?;
    map.end()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::init;
    use crate::error::{ConfigError, Error};
    use tempfile::TempDir;

    #[tokio::test]
    async fn information_uses_resolved_configuration() {
        let temp_dir = TempDir::new().unwrap();
        init::init(temp_dir.path(), Some("custom")).await.unwrap();

        let data_dir = temp_dir.path().join("data");
        std::fs::create_dir_all(&data_dir).unwrap();
        let data_path = data_dir.join("issues.jsonl");
        std::fs::write(&data_path, &[] as &[u8]).unwrap();

        let config_path = temp_dir.path().join(RIVETS_DIR_NAME).join(CONFIG_FILE_NAME);
        let mut config = RivetsConfig::load(&config_path).await.unwrap();
        config.storage.data_file = "data/issues.jsonl".to_string();
        config.save(&config_path).await.unwrap();
        let backend = config.storage.to_backend(temp_dir.path()).unwrap();

        let information =
            WorkspaceInformation::from_config(temp_dir.path(), &config, &backend).unwrap();
        let expected_root = temp_dir.path().canonicalize().unwrap();
        let expected_config = expected_root.join(RIVETS_DIR_NAME).join(CONFIG_FILE_NAME);
        let expected_database = data_path.canonicalize().unwrap();

        assert_eq!(information.workspace_root(), expected_root);
        assert_eq!(information.config_path(), expected_config);
        assert_eq!(information.database_path(), expected_database);
        assert_eq!(information.storage_backend(), "jsonl");
        assert_eq!(information.issue_prefix(), "custom");
        assert_eq!(
            serde_json::to_value(&information).unwrap(),
            serde_json::json!({
                "workspace_root": expected_root,
                "database_path": expected_database,
                "config_path": expected_config,
                "storage_backend": "jsonl",
                "issue_prefix": "custom",
            })
        );
    }

    #[tokio::test]
    async fn information_rejects_unsupported_backend() {
        let temp_dir = TempDir::new().unwrap();
        init::init(temp_dir.path(), Some("custom")).await.unwrap();
        let config = RivetsConfig::new("custom");

        let result =
            WorkspaceInformation::from_config(temp_dir.path(), &config, &StorageBackend::InMemory);
        assert!(matches!(
            result,
            Err(Error::Config(ConfigError::UnsupportedBackend(_)))
        ));
    }

    #[test]
    fn statistics_serialization_includes_zero_buckets() {
        let statistics = WorkspaceStatistics::default();
        assert_eq!(
            serde_json::to_value(statistics).unwrap(),
            serde_json::json!({
                "total": 0,
                "by_status": {
                    "open": 0,
                    "in_progress": 0,
                    "closed": 0,
                },
                "ready": 0,
                "blocked_by_dependencies": 0,
                "by_priority": {
                    "p0_critical": 0,
                    "p1_high": 0,
                    "p2_medium": 0,
                    "p3_low": 0,
                    "p4_backlog": 0,
                },
            })
        );
    }
}
