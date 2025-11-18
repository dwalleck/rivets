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
//! # Example
//!
//! ```no_run
//! use rivets::storage::{IssueStorage, StorageBackend, create_storage};
//! use rivets::domain::{NewIssue, IssueType};
//!
//! #[tokio::main(flavor = "current_thread")]
//! async fn main() -> anyhow::Result<()> {
//!     // Create in-memory storage
//!     let mut storage = create_storage(StorageBackend::InMemory).await?;
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
    Dependency, DependencyType, Issue, IssueFilter, IssueId, IssueUpdate, NewIssue,
};
use crate::error::Result;
use async_trait::async_trait;
use std::path::PathBuf;

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
    /// Results are sorted by priority and age (configurable).
    async fn ready_to_work(&self, filter: Option<&IssueFilter>) -> Result<Vec<Issue>>;

    /// Get all blocked issues.
    ///
    /// Returns tuples of (blocked issue, blocking issues).
    async fn blocked_issues(&self) -> Result<Vec<(Issue, Vec<Issue>)>>;

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

/// Create a storage instance for the given backend.
///
/// This factory function returns a trait object that can be used
/// polymorphically regardless of the backend implementation.
///
/// # Example
///
/// ```no_run
/// use rivets::storage::{create_storage, StorageBackend};
///
/// #[tokio::main(flavor = "current_thread")]
/// async fn main() -> anyhow::Result<()> {
///     let storage = create_storage(StorageBackend::InMemory).await?;
///     // Use storage...
///     Ok(())
/// }
/// ```
///
/// # Errors
///
/// - `Error::Io` if file operations fail (JSONL backend)
/// - `Error::Storage` for backend-specific initialization errors
pub async fn create_storage(_backend: StorageBackend) -> Result<Box<dyn IssueStorage>> {
    // TODO: Implement backends in subsequent tasks
    // For now, return a placeholder error
    Err(crate::error::Error::Storage(
        "Storage backends not yet implemented".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{IssueStatus, IssueType};
    use chrono::Utc;

    /// Mock implementation of IssueStorage for testing trait object usage
    struct MockStorage;

    #[async_trait]
    impl IssueStorage for MockStorage {
        async fn create(&mut self, _issue: NewIssue) -> Result<Issue> {
            Ok(Issue {
                id: IssueId::new("test-1"),
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
                created_at: Utc::now(),
                updated_at: Utc::now(),
                closed_at: None,
            })
        }

        async fn get(&self, id: &IssueId) -> Result<Option<Issue>> {
            if id.as_str() == "test-1" {
                Ok(Some(Issue {
                    id: id.clone(),
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
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                    closed_at: None,
                }))
            } else {
                Ok(None)
            }
        }

        async fn update(&mut self, _id: &IssueId, _updates: IssueUpdate) -> Result<Issue> {
            unimplemented!("Mock storage: update not implemented")
        }

        async fn delete(&mut self, _id: &IssueId) -> Result<()> {
            unimplemented!("Mock storage: delete not implemented")
        }

        async fn add_dependency(
            &mut self,
            _from: &IssueId,
            _to: &IssueId,
            _dep_type: DependencyType,
        ) -> Result<()> {
            unimplemented!("Mock storage: add_dependency not implemented")
        }

        async fn remove_dependency(&mut self, _from: &IssueId, _to: &IssueId) -> Result<()> {
            unimplemented!("Mock storage: remove_dependency not implemented")
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

        async fn list(&self, _filter: &IssueFilter) -> Result<Vec<Issue>> {
            Ok(vec![])
        }

        async fn ready_to_work(&self, _filter: Option<&IssueFilter>) -> Result<Vec<Issue>> {
            Ok(vec![])
        }

        async fn blocked_issues(&self) -> Result<Vec<(Issue, Vec<Issue>)>> {
            Ok(vec![])
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
    }

    #[tokio::test]
    async fn test_trait_object_usage() {
        // Verify that IssueStorage is object-safe and can be used with Box<dyn>
        let mut storage: Box<dyn IssueStorage> = Box::new(MockStorage);

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
        assert_eq!(issue.id.as_str(), "test-1");
        assert_eq!(issue.title, "Test Issue");
    }

    #[tokio::test]
    async fn test_get_issue() {
        let storage: Box<dyn IssueStorage> = Box::new(MockStorage);

        // Test existing issue
        let result = storage.get(&IssueId::new("test-1")).await.unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().id.as_str(), "test-1");

        // Test non-existing issue
        let result = storage.get(&IssueId::new("test-99")).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_empty_queries() {
        let storage: Box<dyn IssueStorage> = Box::new(MockStorage);

        // Test that query methods return empty results
        let filter = IssueFilter::default();
        assert!(storage.list(&filter).await.unwrap().is_empty());
        assert!(storage.ready_to_work(None).await.unwrap().is_empty());
        assert!(storage.blocked_issues().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_dependencies() {
        let storage: Box<dyn IssueStorage> = Box::new(MockStorage);

        let id = IssueId::new("test-1");
        assert!(storage.get_dependencies(&id).await.unwrap().is_empty());
        assert!(storage.get_dependents(&id).await.unwrap().is_empty());
        assert!(!storage
            .has_cycle(&id, &IssueId::new("test-2"))
            .await
            .unwrap());
    }
}
