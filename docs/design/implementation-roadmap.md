# Implementation Roadmap

This document outlines the phased implementation plan for the event-sourced daemon architecture.

## Crate Structure

### New Crates

```
crates/
├── rivets-events/         # Event types, EventStore trait, projections
│   ├── src/
│   │   ├── lib.rs         # Public API exports
│   │   ├── events.rs      # DomainEvent enum, EventEnvelope
│   │   ├── store.rs       # EventStore trait
│   │   ├── jsonl.rs       # JsonlEventStore implementation
│   │   └── projection.rs  # WorkspaceProjection
│   └── Cargo.toml
│
├── rivets-daemon/         # axum HTTP/WebSocket server
│   ├── src/
│   │   ├── lib.rs         # Library for embedding/testing
│   │   ├── main.rs        # Standalone binary (optional)
│   │   ├── server.rs      # Server setup, state management
│   │   ├── api/
│   │   │   ├── mod.rs     # Router construction
│   │   │   ├── issues.rs  # Issue CRUD handlers
│   │   │   ├── queries.rs # Ready, blocked, stale handlers
│   │   │   ├── deps.rs    # Dependency handlers
│   │   │   ├── labels.rs  # Label handlers
│   │   │   └── mgmt.rs    # Health, shutdown handlers
│   │   ├── ws.rs          # WebSocket event streaming
│   │   ├── error.rs       # API error types
│   │   └── types.rs       # Request/response types
│   └── Cargo.toml
│
└── rivets-client/         # HTTP client SDK
    ├── src/
    │   ├── lib.rs         # Public API
    │   ├── client.rs      # RivetsClient (HTTP operations)
    │   ├── manager.rs     # DaemonManager (auto-start/stop)
    │   └── storage.rs     # IssueStorage trait implementation
    └── Cargo.toml
```

### Modified Crates

```
crates/
├── rivets/                # CLI + daemon subcommand
│   ├── src/
│   │   ├── cli/
│   │   │   ├── mod.rs     # Add daemon subcommand
│   │   │   ├── daemon.rs  # NEW: daemon start/stop/status
│   │   │   └── ...
│   │   └── ...
│   └── Cargo.toml         # Add rivets-client, rivets-daemon deps
│
└── rivets-mcp/            # MCP server
    └── src/
        └── context.rs     # Use RivetsClient instead of direct storage
```

## Implementation Phases

### Phase 1: rivets-events Crate

**Goal**: Define events and event store, independent of daemon.

**Deliverables**:
- `DomainEvent` enum with all event types
- `EventEnvelope` with metadata
- `EventStore` trait
- `JsonlEventStore` implementation
- `WorkspaceProjection` for state reconstruction

**Dependencies**: rivets-jsonl, existing domain types

**Testing**:
- Unit tests for each event application
- Property tests: `rebuild(events) == incremental_apply(events)`
- Integration tests with temp files

**Estimated effort**: 2-3 days

---

### Phase 2: rivets-daemon Crate (REST Only)

**Goal**: axum server with REST API, no WebSocket yet.

**Deliverables**:
- `DaemonConfig` and server setup
- All REST endpoints from API spec
- `AppState` with EventStore + Projection
- Request validation and error handling
- Health endpoint

**Dependencies**: Phase 1, axum, tower, tokio

**Testing**:
- Integration tests with test server
- Each endpoint tested for success and error cases
- Validation edge cases

**Estimated effort**: 3-4 days

---

### Phase 3: rivets-client Crate

**Goal**: HTTP client SDK that implements IssueStorage trait.

**Deliverables**:
- `RivetsClient` for HTTP operations
- `DaemonManager` for auto-start/stop
- `RivetsStorageClient` implementing `IssueStorage` trait
- Unix socket support (hyper with unix transport)

**Dependencies**: Phase 2, reqwest/hyper, tokio

**Testing**:
- Unit tests with mock server
- Integration tests with real daemon

**Estimated effort**: 2 days

---

### Phase 4: CLI Integration

**Goal**: CLI uses client instead of direct storage.

**Deliverables**:
- Add `daemon` subcommand to CLI
  - `rivets daemon start`
  - `rivets daemon stop`
  - `rivets daemon status`
- Modify CLI to use `RivetsStorageClient`
- Auto-start daemon on first command
- Graceful fallback messaging

**Changes to existing code**:
- `crates/rivets/src/cli/mod.rs` - Add daemon commands
- `crates/rivets/src/app.rs` - Use client storage

**Testing**:
- All existing CLI tests should pass
- New tests for daemon subcommand
- Integration test: CLI → daemon → storage

**Estimated effort**: 2 days

---

### Phase 5: MCP Integration

**Goal**: MCP server uses client instead of direct storage.

**Deliverables**:
- Update `Context` to use `RivetsClient`
- Handle daemon auto-start
- Ensure all MCP tools work as before

**Changes to existing code**:
- `crates/rivets-mcp/src/context.rs`
- `crates/rivets-mcp/src/tools.rs`

**Testing**:
- All existing MCP tests should pass
- Integration test with daemon

**Estimated effort**: 1-2 days

---

### Phase 6: WebSocket Streaming

**Goal**: Real-time event streaming for GUI.

**Deliverables**:
- WebSocket handler in daemon
- Event broadcast on append
- Subscription with replay support
- Filter and watch capabilities

**Dependencies**: Phase 2, tokio-tungstenite

**Testing**:
- Connection lifecycle tests
- Replay accuracy tests
- Multiple subscriber tests

**Estimated effort**: 2 days

---

### Phase 7: Client WebSocket Support

**Goal**: Client can subscribe to events.

**Deliverables**:
- `EventSubscription` type in client
- Reconnection handling
- Async stream interface

**Testing**:
- Subscription tests
- Reconnection tests

**Estimated effort**: 1 day

---

### Phase 8: Tauri GUI Foundation

**Goal**: Basic Tauri app structure.

**Deliverables**:
- Tauri project setup
- Use `rivets-client` from Rust backend
- Basic issue list view
- Real-time updates via WebSocket

**This phase is a separate project** and would have its own detailed roadmap.

## Dependency Graph

```
Phase 1: rivets-events
    │
    ▼
Phase 2: rivets-daemon (REST)
    │
    ├───────────────┐
    ▼               ▼
Phase 3: client   Phase 6: WebSocket
    │               │
    ├───────┬───────┤
    ▼       ▼       ▼
Phase 4  Phase 5  Phase 7
 (CLI)   (MCP)   (client WS)
    │       │       │
    └───────┴───────┘
            │
            ▼
      Phase 8: Tauri GUI
```

## Critical Files to Modify

### rivets crate

| File | Changes |
|------|---------|
| `src/cli/mod.rs` | Add `daemon` subcommand, switch to client storage |
| `src/cli/args.rs` | Add `DaemonArgs` struct |
| `src/app.rs` | Support both direct and client storage modes |
| `Cargo.toml` | Add rivets-client, rivets-daemon dependencies |

### rivets-mcp crate

| File | Changes |
|------|---------|
| `src/context.rs` | Use `RivetsClient` instead of direct storage |
| `src/tools.rs` | No changes expected (uses context abstraction) |
| `Cargo.toml` | Add rivets-client dependency |

### Domain types (reused, not modified)

| File | Notes |
|------|-------|
| `rivets/src/domain/mod.rs` | Reused by rivets-events |
| `rivets/src/storage/mod.rs` | `IssueStorage` trait implemented by client |

## Testing Strategy

### Unit Tests

Each crate has comprehensive unit tests:

```rust
// rivets-events/src/projection.rs
#[cfg(test)]
mod tests {
    #[test]
    fn apply_issue_created() { ... }

    #[test]
    fn apply_status_changed() { ... }

    #[test]
    fn rebuild_matches_incremental() { ... }
}
```

### Integration Tests

Test the full stack:

```rust
// rivets-daemon/tests/api_tests.rs
#[tokio::test]
async fn test_create_and_list_issues() {
    let daemon = TestDaemon::start().await;

    let created = daemon.client
        .post("/api/v1/issues")
        .json(&json!({"title": "Test"}))
        .send().await.unwrap();

    assert_eq!(created.status(), 201);

    let list = daemon.client
        .get("/api/v1/issues")
        .send().await.unwrap()
        .json::<ListResponse>().await.unwrap();

    assert_eq!(list.issues.len(), 1);
}
```

### Backwards Compatibility Tests

Ensure existing functionality works:

```rust
// rivets/tests/cli_integration.rs
#[tokio::test]
async fn test_cli_create_with_daemon() {
    let workspace = TestWorkspace::new();

    // Start daemon
    Command::new("rivets")
        .args(["daemon", "start"])
        .current_dir(&workspace)
        .spawn().unwrap();

    // Use CLI as before
    let output = Command::new("rivets")
        .args(["create", "Test issue"])
        .current_dir(&workspace)
        .output().unwrap();

    assert!(output.status.success());
}
```

### Property-Based Tests

For event sourcing correctness:

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn projection_deterministic(events in arb_events(1..100)) {
        let proj1 = WorkspaceProjection::rebuild(events.clone().into_iter());
        let proj2 = WorkspaceProjection::rebuild(events.into_iter());

        prop_assert_eq!(proj1.issues, proj2.issues);
    }
}
```

## Rollout Considerations

### Feature Flags

During development, use feature flags to enable daemon mode:

```rust
// rivets/src/app.rs
pub async fn create_storage(config: &Config) -> Result<Box<dyn IssueStorage>> {
    #[cfg(feature = "daemon")]
    if config.use_daemon {
        return Ok(Box::new(RivetsStorageClient::connect().await?));
    }

    // Fallback to direct storage
    Ok(Box::new(InMemoryStorage::load(path).await?))
}
```

### Backwards Compatibility

1. **Direct mode**: Keep working without daemon for simple use cases
2. **Gradual adoption**: Users opt into daemon mode initially
3. **Default flip**: Once stable, daemon becomes default

### Deprecation Path

Once daemon mode is stable:

1. Direct storage becomes "offline mode" for special cases
2. Document when to use each mode
3. Eventually, daemon-first for all normal operation

## Risk Mitigation

| Risk | Mitigation |
|------|------------|
| Daemon fails to start | Clear error messages, fallback to direct mode |
| Socket permission issues | Document requirements, auto-create with correct perms |
| Event store corruption | Validation on load, warn but continue with partial data |
| Performance regression | Benchmark before/after, optimize hot paths |

## Success Criteria

### Phase 1-3 Complete

- [ ] Events can be appended and replayed
- [ ] Daemon starts and serves REST API
- [ ] Client can perform all operations via daemon

### Phase 4-5 Complete

- [ ] All CLI commands work via daemon
- [ ] All MCP tools work via daemon
- [ ] Existing tests pass

### Phase 6-7 Complete

- [ ] WebSocket connects and receives events
- [ ] Client can subscribe and receive updates
- [ ] Multiple clients see consistent state

### Ready for GUI

- [ ] Real-time sync verified with multiple clients
- [ ] API stable (no breaking changes expected)
- [ ] Performance acceptable for interactive use
