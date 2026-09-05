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
//! use rivets::domain::{IssueKind, NewIssue};
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
//!         issue_kind: IssueKind::Feature,
//!         assignee: Some("alice".to_string()),
//!         labels: vec![],
//!         design: None,
//!         acceptance_criteria: None,
//!         initial_note: None,
//!         prerequisites: vec![],
//!     };
//!
//!     let issue = storage.create(new_issue).await?;
//!     println!("Created issue: {}", issue.id);
//!
//!     Ok(())
//! }
//! ```

use crate::domain::{
    BlockingDependency, DiscoveryOrigin, Issue, IssueFilter, IssueId, IssueUpdate, Label, NewIssue,
    NewResource, Parentage, ReadyFilter, RelatedAssociation, ResourceId, ResourceUpdate,
    SortPolicy,
};
use crate::error::{PartialLoadError, Result, SkippedIssueRecordCause, StorageError};
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use tokio::io::AsyncReadExt;
use tokio::sync::RwLock;

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
/// - **Relationships**: role-safe typed Blocking, Related, and Discovery operations
/// - **Queries**: `list`, `ready_to_work`, `blocked_issues`, and relationship lists
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

    /// Atomically Claim an Open, unblocked Issue for one Assignee.
    ///
    /// Repeating the current Assignee is an idempotent success. A different
    /// current Assignee returns [`crate::domain::AssignmentError::AlreadyClaimed`].
    async fn claim(&mut self, id: &IssueId, claimant: &str) -> Result<Issue>;

    /// Atomically Release an Open Issue from its expected current Assignee.
    ///
    /// Blocked Open Issues may be released; In Progress and Closed Issues may
    /// not.
    async fn release(&mut self, id: &IssueId, expected_assignee: &str) -> Result<Issue>;

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
    // ========== Blocking Dependency Management ==========

    /// Add a directed Blocking Dependency.
    ///
    /// # Errors
    ///
    /// Returns an error when either endpoint is missing, the relationship
    /// already exists, or the Blocking-only graph would become cyclic.
    async fn add_blocking_dependency(&mut self, dependency: BlockingDependency) -> Result<()>;

    /// Remove one directed Blocking Dependency without affecting other kinds.
    async fn remove_blocking_dependency(&mut self, dependency: &BlockingDependency) -> Result<()>;

    /// Return Blocking Dependencies whose dependent is `dependent_id`.
    async fn blocking_prerequisites(
        &self,
        dependent_id: &IssueId,
    ) -> Result<Vec<BlockingDependency>>;

    /// Return Blocking Dependencies whose prerequisite is `prerequisite_id`.
    async fn blocking_dependents(
        &self,
        prerequisite_id: &IssueId,
    ) -> Result<Vec<BlockingDependency>>;

    /// Return the transitive Blocking prerequisite tree in breadth-first order.
    async fn blocking_dependency_tree(
        &self,
        dependent_id: &IssueId,
        max_depth: Option<usize>,
    ) -> Result<Vec<(BlockingDependency, usize)>>;

    // ========== Related Association Operations ==========

    /// Add a symmetric, non-blocking Related Association.
    ///
    /// Endpoint order is canonicalized by [`RelatedAssociation`]. Re-adding
    /// an existing association in either order is idempotent.
    async fn add_related_association(&mut self, association: RelatedAssociation) -> Result<()>;

    /// Remove a Related Association, accepting either endpoint order.
    async fn remove_related_association(&mut self, association: &RelatedAssociation) -> Result<()>;

    /// List Related Associations touching an Issue in deterministic order.
    async fn related_associations(&self, issue_id: &IssueId) -> Result<Vec<RelatedAssociation>>;

    // ========== Discovery Origin Operations ==========

    /// Add directed provenance from a discovered Issue to its source Issue.
    async fn add_discovery_origin(&mut self, origin: DiscoveryOrigin) -> Result<()>;

    /// Remove directed provenance with the exact discovered/source roles.
    async fn remove_discovery_origin(&mut self, origin: &DiscoveryOrigin) -> Result<()>;

    /// List provenance origins for an Issue in its discovered role.
    async fn discovery_origins(
        &self,
        discovered_issue_id: &IssueId,
    ) -> Result<Vec<DiscoveryOrigin>>;
    // ========== Parentage Management ==========

    /// Attach an unparented child to one Epic parent.
    ///
    /// Repeating the same Parentage is idempotent. A different existing parent
    /// must be replaced through [`move_parent`](Self::move_parent).
    async fn set_parent(&mut self, parentage: Parentage) -> Result<Parentage>;

    /// Remove and return one child's Parentage.
    async fn clear_parent(&mut self, child_id: &IssueId) -> Result<Parentage>;

    /// Atomically validate and replace one child's existing Parentage.
    ///
    /// Returns the previous Parentage from the same atomic operation. A retry
    /// targeting the current parent returns that unchanged Parentage.
    async fn move_parent(&mut self, parentage: Parentage) -> Result<Parentage>;

    /// Return one child's Parentage, or `None` when the existing child is unparented.
    async fn parent_of(&self, child_id: &IssueId) -> Result<Option<Parentage>>;
    // ========== Queries ==========

    /// List issues matching the given filter.
    ///
    /// If no filter is provided, returns all non-closed issues.
    async fn list(&self, filter: &IssueFilter) -> Result<Vec<Issue>>;

    /// Find issues ready to work on.
    ///
    /// An Issue is Ready when all of these conditions hold:
    ///
    /// - Workflow State is `Open`
    /// - No explicit Blocking Dependency points to an unresolved prerequisite
    /// - Assignment matches the query's [`ReadyFilter`]
    ///
    /// Priority, Issue Kind, label, ordering, and limit are applied only after
    /// eligibility is established.
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
    /// * `filter` - Ready-specific Assignment, priority, Kind, label, and limit criteria
    /// * `sort_policy` - Sort order for results (defaults to Hybrid if None)
    async fn ready_to_work(
        &self,
        filter: &ReadyFilter,
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
    async fn add_label(&mut self, id: &IssueId, label: &Label) -> Result<Issue>;

    /// Atomically remove a label from an issue.
    ///
    /// This operation is atomic - no TOCTOU race condition between read and write.
    /// If the label doesn't exist, this is a no-op and returns success.
    ///
    /// # Errors
    ///
    /// - `Error::IssueNotFound` if the issue doesn't exist
    async fn remove_label(&mut self, id: &IssueId, label: &Label) -> Result<Issue>;

    // ========== Associated Resource Operations ==========

    /// Atomically associate a resource with an issue.
    ///
    /// The domain assigns a stable, opaque identifier and rejects an exact
    /// target-and-role duplicate.
    ///
    /// # Errors
    ///
    /// - `Error::IssueNotFound` if the issue doesn't exist
    /// - `Error::Storage(StorageError::Resource(ResourceError::DuplicateTargetRole))` on duplicate
    async fn add_resource(&mut self, id: &IssueId, resource: NewResource) -> Result<Issue>;

    /// Atomically update an existing Associated Resource by its stable
    /// identifier.
    ///
    /// Only the provided fields change; the resource keeps its identifier and
    /// position. The duplicate check runs against the post-update state.
    ///
    /// # Errors
    ///
    /// - `Error::IssueNotFound` if the issue doesn't exist
    /// - `Error::Storage(StorageError::Resource(ResourceError::ResourceNotFound))` if the resource doesn't exist
    /// - `Error::Storage(StorageError::Resource(ResourceError::EmptyUpdate))` if no field is provided
    /// - `Error::Storage(StorageError::Resource(ResourceError::DuplicateTargetRole))` on duplicate
    async fn update_resource(
        &mut self,
        id: &IssueId,
        resource_id: &ResourceId,
        update: ResourceUpdate,
    ) -> Result<Issue>;

    /// Atomically remove an Associated Resource by its stable identifier.
    ///
    /// The remaining resources keep their identifiers and positions, and
    /// identifiers are never reused.
    ///
    /// # Errors
    ///
    /// - `Error::IssueNotFound` if the issue doesn't exist
    /// - `Error::Storage(StorageError::Resource(ResourceError::ResourceNotFound))` if the resource doesn't exist
    async fn remove_resource(&mut self, id: &IssueId, resource_id: &ResourceId) -> Result<Issue>;

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
    ///
    /// # Errors
    ///
    /// JSONL-backed storage returns [`StorageError::UnsafePartialLoad`] when
    /// resilient loading omitted any Issue record. No file write is attempted.
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

const SOURCE_REVISION_BUFFER_SIZE: usize = 64 * 1024;

/// Incremental hasher for the canonical bytes used as a JSONL source revision.
///
/// This stays crate-private so both the storage guard and JSONL writer share
/// exactly the same revision algorithm without exposing hashing as public API.
pub(crate) struct RevisionHasher(Sha256);

impl RevisionHasher {
    pub(crate) fn new() -> Self {
        Self(Sha256::new())
    }

    pub(crate) fn update(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }

    pub(crate) fn finalize(self) -> [u8; 32] {
        self.0.finalize().into()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum SourceRevision {
    Missing,
    Present([u8; 32]),
}

impl SourceRevision {
    async fn read(path: &Path) -> Result<Self> {
        let mut file = match tokio::fs::File::open(path).await {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Self::Missing),
            Err(error) => return Err(error.into()),
        };
        let mut hasher = RevisionHasher::new();
        let mut buffer = vec![0_u8; SOURCE_REVISION_BUFFER_SIZE];
        loop {
            let bytes_read = file.read(&mut buffer).await?;
            if bytes_read == 0 {
                break;
            }
            hasher.update(&buffer[..bytes_read]);
        }
        Ok(Self::Present(hasher.finalize()))
    }
}

/// Wrapper that adds guarded JSONL file persistence to an in-memory backend.
///
/// Reads remain available after a resilient partial load. Before mutation, an
/// externally changed source is reloaded under the caller's storage write lock.
/// Mutations and saves are rejected when any Issue record was omitted, and save
/// rejects a source revision changed after mutation, so incomplete or stale
/// in-memory state cannot replace the source file.
struct JsonlBackedStorage {
    inner: Box<dyn IssueStorage>,
    path: PathBuf,
    prefix: String,
    load_warnings: Vec<in_memory::LoadWarning>,
    source_revision: RwLock<SourceRevision>,
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

    fn unsafe_partial_load(&self) -> Option<StorageError> {
        let causes = self
            .load_warnings
            .iter()
            .filter_map(|warning| match warning {
                in_memory::LoadWarning::MalformedJson { line_number, error } => {
                    Some(SkippedIssueRecordCause::MalformedJson {
                        line_number: *line_number,
                        error: error.clone(),
                    })
                }
                in_memory::LoadWarning::MigrationConflict { .. }
                | in_memory::LoadWarning::AssignmentStateMigrated { .. }
                | in_memory::LoadWarning::WorkflowStateMigrated { .. }
                | in_memory::LoadWarning::OrphanedDependency { .. }
                | in_memory::LoadWarning::CircularDependency { .. } => None,
                in_memory::LoadWarning::InvalidIssueData {
                    issue_id,
                    line_number,
                    error,
                } => Some(SkippedIssueRecordCause::InvalidIssueData {
                    line_number: *line_number,
                    issue_id: issue_id.clone(),
                    error: error.clone(),
                }),
                in_memory::LoadWarning::InvalidResourceData {
                    issue_id,
                    line_number,
                    source,
                } => Some(SkippedIssueRecordCause::InvalidResourceData {
                    line_number: *line_number,
                    issue_id: issue_id.clone(),
                    source: source.clone(),
                }),
            })
            .collect();

        PartialLoadError::new(causes).map(StorageError::from)
    }

    fn ensure_writable(&self) -> Result<()> {
        match self.unsafe_partial_load() {
            Some(error) => Err(error.into()),
            None => Ok(()),
        }
    }

    async fn source_changed(&self) -> Result<bool> {
        let current = SourceRevision::read(&self.path).await?;
        Ok(current != *self.source_revision.read().await)
    }

    async fn prepare_mutation(&mut self) -> Result<()> {
        if self.source_changed().await? {
            self.reload().await?;
        }
        self.ensure_writable()
    }

    async fn ensure_source_unchanged(&self) -> Result<()> {
        if self.source_changed().await? {
            return Err(StorageError::ExternalChange {
                path: self.path.clone(),
            }
            .into());
        }
        Ok(())
    }
}

#[async_trait]
impl IssueStorage for JsonlBackedStorage {
    async fn create(&mut self, issue: NewIssue) -> Result<Issue> {
        self.prepare_mutation().await?;
        self.inner.create(issue).await
    }

    async fn get(&self, id: &IssueId) -> Result<Option<Issue>> {
        self.inner.get(id).await
    }

    async fn update(&mut self, id: &IssueId, updates: IssueUpdate) -> Result<Issue> {
        self.prepare_mutation().await?;
        self.inner.update(id, updates).await
    }

    async fn claim(&mut self, id: &IssueId, claimant: &str) -> Result<Issue> {
        self.prepare_mutation().await?;
        self.inner.claim(id, claimant).await
    }

    async fn release(&mut self, id: &IssueId, expected_assignee: &str) -> Result<Issue> {
        self.prepare_mutation().await?;
        self.inner.release(id, expected_assignee).await
    }

    async fn delete(&mut self, id: &IssueId) -> Result<()> {
        self.prepare_mutation().await?;
        self.inner.delete(id).await
    }
    async fn add_blocking_dependency(&mut self, dependency: BlockingDependency) -> Result<()> {
        self.prepare_mutation().await?;
        self.inner.add_blocking_dependency(dependency).await
    }

    async fn remove_blocking_dependency(&mut self, dependency: &BlockingDependency) -> Result<()> {
        self.prepare_mutation().await?;
        self.inner.remove_blocking_dependency(dependency).await
    }

    async fn blocking_prerequisites(
        &self,
        dependent_id: &IssueId,
    ) -> Result<Vec<BlockingDependency>> {
        self.inner.blocking_prerequisites(dependent_id).await
    }

    async fn blocking_dependents(
        &self,
        prerequisite_id: &IssueId,
    ) -> Result<Vec<BlockingDependency>> {
        self.inner.blocking_dependents(prerequisite_id).await
    }

    async fn blocking_dependency_tree(
        &self,
        dependent_id: &IssueId,
        max_depth: Option<usize>,
    ) -> Result<Vec<(BlockingDependency, usize)>> {
        self.inner
            .blocking_dependency_tree(dependent_id, max_depth)
            .await
    }

    async fn add_related_association(&mut self, association: RelatedAssociation) -> Result<()> {
        self.prepare_mutation().await?;
        self.inner.add_related_association(association).await
    }

    async fn remove_related_association(&mut self, association: &RelatedAssociation) -> Result<()> {
        self.prepare_mutation().await?;
        self.inner.remove_related_association(association).await
    }

    async fn related_associations(&self, issue_id: &IssueId) -> Result<Vec<RelatedAssociation>> {
        self.inner.related_associations(issue_id).await
    }

    async fn add_discovery_origin(&mut self, origin: DiscoveryOrigin) -> Result<()> {
        self.prepare_mutation().await?;
        self.inner.add_discovery_origin(origin).await
    }

    async fn remove_discovery_origin(&mut self, origin: &DiscoveryOrigin) -> Result<()> {
        self.prepare_mutation().await?;
        self.inner.remove_discovery_origin(origin).await
    }

    async fn discovery_origins(
        &self,
        discovered_issue_id: &IssueId,
    ) -> Result<Vec<DiscoveryOrigin>> {
        self.inner.discovery_origins(discovered_issue_id).await
    }
    async fn set_parent(&mut self, parentage: Parentage) -> Result<Parentage> {
        self.prepare_mutation().await?;
        self.inner.set_parent(parentage).await
    }

    async fn clear_parent(&mut self, child_id: &IssueId) -> Result<Parentage> {
        self.prepare_mutation().await?;
        self.inner.clear_parent(child_id).await
    }

    async fn move_parent(&mut self, parentage: Parentage) -> Result<Parentage> {
        self.prepare_mutation().await?;
        self.inner.move_parent(parentage).await
    }

    async fn parent_of(&self, child_id: &IssueId) -> Result<Option<Parentage>> {
        self.inner.parent_of(child_id).await
    }
    async fn list(&self, filter: &IssueFilter) -> Result<Vec<Issue>> {
        self.inner.list(filter).await
    }

    async fn ready_to_work(
        &self,
        filter: &ReadyFilter,
        sort_policy: Option<SortPolicy>,
    ) -> Result<Vec<Issue>> {
        self.inner.ready_to_work(filter, sort_policy).await
    }

    async fn blocked_issues(&self) -> Result<Vec<(Issue, Vec<Issue>)>> {
        self.inner.blocked_issues().await
    }

    async fn add_label(&mut self, id: &IssueId, label: &Label) -> Result<Issue> {
        self.prepare_mutation().await?;
        self.inner.add_label(id, label).await
    }

    async fn remove_label(&mut self, id: &IssueId, label: &Label) -> Result<Issue> {
        self.prepare_mutation().await?;
        self.inner.remove_label(id, label).await
    }

    async fn add_resource(&mut self, id: &IssueId, resource: NewResource) -> Result<Issue> {
        self.prepare_mutation().await?;
        self.inner.add_resource(id, resource).await
    }

    async fn update_resource(
        &mut self,
        id: &IssueId,
        resource_id: &ResourceId,
        update: ResourceUpdate,
    ) -> Result<Issue> {
        self.prepare_mutation().await?;
        self.inner.update_resource(id, resource_id, update).await
    }

    async fn remove_resource(&mut self, id: &IssueId, resource_id: &ResourceId) -> Result<Issue> {
        self.prepare_mutation().await?;
        self.inner.remove_resource(id, resource_id).await
    }

    async fn import_issues(&mut self, issues: Vec<Issue>) -> Result<()> {
        self.prepare_mutation().await?;
        self.inner.import_issues(issues).await
    }

    async fn export_all(&self) -> Result<Vec<Issue>> {
        self.inner.export_all().await
    }

    async fn save(&self) -> Result<()> {
        self.ensure_writable()?;
        self.ensure_source_unchanged().await?;
        let revision =
            in_memory::save_to_jsonl_with_revision(self.inner.as_ref(), &self.path).await?;
        *self.source_revision.write().await = SourceRevision::Present(revision);
        Ok(())
    }

    async fn reload(&mut self) -> Result<()> {
        let revision_before = SourceRevision::read(&self.path).await?;
        let (new_storage, warnings) = match revision_before {
            SourceRevision::Present(_) => {
                let (storage, warnings) =
                    in_memory::load_from_jsonl(&self.path, self.prefix.clone()).await?;
                if !warnings.is_empty() {
                    for warning in &warnings {
                        tracing::warn!(warning = ?warning, "JSONL reload warning");
                    }
                }
                (storage, warnings)
            }
            SourceRevision::Missing => (
                in_memory::new_in_memory_storage(self.prefix.clone()),
                Vec::new(),
            ),
        };
        self.inner = new_storage;
        self.load_warnings = warnings;
        *self.source_revision.get_mut() = revision_before;
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
            let revision_before = SourceRevision::read(&path).await?;
            let (inner, load_warnings) = match revision_before {
                SourceRevision::Present(_) => {
                    let (storage, warnings) =
                        in_memory::load_from_jsonl(&path, prefix.clone()).await?;
                    if !warnings.is_empty() {
                        for warning in &warnings {
                            tracing::warn!(warning = ?warning, "JSONL load warning");
                        }
                    }
                    (storage, warnings)
                }
                SourceRevision::Missing => {
                    (in_memory::new_in_memory_storage(prefix.clone()), Vec::new())
                }
            };
            Ok(Box::new(JsonlBackedStorage {
                inner,
                path,
                prefix,
                load_warnings,
                source_revision: RwLock::new(revision_before),
            }) as Box<dyn IssueStorage>)
        }
        StorageBackend::PostgreSQL(_conn_str) => {
            // TODO: Implement PostgreSQL backend
            Err(crate::error::ConfigError::UnsupportedBackend("PostgreSQL".to_string()).into())
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
/// - Blocking prerequisite/dependent/tree queries: Return empty vectors
/// - Mutations that require state: Return a typed unsupported-operation error
/// - Related and Discovery queries: Return empty vectors
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
    fn unsupported<T>(operation: &'static str) -> Result<T> {
        Err(StorageError::UnsupportedOperation { operation }.into())
    }

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
        use crate::domain::{IssueKind, IssueStatus};
        use chrono::Utc;

        Issue {
            id,
            title: "Test Issue".to_string(),
            description: "Test description".to_string(),
            status: IssueStatus::Open,
            priority: 1,
            issue_kind: IssueKind::Task,
            assignee: None,
            labels: vec![],
            design: None,
            resources: vec![],
            next_resource_id: 1,
            notes: vec![],
            acceptance_criteria: None,
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
        Self::unsupported("MockStorage::update")
    }

    async fn claim(&mut self, _id: &IssueId, _claimant: &str) -> Result<Issue> {
        Self::unsupported("MockStorage::claim")
    }

    async fn release(&mut self, _id: &IssueId, _expected_assignee: &str) -> Result<Issue> {
        Self::unsupported("MockStorage::release")
    }

    async fn delete(&mut self, _id: &IssueId) -> Result<()> {
        Self::unsupported("MockStorage::delete")
    }
    async fn add_blocking_dependency(&mut self, _dependency: BlockingDependency) -> Result<()> {
        Self::unsupported("MockStorage::add_blocking_dependency")
    }

    async fn remove_blocking_dependency(&mut self, _dependency: &BlockingDependency) -> Result<()> {
        Self::unsupported("MockStorage::remove_blocking_dependency")
    }

    async fn blocking_prerequisites(
        &self,
        _dependent_id: &IssueId,
    ) -> Result<Vec<BlockingDependency>> {
        Ok(vec![])
    }

    async fn blocking_dependents(
        &self,
        _prerequisite_id: &IssueId,
    ) -> Result<Vec<BlockingDependency>> {
        Ok(vec![])
    }

    async fn blocking_dependency_tree(
        &self,
        _dependent_id: &IssueId,
        _max_depth: Option<usize>,
    ) -> Result<Vec<(BlockingDependency, usize)>> {
        Ok(vec![])
    }

    async fn add_related_association(&mut self, _association: RelatedAssociation) -> Result<()> {
        Self::unsupported("MockStorage::add_related_association")
    }

    async fn remove_related_association(
        &mut self,
        _association: &RelatedAssociation,
    ) -> Result<()> {
        Self::unsupported("MockStorage::remove_related_association")
    }

    async fn related_associations(&self, _issue_id: &IssueId) -> Result<Vec<RelatedAssociation>> {
        Ok(vec![])
    }

    async fn add_discovery_origin(&mut self, _origin: DiscoveryOrigin) -> Result<()> {
        Self::unsupported("MockStorage::add_discovery_origin")
    }

    async fn remove_discovery_origin(&mut self, _origin: &DiscoveryOrigin) -> Result<()> {
        Self::unsupported("MockStorage::remove_discovery_origin")
    }

    async fn discovery_origins(
        &self,
        _discovered_issue_id: &IssueId,
    ) -> Result<Vec<DiscoveryOrigin>> {
        Ok(vec![])
    }
    async fn set_parent(&mut self, _parentage: Parentage) -> Result<Parentage> {
        unimplemented!(
            "MockStorage::set_parent() is not implemented. Use in_memory::new_in_memory_storage() for Parentage."
        )
    }

    async fn clear_parent(&mut self, _child_id: &IssueId) -> Result<Parentage> {
        unimplemented!(
            "MockStorage::clear_parent() is not implemented. Use in_memory::new_in_memory_storage() for Parentage."
        )
    }

    async fn move_parent(&mut self, _parentage: Parentage) -> Result<Parentage> {
        unimplemented!(
            "MockStorage::move_parent() is not implemented. Use in_memory::new_in_memory_storage() for Parentage."
        )
    }

    async fn parent_of(&self, _child_id: &IssueId) -> Result<Option<Parentage>> {
        Ok(None)
    }
    async fn list(&self, _filter: &IssueFilter) -> Result<Vec<Issue>> {
        Ok(vec![])
    }

    async fn ready_to_work(
        &self,
        _filter: &ReadyFilter,
        _sort_policy: Option<SortPolicy>,
    ) -> Result<Vec<Issue>> {
        Ok(vec![])
    }

    async fn blocked_issues(&self) -> Result<Vec<(Issue, Vec<Issue>)>> {
        Ok(vec![])
    }

    async fn add_label(&mut self, _id: &IssueId, _label: &Label) -> Result<Issue> {
        Self::unsupported("MockStorage::add_label")
    }

    async fn remove_label(&mut self, _id: &IssueId, _label: &Label) -> Result<Issue> {
        Self::unsupported("MockStorage::remove_label")
    }

    async fn add_resource(&mut self, _id: &IssueId, _resource: NewResource) -> Result<Issue> {
        Self::unsupported("MockStorage::add_resource")
    }

    async fn update_resource(
        &mut self,
        _id: &IssueId,
        _resource_id: &ResourceId,
        _update: ResourceUpdate,
    ) -> Result<Issue> {
        Self::unsupported("MockStorage::update_resource")
    }

    async fn remove_resource(&mut self, _id: &IssueId, _resource_id: &ResourceId) -> Result<Issue> {
        Self::unsupported("MockStorage::remove_resource")
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
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::IssueKind;
    fn issue_named(title: &str) -> NewIssue {
        NewIssue {
            title: title.to_string(),
            ..Default::default()
        }
    }

    async fn create_external_issue(path: &Path, title: &str) -> Issue {
        let mut storage = create_storage(StorageBackend::Jsonl(path.to_path_buf()), "test".into())
            .await
            .expect("external storage should open");
        let issue = storage
            .create(issue_named(title))
            .await
            .expect("external issue should be created");
        storage.save().await.expect("external issue should persist");
        issue
    }

    #[tokio::test]
    async fn test_trait_object_usage() {
        // Verify that IssueStorage is object-safe and can be used with Box<dyn>
        let mut storage: Box<dyn IssueStorage> = Box::new(MockStorage::new());

        let new_issue = NewIssue {
            title: "Test".to_string(),
            description: "Test".to_string(),
            priority: 1,
            issue_kind: IssueKind::Task,
            assignee: None,
            labels: vec![],
            design: None,
            acceptance_criteria: None,
            initial_note: None,
            prerequisites: vec![],
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
        assert!(
            storage
                .ready_to_work(&ReadyFilter::default(), None)
                .await
                .expect("empty ready query should succeed")
                .is_empty()
        );
        assert!(storage.blocked_issues().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_mock_storage_copy_semantics() {
        let mock = MockStorage::new();
        let _copy1 = mock;
        let _copy2 = mock; // Still usable - Copy semantics work
        let _: Box<dyn IssueStorage> = Box::new(mock);
    }
    #[tokio::test]
    async fn mock_storage_persistence_methods_are_no_ops() {
        let mut storage = MockStorage::new();
        let imported = MockStorage::create_test_issue(IssueId::new("test-import"));

        storage
            .import_issues(vec![imported])
            .await
            .expect("MockStorage import should remain a no-op");
        storage
            .save()
            .await
            .expect("MockStorage save should remain a no-op");
        storage
            .reload()
            .await
            .expect("MockStorage reload should remain a no-op");
    }
    #[tokio::test]
    async fn mock_storage_assignment_mutations_return_typed_errors() {
        let mut storage = MockStorage::new();
        let issue_id = IssueId::new(MOCK_ISSUE_ID);

        let claim_error = storage
            .claim(&issue_id, "alice")
            .await
            .expect_err("MockStorage claim should return an unsupported error");
        assert!(matches!(
            claim_error,
            crate::error::Error::Storage(StorageError::UnsupportedOperation {
                operation: "MockStorage::claim"
            })
        ));

        let release_error = storage
            .release(&issue_id, "alice")
            .await
            .expect_err("MockStorage release should return an unsupported error");
        assert!(matches!(
            release_error,
            crate::error::Error::Storage(StorageError::UnsupportedOperation {
                operation: "MockStorage::release"
            })
        ));
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
            issue_kind: IssueKind::Task,
            assignee: None,
            labels: vec![],
            design: None,
            acceptance_criteria: None,
            initial_note: None,
            prerequisites: vec![],
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
    async fn partial_jsonl_load_rejects_mutation_before_changing_memory() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let jsonl_path = temp_dir.path().join("issues.jsonl");
        let original = b"{\"id\":\"broken\",\"notes\":[}\n";
        std::fs::write(&jsonl_path, original).unwrap();

        let mut storage = create_storage(StorageBackend::Jsonl(jsonl_path.clone()), "test".into())
            .await
            .unwrap();
        let result = storage
            .create(NewIssue {
                title: "Phantom issue".to_string(),
                ..Default::default()
            })
            .await;

        match result {
            Err(crate::error::Error::Storage(StorageError::UnsafePartialLoad(error))) => {
                assert_eq!(error.skipped_records(), 1);
                assert!(matches!(
                    error.causes(),
                    [crate::error::SkippedIssueRecordCause::MalformedJson { line_number: 1, .. }]
                ));
            }
            other => panic!("expected a typed partial-load error, got {other:?}"),
        }
        assert!(
            storage
                .list(&IssueFilter::default())
                .await
                .unwrap()
                .is_empty(),
            "rejected mutation must not change in-memory state"
        );
        assert_eq!(std::fs::read(&jsonl_path).unwrap(), original);
    }

    #[tokio::test]
    async fn partial_resource_load_preserves_typed_cause() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("temp dir should be created");
        let jsonl_path = temp_dir.path().join("issues.jsonl");
        let invalid_resource = br#"{"id":"test-invalid-resource","title":"Invalid resource","description":"","status":"open","priority":2,"issue_kind":"task","assignee":null,"labels":[],"design":null,"acceptance_criteria":null,"notes":[],"resources":[{"id":"","target":{"type":"web","url":"https://example.com"},"role":"reference","label":null}],"dependencies":[],"created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z","closed_at":null}
"#;
        std::fs::write(&jsonl_path, invalid_resource)
            .expect("fixture JSONL file should be written");

        let mut storage = create_storage(StorageBackend::Jsonl(jsonl_path), "test".into())
            .await
            .expect("storage should open with a resource load warning");
        let result = storage
            .create(NewIssue {
                title: "Rejected issue".to_string(),
                ..Default::default()
            })
            .await;

        match result {
            Err(crate::error::Error::Storage(StorageError::UnsafePartialLoad(error))) => {
                assert!(matches!(
                    error.causes(),
                    [crate::error::SkippedIssueRecordCause::InvalidResourceData {
                        line_number: 1,
                        source: crate::domain::ResourceError::EmptyResourceId,
                        ..
                    }]
                ));
            }
            other => panic!("expected a typed resource load error, got {other:?}"),
        }
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
            issue_kind: IssueKind::Task,
            assignee: None,
            labels: vec![],
            design: None,
            acceptance_criteria: None,
            initial_note: None,
            prerequisites: vec![],
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
            issue_kind: IssueKind::Task,
            assignee: None,
            labels: vec![],
            design: None,
            acceptance_criteria: None,
            initial_note: None,
            prerequisites: vec![],
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
    #[tokio::test]
    async fn stale_source_change_before_mutation_is_preserved() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("temporary directory should be created");
        let jsonl_path = temp_dir.path().join("issues.jsonl");
        let mut cached = create_storage(StorageBackend::Jsonl(jsonl_path.clone()), "test".into())
            .await
            .expect("cached storage should open");
        let cached_issue = cached
            .create(issue_named("Cached issue"))
            .await
            .expect("cached issue should be created");
        cached.save().await.expect("cached issue should persist");

        let mut external = create_storage(StorageBackend::Jsonl(jsonl_path.clone()), "test".into())
            .await
            .expect("external storage should open");
        let external_issue = external
            .create(issue_named("External issue"))
            .await
            .expect("external issue should be created");
        external
            .save()
            .await
            .expect("external issue should persist");

        cached
            .update(
                &cached_issue.id,
                IssueUpdate {
                    title: Some("Cached issue updated".to_string()),
                    ..Default::default()
                },
            )
            .await
            .expect("mutation should refresh the external revision");
        cached.save().await.expect("merged state should persist");

        let reloaded = create_storage(StorageBackend::Jsonl(jsonl_path), "test".into())
            .await
            .expect("result should reload");
        assert_eq!(
            reloaded
                .get(&cached_issue.id)
                .await
                .expect("cached issue lookup should succeed")
                .expect("cached issue should remain")
                .title,
            "Cached issue updated"
        );
        assert!(
            reloaded
                .get(&external_issue.id)
                .await
                .expect("external issue lookup should succeed")
                .is_some(),
            "external issue must survive the cached mutation"
        );
    }

    #[tokio::test]
    async fn relationship_mutators_refresh_stale_source_before_change() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("temporary directory should be created");
        let jsonl_path = temp_dir.path().join("issues.jsonl");
        let mut cached = create_storage(StorageBackend::Jsonl(jsonl_path.clone()), "test".into())
            .await
            .expect("cached storage should open");
        let left = cached
            .create(issue_named("Left"))
            .await
            .expect("left issue should be created");
        let right = cached
            .create(issue_named("Right"))
            .await
            .expect("right issue should be created");
        let source = cached
            .create(issue_named("Source"))
            .await
            .expect("source issue should be created");
        cached.save().await.expect("seed issues should persist");

        let related = RelatedAssociation::new(left.id.clone(), right.id.clone())
            .expect("related endpoints should differ");
        let origin = DiscoveryOrigin::new(left.id.clone(), source.id.clone())
            .expect("discovery endpoints should differ");
        let mut external_ids = Vec::new();

        external_ids.push(
            create_external_issue(&jsonl_path, "Before related add")
                .await
                .id,
        );
        cached
            .add_related_association(related.clone())
            .await
            .expect("related add should reload stale source");
        cached.save().await.expect("related add should persist");

        external_ids.push(
            create_external_issue(&jsonl_path, "Before related remove")
                .await
                .id,
        );
        cached
            .remove_related_association(&related)
            .await
            .expect("related remove should reload stale source");
        cached.save().await.expect("related remove should persist");

        external_ids.push(
            create_external_issue(&jsonl_path, "Before discovery add")
                .await
                .id,
        );
        cached
            .add_discovery_origin(origin.clone())
            .await
            .expect("discovery add should reload stale source");
        cached.save().await.expect("discovery add should persist");

        external_ids.push(
            create_external_issue(&jsonl_path, "Before discovery remove")
                .await
                .id,
        );
        cached
            .remove_discovery_origin(&origin)
            .await
            .expect("discovery remove should reload stale source");
        cached
            .save()
            .await
            .expect("discovery remove should persist");

        let reloaded = create_storage(StorageBackend::Jsonl(jsonl_path), "test".into())
            .await
            .expect("result should reload");
        for external_id in external_ids {
            assert!(
                reloaded
                    .get(&external_id)
                    .await
                    .expect("external issue lookup should succeed")
                    .is_some(),
                "every external write must survive each relationship mutation"
            );
        }
        assert!(
            reloaded
                .related_associations(&left.id)
                .await
                .expect("related query should succeed")
                .is_empty()
        );
        assert!(
            reloaded
                .discovery_origins(&left.id)
                .await
                .expect("discovery query should succeed")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn stale_source_change_after_mutation_rejects_save_without_writing() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("temporary directory should be created");
        let jsonl_path = temp_dir.path().join("issues.jsonl");
        let mut cached = create_storage(StorageBackend::Jsonl(jsonl_path.clone()), "test".into())
            .await
            .expect("cached storage should open");
        let cached_issue = cached
            .create(issue_named("Cached issue"))
            .await
            .expect("cached issue should be created");
        cached.save().await.expect("cached issue should persist");
        cached
            .update(
                &cached_issue.id,
                IssueUpdate {
                    title: Some("Unsaved title".to_string()),
                    ..Default::default()
                },
            )
            .await
            .expect("in-memory mutation should succeed");

        let mut external = create_storage(StorageBackend::Jsonl(jsonl_path.clone()), "test".into())
            .await
            .expect("external storage should open");
        external
            .create(issue_named("External issue"))
            .await
            .expect("external issue should be created");
        external
            .save()
            .await
            .expect("external issue should persist");
        let external_bytes =
            std::fs::read(&jsonl_path).expect("external source bytes should be readable");

        let error = cached
            .save()
            .await
            .expect_err("save must reject a newer external revision");
        assert!(matches!(
            error,
            crate::error::Error::Storage(StorageError::ExternalChange { ref path })
                if path == &jsonl_path
        ));
        assert_eq!(
            std::fs::read(&jsonl_path).expect("source bytes should remain readable"),
            external_bytes,
            "rejected save must not replace external bytes"
        );
    }

    #[tokio::test]
    async fn stale_source_partial_reload_rejects_mutation() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("temporary directory should be created");
        let jsonl_path = temp_dir.path().join("issues.jsonl");
        let mut cached = create_storage(StorageBackend::Jsonl(jsonl_path.clone()), "test".into())
            .await
            .expect("empty cached storage should open");
        let malformed = b"{\"id\":\"broken\",\"notes\":[}\n";
        std::fs::write(&jsonl_path, malformed).expect("malformed source should be written");

        let error = cached
            .create(issue_named("Must not exist"))
            .await
            .expect_err("stale partial reload must reject mutation");
        assert!(matches!(
            error,
            crate::error::Error::Storage(StorageError::UnsafePartialLoad(_))
        ));
        assert_eq!(
            std::fs::read(&jsonl_path).expect("malformed source should remain readable"),
            malformed
        );
    }

    #[tokio::test]
    async fn stale_source_own_save_advances_revision() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("temporary directory should be created");
        let jsonl_path = temp_dir.path().join("issues.jsonl");
        let mut storage = create_storage(StorageBackend::Jsonl(jsonl_path), "test".into())
            .await
            .expect("storage should open");
        let issue = storage
            .create(issue_named("First title"))
            .await
            .expect("issue should be created");
        storage.save().await.expect("first save should succeed");
        storage
            .update(
                &issue.id,
                IssueUpdate {
                    title: Some("Second title".to_string()),
                    ..Default::default()
                },
            )
            .await
            .expect("own save must not look external");
        storage
            .save()
            .await
            .expect("second save should not false-conflict");
    }
    #[tokio::test]
    async fn stale_source_missing_transitions_are_observed() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("temporary directory should be created");
        let jsonl_path = temp_dir.path().join("issues.jsonl");
        let mut cached = create_storage(StorageBackend::Jsonl(jsonl_path.clone()), "test".into())
            .await
            .expect("missing source should open as empty");
        let mut external = create_storage(StorageBackend::Jsonl(jsonl_path.clone()), "test".into())
            .await
            .expect("second missing-source cache should open");
        let external_issue = external
            .create(issue_named("External first issue"))
            .await
            .expect("external issue should be created");
        external
            .save()
            .await
            .expect("external file should be created");

        let cached_issue = cached
            .create(issue_named("Cached second issue"))
            .await
            .expect("missing-to-present transition should refresh before mutation");
        cached.save().await.expect("merged source should persist");

        let merged = create_storage(StorageBackend::Jsonl(jsonl_path.clone()), "test".into())
            .await
            .expect("merged source should reload");
        assert!(
            merged
                .get(&external_issue.id)
                .await
                .expect("merged external issue lookup should succeed")
                .is_some()
        );
        assert!(
            merged
                .get(&cached_issue.id)
                .await
                .expect("merged cached issue lookup should succeed")
                .is_some()
        );

        std::fs::remove_file(&jsonl_path).expect("source should be deleted externally");
        let error = cached
            .update(
                &cached_issue.id,
                IssueUpdate {
                    title: Some("Must not reappear".to_string()),
                    ..Default::default()
                },
            )
            .await
            .expect_err("present-to-missing transition should discard stale cache");
        assert!(matches!(
            error,
            crate::error::Error::IssueNotFound(ref id) if id == &cached_issue.id
        ));
        assert!(
            !jsonl_path.exists(),
            "rejected stale mutation must not recreate a deleted source"
        );
    }

    #[tokio::test]
    #[ignore = "production-scale revision budget checkpoint"]
    async fn stale_source_10k_preserves_records() {
        use std::time::Instant;
        use tempfile::TempDir;

        const ISSUE_COUNT: usize = 10_000;
        let temp_dir = TempDir::new().expect("temporary directory should be created");
        let jsonl_path = temp_dir.path().join("issues.jsonl");
        let mut cached = create_storage(StorageBackend::Jsonl(jsonl_path.clone()), "scale".into())
            .await
            .expect("scale storage should open");
        for index in 0..ISSUE_COUNT {
            cached
                .create(NewIssue {
                    title: format!("Issue {index}: λ"),
                    description: "context\nline".to_string(),
                    ..Default::default()
                })
                .await
                .expect("scale issue should be created");
        }
        cached.save().await.expect("scale fixture should persist");
        let initial_source =
            std::fs::read_to_string(&jsonl_path).expect("scale fixture should be readable");
        let first_id = IssueId::new(
            serde_json::from_str::<serde_json::Value>(
                initial_source
                    .lines()
                    .next()
                    .expect("fixture should contain a first line"),
            )
            .expect("first canonical line should parse")["id"]
                .as_str()
                .expect("first record should have an id"),
        );
        let last_id = IssueId::new(
            serde_json::from_str::<serde_json::Value>(
                initial_source
                    .lines()
                    .next_back()
                    .expect("fixture should contain a last line"),
            )
            .expect("last canonical line should parse")["id"]
                .as_str()
                .expect("last record should have an id"),
        );
        let mut external =
            create_storage(StorageBackend::Jsonl(jsonl_path.clone()), "scale".into())
                .await
                .expect("external scale storage should open");
        external
            .update(
                &last_id,
                IssueUpdate {
                    title: Some("External λ".to_string()),
                    ..Default::default()
                },
            )
            .await
            .expect("external scale mutation should succeed");
        external
            .save()
            .await
            .expect("external scale mutation should persist");

        let started = Instant::now();
        cached
            .update(
                &first_id,
                IssueUpdate {
                    title: Some("Cached λ".to_string()),
                    ..Default::default()
                },
            )
            .await
            .expect("cached scale mutation should refresh");
        cached
            .save()
            .await
            .expect("cached scale mutation should persist");
        let elapsed = started.elapsed();

        let source =
            std::fs::read_to_string(&jsonl_path).expect("scale source should be directly readable");
        assert_eq!(source.lines().count(), ISSUE_COUNT);
        let records: Vec<serde_json::Value> = source
            .lines()
            .map(|line| serde_json::from_str(line).expect("canonical line should parse"))
            .collect();
        assert!(records.iter().any(|record| {
            record["id"] == last_id.as_str()
                && record["title"] == "External λ"
                && record["description"] == "context\nline"
        }));
        assert!(records.iter().any(|record| {
            record["id"] == first_id.as_str()
                && record["title"] == "Cached λ"
                && record["description"] == "context\nline"
        }));
        eprintln!("10k guarded mutation elapsed: {elapsed:?}");
    }
    #[tokio::test]
    async fn stale_source_revision_scans_every_buffer() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("temporary directory should be created");
        let path = temp_dir.path().join("revision-source");
        let mut bytes = vec![b'a'; SOURCE_REVISION_BUFFER_SIZE + 1];
        std::fs::write(&path, &bytes).expect("first revision should be written");
        let first = SourceRevision::read(&path)
            .await
            .expect("first revision should be readable");
        bytes[SOURCE_REVISION_BUFFER_SIZE] = b'b';
        std::fs::write(&path, bytes).expect("second revision should be written");
        let second = SourceRevision::read(&path)
            .await
            .expect("second revision should be readable");

        assert_ne!(
            first, second,
            "bytes after the first buffer must affect revision"
        );
    }

    #[tokio::test]
    async fn stale_source_revision_distinguishes_missing_and_empty() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("temporary directory should be created");
        let path = temp_dir.path().join("revision-source");
        let missing = SourceRevision::read(&path)
            .await
            .expect("missing revision should be readable");
        std::fs::write(&path, []).expect("empty source should be written");
        let empty = SourceRevision::read(&path)
            .await
            .expect("empty revision should be readable");

        assert_ne!(missing, empty);
    }
    #[tokio::test]
    async fn stale_source_own_save_revision_detects_later_deletion() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("temporary directory should be created");
        let jsonl_path = temp_dir.path().join("issues.jsonl");
        let mut storage = create_storage(StorageBackend::Jsonl(jsonl_path.clone()), "test".into())
            .await
            .expect("missing source should open");
        let issue = storage
            .create(issue_named("Persisted issue"))
            .await
            .expect("issue should be created");
        storage.save().await.expect("source should be created");
        std::fs::remove_file(&jsonl_path).expect("source should be deleted externally");

        let error = storage
            .update(
                &issue.id,
                IssueUpdate {
                    title: Some("Must not reappear".to_string()),
                    ..Default::default()
                },
            )
            .await
            .expect_err("own save revision must detect later deletion");
        assert!(matches!(
            error,
            crate::error::Error::IssueNotFound(ref id) if id == &issue.id
        ));
        assert!(!jsonl_path.exists());
    }
}
