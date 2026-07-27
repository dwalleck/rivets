# Event Sourcing Design

This document defines the event sourcing model for Rivets, including event types, the EventStore trait, and projection logic.

## Overview

Instead of storing issue snapshots, Rivets stores a sequence of events that describe what happened. Current state is derived by replaying events.

```
Traditional (snapshot):     Event-sourced:
┌─────────────────────┐     ┌─────────────────────────────────────┐
│ { id: "r-001",      │     │ {"type":"issue_created","id":"r-001"}
│   title: "Bug",     │     │ {"type":"status_changed","status":"in_progress"}
│   status: "closed"} │     │ {"type":"status_changed","status":"closed"}
└─────────────────────┘     └─────────────────────────────────────┘
     Current state               History of changes
```

## Benefits

- **Git-friendly**: Append-only log produces clean diffs
- **Audit trail**: Full history of what happened and when
- **Real-time sync**: Events are the natural unit of change notification
- **Time travel**: Reconstruct state at any point in time
- **Debuggability**: See exactly what operations occurred

## Event Envelope

Every event is wrapped in an envelope with metadata:

```rust
/// Unique event identifier
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EventId(pub Uuid);

impl EventId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

/// Event envelope with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope {
    /// Unique identifier for this event
    pub id: EventId,

    /// Monotonically increasing sequence number (per workspace)
    pub sequence: u64,

    /// When the event occurred
    pub timestamp: DateTime<Utc>,

    /// Hash of workspace path (for multi-workspace scenarios)
    pub workspace_id: String,

    /// The domain event payload
    pub event: DomainEvent,

    /// Optional correlation ID for tracing related operations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
}
```

### JSONL Format

Each line in `events.jsonl` is one `EventEnvelope`:

```jsonl
{"id":"550e8400-e29b-41d4-a716-446655440000","sequence":1,"timestamp":"2024-01-15T10:30:00Z","workspace_id":"a1b2c3d4","event":{"type":"issue_created","id":"rivets-001","title":"Add daemon support",...}}
{"id":"550e8400-e29b-41d4-a716-446655440001","sequence":2,"timestamp":"2024-01-15T11:00:00Z","workspace_id":"a1b2c3d4","event":{"type":"status_changed","id":"rivets-001","old_status":"open","new_status":"in_progress"}}
{"id":"550e8400-e29b-41d4-a716-446655440002","sequence":3,"timestamp":"2024-01-15T14:30:00Z","workspace_id":"a1b2c3d4","event":{"type":"issue_created","id":"rivets-002","title":"Fix bug in parser",...}}
```

## Domain Events

### Event Enum

```rust
/// All domain events (tagged union for serde)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DomainEvent {
    // Issue lifecycle
    IssueCreated(IssueCreated),
    IssueUpdated(IssueUpdated),
    IssueDeleted(IssueDeleted),

    // Status transitions (semantic events)
    StatusChanged(StatusChanged),

    // Assignment
    AssigneeChanged(AssigneeChanged),

    // Labels
    LabelAdded(LabelAdded),
    LabelRemoved(LabelRemoved),

    // Dependencies
    DependencyAdded(DependencyAdded),
    DependencyRemoved(DependencyRemoved),
}
```

### Event Type Definitions

#### IssueCreated

Emitted when a new issue is created.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueCreated {
    pub id: IssueId,
    pub title: String,
    pub description: String,
    pub priority: u8,
    pub issue_type: IssueType,
    pub assignee: Option<String>,
    pub labels: Vec<String>,
    pub design: Option<String>,
    pub acceptance_criteria: Option<String>,
    pub notes: Option<String>,
    pub external_ref: Option<String>,
    /// Initial dependencies (created atomically with issue)
    pub dependencies: Vec<(IssueId, DependencyType)>,
}
```

#### IssueUpdated

Emitted for field-level updates. Uses `FieldChange<T>` to capture before/after values.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueUpdated {
    pub id: IssueId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<FieldChange<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<FieldChange<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<FieldChange<u8>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issue_type: Option<FieldChange<IssueType>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub design: Option<FieldChange<Option<String>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acceptance_criteria: Option<FieldChange<Option<String>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<FieldChange<Option<String>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_ref: Option<FieldChange<Option<String>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldChange<T> {
    pub old: T,
    pub new: T,
}
```

#### StatusChanged

Semantic event for status transitions. Separate from `IssueUpdated` for easier subscription filtering.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusChanged {
    pub id: IssueId,
    pub old_status: IssueStatus,
    pub new_status: IssueStatus,
    /// Set when transitioning to Closed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closed_at: Option<DateTime<Utc>>,
    /// Optional reason (for close/reopen)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}
```

#### AssigneeChanged

Semantic event for assignment changes.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssigneeChanged {
    pub id: IssueId,
    pub old_assignee: Option<String>,
    pub new_assignee: Option<String>,
}
```

#### IssueDeleted

Emitted when an issue is permanently deleted.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueDeleted {
    pub id: IssueId,
}
```

#### Label Events

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelAdded {
    pub id: IssueId,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LabelRemoved {
    pub id: IssueId,
    pub label: String,
}
```

#### Dependency Events

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyAdded {
    /// The issue that depends on another
    pub from: IssueId,
    /// The issue being depended on
    pub to: IssueId,
    pub dep_type: DependencyType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyRemoved {
    pub from: IssueId,
    pub to: IssueId,
}
```

## EventStore Trait

```rust
use async_trait::async_trait;
use tokio::sync::broadcast;

#[derive(Debug, thiserror::Error)]
pub enum EventStoreError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Concurrency conflict: expected sequence {expected}, got {actual}")]
    ConcurrencyConflict { expected: u64, actual: u64 },
}

pub type Result<T> = std::result::Result<T, EventStoreError>;

#[async_trait]
pub trait EventStore: Send + Sync {
    /// Append events to the store.
    ///
    /// If `expected_sequence` is provided, fails if current sequence doesn't match
    /// (optimistic concurrency control).
    ///
    /// Returns the sequence numbers assigned to each event.
    async fn append(
        &self,
        events: Vec<DomainEvent>,
        expected_sequence: Option<u64>,
    ) -> Result<Vec<u64>>;

    /// Read all events starting from a sequence number (inclusive).
    async fn read_from(&self, from_sequence: u64) -> Result<Vec<EventEnvelope>>;

    /// Get the current (highest) sequence number, or 0 if empty.
    async fn current_sequence(&self) -> Result<u64>;

    /// Subscribe to new events.
    ///
    /// Returns a broadcast receiver that will receive all events
    /// appended after subscription.
    fn subscribe(&self) -> broadcast::Receiver<EventEnvelope>;
}
```

### JsonlEventStore Implementation

```rust
pub struct JsonlEventStore {
    /// Path to events.jsonl
    path: PathBuf,

    /// Workspace ID (hash of workspace path)
    workspace_id: String,

    /// Current sequence number
    sequence: AtomicU64,

    /// Broadcast channel for subscribers
    broadcast: broadcast::Sender<EventEnvelope>,

    /// File lock for writes
    write_lock: Mutex<()>,
}

impl JsonlEventStore {
    /// Create or open an event store for the given workspace.
    pub async fn open(workspace_root: &Path) -> Result<Self> {
        let path = workspace_root.join(".rivets/events.jsonl");
        let workspace_id = hash_workspace_path(workspace_root);

        // Determine current sequence by reading existing events
        let sequence = if path.exists() {
            Self::read_last_sequence(&path).await?
        } else {
            0
        };

        let (broadcast, _) = broadcast::channel(1024);

        Ok(Self {
            path,
            workspace_id,
            sequence: AtomicU64::new(sequence),
            broadcast,
            write_lock: Mutex::new(()),
        })
    }

    /// Read all events (for initial projection rebuild).
    pub async fn read_all(&self) -> Result<Vec<EventEnvelope>> {
        self.read_from(0).await
    }
}

#[async_trait]
impl EventStore for JsonlEventStore {
    async fn append(
        &self,
        events: Vec<DomainEvent>,
        expected_sequence: Option<u64>,
    ) -> Result<Vec<u64>> {
        let _lock = self.write_lock.lock().await;

        // Optimistic concurrency check
        let current = self.sequence.load(Ordering::SeqCst);
        if let Some(expected) = expected_sequence {
            if current != expected {
                return Err(EventStoreError::ConcurrencyConflict {
                    expected,
                    actual: current,
                });
            }
        }

        let timestamp = Utc::now();
        let mut sequences = Vec::with_capacity(events.len());
        let mut envelopes = Vec::with_capacity(events.len());

        // Prepare envelopes
        for event in events {
            let seq = current + sequences.len() as u64 + 1;
            sequences.push(seq);

            envelopes.push(EventEnvelope {
                id: EventId::new(),
                sequence: seq,
                timestamp,
                workspace_id: self.workspace_id.clone(),
                event,
                correlation_id: None,
            });
        }

        // Append to file (atomic via rivets-jsonl)
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .await?;

        let mut writer = BufWriter::new(file);
        for envelope in &envelopes {
            let line = serde_json::to_string(envelope)?;
            writer.write_all(line.as_bytes()).await?;
            writer.write_all(b"\n").await?;
        }
        writer.flush().await?;

        // Update sequence
        self.sequence.store(
            current + envelopes.len() as u64,
            Ordering::SeqCst,
        );

        // Broadcast to subscribers
        for envelope in envelopes {
            let _ = self.broadcast.send(envelope);
        }

        Ok(sequences)
    }

    async fn read_from(&self, from_sequence: u64) -> Result<Vec<EventEnvelope>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }

        let content = tokio::fs::read_to_string(&self.path).await?;
        let mut events = Vec::new();

        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let envelope: EventEnvelope = serde_json::from_str(line)?;
            if envelope.sequence >= from_sequence {
                events.push(envelope);
            }
        }

        Ok(events)
    }

    async fn current_sequence(&self) -> Result<u64> {
        Ok(self.sequence.load(Ordering::SeqCst))
    }

    fn subscribe(&self) -> broadcast::Receiver<EventEnvelope> {
        self.broadcast.subscribe()
    }
}
```

## Projection (State Reconstruction)

The `WorkspaceProjection` maintains current state by applying events:

```rust
use petgraph::graph::{DiGraph, NodeIndex};
use std::collections::HashMap;

/// Materialized view of workspace state derived from events
pub struct WorkspaceProjection {
    issues: HashMap<IssueId, Issue>,
    graph: DiGraph<IssueId, DependencyType>,
    node_map: HashMap<IssueId, NodeIndex>,
    last_sequence: u64,
    id_generator: IdGenerator,
    prefix: String,
}

impl WorkspaceProjection {
    pub fn new(prefix: String) -> Self {
        Self {
            issues: HashMap::new(),
            graph: DiGraph::new(),
            node_map: HashMap::new(),
            last_sequence: 0,
            id_generator: IdGenerator::new(IdGeneratorConfig {
                prefix: prefix.clone(),
                database_size: 0,
            }),
            prefix,
        }
    }

    /// Apply a single event to update state
    pub fn apply(&mut self, envelope: &EventEnvelope) {
        use DomainEvent::*;

        match &envelope.event {
            IssueCreated(e) => self.apply_issue_created(e, envelope.timestamp),
            IssueUpdated(e) => self.apply_issue_updated(e, envelope.timestamp),
            IssueDeleted(e) => self.apply_issue_deleted(e),
            StatusChanged(e) => self.apply_status_changed(e, envelope.timestamp),
            AssigneeChanged(e) => self.apply_assignee_changed(e, envelope.timestamp),
            LabelAdded(e) => self.apply_label_added(e, envelope.timestamp),
            LabelRemoved(e) => self.apply_label_removed(e, envelope.timestamp),
            DependencyAdded(e) => self.apply_dependency_added(e),
            DependencyRemoved(e) => self.apply_dependency_removed(e),
        }

        self.last_sequence = envelope.sequence;
    }

    /// Rebuild state from event stream
    pub fn rebuild(events: impl Iterator<Item = EventEnvelope>, prefix: String) -> Self {
        let mut projection = Self::new(prefix);
        for envelope in events {
            projection.apply(&envelope);
        }
        projection
    }

    // Event application methods

    fn apply_issue_created(&mut self, e: &IssueCreated, timestamp: DateTime<Utc>) {
        let issue = Issue {
            id: e.id.clone(),
            title: e.title.clone(),
            description: e.description.clone(),
            status: IssueStatus::Open,
            priority: e.priority,
            issue_type: e.issue_type,
            assignee: e.assignee.clone(),
            labels: e.labels.clone(),
            design: e.design.clone(),
            acceptance_criteria: e.acceptance_criteria.clone(),
            notes: e.notes.clone(),
            external_ref: e.external_ref.clone(),
            dependencies: Vec::new(),
            created_at: timestamp,
            updated_at: timestamp,
            closed_at: None,
        };

        // Add to graph
        let node = self.graph.add_node(e.id.clone());
        self.node_map.insert(e.id.clone(), node);

        // Handle initial dependencies
        for (dep_id, dep_type) in &e.dependencies {
            if let Some(&to_node) = self.node_map.get(dep_id) {
                self.graph.add_edge(node, to_node, *dep_type);
            }
        }

        // Register ID with generator
        self.id_generator.register_id(e.id.as_str().to_string());

        self.issues.insert(e.id.clone(), issue);
    }

    fn apply_issue_updated(&mut self, e: &IssueUpdated, timestamp: DateTime<Utc>) {
        if let Some(issue) = self.issues.get_mut(&e.id) {
            if let Some(c) = &e.title {
                issue.title = c.new.clone();
            }
            if let Some(c) = &e.description {
                issue.description = c.new.clone();
            }
            if let Some(c) = &e.priority {
                issue.priority = c.new;
            }
            if let Some(c) = &e.issue_type {
                issue.issue_type = c.new;
            }
            if let Some(c) = &e.design {
                issue.design = c.new.clone();
            }
            if let Some(c) = &e.acceptance_criteria {
                issue.acceptance_criteria = c.new.clone();
            }
            if let Some(c) = &e.notes {
                issue.notes = c.new.clone();
            }
            if let Some(c) = &e.external_ref {
                issue.external_ref = c.new.clone();
            }
            issue.updated_at = timestamp;
        }
    }

    fn apply_issue_deleted(&mut self, e: &IssueDeleted) {
        if let Some(node) = self.node_map.remove(&e.id) {
            self.graph.remove_node(node);
        }
        self.issues.remove(&e.id);
    }

    fn apply_status_changed(&mut self, e: &StatusChanged, timestamp: DateTime<Utc>) {
        if let Some(issue) = self.issues.get_mut(&e.id) {
            issue.status = e.new_status;
            issue.closed_at = e.closed_at;
            issue.updated_at = timestamp;
        }
    }

    fn apply_assignee_changed(&mut self, e: &AssigneeChanged, timestamp: DateTime<Utc>) {
        if let Some(issue) = self.issues.get_mut(&e.id) {
            issue.assignee = e.new_assignee.clone();
            issue.updated_at = timestamp;
        }
    }

    fn apply_label_added(&mut self, e: &LabelAdded, timestamp: DateTime<Utc>) {
        if let Some(issue) = self.issues.get_mut(&e.id) {
            if !issue.labels.contains(&e.label) {
                issue.labels.push(e.label.clone());
            }
            issue.updated_at = timestamp;
        }
    }

    fn apply_label_removed(&mut self, e: &LabelRemoved, timestamp: DateTime<Utc>) {
        if let Some(issue) = self.issues.get_mut(&e.id) {
            issue.labels.retain(|l| l != &e.label);
            issue.updated_at = timestamp;
        }
    }

    fn apply_dependency_added(&mut self, e: &DependencyAdded) {
        if let (Some(&from_node), Some(&to_node)) =
            (self.node_map.get(&e.from), self.node_map.get(&e.to))
        {
            self.graph.add_edge(from_node, to_node, e.dep_type);

            if let Some(issue) = self.issues.get_mut(&e.from) {
                issue.dependencies.push(Dependency {
                    depends_on_id: e.to.clone(),
                    dep_type: e.dep_type,
                });
            }
        }
    }

    fn apply_dependency_removed(&mut self, e: &DependencyRemoved) {
        if let (Some(&from_node), Some(&to_node)) =
            (self.node_map.get(&e.from), self.node_map.get(&e.to))
        {
            if let Some(edge) = self.graph.find_edge(from_node, to_node) {
                self.graph.remove_edge(edge);
            }

            if let Some(issue) = self.issues.get_mut(&e.from) {
                issue.dependencies.retain(|d| d.depends_on_id != e.to);
            }
        }
    }
}
```

### Query Methods

The projection exposes the same query interface as the current storage:

```rust
impl WorkspaceProjection {
    pub fn get(&self, id: &IssueId) -> Option<&Issue> {
        self.issues.get(id)
    }

    pub fn list(&self, filter: &IssueFilter) -> Vec<Issue> {
        self.issues
            .values()
            .filter(|issue| matches_filter(issue, filter))
            .cloned()
            .collect()
    }

    pub fn ready_to_work(&self, filter: Option<&IssueFilter>) -> Vec<Issue> {
        // Same logic as current InMemoryStorageInner
        // Uses graph to find issues with no blocking dependencies
    }

    pub fn blocked_issues(&self) -> Vec<(Issue, Vec<Issue>)> {
        // Same logic as current implementation
    }

    pub fn generate_id(&mut self) -> IssueId {
        self.id_generator.generate()
    }

    pub fn last_sequence(&self) -> u64 {
        self.last_sequence
    }
}
```

## Future: Snapshotting

For workspaces with very large event histories, periodic snapshots can speed up startup:

```rust
/// Optional snapshot support
#[async_trait]
pub trait SnapshotStore: Send + Sync {
    /// Save a snapshot of current state at a sequence number
    async fn save_snapshot(
        &self,
        projection: &WorkspaceProjection,
        at_sequence: u64,
    ) -> Result<()>;

    /// Load the latest snapshot (if any)
    async fn load_snapshot(&self) -> Result<Option<(WorkspaceProjection, u64)>>;
}
```

Startup with snapshots:

```
1. Load latest snapshot (if exists) → projection at sequence N
2. Read events from sequence N+1
3. Apply events to projection
4. Ready to serve
```

This is an optimization for later - initial implementation can replay full history on startup.

## Event Versioning

Events are immutable once written. If the event schema needs to change:

1. **Add new optional fields**: Backwards compatible
2. **New event type**: For significant changes (e.g., `IssueCreatedV2`)
3. **Projection handles both**: Apply logic checks event version

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DomainEvent {
    IssueCreated(IssueCreated),
    IssueCreatedV2(IssueCreatedV2),  // Future version
    // ...
}
```

The projection's `apply` method handles all versions, converting to the same internal state.
