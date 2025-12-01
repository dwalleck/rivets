# Automerge Storage Backend Design

**Status**: Proposal
**Author**: Claude + dwalleck
**Date**: 2025-11-30

## Executive Summary

This document proposes adding an Automerge-based storage backend to Rivets to enable true concurrent access by multiple agents without file locking or merge conflicts. This positions Rivets as a first-class tool for AI-assisted development where multiple agents work simultaneously.

## Problem Statement

### Current State
Rivets uses JSONL (JSON Lines) for persistent storage. While simple and human-readable, this approach has limitations for concurrent access:

1. **File Corruption Risk**: Multiple agents appending simultaneously can interleave lines
2. **Lock Contention**: File locking prevents true parallel work
3. **Merge Complexity**: Git merges of divergent JSONL files require semantic understanding
4. **No Real-time Sync**: Changes only visible after explicit git sync

### The Multi-Agent Future
AI-assisted development is moving toward multiple agents working concurrently:
- Agent A implements a feature while Agent B fixes bugs
- Multiple Claude Code instances in different terminals
- Parallel CI/CD pipelines updating issue status
- Real-time collaboration between team members

## Proposed Solution

### CRDTs via Automerge

[Automerge](https://automerge.org/) is a Conflict-free Replicated Data Type (CRDT) library that enables:
- **Conflict-free merging**: Changes from different agents merge deterministically
- **Offline-first**: Full functionality without network connectivity
- **Automatic sync**: Built-in sync protocol for P2P and client-server
- **Rust-native**: `automerge` crate is the reference implementation

### Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                    Application Layer                         │
│              (CLI, MCP Server, TUI - unchanged)              │
├─────────────────────────────────────────────────────────────┤
│                   IssueStorage Trait                         │
│    (existing interface - no changes to method signatures)    │
├──────────────────┬──────────────────┬───────────────────────┤
│  InMemoryStorage │ AutomergeStorage │  (future backends)    │
│  + JSONL export  │ + JSONL export   │                       │
│                  │ + Sync protocol  │                       │
└──────────────────┴──────────────────┴───────────────────────┘
```

### Key Design Decisions

#### 1. Storage Trait Unchanged
The existing `IssueStorage` trait remains the public API. Automerge is an implementation detail.

```rust
// No changes needed - AutomergeStorage implements the same trait
#[async_trait]
impl IssueStorage for AutomergeStorage {
    async fn create(&mut self, issue: NewIssue) -> Result<Issue>;
    async fn get(&self, id: &IssueId) -> Result<Option<Issue>>;
    // ... all existing methods
}
```

#### 2. Dual Persistence: Automerge Primary, JSONL Secondary

```
.rivets/
├── issues.automerge      # Binary CRDT document (source of truth)
├── issues.jsonl          # Human-readable export (auto-generated)
└── config.yaml
```

- **Automerge file**: Binary format, enables merging, sync, and conflict resolution
- **JSONL file**: Auto-regenerated on save for human inspection and git diffs
- **On load**: Prefer Automerge if exists, fall back to JSONL for migration

#### 3. Document Structure

Automerge documents are structured as nested maps and lists:

```rust
// Conceptual structure (Automerge manages internally)
{
    "issues": {
        "rivets-a3f8": {
            "id": "rivets-a3f8",
            "title": Text("Implement feature X"),  // Automerge Text for concurrent edits
            "description": Text("..."),
            "status": "open",                       // Last-writer-wins
            "priority": 2,                          // Last-writer-wins
            "dependencies": [                       // Automerge List
                {"depends_on_id": "rivets-x9k2", "dep_type": "blocks"}
            ],
            "labels": ["backend", "api"],           // Automerge List
            "created_at": "2025-11-17T10:00:00Z",
            "updated_at": "2025-11-17T10:00:00Z"
        }
    },
    "metadata": {
        "prefix": "rivets",
        "version": 1
    }
}
```

#### 4. Conflict Resolution Strategy

| Field | CRDT Type | Conflict Behavior |
|-------|-----------|-------------------|
| `title` | Text | Character-by-character merge |
| `description` | Text | Character-by-character merge |
| `status` | Register | Last-writer-wins (by wall clock) |
| `priority` | Register | Last-writer-wins |
| `assignee` | Register | Last-writer-wins |
| `labels` | List | All additions preserved |
| `dependencies` | List | All additions preserved |
| `notes` | Text | Character-by-character merge |
| `design` | Text | Character-by-character merge |

**Semantic Merge Example**:
```
Agent A: Changes status open → in_progress
Agent B: Adds label "urgent"
Result:  status = in_progress, labels = [..., "urgent"]  ✓ Both changes preserved

Agent A: Changes priority 2 → 1
Agent B: Changes priority 2 → 0
Result:  priority = 0 or 1 (deterministic based on actor ID + timestamp)
         Note: User may need to review, but no data loss
```

## Implementation Plan

### Phase 1: Core Automerge Backend

#### New Crate: `rivets-automerge`

```
crates/
├── rivets/                 # Main application (unchanged)
├── rivets-jsonl/           # JSONL utilities (unchanged)
├── rivets-mcp/             # MCP server (unchanged)
└── rivets-automerge/       # NEW: Automerge storage backend
    ├── Cargo.toml
    └── src/
        ├── lib.rs
        ├── document.rs     # Automerge document management
        ├── storage.rs      # IssueStorage implementation
        ├── sync.rs         # Sync protocol (Phase 2)
        └── migration.rs    # JSONL ↔ Automerge conversion
```

#### Dependencies

```toml
[dependencies]
automerge = "0.5"           # Core CRDT library
rivets = { path = "../rivets", features = ["domain-only"] }
tokio = { version = "1", features = ["fs", "sync"] }
```

#### Core Types

```rust
// crates/rivets-automerge/src/document.rs

use automerge::{AutoCommit, ObjId, ObjType, Prop, ReadDoc, Value};
use rivets::domain::{Issue, IssueId, Dependency, DependencyType};

/// Manages an Automerge document containing all issues.
pub struct IssueDocument {
    doc: AutoCommit,
    issues_obj: ObjId,  // Reference to the "issues" map
}

impl IssueDocument {
    /// Create a new empty document
    pub fn new(prefix: &str) -> Self {
        let mut doc = AutoCommit::new();

        // Initialize document structure
        let root = automerge::ROOT;
        let issues_obj = doc.put_object(&root, "issues", ObjType::Map).unwrap();
        doc.put(&root, "metadata", ObjType::Map).unwrap();
        // ... set prefix, version

        Self { doc, issues_obj }
    }

    /// Load from bytes
    pub fn load(bytes: &[u8]) -> Result<Self, Error> {
        let doc = AutoCommit::load(bytes)?;
        let issues_obj = doc.get(&automerge::ROOT, "issues")?
            .ok_or(Error::InvalidDocument)?
            .1;
        Ok(Self { doc, issues_obj })
    }

    /// Save to bytes
    pub fn save(&mut self) -> Vec<u8> {
        self.doc.save()
    }

    /// Insert or update an issue
    pub fn put_issue(&mut self, issue: &Issue) -> Result<(), Error> {
        let issue_obj = self.doc.put_object(
            &self.issues_obj,
            issue.id.as_str(),
            ObjType::Map
        )?;

        // Use Text for mergeable string fields
        self.doc.put(&issue_obj, "id", issue.id.as_str())?;
        self.put_text(&issue_obj, "title", &issue.title)?;
        self.put_text(&issue_obj, "description", &issue.description)?;

        // Use simple values for last-writer-wins fields
        self.doc.put(&issue_obj, "status", issue.status.to_string())?;
        self.doc.put(&issue_obj, "priority", issue.priority as i64)?;

        // ... handle other fields

        Ok(())
    }

    /// Get an issue by ID
    pub fn get_issue(&self, id: &IssueId) -> Result<Option<Issue>, Error> {
        // ... read from Automerge document
    }

    /// Merge changes from another document
    pub fn merge(&mut self, other: &mut IssueDocument) -> Result<(), Error> {
        self.doc.merge(&mut other.doc)?;
        Ok(())
    }

    // Helper for Text fields
    fn put_text(&mut self, obj: &ObjId, key: &str, value: &str) -> Result<(), Error> {
        let text_obj = self.doc.put_object(obj, key, ObjType::Text)?;
        self.doc.splice_text(&text_obj, 0, 0, value)?;
        Ok(())
    }
}
```

#### Storage Implementation

```rust
// crates/rivets-automerge/src/storage.rs

use async_trait::async_trait;
use rivets::storage::IssueStorage;
use rivets::domain::*;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct AutomergeStorage {
    document: Arc<RwLock<IssueDocument>>,
    path: PathBuf,
    jsonl_path: Option<PathBuf>,  // Optional JSONL mirror
    prefix: String,
}

impl AutomergeStorage {
    pub async fn new(path: PathBuf, prefix: String) -> Result<Self, Error> {
        let document = if path.exists() {
            let bytes = tokio::fs::read(&path).await?;
            IssueDocument::load(&bytes)?
        } else {
            IssueDocument::new(&prefix)
        };

        Ok(Self {
            document: Arc::new(RwLock::new(document)),
            path,
            jsonl_path: None,
            prefix,
        })
    }

    /// Enable JSONL mirroring for human-readable export
    pub fn with_jsonl_mirror(mut self, path: PathBuf) -> Self {
        self.jsonl_path = Some(path);
        self
    }

    /// Merge with another Automerge file (e.g., from a different branch)
    pub async fn merge_from_file(&self, other_path: &Path) -> Result<(), Error> {
        let other_bytes = tokio::fs::read(other_path).await?;
        let mut other_doc = IssueDocument::load(&other_bytes)?;

        let mut doc = self.document.write().await;
        doc.merge(&mut other_doc)?;

        Ok(())
    }
}

#[async_trait]
impl IssueStorage for AutomergeStorage {
    async fn create(&mut self, new_issue: NewIssue) -> Result<Issue> {
        new_issue.validate().map_err(Error::Validation)?;

        let issue = Issue {
            id: generate_id(&self.prefix),
            title: new_issue.title,
            description: new_issue.description,
            status: IssueStatus::Open,
            priority: new_issue.priority,
            // ... fill in other fields
            created_at: Utc::now(),
            updated_at: Utc::now(),
            closed_at: None,
        };

        let mut doc = self.document.write().await;
        doc.put_issue(&issue)?;

        Ok(issue)
    }

    async fn get(&self, id: &IssueId) -> Result<Option<Issue>> {
        let doc = self.document.read().await;
        doc.get_issue(id)
    }

    async fn update(&mut self, id: &IssueId, updates: IssueUpdate) -> Result<Issue> {
        let mut doc = self.document.write().await;

        let mut issue = doc.get_issue(id)?
            .ok_or_else(|| Error::IssueNotFound(id.clone()))?;

        // Apply updates
        if let Some(title) = updates.title {
            issue.title = title;
        }
        if let Some(status) = updates.status {
            issue.status = status;
        }
        // ... other fields

        issue.updated_at = Utc::now();
        doc.put_issue(&issue)?;

        Ok(issue)
    }

    async fn save(&self) -> Result<()> {
        let mut doc = self.document.write().await;
        let bytes = doc.save();

        // Atomic write to Automerge file
        let temp_path = self.path.with_extension("automerge.tmp");
        tokio::fs::write(&temp_path, &bytes).await?;
        tokio::fs::rename(&temp_path, &self.path).await?;

        // Optional: Also write JSONL mirror
        if let Some(jsonl_path) = &self.jsonl_path {
            let issues = doc.all_issues()?;
            write_jsonl(jsonl_path, &issues).await?;
        }

        Ok(())
    }

    // ... implement remaining trait methods
}
```

### Phase 2: Sync Protocol

Add peer-to-peer synchronization capability:

```rust
// crates/rivets-automerge/src/sync.rs

use automerge::sync::{Message, State as SyncState};

impl AutomergeStorage {
    /// Generate a sync message to send to a peer
    pub async fn generate_sync_message(&self, peer_state: &mut SyncState) -> Option<Message> {
        let doc = self.document.read().await;
        doc.doc.generate_sync_message(peer_state)
    }

    /// Receive and apply a sync message from a peer
    pub async fn receive_sync_message(
        &self,
        peer_state: &mut SyncState,
        message: Message
    ) -> Result<(), Error> {
        let mut doc = self.document.write().await;
        doc.doc.receive_sync_message(peer_state, message)?;
        Ok(())
    }
}
```

This enables future features:
- **P2P sync**: Direct sync between developer machines via libp2p
- **Server sync**: Sync to/from a central server for team collaboration
- **Git hook sync**: Auto-merge Automerge files during git merge

### Phase 3: Migration Path

#### JSONL → Automerge Migration

```rust
// crates/rivets-automerge/src/migration.rs

pub async fn migrate_from_jsonl(
    jsonl_path: &Path,
    automerge_path: &Path,
    prefix: &str,
) -> Result<AutomergeStorage, Error> {
    // Load existing JSONL
    let (old_storage, warnings) = load_from_jsonl(jsonl_path, prefix).await?;

    // Create new Automerge storage
    let mut new_storage = AutomergeStorage::new(automerge_path.to_owned(), prefix).await?;

    // Import all issues
    let issues = old_storage.export_all().await?;
    for issue in issues {
        new_storage.import_issue(issue).await?;
    }

    // Save
    new_storage.save().await?;

    Ok(new_storage)
}
```

#### Git Merge Driver

Create a custom git merge driver for `.automerge` files:

```bash
# .gitattributes
*.automerge merge=automerge-crdt

# .git/config (or global)
[merge "automerge-crdt"]
    name = Automerge CRDT merge driver
    driver = rivets merge-automerge %O %A %B
```

```rust
// In rivets CLI: `rivets merge-automerge <base> <ours> <theirs>`
fn merge_automerge(base: &Path, ours: &Path, theirs: &Path) -> Result<()> {
    let mut base_doc = IssueDocument::load(&fs::read(base)?)?;
    let mut ours_doc = IssueDocument::load(&fs::read(ours)?)?;
    let mut theirs_doc = IssueDocument::load(&fs::read(theirs)?)?;

    // Merge theirs into ours (CRDT merge is associative and commutative)
    ours_doc.merge(&mut theirs_doc)?;

    // Write result back to "ours" (git expects this)
    fs::write(ours, ours_doc.save())?;

    Ok(())
}
```

## Storage Backend Selection

Update `StorageBackend` enum:

```rust
// crates/rivets/src/storage/mod.rs

#[derive(Debug, Clone)]
pub enum StorageBackend {
    /// In-memory storage (ephemeral)
    InMemory,

    /// JSONL file storage (persistent, legacy)
    Jsonl(PathBuf),

    /// Automerge CRDT storage (persistent, concurrent-safe)
    Automerge {
        path: PathBuf,
        /// Optional JSONL mirror for human readability
        jsonl_mirror: Option<PathBuf>,
    },

    /// PostgreSQL database (persistent, production-ready)
    PostgreSQL(String),
}
```

Configuration:

```yaml
# .rivets/config.yaml
issue-prefix: "rivets"

storage:
  # Option 1: Legacy JSONL (default for now)
  backend: "jsonl"
  data_file: ".rivets/issues.jsonl"

  # Option 2: Automerge with JSONL mirror
  # backend: "automerge"
  # data_file: ".rivets/issues.automerge"
  # jsonl_mirror: ".rivets/issues.jsonl"  # Optional human-readable export
```

## Dependency Graph Handling

The current implementation uses petgraph for in-memory dependency graph operations. With Automerge:

### Option A: Rebuild Graph from Document (Recommended)

```rust
impl AutomergeStorage {
    /// Rebuild petgraph from Automerge document
    /// Called on load and after merges
    fn rebuild_graph(&self, doc: &IssueDocument) -> DiGraph<IssueId, DependencyType> {
        let mut graph = DiGraph::new();
        let mut node_map = HashMap::new();

        for issue in doc.all_issues()? {
            let node = graph.add_node(issue.id.clone());
            node_map.insert(issue.id.clone(), node);
        }

        for issue in doc.all_issues()? {
            for dep in &issue.dependencies {
                if let (Some(&from), Some(&to)) = (
                    node_map.get(&issue.id),
                    node_map.get(&dep.depends_on_id)
                ) {
                    graph.add_edge(from, to, dep.dep_type);
                }
            }
        }

        graph
    }
}
```

### Option B: Store Graph in Automerge

More complex but enables graph-level CRDTs. Deferred to future work.

## Testing Strategy

### Unit Tests
- Document creation and manipulation
- Issue CRUD operations through trait
- Conflict resolution scenarios
- Migration from JSONL

### Integration Tests
- Concurrent access simulation
- Merge scenarios
- Git merge driver

### Property-Based Tests (proptest)
- Random operations should never corrupt document
- Merge is associative: merge(A, merge(B, C)) == merge(merge(A, B), C)
- Merge is commutative: merge(A, B) == merge(B, A)

## Performance Considerations

### Memory Usage
Automerge documents grow with history. Mitigation:
- Periodic compaction (save + reload discards edit history)
- Configurable history depth

### Disk Usage
Binary format is typically smaller than JSONL for the same data.

### Load Time
Automerge load is O(operations) on first load, O(1) for subsequent loads from save file.

## Security Considerations

- **No remote code execution**: Automerge documents contain only data
- **Integrity**: Documents are self-validating
- **Sync authentication**: Future sync features must authenticate peers

## Open Questions

1. **Default backend**: Should Automerge become the default, or opt-in?
   - Recommendation: Opt-in initially, default after stabilization

2. **History retention**: How much edit history to keep?
   - Recommendation: Compact on save by default, optional full history

3. **JSONL deprecation timeline**: When to remove JSONL as primary backend?
   - Recommendation: Never remove, keep as import/export format

4. **Sync server**: Should we provide a sync server implementation?
   - Recommendation: Phase 2+ feature, not MVP

## Success Criteria

1. Multiple agents can modify issues simultaneously without corruption
2. Git merges of `.automerge` files resolve automatically via merge driver
3. Human-readable JSONL always available for inspection
4. No performance regression for single-agent use cases
5. Migration from existing JSONL is seamless

## References

- [Automerge](https://automerge.org/) - CRDT library
- [automerge-rs](https://github.com/automerge/automerge) - Rust implementation
- [CRDTs: The Hard Parts](https://www.youtube.com/watch?v=x7drE24geUw) - Martin Kleppmann
- [Local-first software](https://www.inkandswitch.com/local-first/) - Ink & Switch
