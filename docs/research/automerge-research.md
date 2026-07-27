# Automerge Research: Protocol & Rust Implementation

**Status**: Research Complete
**Date**: 2025-12-07
**Related**: [Automerge Storage Design](../design/automerge-storage.md)

This document consolidates in-depth research on the Automerge protocol and its Rust implementation (`automerge-rs`) for use in building a concurrent-safe issue tracking storage backend for Rivets.

---

## Table of Contents

1. [Protocol Fundamentals](#1-protocol-fundamentals)
2. [Data Model & Types](#2-data-model--types)
3. [Conflict Resolution](#3-conflict-resolution)
4. [Sync Protocol](#4-sync-protocol)
5. [Rust Library API](#5-rust-library-api)
6. [Best Practices](#6-best-practices)
7. [Anti-Patterns & Gotchas](#7-anti-patterns--gotchas)
8. [Performance Characteristics](#8-performance-characteristics)
9. [Type Mapping for Rivets](#9-type-mapping-for-rivets)
10. [Implementation Examples](#10-implementation-examples)
11. [References](#11-references)

---

## 1. Protocol Fundamentals

### What is Automerge?

Automerge is a Conflict-free Replicated Data Type (CRDT) library that enables automatic merging of concurrent changes without requiring a central server. The core principle: when two or more devices modify the same document independently, Automerge deterministically merges those changes so all devices converge to identical state.

### Core Guarantees

**Convergence**: Whenever any two documents have applied the same set of changes (in any order), they are guaranteed to be identical. This is the fundamental CRDT property.

**Offline-first**: Full functionality without network connectivity. Changes can be made locally and synced later.

**Network-agnostic**: The sync protocol works over any reliable, in-order transport (TCP, WebSocket, etc.).

### The Actor Model

Every change in Automerge is made by an **actor** represented by an `ActorId`:

- `ActorId` is a random sequence of bytes (UUID by default)
- Each change by the same actor must be sequential
- **Critical**: One actor ID per device/client - never reuse across processes

Operations use Lamport clocks `<counter, actorID>` to establish causal ordering:
- If counters differ: larger counter wins
- If counters equal: lexicographically larger actor ID wins
- Result: Total, deterministic, causal ordering

### Document Structure

Automerge documents function like JSON with version control built-in:

```
Root (Map)
├── "issues" (Map)
│   ├── "rivets-a3f8" (Map)
│   │   ├── "title" (Text)
│   │   ├── "status" (Scalar)
│   │   └── "labels" (List)
│   └── "rivets-b4c9" (Map)
│       └── ...
└── "metadata" (Map)
    ├── "prefix" (Scalar)
    └── "version" (Scalar)
```

Every composite value (Map, List, Text) gets a unique `ObjId` when created. Values are referenced by `(ObjId, key)` pairs where key is either a string (maps) or index (sequences).

---

## 2. Data Model & Types

### Available Types

| Type | Description | Merge Behavior |
|------|-------------|----------------|
| **Map** | String-keyed object | Per-key LWW for scalars |
| **List** | Ordered sequence | Concurrent insertions preserved |
| **Text** | Collaborative string | Character-level merge |
| **Scalar** | Primitive values | Last-writer-wins |
| **Counter** | Incrementable number | All increments sum |

### Scalar Values

Scalars are immutable primitive values:
- Strings (non-collaborative)
- Integers (i64)
- Floats (f64)
- Booleans
- Timestamps
- Bytes
- Null

### Text vs Scalar Strings

**Critical distinction**:

```rust
// Scalar string - entire value replaced on conflict
doc.put(&obj, "status", "open")?;

// Text object - character-level merging
let text_obj = doc.put_object(&obj, "title", ObjType::Text)?;
doc.splice_text(&text_obj, 0, 0, "Issue Title")?;
```

Use **Scalar** for: status, IDs, enums, URLs, timestamps
Use **Text** for: titles, descriptions, comments, any user-editable content

### Counter Type

Counters sum all increments from all actors:

```rust
// Good for: vote counts, reaction counts, metrics
doc.put(&obj, "upvotes", automerge::ScalarValue::Counter(0))?;
doc.increment(&obj, "upvotes", 1)?;
```

**Warning**: Do NOT use counters for auto-increment IDs. Concurrent increments produce different values on different devices, leading to duplicates when merged.

---

## 3. Conflict Resolution

### Automatic Merge Cases (No Conflicts)

Most concurrent edits merge automatically:

| Scenario | Result |
|----------|--------|
| Different properties in same object | Both changes preserved |
| Different objects entirely | Both changes preserved |
| Concurrent list insertions at same position | Both preserved with deterministic ordering |
| Concurrent text edits at different positions | Both edits merged |

### True Conflicts: Same Property, Same Object

The only conflict case: two actors concurrently update the **same property in the same object**:

```
Actor A: set doc.status = "open"     [counter: 42]
Actor B: set doc.status = "closed"   [counter: 50]

Result: "closed" wins (counter 50 > 42)
```

**Resolution mechanism**:
1. Automerge picks one value as "winner" deterministically (by operation ID)
2. The losing value is NOT lost - available via `getConflicts()`
3. Simply reassigning the property resolves the conflict

### Conflict Detection in Rust

```rust
// Check for conflicts after sync/merge
if let Some(conflicts) = doc.get_all(&obj, "status")? {
    if conflicts.len() > 1 {
        // Multiple concurrent values exist
        for (_, value) in conflicts {
            println!("Conflict value: {:?}", value);
        }
    }
}
```

### Text Merge Behavior

Automerge uses the Peritext algorithm for rich text:

```
Agent A: "Hello world" -> "Hello universe"
Agent B: "Hello world" -> "Hello beautiful world"

After merge: "Hello beautiful universe"
```

Character-level changes are tracked, allowing both edits to coexist.

---

## 4. Sync Protocol

### Two Synchronization Methods

**Method 1: Manual Change Transmission**

```rust
// Get changes since a known state
let changes = doc.get_changes(&old_heads)?;

// Send bytes over network
send_to_peer(&changes)?;

// On receiving end
doc.apply_changes(changes)?;
```

Simplest approach for offline-first architectures.

**Method 2: Streaming Sync Protocol**

```rust
use automerge::sync::{Message, State as SyncState};

// Each peer connection needs its own SyncState
let mut peer_state = SyncState::new();

loop {
    // Generate message to send
    if let Some(msg) = doc.generate_sync_message(&mut peer_state) {
        send_to_peer(msg)?;
    }

    // Receive and apply peer's message
    if let Some(incoming) = receive_from_peer()? {
        doc.receive_sync_message(&mut peer_state, incoming)?;
    }

    // Check if sync is complete
    if sync_complete(&peer_state) {
        break;
    }
}
```

The protocol figures out what each peer needs - only missing changes are transmitted.

### Network Requirements

- **Reliable delivery**: Protocol assumes no message loss
- **In-order delivery**: Messages must arrive in order within a connection
- **Cross-connection ordering**: Not required - Automerge handles causality

### Sync Properties

- **Idempotent**: Applying the same changes multiple times is safe
- **Efficient**: Only transmits missing changes
- **Bidirectional**: Both peers can send and receive simultaneously

---

## 5. Rust Library API

### Core Types

| Type | Description | Use Case |
|------|-------------|----------|
| `AutoCommit` | Auto-transaction document | **Recommended for most use** |
| `Automerge` | Manual transaction document | Fine-grained control |
| `ObjId` | Handle to nested object | Reference objects in document |
| `ObjType` | Object type enum | `Map`, `List`, `Text` |
| `Value` | Union of all value types | Reading values |
| `ScalarValue` | Primitive value types | Writing scalars |

### Document Lifecycle

```rust
use automerge::{AutoCommit, ROOT};

// CREATE new document
let mut doc = AutoCommit::new();

// LOAD from bytes
let bytes = std::fs::read("doc.automerge")?;
let mut doc = AutoCommit::load(&bytes)?;

// SAVE to bytes
let bytes = doc.save();
std::fs::write("doc.automerge", bytes)?;

// FORK (create independent copy)
let mut fork = doc.fork();

// MERGE (reconcile changes)
doc.merge(&mut fork)?;

// CLONE (deep copy)
let clone = doc.clone();
```

### Transaction Model

**AutoCommit** (recommended): Transactions are implicit

```rust
let mut doc = AutoCommit::new();

// All changes in one "turn" become one change
doc.put(&ROOT, "a", 1)?;
doc.put(&ROOT, "b", 2)?;
// Single change in history
```

**Automerge** (manual): Explicit transaction control

```rust
let mut doc = Automerge::new();

let mut txn = doc.transaction();
txn.put(&ROOT, "a", 1)?;
txn.put(&ROOT, "b", 2)?;
txn.commit();
```

### Working with Maps

```rust
use automerge::{AutoCommit, ObjType, ROOT};

let mut doc = AutoCommit::new();

// Create nested map
let issue = doc.put_object(&ROOT, "issue-1", ObjType::Map)?;

// Write values
doc.put(&issue, "id", "rivets-abc")?;
doc.put(&issue, "priority", 2i64)?;

// Read values
if let Some((_, value)) = doc.get(&issue, "priority")? {
    match value {
        Value::Scalar(s) => println!("Priority: {:?}", s),
        _ => {}
    }
}

// List keys
for key in doc.keys(&issue) {
    println!("Key: {}", key);
}

// Delete key
doc.delete(&issue, "assignee")?;
```

### Working with Lists

```rust
// Create list
let labels = doc.put_object(&issue, "labels", ObjType::List)?;

// Insert at index
doc.insert(&labels, 0, "backend")?;
doc.insert(&labels, 1, "api")?;

// Read by index
if let Some((_, value)) = doc.get(&labels, 0)? {
    // First element
}

// Iterate
for item in doc.list_range(&labels, ..) {
    println!("{:?}", item);
}

// Length
let len = doc.length(&labels);

// Delete at index
doc.delete(&labels, 1)?;
```

### Working with Text

```rust
// Create text object
let title = doc.put_object(&issue, "title", ObjType::Text)?;

// Set initial content
doc.splice_text(&title, 0, 0, "Initial Title")?;

// Insert at position
doc.splice_text(&title, 8, 0, " New")?;  // "Initial New Title"

// Replace range
doc.splice_text(&title, 0, 7, "Updated")?;  // "Updated New Title"

// Read full text
let text = doc.text(&title)?;
println!("{}", text);
```

### Error Handling

```rust
use automerge::AutomergeError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("Automerge error: {0}")]
    Automerge(#[from] AutomergeError),

    #[error("Issue not found: {0}")]
    NotFound(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
```

### Thread Safety

`AutoCommit` implements `Send + Sync`, making it safe for concurrent access:

```rust
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct AutomergeStorage {
    document: Arc<RwLock<AutoCommit>>,
    path: PathBuf,
}

impl AutomergeStorage {
    pub async fn get(&self, id: &str) -> Result<Option<Issue>> {
        let doc = self.document.read().await;
        // Read operations...
        Ok(issue)
    }

    pub async fn update(&self, id: &str, update: Update) -> Result<Issue> {
        let mut doc = self.document.write().await;
        // Write operations...
        Ok(issue)
    }
}
```

---

## 6. Best Practices

### Document Structure Design

**Recommended: One document per logical entity**

For an issue tracker:
- One Automerge document per issue (not one for all issues)
- Allows targeted sync
- Limits document growth
- Simplifies access control

**Alternative: Single document with careful structure**

```rust
{
    "issues": {
        "<id>": { /* issue data */ },
        // ...
    },
    "metadata": { /* config */ }
}
```

Works well for smaller projects with fewer issues.

### Field Type Selection

| Use Case | Type | Rationale |
|----------|------|-----------|
| User-editable titles | Text | Character merge |
| Descriptions, comments | Text | Collaborative editing |
| Status enums | Scalar | LWW is correct |
| Numeric values | Scalar | LWW acceptable |
| IDs, references | Scalar | Immutable |
| Tags, labels | List | Set semantics |
| Dependencies | List | Preserve all additions |
| Vote counts | Counter | Sum semantics |

### Save Frequency

- Save after each logical operation
- Use atomic writes (write to temp, rename)
- Consider JSONL mirror for human readability

```rust
pub async fn save(&self) -> Result<()> {
    let mut doc = self.document.write().await;
    let bytes = doc.save();

    let temp = self.path.with_extension("tmp");
    tokio::fs::write(&temp, bytes).await?;
    tokio::fs::rename(&temp, &self.path).await?;

    Ok(())
}
```

### History Management

By default, `save()` compacts the document (discards operation history):

```rust
// Compact save (recommended for production)
let bytes = doc.save();

// To preserve history, use the incremental format
let bytes = doc.save_incremental();
```

For issue tracking, compact saves are fine - full history isn't needed.

---

## 7. Anti-Patterns & Gotchas

### Anti-Pattern 1: Counter for Auto-Increment IDs

```rust
// WRONG - causes duplicate IDs
doc.put(&ROOT, "next_id", ScalarValue::Counter(0))?;
doc.increment(&ROOT, "next_id", 1)?;  // Different devices get different values!

// CORRECT - use UUIDs
let id = format!("{}-{}", prefix, generate_uuid_suffix());
```

### Anti-Pattern 2: Scalar Strings for Collaborative Text

```rust
// WRONG - concurrent edits lose data
doc.put(&issue, "description", "Long description text...")?;

// CORRECT - use Text type
let desc = doc.put_object(&issue, "description", ObjType::Text)?;
doc.splice_text(&desc, 0, 0, "Long description text...")?;
```

### Anti-Pattern 3: Reusing Actor IDs

```rust
// WRONG - never share actor IDs across processes
let actor = ActorId::from(b"shared-id");

// CORRECT - each process gets unique actor
let actor = ActorId::random();
```

### Anti-Pattern 4: Massive Single Document

```rust
// WRONG for large projects - sync overhead
{
    "issues": { /* thousands of issues */ }
}

// BETTER - separate documents or sharding
// doc-issues-2024-01.automerge, doc-issues-2024-02.automerge
```

### Gotcha 1: ObjId is Opaque

```rust
// WRONG - can't construct ObjId from string
let obj: ObjId = "some-id".into();  // Compile error

// CORRECT - use returned ObjId
let obj = doc.put_object(&ROOT, "key", ObjType::Map)?;
```

### Gotcha 2: Merge Mutates the Argument

```rust
let mut doc1 = AutoCommit::new();
let mut doc2 = doc1.fork();

// After merge, doc2 may be in unexpected state
doc1.merge(&mut doc2)?;

// If you need doc2 intact, clone first
let mut doc2_clone = doc2.clone();
doc1.merge(&mut doc2_clone)?;
```

### Gotcha 3: Delete vs Null

```rust
// Delete removes the key entirely
doc.delete(&obj, "assignee")?;
doc.get(&obj, "assignee")?;  // Returns None

// There's no "null" value - use delete for optional fields
```

### Gotcha 4: Text Splice Parameters

```rust
// splice_text(obj, start, delete_count, insert_text)
doc.splice_text(&text, 5, 3, "new")?;
//                      ^  ^
//                      |  delete 3 chars starting at position 5
//                      start position

// Common mistake: wrong parameter order
```

### Gotcha 5: Async is Your Responsibility

```rust
// Automerge operations are synchronous
let value = doc.get(&obj, "key")?;  // Not async!

// File I/O needs async wrapper
async fn load(path: &Path) -> Result<AutoCommit> {
    let bytes = tokio::fs::read(path).await?;  // Async
    let doc = AutoCommit::load(&bytes)?;  // Sync
    Ok(doc)
}
```

---

## 8. Performance Characteristics

### Automerge 3.0 Improvements

- **10x memory reduction** over v2
- Moby Dick benchmark: 700MB (v2) -> 1.3MB (v3)
- Documents taking 17 hours to load now open in 9 seconds
- Uses compressed columnar format at runtime

### Operation Complexity

| Operation | Complexity |
|-----------|------------|
| Load from save file | O(1) |
| Load from full history | O(operations) |
| Save | O(document size) |
| Merge | O(incoming changes) |
| Get by key | O(1) average |
| Iterate list | O(n) |

### Document Size Guidelines

- Sweet spot: up to ~2MB serialized
- Beyond 2MB: expect 4+ seconds per change
- For larger datasets: consider sharding

### Memory Usage

- Documents are held fully in memory
- Automerge 3.0 uses compressed runtime representation
- History included in memory (but compressed)

### Benchmarks to Consider

Test with YOUR specific workload:
- Number of issues
- Size of descriptions/comments
- Frequency of changes
- Number of concurrent writers

---

## 9. Type Mapping for Rivets

Based on the Rivets domain model, here's the recommended mapping:

| Issue Field | Automerge Type | Rationale |
|-------------|----------------|-----------|
| `id` | Scalar (string) | Immutable identifier |
| `title` | Text | Concurrent character edits merge |
| `description` | Text | Long-form collaborative text |
| `status` | Scalar (string) | LWW - clear semantics |
| `priority` | Scalar (i64) | LWW acceptable |
| `issue_type` | Scalar (string) | Enum value |
| `assignee` | Scalar (string) | LWW acceptable |
| `labels` | List | All additions preserved |
| `dependencies` | List of Maps | Preserve all dependency additions |
| `design` | Text | Collaborative notes |
| `acceptance_criteria` | Text | Collaborative criteria |
| `notes` | Text | Collaborative notes |
| `external_ref` | Scalar (string) | Immutable reference |
| `created_at` | Scalar (timestamp) | Not editable |
| `updated_at` | Scalar (timestamp) | Auto-updated on save |
| `closed_at` | Scalar (timestamp) | Set once on close |

### Document Schema

```rust
// Conceptual structure
{
    "issues": {
        "rivets-a3f8": {
            "id": "rivets-a3f8",              // Scalar
            "title": Text("Fix login bug"),   // Text for merge
            "description": Text("..."),       // Text for merge
            "status": "open",                 // Scalar LWW
            "priority": 2,                    // Scalar LWW
            "issue_type": "bug",              // Scalar LWW
            "assignee": "alice",              // Scalar LWW
            "labels": ["backend", "urgent"],  // List
            "dependencies": [
                {
                    "depends_on_id": "rivets-b4c9",
                    "dep_type": "blocks"
                }
            ],
            "design": Text("..."),            // Text for merge
            "acceptance_criteria": Text("..."), // Text for merge
            "notes": Text("..."),             // Text for merge
            "external_ref": "JIRA-123",       // Scalar
            "created_at": 1701907200000,      // Scalar timestamp
            "updated_at": 1701993600000,      // Scalar timestamp
            "closed_at": null                 // Deleted when not closed
        }
    },
    "metadata": {
        "prefix": "rivets",
        "version": 1
    }
}
```

---

## 10. Implementation Examples

### Storage Backend Skeleton

```rust
use automerge::{AutoCommit, ObjId, ObjType, Value, ROOT};
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct AutomergeStorage {
    document: Arc<RwLock<AutoCommit>>,
    issues_obj: ObjId,
    path: PathBuf,
    prefix: String,
}

impl AutomergeStorage {
    pub async fn new(path: PathBuf, prefix: String) -> Result<Self> {
        let (doc, issues_obj) = if path.exists() {
            let bytes = tokio::fs::read(&path).await?;
            let doc = AutoCommit::load(&bytes)?;
            let issues_obj = doc.get(&ROOT, "issues")?
                .ok_or_else(|| Error::InvalidDocument)?
                .1
                .into_object()
                .ok_or_else(|| Error::InvalidDocument)?;
            (doc, issues_obj)
        } else {
            let mut doc = AutoCommit::new();
            let issues_obj = doc.put_object(&ROOT, "issues", ObjType::Map)?;
            doc.put(&ROOT, "metadata", ObjType::Map)?;
            (doc, issues_obj)
        };

        Ok(Self {
            document: Arc::new(RwLock::new(doc)),
            issues_obj,
            path,
            prefix,
        })
    }
}
```

### Creating an Issue

```rust
async fn create(&self, new_issue: NewIssue) -> Result<Issue> {
    let id = generate_issue_id(&self.prefix);
    let now = Utc::now();

    let mut doc = self.document.write().await;

    // Create issue object
    let issue_obj = doc.put_object(&self.issues_obj, &id, ObjType::Map)?;

    // Scalar fields
    doc.put(&issue_obj, "id", &id)?;
    doc.put(&issue_obj, "status", "open")?;
    doc.put(&issue_obj, "priority", new_issue.priority as i64)?;
    doc.put(&issue_obj, "issue_type", new_issue.issue_type.to_string())?;
    doc.put(&issue_obj, "created_at", now.timestamp_millis())?;
    doc.put(&issue_obj, "updated_at", now.timestamp_millis())?;

    // Text fields
    let title_obj = doc.put_object(&issue_obj, "title", ObjType::Text)?;
    doc.splice_text(&title_obj, 0, 0, &new_issue.title)?;

    let desc_obj = doc.put_object(&issue_obj, "description", ObjType::Text)?;
    doc.splice_text(&desc_obj, 0, 0, &new_issue.description)?;

    // List fields
    if !new_issue.labels.is_empty() {
        let labels_obj = doc.put_object(&issue_obj, "labels", ObjType::List)?;
        for (i, label) in new_issue.labels.iter().enumerate() {
            doc.insert(&labels_obj, i, label)?;
        }
    }

    // Optional fields
    if let Some(assignee) = &new_issue.assignee {
        doc.put(&issue_obj, "assignee", assignee)?;
    }

    Ok(Issue { id, /* ... */ })
}
```

### Reading an Issue

```rust
async fn get(&self, id: &IssueId) -> Result<Option<Issue>> {
    let doc = self.document.read().await;

    let issue_obj = match doc.get(&self.issues_obj, id.as_str())? {
        Some((_, Value::Object(ObjType::Map))) => {
            // Get the ObjId for further queries
            // This is simplified - actual impl needs object lookup
        }
        _ => return Ok(None),
    };

    // Read scalar fields
    let status = doc.get(&issue_obj, "status")?
        .and_then(|(_, v)| v.to_str())
        .unwrap_or("open")
        .parse()?;

    let priority = doc.get(&issue_obj, "priority")?
        .and_then(|(_, v)| v.to_i64())
        .unwrap_or(2) as u8;

    // Read text fields
    let title_obj = doc.get(&issue_obj, "title")?
        .and_then(|(_, v)| v.into_object())
        .ok_or(Error::InvalidDocument)?;
    let title = doc.text(&title_obj)?;

    // Read list fields
    let mut labels = Vec::new();
    if let Some((_, Value::Object(ObjType::List))) = doc.get(&issue_obj, "labels")? {
        // Iterate list...
    }

    Ok(Some(Issue { /* ... */ }))
}
```

### Merging Documents

```rust
/// Merge changes from another Automerge file (e.g., from git merge)
async fn merge_from_file(&self, other_path: &Path) -> Result<()> {
    let other_bytes = tokio::fs::read(other_path).await?;
    let mut other_doc = AutoCommit::load(&other_bytes)?;

    let mut doc = self.document.write().await;
    doc.merge(&mut other_doc)?;

    Ok(())
}
```

### Sync Implementation

```rust
use automerge::sync::{Message, State as SyncState};

pub struct SyncPeer {
    state: SyncState,
}

impl AutomergeStorage {
    pub async fn generate_sync_message(&self, peer: &mut SyncPeer) -> Option<Vec<u8>> {
        let doc = self.document.read().await;
        doc.generate_sync_message(&mut peer.state)
            .map(|msg| msg.encode())
    }

    pub async fn receive_sync_message(
        &self,
        peer: &mut SyncPeer,
        message: &[u8]
    ) -> Result<()> {
        let msg = Message::decode(message)?;
        let mut doc = self.document.write().await;
        doc.receive_sync_message(&mut peer.state, msg)?;
        Ok(())
    }
}
```

---

## 11. References

### Official Documentation
- [Automerge Official Site](https://automerge.org/)
- [Automerge Concepts](https://automerge.org/docs/reference/concepts/)
- [Automerge Data Model](https://automerge.org/docs/reference/documents/)
- [Automerge Conflict Resolution](https://automerge.org/docs/reference/documents/conflicts/)
- [Automerge 3.0 Release](https://automerge.org/blog/automerge-3/)

### Rust Crate
- [automerge on crates.io](https://crates.io/crates/automerge)
- [automerge on docs.rs](https://docs.rs/automerge)
- [automerge-repo-rs](https://github.com/automerge/automerge-repo-rs)

### Research Papers
- [Peritext: A CRDT for Rich-Text Collaboration](https://www.inkandswitch.com/peritext/)
- [Local-first software](https://www.inkandswitch.com/local-first/)

### Talks & Articles
- [Martin Kleppmann - CRDTs: The Hard Parts](https://martin.kleppmann.com/2020/07/06/crdt-hard-parts-hydra.html)
- [Martin Kleppmann - Creating Local-First Collaboration Software](https://martin.kleppmann.com/2023/09/27/acm-tech-talks.html)

### Community Resources
- [Automerge GitHub Discussions](https://github.com/automerge/automerge/discussions)
- [Data Modeling Best Practices](https://automerge.org/docs/cookbook/modeling-data/)
