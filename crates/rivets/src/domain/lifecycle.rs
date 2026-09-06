//! Dedicated workflow lifecycle mutation intents.

use super::NoteContent;

/// A workflow transition intent owned by storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LifecycleAction {
    /// Move an Open Issue into In Progress; an Assignee is required.
    Start,
    /// Move an In Progress Issue back to Open while retaining its Assignee.
    ReturnToOpen,
    /// Close an Issue and optionally append one already-canonical reason Note.
    Close { reason: Option<NoteContent> },
    /// Reopen a Closed Issue to Open and optionally append one already-canonical reason Note.
    Reopen { reason: Option<NoteContent> },
}
