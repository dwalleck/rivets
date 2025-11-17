# Rivets Data Flow

## Complete Command Lifecycle

```mermaid
sequenceDiagram
    autonumber
    participant User
    participant Shell
    participant main.rs
    participant CLI Parser
    participant App
    participant Command
    participant Storage Trait
    participant InMemoryStorage
    participant Graph
    participant JSONL

    User->>Shell: rivets create --title "Fix bug"
    Shell->>main.rs: Execute binary
    main.rs->>main.rs: #[tokio::main(flavor = "current_thread")]
    main.rs->>CLI Parser: Cli::parse()
    CLI Parser->>CLI Parser: Validate arguments
    CLI Parser-->>main.rs: Commands::Create(args)

    main.rs->>App: Config::load().await
    App->>App: Merge config sources
    App-->>main.rs: Config

    main.rs->>App: App::new(config).await
    App->>App: create_storage(&config.storage).await
    App->>InMemoryStorage: load_from_jsonl(path).await
    InMemoryStorage->>JSONL: Open file, stream read
    loop For each line
        JSONL-->>InMemoryStorage: JSON string
        InMemoryStorage->>InMemoryStorage: Parse Issue
        InMemoryStorage->>Graph: Add node
    end
    InMemoryStorage-->>App: (Storage, warnings)
    App-->>main.rs: App

    main.rs->>Command: command.execute(&mut app).await
    Command->>Command: Gather missing args (interactive)
    Command->>Command: Build NewIssue
    Command->>Storage Trait: storage.create(new_issue).await
    Storage Trait->>InMemoryStorage: (via trait dispatch)

    InMemoryStorage->>InMemoryStorage: Lock mutex
    InMemoryStorage->>InMemoryStorage: generate_hash_id()
    InMemoryStorage->>Graph: Add node
    InMemoryStorage->>Graph: Add edges for dependencies
    InMemoryStorage->>InMemoryStorage: Insert to HashMap
    InMemoryStorage-->>Storage Trait: Issue

    Storage Trait-->>Command: Issue

    Command->>Storage Trait: storage.save().await
    Storage Trait->>InMemoryStorage: (via trait dispatch)
    InMemoryStorage->>JSONL: Atomic write (temp file)
    loop For each issue
        InMemoryStorage->>JSONL: Write JSON + \n
    end
    InMemoryStorage->>JSONL: Rename temp → issues.jsonl
    InMemoryStorage-->>Storage Trait: Ok(())

    Storage Trait-->>Command: Ok(())
    Command-->>User: Created: rivets-a3f8
```

## Initialization Flow (rivets init)

```mermaid
flowchart TD
    Start[User: rivets init --prefix myproj] --> CheckExists{.rivets/<br/>exists?}
    CheckExists -->|Yes| Error[Error: Already initialized]
    CheckExists -->|No| CreateDir[Create .rivets/ directory]

    CreateDir --> CreateConfig[Create config.yaml<br/>backend: memory<br/>data_file: .rivets/issues.jsonl]
    CreateConfig --> CreateJSONL[Create empty issues.jsonl]
    CreateJSONL --> CreateGitignore[Create .rivets/.gitignore<br/>Ignore metadata files]

    CreateGitignore --> CheckGit{Git repo<br/>detected?}
    CheckGit -->|Yes| UpdateRootIgnore[Add .rivets to root .gitignore]
    CheckGit -->|No| Skip

    UpdateRootIgnore --> Success[✓ Initialized rivets]
    Skip --> Success

    Success --> Suggest[Suggest: git add .rivets/config.yaml<br/>git commit -m 'Initialize rivets']

    style Success fill:#90EE90
    style Error fill:#FFB6C1
```

## Create Issue Flow

```mermaid
flowchart TD
    Start[rivets create<br/>--title 'Fix bug'<br/>--priority 1] --> ParseArgs[Parse CLI args]
    ParseArgs --> GatherMissing{All required<br/>fields present?}

    GatherMissing -->|No| Interactive[Interactive prompts<br/>for missing fields]
    Interactive --> BuildIssue
    GatherMissing -->|Yes| BuildIssue[Build NewIssue struct]

    BuildIssue --> Generate[Generate hash-based ID<br/>SHA256 + Base36]
    Generate --> CheckCollision{ID collision?}
    CheckCollision -->|Yes, try nonce| Generate
    CheckCollision -->|No| AddToGraph

    AddToGraph[Add to HashMap<br/>Add node to DiGraph] --> AddDeps{Has<br/>dependencies?}

    AddDeps -->|Yes| CheckCycle{Would create<br/>cycle?}
    CheckCycle -->|Yes| CycleError[Error: Circular dependency]
    CheckCycle -->|No| AddEdges[Add edges to graph]
    AddEdges --> Save
    AddDeps -->|No| Save

    Save[Auto-save to JSONL] --> AtomicWrite[Write to temp file<br/>Rename atomically]
    AtomicWrite --> Display[Display: Created rivets-a3f8]

    style Display fill:#90EE90
    style CycleError fill:#FFB6C1
```

## List/Query Flow

```mermaid
flowchart TD
    Start[rivets list<br/>--status open<br/>--priority 0-2] --> ParseFilter[Parse filter args]
    ParseFilter --> BuildFilter[Build IssueFilter struct]

    BuildFilter --> IterateIssues[Iterate all issues<br/>in HashMap]

    IterateIssues --> ApplyFilters{Match<br/>filters?}
    ApplyFilters -->|Status filter| CheckStatus{status == open?}
    CheckStatus -->|No| Skip[Skip issue]
    CheckStatus -->|Yes| CheckPriority

    CheckPriority{priority<br/>in 0-2?} -->|No| Skip
    CheckPriority -->|Yes| Include[Include in results]

    ApplyFilters -->|All filters pass| Include
    Skip --> MoreIssues{More issues?}
    Include --> MoreIssues
    MoreIssues -->|Yes| IterateIssues
    MoreIssues -->|No| Sort

    Sort[Sort by created_at desc] --> Limit{Limit<br/>specified?}
    Limit -->|Yes| TakeN[Take first N]
    Limit -->|No| All[Return all]

    TakeN --> Display[Display results<br/>as table or JSON]
    All --> Display

    style Display fill:#90EE90
```

## Ready Work Algorithm Flow

```mermaid
flowchart TD
    Start[rivets ready<br/>--assignee alice] --> InitBlocked[blocked = empty set]

    InitBlocked --> Phase1[Phase 1: Direct Blocks]
    Phase1 --> Iterate1{For each issue}

    Iterate1 --> CheckDeps{Has dependencies?}
    CheckDeps -->|Yes| FilterBlocking{Filter type == 'blocks'?}
    FilterBlocking -->|Yes| CheckBlockerStatus{Blocker is<br/>open/in_progress?}
    CheckBlockerStatus -->|Yes| AddBlocked[blocked.insert issue]
    CheckBlockerStatus -->|No| Iterate1
    FilterBlocking -->|No| Iterate1
    CheckDeps -->|No| Iterate1

    Iterate1 -->|Done| Phase2[Phase 2: Transitive Blocking]

    Phase2 --> InitQueue[BFS queue = blocked issues]
    InitQueue --> ProcessQueue{Queue<br/>not empty?}

    ProcessQueue -->|Yes| PopIssue[Pop issue, depth]
    PopIssue --> CheckDepth{depth < 50?}
    CheckDepth -->|No| ProcessQueue
    CheckDepth -->|Yes| FindChildren[Find children via<br/>parent-child edges]

    FindChildren --> MarkChildren[blocked.insert children<br/>queue.push children, depth+1]
    MarkChildren --> ProcessQueue

    ProcessQueue -->|No| FilterResults[Filter: status ≠ closed<br/>AND id ∉ blocked]

    FilterResults --> ApplyUserFilter{Additional<br/>filters?}
    ApplyUserFilter -->|Yes| FilterAssignee{assignee == alice?}
    FilterAssignee -->|Yes| Include[Include in ready]
    FilterAssignee -->|No| Skip[Skip]
    ApplyUserFilter -->|No| Include

    Include --> SortResults[Sort by policy<br/>hybrid/priority/oldest]
    SortResults --> Display[Display ready work]

    style Display fill:#90EE90
```

## Dependency Add Flow with Cycle Detection

```mermaid
flowchart TD
    Start[rivets dep add<br/>rivets-a3f8 blocks rivets-x9k2] --> Parse[Parse IDs and type]
    Parse --> ValidateIDs{Both IDs<br/>exist?}
    ValidateIDs -->|No| ErrorNotFound[Error: Issue not found]
    ValidateIDs -->|Yes| CheckCycle

    CheckCycle[has_path_connecting<br/>graph, to=x9k2, from=a3f8] --> PathExists{Path<br/>exists?}

    PathExists -->|Yes| ErrorCycle[Error: Circular dependency<br/>Would create: x9k2 → ... → a3f8 → x9k2]
    PathExists -->|No| AddEdge[Add edge to DiGraph<br/>a3f8 --blocks--> x9k2]

    AddEdge --> UpdateIssue[Update issue.dependencies<br/>in HashMap]
    UpdateIssue --> Save[Auto-save to JSONL]
    Save --> Success[✓ Dependency added]

    style Success fill:#90EE90
    style ErrorCycle fill:#FFB6C1
    style ErrorNotFound fill:#FFB6C1
```

### Example Cycle Detection

```mermaid
graph LR
    A[rivets-a3f8] -->|blocks| B[rivets-x9k2]
    B -->|blocks| C[rivets-p4m1]
    C -.trying to add.-> A

    style C fill:#FFB6C1
```

**Detection**: When trying to add `C blocks A`, check `has_path_connecting(graph, A, C)`.
Result: **Yes** (path exists: A → B → C), so reject the edge.

## Delete with Safety Checks Flow

```mermaid
flowchart TD
    Start[rivets delete rivets-a3f8] --> GetDependents[Query: get_dependents a3f8]

    GetDependents --> HasDependents{Dependents<br/>exist?}
    HasDependents -->|Yes| ErrorDependent[Error: Cannot delete rivets-a3f8<br/>2 issues depend on it:<br/>- rivets-x9k2<br/>- rivets-p4m1]

    HasDependents -->|No| GetDependencies[Query: get_dependencies a3f8]
    GetDependencies --> RemoveDeps[Remove outgoing edges<br/>from graph]

    RemoveDeps --> RemoveNode[Remove node from graph]
    RemoveNode --> RemoveHashMap[Remove from issues HashMap]
    RemoveHashMap --> RemoveNodeMap[Remove from node_map]

    RemoveNodeMap --> Save[Auto-save to JSONL]
    Save --> Success[✓ Deleted rivets-a3f8]

    style Success fill:#90EE90
    style ErrorDependent fill:#FFB6C1
```

## JSONL Load with Error Recovery

```mermaid
flowchart TD
    Start[Load .rivets/issues.jsonl] --> OpenFile[Open file for reading]
    OpenFile --> Pass1[Pass 1: Import Issues]

    Pass1 --> ReadLine1{Read line}
    ReadLine1 -->|EOF| Pass2
    ReadLine1 -->|Line| ParseJSON{Valid JSON?}

    ParseJSON -->|Yes| DeserializeIssue[Deserialize to Issue]
    DeserializeIssue --> AddToStorage[Add to HashMap<br/>Add node to graph]
    AddToStorage --> ReadLine1

    ParseJSON -->|No| LogWarning1[warnings.push MalformedJson<br/>log::warn Skipping line N]
    LogWarning1 --> ReadLine1

    Pass2[Pass 2: Add Dependency Edges] --> IterateIssues{For each issue}
    IterateIssues --> IterateDeps{For each dependency}

    IterateDeps --> CheckTarget{Target<br/>exists?}
    CheckTarget -->|No| OrphanWarning[warnings.push OrphanedDep<br/>Skip this edge]
    OrphanWarning --> IterateDeps

    CheckTarget -->|Yes| CheckCycleLoad{Would create<br/>cycle?}
    CheckCycleLoad -->|Yes| CycleWarning[warnings.push CircularDep<br/>Skip this edge]
    CycleWarning --> IterateDeps

    CheckCycleLoad -->|No| AddEdge[Add edge to graph]
    AddEdge --> IterateDeps

    IterateDeps -->|Done| IterateIssues
    IterateIssues -->|Done| CheckWarnings{Warnings<br/>present?}

    CheckWarnings -->|Yes| DisplayWarnings[eprintln: Loaded with N warnings<br/>M issues imported]
    CheckWarnings -->|No| Success

    DisplayWarnings --> Success[Return storage + warnings]

    style Success fill:#90EE90
```

## Configuration Loading and Merging

```mermaid
flowchart TD
    Start[Config::load] --> LoadDefaults[Layer 1: Defaults<br/>backend = memory<br/>prefix = 'proj']

    LoadDefaults --> FindConfig{Walk up tree<br/>find .rivets/<br/>config.yaml?}
    FindConfig -->|Not found| CheckHome
    FindConfig -->|Found| LoadProjectYAML[Layer 2: Project config<br/>Parse YAML]

    LoadProjectYAML --> CheckHome{~/.config/<br/>rivets/config.yaml<br/>exists?}
    CheckHome -->|No| LoadEnv
    CheckHome -->|Yes| LoadHomeYAML[Layer 3: User config<br/>Parse YAML]

    LoadHomeYAML --> LoadEnv[Layer 4: Environment<br/>RIVETS_PREFIX<br/>RIVETS_JSON]

    LoadEnv --> LoadCLI[Layer 5: CLI flags<br/>--prefix<br/>--json]

    LoadCLI --> Merge[Merge all layers<br/>Higher layers override lower]

    Merge --> Validate{Valid config?}
    Validate -->|No| ConfigError[Error: Invalid configuration<br/>Show helpful message]
    Validate -->|Yes| Return[Return Config]

    style Return fill:#90EE90
    style ConfigError fill:#FFB6C1
```

### Configuration Precedence

```
CLI flags          (highest priority)
    ↓
Environment vars
    ↓
~/.config/rivets/config.yaml
    ↓
.rivets/config.yaml
    ↓
Built-in defaults  (lowest priority)
```

## Multi-Command Session (Typical Workflow)

```mermaid
sequenceDiagram
    participant User
    participant rivets

    Note over User,rivets: Session 1: Initialize
    User->>rivets: rivets init --prefix myproj
    rivets-->>User: ✓ Initialized

    Note over User,rivets: Session 2: Create issues
    User->>rivets: rivets create --title "Feature A"
    rivets-->>User: Created: myproj-a3f8

    User->>rivets: rivets create --title "Feature B"
    rivets-->>User: Created: myproj-x9k2

    User->>rivets: rivets create --title "Bug fix"
    rivets-->>User: Created: myproj-p4m1

    Note over User,rivets: Session 3: Add dependency
    User->>rivets: rivets dep add myproj-a3f8 blocks myproj-p4m1
    rivets-->>User: ✓ Dependency added

    Note over User,rivets: Session 4: Check ready work
    User->>rivets: rivets ready
    rivets-->>User: myproj-a3f8 Feature A (P2)<br/>myproj-x9k2 Feature B (P2)
    Note over User,rivets: p4m1 not shown (blocked)

    Note over User,rivets: Session 5: Update and complete
    User->>rivets: rivets update myproj-a3f8 --status in_progress
    rivets-->>User: ✓ Updated

    User->>rivets: rivets close myproj-a3f8
    rivets-->>User: ✓ Closed

    Note over User,rivets: Session 6: Check ready again
    User->>rivets: rivets ready
    rivets-->>User: myproj-p4m1 Bug fix (P2)<br/>myproj-x9k2 Feature B (P2)
    Note over User,rivets: p4m1 now ready (blocker closed)
```

## State Transitions

```mermaid
stateDiagram-v2
    [*] --> Open: create
    Open --> InProgress: update --status in_progress
    Open --> Blocked: dependency added (blocking)
    InProgress --> Blocked: dependency added (blocking)
    Blocked --> Open: blocker completed
    Blocked --> InProgress: blocker completed
    InProgress --> Closed: close
    Open --> Closed: close
    Closed --> [*]: delete

    note right of Blocked
        Issue enters blocked state when:
        - Direct 'blocks' dependency added to open/in_progress issue
        - Parent epic is blocked (transitive)
    end note

    note right of Closed
        Issue can be closed from any state
        Blocked issues can be closed if work abandoned
    end note
```

## Data Persistence Points

```mermaid
flowchart LR
    subgraph "Memory (Fast)"
        HashMap[HashMap<br/>O1 lookups]
        Graph[DiGraph<br/>Graph algorithms]
    end

    subgraph "Disk (Durable)"
        JSONL[issues.jsonl<br/>Line-delimited]
    end

    Create[Create] --> HashMap
    Update[Update] --> HashMap
    Delete[Delete] --> HashMap
    AddDep[Add Dep] --> Graph

    HashMap -->|Auto-save<br/>after mutation| JSONL
    Graph -->|Auto-save<br/>after mutation| JSONL

    JSONL -->|Load<br/>on startup| HashMap
    JSONL -->|Load<br/>on startup| Graph

    style JSONL fill:#FFE4B5
    style HashMap fill:#90EE90
    style Graph fill:#ADD8E6
```

**Auto-save triggers**:
- After `create`
- After `update`
- After `close`
- After `delete`
- After `add_dependency`
- After `remove_dependency`

**NOT triggered** by read-only operations:
- `list`
- `show`
- `ready`
- `blocked`

This ensures durability while minimizing I/O overhead.
