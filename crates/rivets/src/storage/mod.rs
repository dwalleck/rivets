//! Storage abstraction layer for rivets.
//!
//! This module provides the core storage trait and factory for creating
//! storage backends. It supports multiple implementations:
//!
//! - **In-memory**: Fast, ephemeral storage backed by HashMap and petgraph
//! - **JSONL**: Persistent file-based storage using JSON Lines format
//! - **PostgreSQL**: Production-ready relational database (future)
//!
//! # Architecture
//!
//! The storage layer uses an async trait to enable both blocking (in-memory)
//! and truly async (PostgreSQL) implementations. The trait is object-safe,
//! allowing for dynamic dispatch via `Box<dyn IssueStorage>`.
//!
//! # Test Utilities
//!
//! This module provides a [`MockStorage`] implementation for testing code that
//! depends on the [`IssueStorage`] trait. To use it in your tests, enable the
//! `test-util` feature:
//!
//! ```toml
//! [dev-dependencies]
//! rivets = { version = "...", features = ["test-util"] }
//! ```
//!
//! Then use `MockStorage` in your tests:
//!
//! ```rust,ignore
//! use rivets::storage::{MockStorage, IssueStorage};
//!
//! #[tokio::test]
//! async fn test_with_mock_storage() {
//!     let storage: Box<dyn IssueStorage> = Box::new(MockStorage::new());
//!     // Use storage in tests...
//! }
//! ```
//!
//! # Example
//!
//! ```no_run
//! use rivets::storage::{IssueStorage, StorageBackend, create_storage};
//! use rivets::domain::{NewIssue, IssueType};
//!
//! #[tokio::main(flavor = "current_thread")]
//! async fn main() -> anyhow::Result<()> {
//!     // Create in-memory storage with a prefix for issue IDs.
//!     // In real applications, the prefix comes from RivetsConfig.issue_prefix.
//!     let mut storage = create_storage(StorageBackend::InMemory, "myapp".to_string()).await?;
//!
//!     // Create an issue
//!     let new_issue = NewIssue {
//!         title: "Implement feature X".to_string(),
//!         description: "Add new functionality".to_string(),
//!         priority: 1,
//!         issue_type: IssueType::Feature,
//!         assignee: Some("alice".to_string()),
//!         labels: vec![],
//!         design: None,
//!         acceptance_criteria: None,
//!         notes: None,
//!         external_ref: None,
//!         dependencies: vec![],
//!     };
//!
//!     let issue = storage.create(new_issue).await?;
//!     println!("Created issue: {}", issue.id);
//!
//!     Ok(())
//! }
//! ```

use crate::domain::{
    Dependency, DependencyType, Issue, IssueFilter, IssueId, IssueUpdate, NewIssue, SortPolicy,
};
use crate::error::Result;
use async_trait::async_trait;
use std::path::{Path, PathBuf};

// Storage backend implementations
pub mod in_memory;

/// Core storage trait for issue management.
///
/// This trait defines the interface for all storage backends. Implementations
/// must be `Send + Sync` to support concurrent access in async contexts.
///
/// # Method Categories
///
/// - **CRUD**: `create`, `get`, `update`, `delete`
/// - **Dependencies**: `add_dependency`, `remove_dependency`, `get_dependencies`, `get_dependents`, `has_cycle`
/// - **Queries**: `list`, `ready_to_work`, `blocked_issues`
/// - **Batch Operations**: `import_issues`, `export_all`
/// - **Persistence**: `save`
///
/// # Error Handling
///
/// All methods return `Result<T>` where the error type includes:
/// - `IssueNotFound`: Requested issue doesn't exist
/// - `HasDependents`: Cannot delete issue with dependents
/// - `CircularDependency`: Operation would create a cycle
/// - `Storage`: Backend-specific errors
///
/// # Thread Safety
///
/// Implementations should use appropriate synchronization primitives
/// (`Arc<Mutex<T>>` for in-memory, database transactions for PostgreSQL)
/// to ensure thread-safe access.
#[async_trait]
pub trait IssueStorage: Send + Sync {
    // ========== CRUD Operations ==========

    /// Create a new issue.
    ///
    /// Generates a unique ID for the issue and sets creation timestamps.
    ///
    /// # Implementation Requirements
    ///
    /// Implementations **MUST** validate input by calling `issue.validate()`
    /// before creating the issue. This ensures consistent validation across
    /// all storage backends.
    ///
    /// # Errors
    ///
    /// Returns `Error::InvalidPriority` if priority is not in range 0-4.
    /// Returns `Error::Storage` if title validation fails or other constraints are violated.
    async fn create(&mut self, issue: NewIssue) -> Result<Issue>;

    /// Get an issue by ID.
    ///
    /// Returns `None` if the issue doesn't exist.
    async fn get(&self, id: &IssueId) -> Result<Option<Issue>>;

    /// Update an existing issue.
    ///
    /// Only fields present in `updates` are modified. Returns the updated issue.
    ///
    /// # Errors
    ///
    /// Returns `Error::IssueNotFound` if the issue doesn't exist.
    async fn update(&mut self, id: &IssueId, updates: IssueUpdate) -> Result<Issue>;

    /// Delete an issue.
    ///
    /// Removes the issue and all its outgoing dependencies. Fails if other
    /// issues depend on this one (to prevent orphaned dependencies).
    ///
    /// # Errors
    ///
    /// - `Error::IssueNotFound` if the issue doesn't exist
    /// - `Error::HasDependents` if other issues depend on this issue
    async fn delete(&mut self, id: &IssueId) -> Result<()>;

    // ========== Dependency Management ==========

    /// Add a dependency between two issues.
    ///
    /// Checks for cycles before adding. The dependency is directional:
    /// `from` depends on `to`.
    ///
    /// # Errors
    ///
    /// - `Error::IssueNotFound` if either issue doesn't exist
    /// - `Error::CircularDependency` if this would create a cycle
    async fn add_dependency(
        &mut self,
        from: &IssueId,
        to: &IssueId,
        dep_type: DependencyType,
    ) -> Result<()>;

    /// Remove a dependency between two issues.
    ///
    /// # Errors
    ///
    /// - `Error::DependencyNotFound` if the dependency doesn't exist
    async fn remove_dependency(&mut self, from: &IssueId, to: &IssueId) -> Result<()>;

    /// Get all dependencies for an issue.
    ///
    /// Returns issues that this issue depends on.
    async fn get_dependencies(&self, id: &IssueId) -> Result<Vec<Dependency>>;

    /// Get all dependents of an issue.
    ///
    /// Returns issues that depend on this issue.
    async fn get_dependents(&self, id: &IssueId) -> Result<Vec<Dependency>>;

    /// Check if adding a dependency would create a cycle.
    ///
    /// Returns `true` if adding `from -> to` would create a circular dependency.
    async fn has_cycle(&self, from: &IssueId, to: &IssueId) -> Result<bool>;

    /// Get the full dependency tree for an issue.
    ///
    /// Performs a breadth-first traversal of the dependency graph starting from
    /// the given issue, returning all transitive dependencies with their depth
    /// in the tree. The result is ordered by traversal order (BFS).
    ///
    /// # Arguments
    ///
    /// * `id` - The root issue ID to start traversal from
    /// * `max_depth` - Optional maximum depth to traverse (None for unlimited)
    ///
    /// # Returns
    ///
    /// A vector of tuples containing:
    /// - The dependency relationship
    /// - The depth in the tree (1 for direct dependencies, 2 for their dependencies, etc.)
    ///
    /// # Example
    ///
    /// For a dependency chain A -> B -> C, calling `get_dependency_tree(&A, None)` returns:
    /// - (B, 1) - direct dependency
    /// - (C, 2) - transitive dependency
    ///
    /// # Errors
    ///
    /// - `Error::IssueNotFound` if the issue doesn't exist
    async fn get_dependency_tree(
        &self,
        id: &IssueId,
        max_depth: Option<usize>,
    ) -> Result<Vec<(Dependency, usize)>>;

    // ========== Queries ==========

    /// List issues matching the given filter.
    ///
    /// If no filter is provided, returns all non-closed issues.
    async fn list(&self, filter: &IssueFilter) -> Result<Vec<Issue>>;

    /// Find issues ready to work on.
    ///
    /// Returns issues that are:
    /// - Not closed
    /// - Not blocked by dependencies
    /// - Not blocked transitively through parent-child relationships
    ///
    /// # Sort Policies
    ///
    /// The `sort_policy` parameter controls result ordering:
    /// - `Hybrid` (default): Recent issues (< 48h) by priority, older by age
    /// - `Priority`: Strict P0 -> P1 -> P2 -> P3 -> P4 ordering
    /// - `Oldest`: Creation date ascending (oldest first)
    ///
    /// # Arguments
    ///
    /// * `filter` - Optional filter to narrow results by status, priority, type, assignee, or label
    /// * `sort_policy` - Sort order for results (defaults to Hybrid if None)
    async fn ready_to_work(
        &self,
        filter: Option<&IssueFilter>,
        sort_policy: Option<SortPolicy>,
    ) -> Result<Vec<Issue>>;

    /// Get all blocked issues.
    ///
    /// Returns tuples of (blocked issue, blocking issues).
    async fn blocked_issues(&self) -> Result<Vec<(Issue, Vec<Issue>)>>;

    // ========== Atomic Label Operations ==========

    /// Atomically add a label to an issue.
    ///
    /// This operation is atomic - no TOCTOU race condition between read and write.
    /// If the label already exists, this is a no-op and returns success.
    ///
    /// # Errors
    ///
    /// - `Error::IssueNotFound` if the issue doesn't exist
    async fn add_label(&mut self, id: &IssueId, label: &str) -> Result<Issue>;

    /// Atomically remove a label from an issue.
    ///
    /// This operation is atomic - no TOCTOU race condition between read and write.
    /// If the label doesn't exist, this is a no-op and returns success.
    ///
    /// # Errors
    ///
    /// - `Error::IssueNotFound` if the issue doesn't exist
    async fn remove_label(&mut self, id: &IssueId, label: &str) -> Result<Issue>;

    // ========== Batch Operations ==========

    /// Import multiple issues.
    ///
    /// Used for bulk loading from JSONL files or database migrations.
    /// Dependencies are resolved after all issues are imported.
    async fn import_issues(&mut self, issues: Vec<Issue>) -> Result<()>;

    /// Export all issues.
    ///
    /// Returns all issues in the storage, suitable for JSONL export or backup.
    async fn export_all(&self) -> Result<Vec<Issue>>;

    // ========== Persistence ==========

    /// Save changes to persistent storage.
    ///
    /// This method takes `&self` (not `&mut self`) to allow saving from shared
    /// references. Implementations use interior mutability (e.g., `Arc<Mutex<>>`)
    /// to handle this safely. This design choice enables:
    /// - Saving after read-only queries without requiring exclusive access
    /// - Periodic auto-save operations from background tasks
    /// - Explicit save points in transaction-like workflows
    ///
    /// For in-memory storage with JSONL backing, this writes to disk.
    /// For database backends, this is typically a no-op (auto-committed).
    async fn save(&self) -> Result<()>;

    /// Reload state from persistent storage, discarding in-memory changes.
    ///
    /// This method restores the storage to match the on-disk state, discarding
    /// any in-memory modifications that haven't been saved. It's essential for
    /// maintaining consistency in long-running processes (like MCP servers)
    /// when a `save()` operation fails.
    ///
    /// # Use Case
    ///
    /// When an operation modifies in-memory state but `save()` fails:
    /// 1. In-memory state has unsaved changes
    /// 2. On-disk state is unchanged
    /// 3. Subsequent operations would see inconsistent state
    /// 4. Call `reload()` to restore in-memory state to match disk
    ///
    /// # Implementation Notes
    ///
    /// - **JSONL backend**: Re-reads the file and rebuilds in-memory state
    /// - **In-memory only**: No-op (there's no persistent state to reload from)
    /// - **Database backends**: No-op (state is always consistent with DB)
    ///
    /// # Errors
    ///
    /// Returns an error if the backing file cannot be read or parsed.
    async fn reload(&mut self) -> Result<()>;
}

/// Storage backend configuration.
///
/// Determines which storage implementation to use.
#[derive(Debug, Clone)]
pub enum StorageBackend {
    /// In-memory storage (ephemeral)
    InMemory,

    /// JSONL file storage (persistent)
    Jsonl(PathBuf),

    /// PostgreSQL database (persistent, production-ready)
    #[allow(dead_code)]
    PostgreSQL(String),
}

impl StorageBackend {
    /// Returns the data file path for file-based backends.
    ///
    /// Returns `Some(path)` for backends that use a file (e.g., JSONL),
    /// or `None` for backends that don't (e.g., InMemory, PostgreSQL).
    pub fn data_path(&self) -> Option<&Path> {
        match self {
            StorageBackend::Jsonl(path) => Some(path),
            StorageBackend::InMemory | StorageBackend::PostgreSQL(_) => None,
        }
    }
}

/// Wrapper that adds JSONL file persistence to any storage backend.
///
/// This wrapper holds a reference to the file path and implements `save()`
/// by writing all issues to the JSONL file atomically.
struct JsonlBackedStorage {
    inner: Box<dyn IssueStorage>,
    path: PathBuf,
    prefix: String,
}

impl JsonlBackedStorage {
    /// Returns an immutable reference to the inner storage implementation.
    ///
    /// This is useful for testing or when you need to access the underlying
    /// storage without the JSONL persistence wrapper.
    #[allow(dead_code)]
    pub(crate) fn inner(&self) -> &dyn IssueStorage {
        self.inner.as_ref()
    }
}

#[async_trait]
impl IssueStorage for JsonlBackedStorage {
    async fn create(&mut self, issue: NewIssue) -> Result<Issue> {
        self.inner.create(issue).await
    }

    async fn get(&self, id: &IssueId) -> Result<Option<Issue>> {
        self.inner.get(id).await
    }

    async fn update(&mut self, id: &IssueId, updates: IssueUpdate) -> Result<Issue> {
        self.inner.update(id, updates).await
    }

    async fn delete(&mut self, id: &IssueId) -> Result<()> {
        self.inner.delete(id).await
    }

    async fn add_dependency(
        &mut self,
        from: &IssueId,
        to: &IssueId,
        dep_type: DependencyType,
    ) -> Result<()> {
        self.inner.add_dependency(from, to, dep_type).await
    }

    async fn remove_dependency(&mut self, from: &IssueId, to: &IssueId) -> Result<()> {
        self.inner.remove_dependency(from, to).await
    }

    async fn get_dependencies(&self, id: &IssueId) -> Result<Vec<Dependency>> {
        self.inner.get_dependencies(id).await
    }

    async fn get_dependents(&self, id: &IssueId) -> Result<Vec<Dependency>> {
        self.inner.get_dependents(id).await
    }

    async fn has_cycle(&self, from: &IssueId, to: &IssueId) -> Result<bool> {
        self.inner.has_cycle(from, to).await
    }

    async fn get_dependency_tree(
        &self,
        id: &IssueId,
        max_depth: Option<usize>,
    ) -> Result<Vec<(Dependency, usize)>> {
        self.inner.get_dependency_tree(id, max_depth).await
    }

    async fn list(&self, filter: &IssueFilter) -> Result<Vec<Issue>> {
        self.inner.list(filter).await
    }

    async fn ready_to_work(
        &self,
        filter: Option<&IssueFilter>,
        sort_policy: Option<SortPolicy>,
    ) -> Result<Vec<Issue>> {
        self.inner.ready_to_work(filter, sort_policy).await
    }

    async fn blocked_issues(&self) -> Result<Vec<(Issue, Vec<Issue>)>> {
        self.inner.blocked_issues().await
    }

    async fn add_label(&mut self, id: &IssueId, label: &str) -> Result<Issue> {
        self.inner.add_label(id, label).await
    }

    async fn remove_label(&mut self, id: &IssueId, label: &str) -> Result<Issue> {
        self.inner.remove_label(id, label).await
    }

    async fn import_issues(&mut self, issues: Vec<Issue>) -> Result<()> {
        self.inner.import_issues(issues).await
    }

    async fn export_all(&self) -> Result<Vec<Issue>> {
        self.inner.export_all().await
    }

    async fn save(&self) -> Result<()> {
        in_memory::save_to_jsonl(self.inner.as_ref(), &self.path).await
    }

    async fn reload(&mut self) -> Result<()> {
        // Reload from the JSONL file, replacing the inner storage
        if self.path.exists() {
            let (new_storage, warnings) =
                in_memory::load_from_jsonl(&self.path, self.prefix.clone()).await?;
            if !warnings.is_empty() {
                for warning in &warnings {
                    tracing::warn!(warning = ?warning, "JSONL reload warning");
                }
            }
            self.inner = new_storage;
        } else {
            // File doesn't exist - reset to empty storage
            self.inner = in_memory::new_in_memory_storage(self.prefix.clone());
        }
        Ok(())
    }
}

/// Create a storage instance for the given backend.
///
/// This factory function returns a trait object that can be used
/// polymorphically regardless of the backend implementation.
///
/// # Arguments
///
/// * `backend` - The storage backend to use
/// * `prefix` - The prefix for generated issue IDs (e.g., "proj", "myapp")
///
/// # Example
///
/// ```no_run
/// use rivets::storage::{create_storage, StorageBackend};
///
/// #[tokio::main(flavor = "current_thread")]
/// async fn main() -> anyhow::Result<()> {
///     let storage = create_storage(StorageBackend::InMemory, "proj".to_string()).await?;
///     // Use storage...
///     Ok(())
/// }
/// ```
///
/// # Errors
///
/// - `Error::Io` if file operations fail (JSONL backend)
/// - `Error::Storage` for backend-specific initialization errors
pub async fn create_storage(
    backend: StorageBackend,
    prefix: String,
) -> Result<Box<dyn IssueStorage>> {
    match backend {
        StorageBackend::InMemory => Ok(in_memory::new_in_memory_storage(prefix)),
        StorageBackend::Jsonl(path) => {
            // JSONL backend uses InMemoryStorage with file persistence
            let inner = if path.exists() {
                let (storage, warnings) = in_memory::load_from_jsonl(&path, prefix.clone()).await?;
                if !warnings.is_empty() {
                    // Log warnings but continue - storage is still usable
                    for warning in &warnings {
                        tracing::warn!(warning = ?warning, "JSONL load warning");
                    }
                }
                storage
            } else {
                // File doesn't exist yet (first run) - create empty storage
                in_memory::new_in_memory_storage(prefix.clone())
            };
            // Wrap in JsonlBackedStorage so save() writes to file
            Ok(Box::new(JsonlBackedStorage {
                inner,
                path,
                prefix,
            }))
        }
        StorageBackend::PostgreSQL(_conn_str) => {
            // TODO: Implement PostgreSQL backend
            Err(crate::error::Error::Storage(
                "PostgreSQL storage backend not yet implemented".to_string(),
            ))
        }
    }
}

// ========== Test Utilities ==========

/// The hardcoded issue ID returned by [`MockStorage`].
#[cfg(any(test, feature = "test-util"))]
pub const MOCK_ISSUE_ID: &str = "test-1";

/// Mock implementation of [`IssueStorage`] for testing.
///
/// This is a **stateless** mock that provides a minimal implementation of the storage
/// trait for verifying trait object usage. It always returns hardcoded data for issue
/// "test-1" but does not persist any data between calls. Timestamps are generated fresh
/// on each call.
///
/// # Availability
///
/// This type is available when:
/// - Running tests (`#[cfg(test)]`)
/// - The `test-util` feature is enabled
///
/// # Example
///
/// ```rust,ignore
/// // In your Cargo.toml:
/// // [dev-dependencies]
/// // rivets = { path = "...", features = ["test-util"] }
///
/// use rivets::storage::{MockStorage, IssueStorage};
///
/// #[tokio::test]
/// async fn test_my_code_with_mock_storage() {
///     let storage: Box<dyn IssueStorage> = Box::new(MockStorage::new());
///     // Use storage in tests...
/// }
/// ```
///
/// # Behavior
///
/// - `create`: Always returns a new issue with ID "test-1"
/// - `get`: Returns `Some` only for ID "test-1", `None` otherwise
/// - `list`, `ready_to_work`, `blocked_issues`: Return empty vectors
/// - `get_dependencies`, `get_dependents`: Return empty vectors
/// - `has_cycle`: Always returns `false`
/// - Other methods: Unimplemented (will panic if called)
///
/// # When to Use MockStorage vs In-Memory Storage
///
/// **Use `MockStorage` when:**
/// - You only need to verify trait object compilation and basic usage
/// - You don't need to actually store or retrieve real data
/// - You're testing code paths that accept `Box<dyn IssueStorage>`
///
/// **Use [`in_memory::new_in_memory_storage`] when:**
/// - You need actual CRUD functionality in tests
/// - You're testing dependency graphs and relationships
/// - You need to verify business logic with real data persistence
///
/// # Thread Safety
///
/// `MockStorage` is inherently thread-safe as it contains no mutable state
/// (it's a zero-sized type). However, it doesn't provide any actual storage
/// functionality. For testing concurrent access patterns, use the in-memory
/// backend which properly handles synchronization.
#[cfg(any(test, feature = "test-util"))]
#[derive(Clone, Copy)]
#[non_exhaustive]
pub struct MockStorage;

#[cfg(any(test, feature = "test-util"))]
impl MockStorage {
    /// Create a new MockStorage instance.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use rivets::storage::MockStorage;
    ///
    /// let storage = MockStorage::new();
    /// ```
    pub fn new() -> Self {
        Self
    }

    /// Creates a test issue with the given ID.
    ///
    /// This is useful for creating expected values in downstream tests that need
    /// to match the format returned by [`MockStorage`].
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use rivets::storage::{MockStorage, MOCK_ISSUE_ID};
    /// use rivets::domain::IssueId;
    ///
    /// let expected = MockStorage::create_test_issue(IssueId::new(MOCK_ISSUE_ID));
    /// ```
    pub fn create_test_issue(id: IssueId) -> Issue {
        use crate::domain::{IssueStatus, IssueType};
        use chrono::Utc;

        Issue {
            id,
            title: "Test Issue".to_string(),
            description: "Test description".to_string(),
            status: IssueStatus::Open,
            priority: 1,
            issue_type: IssueType::Task,
            assignee: None,
            labels: vec![],
            design: None,
            acceptance_criteria: None,
            notes: None,
            external_ref: None,
            dependencies: vec![],
            created_at: Utc::now(),
            updated_at: Utc::now(),
            closed_at: None,
        }
    }
}

#[cfg(any(test, feature = "test-util"))]
impl Default for MockStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(any(test, feature = "test-util"))]
#[async_trait]
impl IssueStorage for MockStorage {
    async fn create(&mut self, _issue: NewIssue) -> Result<Issue> {
        Ok(Self::create_test_issue(IssueId::new(MOCK_ISSUE_ID)))
    }

    async fn get(&self, id: &IssueId) -> Result<Option<Issue>> {
        if id.as_str() == MOCK_ISSUE_ID {
            Ok(Some(Self::create_test_issue(id.clone())))
        } else {
            Ok(None)
        }
    }

    async fn update(&mut self, _id: &IssueId, _updates: IssueUpdate) -> Result<Issue> {
        unimplemented!(
            "MockStorage::update() is not implemented. Use in_memory::new_in_memory_storage() for full CRUD."
        )
    }

    async fn delete(&mut self, _id: &IssueId) -> Result<()> {
        unimplemented!(
            "MockStorage::delete() is not implemented. Use in_memory::new_in_memory_storage() for full CRUD."
        )
    }

    async fn add_dependency(
        &mut self,
        _from: &IssueId,
        _to: &IssueId,
        _dep_type: DependencyType,
    ) -> Result<()> {
        unimplemented!(
            "MockStorage::add_dependency() is not implemented. Use in_memory::new_in_memory_storage() for full CRUD."
        )
    }

    async fn remove_dependency(&mut self, _from: &IssueId, _to: &IssueId) -> Result<()> {
        unimplemented!(
            "MockStorage::remove_dependency() is not implemented. Use in_memory::new_in_memory_storage() for full CRUD."
        )
    }

    async fn get_dependencies(&self, _id: &IssueId) -> Result<Vec<Dependency>> {
        Ok(vec![])
    }

    async fn get_dependents(&self, _id: &IssueId) -> Result<Vec<Dependency>> {
        Ok(vec![])
    }

    async fn has_cycle(&self, _from: &IssueId, _to: &IssueId) -> Result<bool> {
        Ok(false)
    }

    async fn get_dependency_tree(
        &self,
        _id: &IssueId,
        _max_depth: Option<usize>,
    ) -> Result<Vec<(Dependency, usize)>> {
        Ok(vec![])
    }

    async fn list(&self, _filter: &IssueFilter) -> Result<Vec<Issue>> {
        Ok(vec![])
    }

    async fn ready_to_work(
        &self,
        _filter: Option<&IssueFilter>,
        _sort_policy: Option<SortPolicy>,
    ) -> Result<Vec<Issue>> {
        Ok(vec![])
    }

    async fn blocked_issues(&self) -> Result<Vec<(Issue, Vec<Issue>)>> {
        Ok(vec![])
    }

    async fn add_label(&mut self, _id: &IssueId, _label: &str) -> Result<Issue> {
        unimplemented!(
            "MockStorage::add_label() is not implemented. Use in_memory::new_in_memory_storage() for full CRUD."
        )
    }

    async fn remove_label(&mut self, _id: &IssueId, _label: &str) -> Result<Issue> {
        unimplemented!(
            "MockStorage::remove_label() is not implemented. Use in_memory::new_in_memory_storage() for full CRUD."
        )
    }

    async fn import_issues(&mut self, _issues: Vec<Issue>) -> Result<()> {
        Ok(())
    }

    async fn export_all(&self) -> Result<Vec<Issue>> {
        Ok(vec![])
    }

    async fn save(&self) -> Result<()> {
        Ok(())
    }

    async fn reload(&mut self) -> Result<()> {
        // MockStorage has no backing store, so reload is a no-op
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::IssueType;

    #[tokio::test]
    async fn test_trait_object_usage() {
        // Verify that IssueStorage is object-safe and can be used with Box<dyn>
        let mut storage: Box<dyn IssueStorage> = Box::new(MockStorage::new());

        let new_issue = NewIssue {
            title: "Test".to_string(),
            description: "Test".to_string(),
            priority: 1,
            issue_type: IssueType::Task,
            assignee: None,
            labels: vec![],
            design: None,
            acceptance_criteria: None,
            notes: None,
            external_ref: None,
            dependencies: vec![],
        };

        let issue = storage.create(new_issue).await.unwrap();
        assert_eq!(issue.id.as_str(), MOCK_ISSUE_ID);
        assert_eq!(issue.title, "Test Issue");
    }

    #[tokio::test]
    async fn test_get_issue() {
        let storage: Box<dyn IssueStorage> = Box::new(MockStorage::new());

        // Test existing issue
        let result = storage.get(&IssueId::new(MOCK_ISSUE_ID)).await.unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().id.as_str(), MOCK_ISSUE_ID);

        // Test non-existing issue
        let result = storage.get(&IssueId::new("test-99")).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_empty_queries() {
        let storage: Box<dyn IssueStorage> = Box::new(MockStorage::new());

        // Test that query methods return empty results
        let filter = IssueFilter::default();
        assert!(storage.list(&filter).await.unwrap().is_empty());
        assert!(storage.ready_to_work(None, None).await.unwrap().is_empty());
        assert!(storage.blocked_issues().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_dependencies() {
        let storage: Box<dyn IssueStorage> = Box::new(MockStorage::new());

        let id = IssueId::new(MOCK_ISSUE_ID);
        assert!(storage.get_dependencies(&id).await.unwrap().is_empty());
        assert!(storage.get_dependents(&id).await.unwrap().is_empty());
        assert!(!storage
            .has_cycle(&id, &IssueId::new("test-2"))
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn test_mock_storage_copy_semantics() {
        let mock = MockStorage::new();
        let _copy1 = mock;
        let _copy2 = mock; // Still usable - Copy semantics work
        let _: Box<dyn IssueStorage> = Box::new(mock);
    }

    #[tokio::test]
    async fn test_jsonl_reload_restores_disk_state() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let jsonl_path = temp_dir.path().join("issues.jsonl");

        // Create storage and add an issue
        let mut storage = create_storage(StorageBackend::Jsonl(jsonl_path.clone()), "test".into())
            .await
            .unwrap();

        let new_issue = NewIssue {
            title: "Original Title".to_string(),
            description: "Original description".to_string(),
            priority: 2,
            issue_type: IssueType::Task,
            assignee: None,
            labels: vec![],
            design: None,
            acceptance_criteria: None,
            notes: None,
            external_ref: None,
            dependencies: vec![],
        };

        let created = storage.create(new_issue).await.unwrap();
        let issue_id = created.id.clone();
        storage.save().await.unwrap();

        // Modify in memory without saving
        let update = IssueUpdate {
            title: Some("Modified Title".to_string()),
            ..Default::default()
        };
        let modified = storage.update(&issue_id, update).await.unwrap();
        assert_eq!(modified.title, "Modified Title");

        // Verify in-memory state is modified
        let before_reload = storage.get(&issue_id).await.unwrap().unwrap();
        assert_eq!(before_reload.title, "Modified Title");

        // Reload from disk
        storage.reload().await.unwrap();

        // Verify in-memory state matches disk (original title)
        let after_reload = storage.get(&issue_id).await.unwrap().unwrap();
        assert_eq!(after_reload.title, "Original Title");
    }

    #[tokio::test]
    async fn test_jsonl_reload_empty_file() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let jsonl_path = temp_dir.path().join("issues.jsonl");

        // Create storage, add issue, save
        let mut storage = create_storage(StorageBackend::Jsonl(jsonl_path.clone()), "test".into())
            .await
            .unwrap();

        let new_issue = NewIssue {
            title: "Test Issue".to_string(),
            description: "".to_string(),
            priority: 2,
            issue_type: IssueType::Task,
            assignee: None,
            labels: vec![],
            design: None,
            acceptance_criteria: None,
            notes: None,
            external_ref: None,
            dependencies: vec![],
        };

        let created = storage.create(new_issue).await.unwrap();
        let issue_id = created.id.clone();
        storage.save().await.unwrap();

        // Delete the file to simulate corruption/missing file
        std::fs::remove_file(&jsonl_path).unwrap();

        // Reload should reset to empty storage
        storage.reload().await.unwrap();

        // Issue should no longer exist
        let result = storage.get(&issue_id).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_in_memory_reload_is_noop() {
        let mut storage = create_storage(StorageBackend::InMemory, "test".into())
            .await
            .unwrap();

        let new_issue = NewIssue {
            title: "Test Issue".to_string(),
            description: "".to_string(),
            priority: 2,
            issue_type: IssueType::Task,
            assignee: None,
            labels: vec![],
            design: None,
            acceptance_criteria: None,
            notes: None,
            external_ref: None,
            dependencies: vec![],
        };

        let created = storage.create(new_issue).await.unwrap();
        let issue_id = created.id.clone();

        // Reload for in-memory is a no-op, data should persist
        storage.reload().await.unwrap();

        // Issue should still exist
        let result = storage.get(&issue_id).await.unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().title, "Test Issue");
    }
}
