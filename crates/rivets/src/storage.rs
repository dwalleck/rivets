//! Storage layer for rivets using rivets-jsonl.
//!
//! This module provides the storage interface for persisting issues
//! using the rivets-jsonl library.

/// Storage backend for issues
#[allow(dead_code)]
pub struct Storage;

#[allow(dead_code)]
impl Storage {
    /// Create a new storage instance
    pub fn new() -> Self {
        Storage
    }
}

impl Default for Storage {
    fn default() -> Self {
        Self::new()
    }
}
