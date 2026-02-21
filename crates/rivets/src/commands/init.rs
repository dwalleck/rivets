//! Implementation of the `init` command.
//!
//! This module handles initialization of a new rivets repository, creating
//! the `.rivets/` directory structure with configuration and data files.
//!
//! # Example
//!
//! ```no_run
//! use rivets::commands::init;
//! use std::path::Path;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Initialize with default prefix "proj"
//! let result = init::init(Path::new("/my/project"), None).await?;
//! println!("Initialized at: {}", result.rivets_dir.display());
//!
//! // Initialize with custom prefix
//! let result = init::init(Path::new("/another/project"), Some("myapp")).await?;
//! assert_eq!(result.prefix, "myapp");
//! # Ok(())
//! # }
//! ```
//!
//! # Validation
//!
//! The [`validate_prefix`] function can be used to check prefix validity:
//!
//! ```
//! use rivets::commands::init::validate_prefix;
//!
//! assert!(validate_prefix("myproj").is_ok());
//! assert!(validate_prefix("a").is_err());  // too short
//! assert!(validate_prefix("my-proj").is_err());  // contains hyphen
//! ```

use crate::error::{ConfigError, Result};
use crate::storage::StorageBackend;
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};
use tokio::fs;

/// Default issue prefix if none specified
pub const DEFAULT_PREFIX: &str = "proj";

/// Name of the rivets directory
pub const RIVETS_DIR_NAME: &str = ".rivets";

/// Name of the configuration file
pub const CONFIG_FILE_NAME: &str = "config.yaml";

/// Name of the issues data file
pub const ISSUES_FILE_NAME: &str = "issues.jsonl";

/// Name of the gitignore file within .rivets
pub const GITIGNORE_FILE_NAME: &str = ".gitignore";

/// Minimum prefix length
pub const MIN_PREFIX_LENGTH: usize = 2;

/// Maximum prefix length
pub const MAX_PREFIX_LENGTH: usize = 20;

/// Maximum directory depth to traverse when searching for rivets root
pub const MAX_TRAVERSAL_DEPTH: usize = 256;

/// Default storage backend type
pub const DEFAULT_BACKEND: &str = "jsonl";

/// Configuration file structure for rivets
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RivetsConfig {
    /// Issue ID prefix (e.g., "proj" for "proj-abc")
    #[serde(rename = "issue-prefix")]
    pub issue_prefix: String,

    /// Storage configuration
    pub storage: StorageConfig,
}

/// Storage configuration section
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StorageConfig {
    /// Storage backend type ("jsonl" or "postgresql")
    pub backend: String,

    /// Path to the data file
    pub data_file: String,
}

impl StorageConfig {
    /// Convert the storage config to a [`StorageBackend`].
    ///
    /// # Arguments
    ///
    /// * `root_dir` - The workspace root directory to resolve relative paths
    ///
    /// # Errors
    ///
    /// Returns an error if the backend type is not recognized or not supported.
    pub fn to_backend(&self, root_dir: impl AsRef<Path>) -> Result<StorageBackend> {
        let root_dir = root_dir.as_ref();

        // Validate that data_file is a relative path with no parent traversal
        let data_file_path = Path::new(&self.data_file);
        if data_file_path.is_absolute() {
            return Err(ConfigError::AbsoluteDataPath.into());
        }
        if data_file_path
            .components()
            .any(|c| matches!(c, Component::ParentDir))
        {
            return Err(ConfigError::PathTraversal.into());
        }

        let data_path = root_dir.join(&self.data_file);

        match self.backend.as_str() {
            "jsonl" => Ok(StorageBackend::Jsonl(data_path)),
            "postgresql" => Err(ConfigError::UnsupportedBackend("PostgreSQL".to_string()).into()),
            other => Err(ConfigError::UnknownBackend(other.to_string()).into()),
        }
    }
}

impl RivetsConfig {
    /// Create a new configuration with the given prefix
    pub fn new(prefix: &str) -> Self {
        Self {
            issue_prefix: prefix.to_string(),
            storage: StorageConfig {
                backend: DEFAULT_BACKEND.to_string(),
                data_file: format!("{}/{}", RIVETS_DIR_NAME, ISSUES_FILE_NAME),
            },
        }
    }

    /// Load configuration from a file
    ///
    /// Validates the configuration after loading, including prefix validation.
    pub async fn load(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path).await?;
        let config: Self = serde_yaml::from_str(&content).map_err(|e| ConfigError::Parse {
            path: path.display().to_string(),
            source: e,
        })?;

        // Validate the prefix
        validate_prefix(&config.issue_prefix)?;

        Ok(config)
    }

    /// Save configuration to a file
    pub async fn save(&self, path: &Path) -> Result<()> {
        let content = serde_yaml::to_string(self).map_err(ConfigError::Yaml)?;
        fs::write(path, content).await?;
        Ok(())
    }
}

impl Default for RivetsConfig {
    fn default() -> Self {
        Self::new(DEFAULT_PREFIX)
    }
}

/// Result of the init command
#[derive(Debug)]
pub struct InitResult {
    /// Path to the created rivets directory
    pub rivets_dir: PathBuf,
    /// Path to the created config file
    pub config_file: PathBuf,
    /// Path to the created issues file
    pub issues_file: PathBuf,
    /// Path to the created gitignore file
    pub gitignore_file: PathBuf,
    /// The prefix used for issue IDs
    pub prefix: String,
}

/// Validate issue ID prefix format.
///
/// Requirements:
/// - 2-20 characters
/// - Alphanumeric only (letters and digits)
/// - No special characters or spaces
///
/// Note: Expects pre-trimmed input. Callers should trim whitespace before calling.
pub fn validate_prefix(prefix: &str) -> Result<()> {
    if prefix.len() < MIN_PREFIX_LENGTH {
        return Err(ConfigError::InvalidPrefix(format!(
            "Prefix must be at least {MIN_PREFIX_LENGTH} characters"
        ))
        .into());
    }

    if prefix.len() > MAX_PREFIX_LENGTH {
        return Err(ConfigError::InvalidPrefix(format!(
            "Prefix cannot exceed {MAX_PREFIX_LENGTH} characters"
        ))
        .into());
    }

    if !prefix.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(ConfigError::InvalidPrefix(
            "Prefix must contain only alphanumeric characters".to_string(),
        )
        .into());
    }

    Ok(())
}

/// Initialize a new rivets repository in the given directory.
///
/// # Arguments
///
/// - `base_dir` - The base directory where `.rivets/` will be created
/// - `prefix` - Optional issue ID prefix (defaults to "proj")
///
/// # Returns
///
/// Returns an `InitResult` containing paths to all created files.
///
/// # Errors
///
/// Returns an error if:
/// - The `.rivets/` directory already exists
/// - The prefix is invalid
/// - File system operations fail
pub async fn init(base_dir: &Path, prefix: Option<&str>) -> Result<InitResult> {
    // Trim whitespace and use the trimmed version consistently
    let prefix = prefix.unwrap_or(DEFAULT_PREFIX).trim();

    // Validate prefix (uses trimmed value)
    validate_prefix(prefix)?;

    let rivets_dir = base_dir.join(RIVETS_DIR_NAME);

    // Create the .rivets directory atomically (avoids TOCTOU race)
    match fs::create_dir(&rivets_dir).await {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(ConfigError::AlreadyInitialized(RIVETS_DIR_NAME.to_string()).into());
        }
        Err(e) => return Err(e.into()),
    }

    // Create config.yaml
    let config_file = rivets_dir.join(CONFIG_FILE_NAME);
    let config = RivetsConfig::new(prefix);
    config.save(&config_file).await?;

    // Create empty issues.jsonl
    let issues_file = rivets_dir.join(ISSUES_FILE_NAME);
    fs::write(&issues_file, &[] as &[u8]).await?;

    // Create .gitignore inside .rivets
    let gitignore_file = rivets_dir.join(GITIGNORE_FILE_NAME);
    let gitignore_content = "\
# Rivets metadata files that should not be tracked
# The issues.jsonl file should be tracked for collaboration
";
    fs::write(&gitignore_file, gitignore_content).await?;

    Ok(InitResult {
        rivets_dir,
        config_file,
        issues_file,
        gitignore_file,
        prefix: prefix.to_string(),
    })
}

/// Check if a directory has been initialized with rivets.
///
/// Returns `true` if the `.rivets/` directory exists.
pub fn is_initialized(base_dir: &Path) -> bool {
    base_dir.join(RIVETS_DIR_NAME).exists()
}

/// Find the rivets root directory by searching up the directory tree.
///
/// Starts from the given directory and traverses parent directories
/// until a `.rivets/` directory is found, the root is reached, or
/// the maximum traversal depth is exceeded.
///
/// # Returns
///
/// Returns `Some(path)` with the directory containing `.rivets/`,
/// or `None` if no rivets repository is found within the depth limit.
pub fn find_rivets_root(start_dir: &Path) -> Option<PathBuf> {
    let mut current = start_dir.to_path_buf();
    let mut depth = 0;

    loop {
        if current.join(RIVETS_DIR_NAME).exists() {
            return Some(current);
        }

        depth += 1;
        if depth > MAX_TRAVERSAL_DEPTH || !current.pop() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use tempfile::TempDir;

    // ========== Prefix Validation Tests ==========

    #[rstest]
    #[case::valid_short("ab")]
    #[case::valid_medium("proj")]
    #[case::valid_long("rivets")]
    #[case::valid_alphanumeric("test123")]
    #[case::valid_uppercase("PROJ")]
    #[case::valid_mixed_case("ProjTest123")]
    #[case::valid_max_length("a1b2c3d4e5f6g7h8i9j0")]
    fn test_validate_prefix_valid(#[case] prefix: &str) {
        assert!(validate_prefix(prefix).is_ok());
    }

    #[rstest]
    #[case::too_short_single("a", "at least 2")]
    #[case::too_short_empty("", "at least 2")]
    #[case::too_long("a".repeat(21), "cannot exceed 20")]
    #[case::hyphen("proj-test", "alphanumeric")]
    #[case::underscore("proj_test", "alphanumeric")]
    #[case::space("proj test", "alphanumeric")]
    #[case::dot("proj.test", "alphanumeric")]
    fn test_validate_prefix_invalid(#[case] prefix: impl AsRef<str>, #[case] expected_error: &str) {
        let result = validate_prefix(prefix.as_ref());
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string().to_lowercase();
        assert!(
            err_msg.contains(&expected_error.to_lowercase()),
            "Expected error to contain '{}', got: '{}'",
            expected_error,
            err_msg
        );
    }

    #[test]
    fn test_validate_prefix_rejects_whitespace() {
        // validate_prefix expects pre-trimmed input; whitespace is not alphanumeric
        let result = validate_prefix("  ab  ");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .to_lowercase()
            .contains("alphanumeric"));
    }

    // ========== RivetsConfig Tests ==========

    #[test]
    fn test_config_new() {
        let config = RivetsConfig::new("myproj");
        assert_eq!(config.issue_prefix, "myproj");
        assert_eq!(config.storage.backend, "jsonl");
        assert_eq!(config.storage.data_file, ".rivets/issues.jsonl");
    }

    #[test]
    fn test_config_default() {
        let config = RivetsConfig::default();
        assert_eq!(config.issue_prefix, DEFAULT_PREFIX);
    }

    #[tokio::test]
    async fn test_config_save_and_load() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.yaml");

        let original = RivetsConfig::new("test123");
        original.save(&config_path).await.unwrap();

        let loaded = RivetsConfig::load(&config_path).await.unwrap();
        assert_eq!(original, loaded);
    }

    #[tokio::test]
    async fn test_config_yaml_format() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.yaml");

        let config = RivetsConfig::new("myproj");
        config.save(&config_path).await.unwrap();

        let content = tokio::fs::read_to_string(&config_path).await.unwrap();

        // Verify YAML structure
        assert!(content.contains("issue-prefix: myproj"));
        assert!(content.contains("backend: jsonl"));
        assert!(content.contains("data_file: .rivets/issues.jsonl"));
    }

    #[tokio::test]
    async fn test_config_load_validates_prefix() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.yaml");

        // Write a config with an invalid prefix (too short)
        let invalid_config = r#"issue-prefix: x
storage:
  backend: jsonl
  data_file: .rivets/issues.jsonl
"#;
        tokio::fs::write(&config_path, invalid_config)
            .await
            .unwrap();

        let result = RivetsConfig::load(&config_path).await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("at least"));
    }

    // ========== StorageConfig Tests ==========

    #[test]
    fn test_to_backend_jsonl_success() {
        let temp_dir = TempDir::new().unwrap();
        let config = StorageConfig {
            backend: "jsonl".to_string(),
            data_file: "data/issues.jsonl".to_string(),
        };

        let result = config.to_backend(temp_dir.path());
        assert!(result.is_ok());

        let backend = result.unwrap();
        assert!(matches!(backend, StorageBackend::Jsonl(_)));
        assert_eq!(
            backend.data_path().unwrap(),
            temp_dir.path().join("data/issues.jsonl")
        );
    }

    #[test]
    fn test_to_backend_unknown_backend_error() {
        let temp_dir = TempDir::new().unwrap();
        let config = StorageConfig {
            backend: "unknown".to_string(),
            data_file: "issues.jsonl".to_string(),
        };

        let result = config.to_backend(temp_dir.path());
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Unknown storage backend"));
        assert!(err_msg.contains("unknown"));
    }

    #[test]
    fn test_to_backend_postgresql_not_implemented() {
        let temp_dir = TempDir::new().unwrap();
        let config = StorageConfig {
            backend: "postgresql".to_string(),
            data_file: "".to_string(),
        };

        let result = config.to_backend(temp_dir.path());
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("not yet implemented"));
    }

    #[test]
    fn test_to_backend_absolute_path_rejected() {
        let temp_dir = TempDir::new().unwrap();

        // Use a platform-appropriate absolute path
        #[cfg(windows)]
        let absolute_path = "C:\\absolute\\path\\issues.jsonl";
        #[cfg(not(windows))]
        let absolute_path = "/absolute/path/issues.jsonl";

        let config = StorageConfig {
            backend: "jsonl".to_string(),
            data_file: absolute_path.to_string(),
        };

        let result = config.to_backend(temp_dir.path());
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("relative path"));
    }

    #[rstest]
    #[case::simple_parent("..", "parent directory")]
    #[case::nested_traversal("../../etc/passwd", "parent directory")]
    #[case::mid_path_traversal("data/../../../etc/shadow", "parent directory")]
    fn test_to_backend_path_traversal_rejected(#[case] path: &str, #[case] expected_msg: &str) {
        let temp_dir = TempDir::new().unwrap();
        let config = StorageConfig {
            backend: "jsonl".to_string(),
            data_file: path.to_string(),
        };

        let result = config.to_backend(temp_dir.path());
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains(expected_msg),
            "Expected error containing '{expected_msg}', got: '{err_msg}'"
        );
    }

    // ========== Init Command Tests ==========

    #[tokio::test]
    async fn test_init_creates_directory_structure() {
        let temp_dir = TempDir::new().unwrap();

        let result = init(temp_dir.path(), None).await.unwrap();

        assert!(result.rivets_dir.exists());
        assert!(result.config_file.exists());
        assert!(result.issues_file.exists());
        assert!(result.gitignore_file.exists());
    }

    #[tokio::test]
    async fn test_init_with_custom_prefix() {
        let temp_dir = TempDir::new().unwrap();

        let result = init(temp_dir.path(), Some("myproj")).await.unwrap();

        assert_eq!(result.prefix, "myproj");

        // Verify config has the correct prefix
        let config = RivetsConfig::load(&result.config_file).await.unwrap();
        assert_eq!(config.issue_prefix, "myproj");
    }

    #[tokio::test]
    async fn test_init_trims_prefix_whitespace() {
        let temp_dir = TempDir::new().unwrap();

        // Pass prefix with surrounding whitespace
        let result = init(temp_dir.path(), Some("  myproj  ")).await.unwrap();

        // Result should have trimmed prefix
        assert_eq!(result.prefix, "myproj");

        // Config should also have trimmed prefix
        let config = RivetsConfig::load(&result.config_file).await.unwrap();
        assert_eq!(config.issue_prefix, "myproj");
    }

    #[tokio::test]
    async fn test_init_with_default_prefix() {
        let temp_dir = TempDir::new().unwrap();

        let result = init(temp_dir.path(), None).await.unwrap();

        assert_eq!(result.prefix, DEFAULT_PREFIX);
    }

    #[tokio::test]
    async fn test_init_fails_if_already_initialized() {
        let temp_dir = TempDir::new().unwrap();

        // First init should succeed
        init(temp_dir.path(), None).await.unwrap();

        // Second init should fail
        let result = init(temp_dir.path(), None).await;
        assert!(result.is_err());

        let err_msg = result.unwrap_err().to_string().to_lowercase();
        assert!(err_msg.contains("already initialized"));
    }

    #[tokio::test]
    async fn test_init_fails_with_invalid_prefix() {
        let temp_dir = TempDir::new().unwrap();

        let result = init(temp_dir.path(), Some("a")).await;
        assert!(result.is_err());

        let err_msg = result.unwrap_err().to_string().to_lowercase();
        assert!(err_msg.contains("at least 2"));
    }

    #[tokio::test]
    async fn test_init_creates_empty_issues_file() {
        let temp_dir = TempDir::new().unwrap();

        let result = init(temp_dir.path(), None).await.unwrap();

        let content = tokio::fs::read_to_string(&result.issues_file)
            .await
            .unwrap();
        assert!(content.is_empty());
    }

    #[tokio::test]
    async fn test_init_creates_gitignore() {
        let temp_dir = TempDir::new().unwrap();

        let result = init(temp_dir.path(), None).await.unwrap();

        let content = tokio::fs::read_to_string(&result.gitignore_file)
            .await
            .unwrap();
        assert!(content.contains("Rivets"));
    }

    // ========== Utility Function Tests ==========

    #[test]
    fn test_is_initialized_true() {
        let temp_dir = TempDir::new().unwrap();
        std::fs::create_dir(temp_dir.path().join(RIVETS_DIR_NAME)).unwrap();

        assert!(is_initialized(temp_dir.path()));
    }

    #[test]
    fn test_is_initialized_false() {
        let temp_dir = TempDir::new().unwrap();

        assert!(!is_initialized(temp_dir.path()));
    }

    #[test]
    fn test_find_rivets_root_in_current_dir() {
        let temp_dir = TempDir::new().unwrap();
        std::fs::create_dir(temp_dir.path().join(RIVETS_DIR_NAME)).unwrap();

        let found = find_rivets_root(temp_dir.path());
        assert_eq!(found, Some(temp_dir.path().to_path_buf()));
    }

    #[test]
    fn test_find_rivets_root_in_parent_dir() {
        let temp_dir = TempDir::new().unwrap();

        // Create .rivets in root
        std::fs::create_dir(temp_dir.path().join(RIVETS_DIR_NAME)).unwrap();

        // Create a subdirectory
        let sub_dir = temp_dir.path().join("sub").join("nested");
        std::fs::create_dir_all(&sub_dir).unwrap();

        let found = find_rivets_root(&sub_dir);
        assert_eq!(found, Some(temp_dir.path().to_path_buf()));
    }

    #[test]
    fn test_find_rivets_root_not_found() {
        let temp_dir = TempDir::new().unwrap();

        let found = find_rivets_root(temp_dir.path());
        assert!(found.is_none());
    }
}
