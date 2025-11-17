# Storage Layer Architecture

## Storage Trait Hierarchy

```mermaid
classDiagram
    class IssueStorage {
        <<trait>>
        +create(NewIssue) Future~Issue~
        +get(IssueId) Future~Option~Issue~~
        +update(IssueId, IssueUpdate) Future~Issue~
        +delete(IssueId) Future~void~
        +add_dependency(from, to, type) Future~void~
        +remove_dependency(from, to) Future~void~
        +get_dependencies(IssueId) Future~Vec~Dependency~~
        +get_dependents(IssueId) Future~Vec~Dependency~~
        +has_cycle(from, to) Future~bool~
        +list(IssueFilter) Future~Vec~Issue~~
        +ready_to_work(filter) Future~Vec~Issue~~
        +blocked_issues() Future~Vec~Tuple~~
        +import_issues(Vec~Issue~) Future~void~
        +export_all() Future~Vec~Issue~~
        +save() Future~void~
    }

    class InMemoryStorage {
        Arc~Mutex~InMemoryStorageInner~~
        +new() Self
        +load_from_jsonl(Path) Future~(Self, Vec~Warning~)~
        +save_to_jsonl(Path) Future~void~
    }

    class InMemoryStorageInner {
        -HashMap~IssueId, Issue~ issues
        -DiGraph~IssueId, DependencyType~ graph
        -HashMap~IssueId, NodeIndex~ node_map
        +create_internal(NewIssue) Issue
        +has_cycle_internal(from, to) bool
        +add_dependency_edge_internal(Dependency) void
    }

    class PostgresStorage {
        <<future>>
        Pool~Postgres~ pool
        +new(connection_string) Future~Self~
        +execute_cte(query) Future~Vec~Issue~~
    }

    IssueStorage <|.. InMemoryStorage : implements
    IssueStorage <|.. PostgresStorage : implements
    InMemoryStorage --> InMemoryStorageInner : wraps
```

## InMemoryStorage Structure

```mermaid
graph TB
    subgraph "Thread-Safe Wrapper"
        Arc[Arc&lt;Mutex&lt;InMemoryStorageInner&gt;&gt;]
    end

    subgraph "Inner Storage (Private)"
        HashMap[HashMap&lt;IssueId, Issue&gt;<br/>Fast O(1) lookups]
        DiGraph[DiGraph&lt;IssueId, DependencyType&gt;<br/>Directed graph for dependencies]
        NodeMap[HashMap&lt;IssueId, NodeIndex&gt;<br/>ID to graph node mapping]
    end

    subgraph "Graph Structure"
        Node1((rivets-a3f8))
        Node2((rivets-x9k2))
        Node3((rivets-p4m1))

        Node1 -->|blocks| Node2
        Node1 -->|parent-child| Node3
    end

    Arc --> HashMap
    Arc --> DiGraph
    Arc --> NodeMap
    DiGraph -.represents.-> Node1
    DiGraph -.represents.-> Node2
    DiGraph -.represents.-> Node3

    style Arc fill:#FFE4B5
    style HashMap fill:#90EE90
    style DiGraph fill:#ADD8E6
    style NodeMap fill:#FFB6C1
```

### Data Structure Details

#### HashMap<IssueId, Issue>
- **Purpose**: Fast O(1) lookup by ID
- **Contains**: Full issue data (title, description, status, dependencies, etc.)
- **Memory**: ~1KB per issue average

#### DiGraph<IssueId, DependencyType>
- **Purpose**: Efficient graph algorithms (cycle detection, traversal)
- **Nodes**: Issue IDs
- **Edges**: Dependency type (blocks, related, parent-child, discovered-from)
- **Library**: petgraph 0.6
- **Algorithms**: `has_path_connecting`, `edges_directed`

#### HashMap<IssueId, NodeIndex>
- **Purpose**: Map issue IDs to graph node indices
- **Needed**: petgraph uses numeric NodeIndex, we use IssueId strings
- **Synchronization**: Must stay in sync with DiGraph and issues HashMap

## JSONL Persistence Layer

```mermaid
sequenceDiagram
    participant App
    participant Storage as InMemoryStorage
    participant Inner as InMemoryStorageInner
    participant FS as tokio::fs

    Note over App,FS: SAVE Operation

    App->>Storage: save()
    Storage->>Inner: lock().await
    Storage->>FS: create(temp.jsonl)
    loop For each issue
        Inner->>FS: write_all(json + \n)
    end
    Storage->>FS: flush()
    Storage->>FS: rename(temp → issues.jsonl)
    Storage-->>App: Ok(())

    Note over App,FS: LOAD Operation

    App->>Storage: load_from_jsonl(path)
    Storage->>FS: open(issues.jsonl)
    loop Pass 1: Import issues
        FS->>Storage: read_line()
        Storage->>Storage: parse JSON
        alt Valid JSON
            Storage->>Inner: insert issue (no deps)
            Storage->>Inner: add graph node
        else Invalid JSON
            Storage->>Storage: warnings.push(MalformedJson)
        end
    end
    loop Pass 2: Add dependencies
        Storage->>Storage: for each issue.dependencies
        alt Target exists
            Storage->>Storage: has_cycle_internal()
            alt No cycle
                Storage->>Inner: add_edge()
            else Cycle detected
                Storage->>Storage: warnings.push(CircularDep)
            end
        else Target missing
            Storage->>Storage: warnings.push(OrphanedDep)
        end
    end
    Storage-->>App: Ok((storage, warnings))
```

### JSONL Format Example

```json
{"id":"rivets-a3f8","title":"Implement feature X","description":"...","status":"open","priority":2,"issue_type":"feature","created_at":"2025-11-17T10:00:00Z","updated_at":"2025-11-17T10:00:00Z","dependencies":[{"depends_on_id":"rivets-x9k2","dep_type":"blocks"}],"labels":["backend","api"]}
{"id":"rivets-x9k2","title":"Fix bug Y","description":"...","status":"in_progress","priority":1,"issue_type":"bug","created_at":"2025-11-17T09:00:00Z","updated_at":"2025-11-17T11:00:00Z","dependencies":[],"labels":["urgent"]}
```

### Error Recovery Strategies

#### Malformed JSON Line
```
Line 42: Invalid JSON, skipping: expected ',' at line 1 column 234
Warning: Loaded with 1 errors. 99 issues imported.
```
- **Action**: Skip line, log warning, continue
- **Result**: Partial data loss (that one issue)
- **Recovery**: User can manually fix JSONL file

#### Orphaned Dependency
```
Issue rivets-a3f8 depends on rivets-MISSING (not found in file)
Warning: 1 orphaned dependencies skipped
```
- **Action**: Skip dependency edge, import issue without that dep
- **Result**: Issue exists but missing one dependency
- **Recovery**: Resilient to partial exports/imports

#### Circular Dependency
```
Cycle detected: rivets-a3f8 → rivets-x9k2 → rivets-a3f8
Warning: 1 circular dependencies skipped
```
- **Action**: Skip edge that would create cycle
- **Result**: Breaks cycle, maintains graph integrity
- **Prevention**: Runtime operations prevent cycle creation

## Cycle Detection Algorithm

```mermaid
graph TD
    Start[add_dependency from→to] --> Check{has_cycle?}
    Check -->|Check path| Path[has_path_connecting<br/>graph, to, from]
    Path -->|Yes| Reject[Err: CircularDependency]
    Path -->|No| Add[add_edge from→to]
    Add --> Update[Update issue.dependencies]
    Update --> Success[Ok]

    style Reject fill:#FFB6C1
    style Success fill:#90EE90
```

### Implementation Details

```rust
async fn has_cycle(&self, from: &IssueId, to: &IssueId) -> Result<bool> {
    let inner = self.lock().await;

    // Get graph node indices
    let from_node = inner.node_map.get(from)
        .ok_or_else(|| Error::IssueNotFound(from.clone()))?;
    let to_node = inner.node_map.get(to)
        .ok_or_else(|| Error::IssueNotFound(to.clone()))?;

    // Check if path exists from 'to' back to 'from'
    // If adding edge from→to creates this path, we have a cycle
    Ok(petgraph::algo::has_path_connecting(
        &inner.graph,
        *to_node,      // Start at 'to'
        *from_node,    // Try to reach 'from'
        None           // No edge filter
    ))
}
```

**Time Complexity**: O(V + E) worst case (full graph traversal)
**Space Complexity**: O(V) for visited set
**Optimization**: Early termination when path found

## Ready Work Algorithm

```mermaid
graph TD
    Start[ready_to_work filter] --> Init[blocked = empty set]

    Init --> Phase1[Phase 1: Direct Blocks]
    Phase1 --> Loop1{For each issue}
    Loop1 --> Check1{Has blocking<br/>dependency?}
    Check1 -->|Yes| CheckStatus{Blocker is<br/>open/in_progress?}
    CheckStatus -->|Yes| AddBlocked[blocked.insert issue]
    CheckStatus -->|No| Loop1
    Check1 -->|No| Loop1
    Loop1 -->|Done| Phase2

    Phase2[Phase 2: Transitive via parent-child] --> BFS[BFS queue = blocked]
    BFS --> Loop2{Queue not empty?}
    Loop2 -->|Yes| Pop[pop issue, depth]
    Pop --> DepthCheck{depth < 50?}
    DepthCheck -->|Yes| Children[Find child issues<br/>via parent-child edges]
    Children --> AddChildren[blocked.insert children<br/>queue.push children, depth+1]
    AddChildren --> Loop2
    DepthCheck -->|No| Loop2
    Loop2 -->|No| Filter

    Filter[Filter: status ≠ closed<br/>AND id ∉ blocked] --> ApplyFilter[Apply additional filters]
    ApplyFilter --> Sort[Sort by policy]
    Sort --> Result[Return ready issues]

    style AddBlocked fill:#FFB6C1
    style Result fill:#90EE90
```

### Blocking Propagation Example

```mermaid
graph TD
    Epic[Epic: rivets-epic1<br/>BLOCKED by feature1] -->|parent-child| Task1[Task: rivets-task1<br/>TRANSITIVELY BLOCKED]
    Epic -->|parent-child| Task2[Task: rivets-task2<br/>TRANSITIVELY BLOCKED]
    Task1 -->|parent-child| Subtask1[Subtask: rivets-sub1<br/>TRANSITIVELY BLOCKED]

    Feature1[Feature: rivets-feat1<br/>Status: in_progress] -->|blocks| Epic

    style Epic fill:#FFB6C1
    style Task1 fill:#FFB6C1
    style Task2 fill:#FFB6C1
    style Subtask1 fill:#FFB6C1
    style Feature1 fill:#FFE4B5
```

**Result**: None of these issues appear in "ready work" because they're all blocked (directly or transitively)

## Delete Operation with Referential Integrity

```mermaid
sequenceDiagram
    participant User
    participant Storage
    participant Graph

    User->>Storage: delete(rivets-a3f8)
    Storage->>Graph: get_dependents(rivets-a3f8)
    Graph-->>Storage: [rivets-x9k2, rivets-p4m1]

    alt Has dependents
        Storage-->>User: Error: Cannot delete rivets-a3f8<br/>2 issues depend on it:<br/>rivets-x9k2, rivets-p4m1
    else No dependents
        Storage->>Graph: get_dependencies(rivets-a3f8)
        Graph-->>Storage: [rivets-old1 blocks, rivets-old2 related]
        loop For each dependency
            Storage->>Graph: remove_edge(rivets-a3f8 → dep)
        end
        Storage->>Storage: issues.remove(rivets-a3f8)
        Storage->>Graph: remove_node(rivets-a3f8)
        Storage->>Storage: node_map.remove(rivets-a3f8)
        Storage-->>User: Ok: Deleted rivets-a3f8
    end
```

### Safety Guarantees

1. **No orphaned dependents**: Cannot delete if other issues depend on it
2. **Clean outgoing deps**: Automatically removes all outgoing dependency edges
3. **Graph consistency**: Maintains sync between HashMap, DiGraph, and node_map
4. **Clear errors**: Lists all dependent issues preventing deletion

## Backend Factory Pattern

```mermaid
graph TD
    Config[config.yaml] --> Factory{create_storage}

    Factory -->|backend: memory| InMem[InMemoryStorage::new]
    Factory -->|backend: memory<br/>+ data_file exists| Load[InMemoryStorage::load_from_jsonl]
    Factory -->|backend: postgres| PG[PostgresStorage::new]

    InMem --> Box[Box&lt;dyn IssueStorage&gt;]
    Load --> Box
    PG --> Box

    Box --> App[App uses trait methods]

    style InMem fill:#90EE90
    style Load fill:#90EE90
    style PG fill:#FFE4B5
    style Box fill:#ADD8E6
```

### Configuration Example

```yaml
# .rivets/config.yaml
issue-prefix: "rivets"

storage:
  backend: "memory"
  data_file: ".rivets/issues.jsonl"

  # Future Phase 3
  # backend: "postgres"
  # postgres:
  #   host: "localhost"
  #   port: 5432
  #   database: "rivets"
  #   user: "rivets"
```

## Performance Characteristics

| Operation | Time Complexity | Space Complexity | Notes |
|-----------|----------------|------------------|-------|
| create | O(1) | O(1) | HashMap insert + graph node |
| get | O(1) | O(1) | HashMap lookup |
| update | O(1) | O(1) | HashMap update |
| delete | O(D) | O(D) | D = number of dependencies |
| add_dependency | O(V + E) | O(V) | Cycle detection via path search |
| has_cycle | O(V + E) | O(V) | DFS/BFS traversal |
| list (no filter) | O(N) | O(N) | Iterate all issues |
| ready_to_work | O(V + E) | O(V) | BFS for transitive blocks |
| save_to_jsonl | O(N) | O(1) | Streaming write |
| load_from_jsonl | O(N * E) | O(N) | N issues, E edges per issue |

Where:
- V = number of vertices (issues)
- E = number of edges (dependencies)
- N = total issues
- D = dependencies per issue

## Memory Layout (1000 Issues)

```
Total: ~2-3 MB

- HashMap<IssueId, Issue>: ~1 MB
  └─ Issue struct: ~1 KB each
     ├─ Strings: title (50), description (200), notes (100)
     ├─ Timestamps: 24 bytes each
     ├─ Enums: 1 byte each
     └─ Vec<Dependency>: ~8 bytes per dep

- DiGraph: ~200 KB
  └─ Nodes: 1000 × 8 bytes (NodeIndex)
  └─ Edges: ~500 × 24 bytes (from, to, weight)

- HashMap<IssueId, NodeIndex>: ~64 KB
  └─ 1000 entries × 64 bytes (String + u64)

- Arc + Mutex overhead: ~100 bytes
```

**Scales linearly** with number of issues and dependencies.
