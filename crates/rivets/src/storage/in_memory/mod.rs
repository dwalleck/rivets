//! In-memory storage backend using HashMap and petgraph.
//!
//! This module provides a fast, **ephemeral** storage implementation where all data
//! is held in RAM and **lost when the process exits**. It is suitable for:
//!
//! - Testing and development
//! - Short-lived CLI sessions
//! - MVP development phase
//! - Performance benchmarking
//!
//! # Persistence
//!
//! This backend supports **optional JSONL persistence** via the `load_from_jsonl()` and
//! `save_to_jsonl()` functions. Data can be loaded from and saved to disk while maintaining
//! fast in-memory operations.
//!
//! - **In-memory only**: Use `new_in_memory_storage()` for ephemeral storage
//! - **With persistence**: Use `load_from_jsonl()` to load from disk, then periodically
//!   call `save_to_jsonl()` to persist changes
//!
//! The trait's `save()` method is a no-op for in-memory storage. Use `save_to_jsonl()`
//! directly for file-based persistence.
//!
//! # Architecture
//!
//! The implementation uses:
//! - `HashMap<IssueId, Issue>` for O(1) issue lookups
//! - `petgraph::DiGraph` for dependency graph with cycle detection
//! - `HashMap<IssueId, NodeIndex>` for mapping issues to graph nodes
//! - Hash-based ID generation with adaptive length (4-6 chars)
//!
//! ## Graph Representation and Edge Direction Convention
//!
//! The dependency graph uses a **dependent -> dependency** edge direction pattern:
//!
//! - **Edge source**: The issue that has the dependency (the dependent)
//! - **Edge target**: The issue being depended upon (the dependency)
//! - **Edge weight**: The [`DependencyType`] indicating the relationship kind
//!
//! **Concrete examples:**
//!
//! - **Blocks**: If issue A is blocked by issue B, edge is `A -> B` with weight `Blocks`
//! - **ParentChild**: If task C is a child of epic E, edge is `C -> E` with weight `ParentChild`
//! - **Related**: If issue X is related to issue Y, edge is `X -> Y` with weight `Related`
//! - **DiscoveredFrom**: If bug D was discovered from task T, edge is `D -> T` with weight `DiscoveredFrom`
//!
//! ## Blocking Semantics
//!
//! An issue is considered **blocked** and not ready to work on if:
//!
//! 1. **Direct blocking**: The issue has a `Blocks` dependency on an unclosed issue
//! 2. **Transitive blocking via ParentChild**: The issue's parent (via `ParentChild`) is blocked
//!
//! **Non-blocking dependency types:**
//! - `Related`: Informational link only, does not block work
//! - `DiscoveredFrom`: Provenance tracking only, does not block work
//!
//! The blocking propagation is limited to 50 levels of depth to prevent infinite loops
//! and handle extremely deep hierarchies gracefully.
//!
//! # Thread Safety
//!
//! The storage is wrapped in `Arc<Mutex<InMemoryStorageInner>>` to provide thread-safe
//! access in async contexts. All operations acquire the mutex lock, ensuring safe
//! concurrent access from multiple tasks.
//!
//! # Performance Characteristics
//!
//! - Create: O(1) amortized, O(n) when crossing ID length thresholds (500, 1500 issues)
//! - Read: O(1) for single issue lookups
//! - Update: O(1) for issue updates
//! - Delete: O(d) where d is number of dependencies
//! - Dependencies: O(d) where d is number of edges in the graph
//! - Ready to work: O(n + e) where n is issues, e is edges (BFS traversal)

mod graph;
mod inner;
mod jsonl;
mod sorting;
mod trait_impl;

use crate::storage::IssueStorage;
use inner::InMemoryStorageInner;
use std::sync::Arc;
use tokio::sync::Mutex;

// Re-export public API
pub use jsonl::{load_from_jsonl, save_to_jsonl, LoadWarning};

/// Thread-safe in-memory storage.
///
/// This type alias wraps the inner storage in `Arc<Mutex<>>` for thread-safe
/// async access. It implements [`IssueStorage`] via the trait implementation
/// in `trait_impl.rs`.
pub(crate) type InMemoryStorage = Arc<Mutex<InMemoryStorageInner>>;

/// Create a new in-memory storage instance.
///
/// # Arguments
///
/// * `prefix` - The prefix for issue IDs (e.g., "rivets")
///
/// # Example
///
/// ```
/// use rivets::storage::in_memory::new_in_memory_storage;
///
/// #[tokio::main(flavor = "current_thread")]
/// async fn main() {
///     let storage = new_in_memory_storage("rivets".to_string());
///     // Use storage...
/// }
/// ```
pub fn new_in_memory_storage(prefix: String) -> Box<dyn IssueStorage> {
    Box::new(Arc::new(Mutex::new(InMemoryStorageInner::new(prefix))))
}
