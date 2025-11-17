//! Domain types for issue tracking.
//!
//! This module contains the core domain types for the rivets issue tracker.

use serde::{Deserialize, Serialize};

/// Represents an issue in the tracking system
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
    /// Unique identifier for the issue
    pub id: String,

    /// Issue title
    pub title: String,

    /// Issue description
    pub description: String,

    /// Current status
    pub status: IssueStatus,
}

/// Status of an issue
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum IssueStatus {
    /// Issue is open and ready to work on
    Open,

    /// Issue is currently being worked on
    InProgress,

    /// Issue is blocked by dependencies
    Blocked,

    /// Issue has been completed
    Closed,
}
