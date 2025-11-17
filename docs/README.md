# Rivets Documentation

## Overview

Rivets is a high-performance, Rust-based issue tracking system using in-memory storage with JSONL persistence for the MVP, designed to scale to PostgreSQL for production multi-user scenarios.

## Architecture Documents

### 🏗️ [Architecture Overview](./architecture.md)
**Start here for system-level understanding**

Complete system architecture including:
- Component diagram with all layers (CLI, App, Storage, Domain)
- Technology stack and dependencies
- Phase roadmap (MVP → Configuration → Production)
- Performance targets and benchmarks
- Error handling strategy
- Thread safety model

**Key Highlights**:
- Async-first architecture with tokio current-thread runtime
- Storage abstraction via async-trait
- In-memory + petgraph for MVP, PostgreSQL for Phase 3
- Auto-save after every mutating command

### 💾 [Storage Architecture](./storage-architecture.md)
**Deep dive into storage layer**

Detailed storage design including:
- Storage trait hierarchy and method signatures
- InMemoryStorage internal structure (HashMap + DiGraph + node_map)
- JSONL persistence with error recovery
- Cycle detection algorithm
- Ready work algorithm with BFS blocking propagation
- Delete operation with referential integrity
- Performance characteristics and memory layout

**Key Highlights**:
- Arc<Mutex<InMemoryStorageInner>> for thread safety
- Two-pass JSONL loading (issues → dependencies)
- Graceful error recovery (skip orphans, cycles, malformed JSON)
- O(V+E) cycle detection via petgraph

### 📦 [Module Structure](./module-structure.md)
**Code organization and dependencies**

Module-by-module breakdown:
- Workspace organization (rivets-jsonl library + rivets CLI)
- Dependency graph between modules
- Public API surfaces
- File structure and naming conventions
- Testing structure and examples

**Key Highlights**:
- Standalone rivets-jsonl library (reusable)
- Domain layer at core (no external dependencies)
- Commands use storage trait (not concrete types)
- No circular dependencies (DAG structure)

### 🔄 [Data Flow](./data-flow.md)
**How data moves through the system**

End-to-end flows including:
- Complete command lifecycle (user → CLI → storage → JSONL)
- Initialization flow (rivets init)
- Create, list, query, delete flows
- Dependency add with cycle detection
- Ready work algorithm execution
- JSONL load with error recovery
- Configuration loading and merging
- State transitions diagram

**Key Highlights**:
- Async all the way down (tokio runtime)
- Auto-save triggers after mutations
- Atomic JSONL writes (temp file + rename)
- Multi-layer config precedence (CLI → env → YAML → defaults)

### 🗺️ [Task Dependency Graph](./task-dependency-graph.md)
**Implementation roadmap and task order**

Implementation planning:
- Critical path for MVP completion
- Task dependency visualization
- 4-week iteration plan
- Parallel work opportunities
- Risk mitigation strategies
- Success metrics and checkpoints

**Key Highlights**:
- 14 P1 tasks for MVP completion
- 4 tasks clarified with architectural decisions
- Week-by-week breakdown
- Parallel tracks for efficient development

### 📖 [Terminology Reference](./terminology.md)
**Consistent language across project**

Standard terminology for:
- Storage layers (in-memory, JSONL, PostgreSQL)
- Data structures (Issue, Dependency, IssueId, IssueFilter)
- Operations (CRUD, ready work, blocked issues, cycle detection)
- Dependency types (blocks, related, parent-child, discovered-from)
- Development phases

## Quick Start for Developers

### Understanding the System

1. **New to the project?** Start with [Architecture Overview](./architecture.md)
2. **Working on storage?** Read [Storage Architecture](./storage-architecture.md)
3. **Adding a command?** Check [Module Structure](./module-structure.md) → commands/
4. **Debugging a flow?** Reference [Data Flow](./data-flow.md)
5. **Planning work?** See [Task Dependency Graph](./task-dependency-graph.md)

### Key Architectural Decisions (Already Made)

#### ✅ Async Architecture
- **Decision**: Use async-trait for storage, tokio current-thread runtime
- **Rationale**: Future-proof for PostgreSQL, simpler for CLI
- **Tasks**: rivets-0gc, rivets-bz5, rivets-l66, rivets-cgl

#### ✅ Thread Safety
- **Decision**: Arc<Mutex<InMemoryStorageInner>>
- **Rationale**: Simple, correct, sufficient for CLI use case
- **Tasks**: rivets-0gc, rivets-bz5

#### ✅ Persistence Strategy
- **Decision**: Auto-save after every mutating command
- **Rationale**: Resilient to crashes, simple to reason about
- **Tasks**: rivets-cgl, rivets-l66

#### ✅ Error Recovery
- **Decision**: Graceful degradation for JSONL corruption
- **Rationale**: Partial recovery better than total failure
- **Tasks**: rivets-l66, rivets-0gc
- **Behavior**:
  - Skip malformed JSON lines (log warning)
  - Skip orphaned dependencies (log warning)
  - Detect and skip circular dependencies (log warning)
  - Return (Storage, Vec<Warning>) for user awareness

#### ✅ Referential Integrity
- **Decision**: Safe deletion with dependent check
- **Rationale**: Prevent orphaned references, clear errors
- **Tasks**: rivets-0gc
- **Behavior**:
  - Check for dependents before delete
  - Fail with list of dependent issues if found
  - Auto-remove outgoing dependencies on successful delete

## Implementation Status

### ✅ Completed (1)
- **rivets-p7v**: CLI skeleton with basic command structure

### ✓ Clarified (4)
Architecture defined, ready for implementation:
- **rivets-0gc**: Storage trait abstraction (5 clarifications)
- **rivets-bz5**: InMemoryStorage implementation (2 clarifications)
- **rivets-l66**: JSONL persistence (3 clarifications)
- **rivets-cgl**: CLI integration with storage (3 clarifications)

### ⏳ Ready to Implement (10)
Clear specifications, no blockers:
- **rivets-fk9**: JSONL library research
- **rivets-zp3**: JSONL library skeleton
- **rivets-06w**: Core domain types
- **rivets-x1e**: Hash-based ID generation
- **rivets-6op**: Dependency system
- **rivets-qeb**: Ready work algorithm
- **rivets-ceg**: CLI argument parsing
- **rivets-bsp**: Core CLI commands
- **rivets-4l2**: Init command
- **rivets-yis**: Storage backend selection

## Critical Paths

### Shortest Path to Working MVP
```
rivets-fk9 → rivets-zp3 → rivets-06w → rivets-x1e
    ↓
rivets-0gc → rivets-bz5 → rivets-l66
    ↓
rivets-cgl → rivets-ceg → rivets-bsp
```

**Estimated**: 2-3 weeks for end-to-end create/list/show

### Complete MVP (All P1 Tasks)
```
Foundation (Week 1)
    rivets-fk9, rivets-zp3, rivets-06w, rivets-x1e

Storage Layer (Week 2)
    rivets-0gc, rivets-bz5, rivets-6op, rivets-qeb, rivets-l66

CLI Integration (Week 3)
    rivets-ceg, rivets-cgl, rivets-bsp, rivets-4l2

Configuration (Week 4)
    rivets-yis
```

**Estimated**: 4 weeks for full MVP with all features

## Key Design Patterns

### Storage Trait Pattern
```rust
#[async_trait]
pub trait IssueStorage: Send + Sync {
    async fn create(&mut self, issue: NewIssue) -> Result<Issue>;
    async fn save(&self) -> Result<()>;
    // ... other methods
}

// Usage (commands never know concrete type)
pub async fn execute(args: &Args, app: &mut App) -> Result<()> {
    let issue = app.storage().create(new_issue).await?;
    app.storage().save().await?;
    Ok(())
}
```

### Builder Pattern (Filters)
```rust
let filter = IssueFilter::builder()
    .status(vec![Status::Open, Status::InProgress])
    .priority_range(0, 2)?
    .labels_all(vec!["bug", "urgent"])
    .limit(10)
    .build();
```

### Error Recovery Pattern (JSONL)
```rust
pub async fn load_from_jsonl(path: &Path)
    -> Result<(Self, Vec<LoadWarning>)>
{
    // Two-pass: issues first, then dependencies
    // Skip invalid lines, collect warnings
    // Return both storage and warnings
}
```

### Atomic Write Pattern (Persistence)
```rust
let temp = path.with_extension("tmp");
write_to(&temp).await?;
tokio::fs::rename(&temp, path).await?;  // Atomic on POSIX
```

## Performance Targets

| Operation | Target | Implementation |
|-----------|--------|----------------|
| Create issue | <1ms | HashMap insert + graph node |
| Cycle detection | <10ms (1000 issues) | petgraph path finding |
| Ready work | <10ms (1000 issues) | BFS traversal |
| JSONL save | <100ms (1000 issues) | Async streaming write |
| JSONL load | <200ms (1000 issues) | Async streaming read |

## Testing Strategy

### Unit Tests
- All domain types with serde round-trip
- ID generation with collision handling
- Cycle detection with complex graphs
- Filter builder with all combinations
- JSONL corruption recovery

### Integration Tests
- End-to-end command workflows
- Multi-issue dependency scenarios
- JSONL save/load round-trips
- Configuration merging
- Error handling paths

### Benchmarks (criterion)
- Storage operations at scale (100, 1000, 10000 issues)
- Graph algorithms (cycle detection, ready work)
- JSONL I/O throughput
- Memory usage tracking

## Common Questions

### Q: Why async for a CLI application?
**A**: Future-proofs for PostgreSQL (requires async I/O), enables non-blocking file operations for large JSONL files, and allows concurrent operations if needed later. The current-thread runtime keeps complexity low for MVP.

### Q: Why Arc<Mutex<>> instead of message passing?
**A**: Simpler mental model for CLI use case, easier to debug, sufficient performance (no contention in single-threaded runtime), and standard Rust pattern for shared mutable state.

### Q: Why auto-save after every command?
**A**: Maximizes durability (resilient to crashes), simple to reason about (no complex flush logic), acceptable I/O overhead for CLI, and clear user expectations (command completes = persisted).

### Q: Why petgraph instead of custom graph?
**A**: Battle-tested algorithms (cycle detection, traversal), well-documented API, good performance characteristics, and maintained by community. Trade-off: extra dependency vs. correctness guarantee.

### Q: Why two-pass JSONL loading?
**A**: Prevents orphaned dependencies (all issues must exist before adding edges), enables cycle detection (full graph needed), and allows graceful recovery (skip invalid edges, continue loading).

## Next Steps

1. **For implementers**: Pick a task from "Ready to Implement", read relevant architecture docs, follow TDD approach
2. **For reviewers**: Check code against architecture decisions in this documentation
3. **For planners**: Update task-dependency-graph.md with actuals vs. estimates

## Documentation Maintenance

When making architectural changes:
- [ ] Update affected diagrams in architecture.md
- [ ] Update module structure if modules added/renamed
- [ ] Update data flow if new flows introduced
- [ ] Add decision to this README
- [ ] Update terminology.md if new terms introduced
- [ ] Keep task-dependency-graph.md in sync with beads

## Related Resources

- [Rust Book](https://doc.rust-lang.org/book/) - Rust fundamentals
- [async-trait docs](https://docs.rs/async-trait/) - Async trait patterns
- [petgraph docs](https://docs.rs/petgraph/) - Graph algorithms
- [tokio docs](https://docs.rs/tokio/) - Async runtime
- [clap docs](https://docs.rs/clap/) - CLI parsing
