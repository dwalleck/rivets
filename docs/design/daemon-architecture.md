# Daemon Architecture

This document describes the architecture for Rivets' daemon-based system, enabling real-time synchronization across CLI, MCP, and GUI clients.

## System Context

```
┌─────────────────────────────────────────────────────────────────┐
│                         Clients                                 │
│  ┌─────────┐      ┌─────────────┐      ┌──────────────────┐    │
│  │   CLI   │      │ MCP Server  │      │    Tauri GUI     │    │
│  │ rivets  │      │ rivets-mcp  │      │  (future)        │    │
│  └────┬────┘      └──────┬──────┘      └────────┬─────────┘    │
│       │                  │                      │               │
│       └──────────────────┼──────────────────────┘               │
│                          │ HTTP (REST) / WebSocket              │
│                          ▼                                      │
│              ┌───────────────────────┐                          │
│              │    Unix Socket        │                          │
│              │ ~/.rivets/daemon/     │                          │
│              │   {workspace}.sock    │                          │
│              └───────────┬───────────┘                          │
└──────────────────────────┼──────────────────────────────────────┘
                           │
┌──────────────────────────┼──────────────────────────────────────┐
│                          ▼                                      │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │                   rivets daemon                           │  │
│  │                                                           │  │
│  │  ┌─────────────┐    ┌─────────────┐    ┌──────────────┐  │  │
│  │  │  REST API   │    │  WebSocket  │    │    Idle      │  │  │
│  │  │  Handlers   │    │     Hub     │    │   Tracker    │  │  │
│  │  └──────┬──────┘    └──────┬──────┘    └──────────────┘  │  │
│  │         │                  │                              │  │
│  │         └────────┬─────────┘                              │  │
│  │                  ▼                                        │  │
│  │  ┌──────────────────────────────────────────────────┐    │  │
│  │  │              Command Handler                      │    │  │
│  │  │  (validates, generates events, updates state)     │    │  │
│  │  └──────────────────────┬───────────────────────────┘    │  │
│  │                         │                                 │  │
│  │         ┌───────────────┼───────────────┐                │  │
│  │         ▼               ▼               ▼                │  │
│  │  ┌────────────┐  ┌────────────┐  ┌────────────────┐     │  │
│  │  │   Event    │  │ Projection │  │   Broadcast    │     │  │
│  │  │   Store    │  │  (state)   │  │  to WebSocket  │     │  │
│  │  └─────┬──────┘  └────────────┘  └────────────────┘     │  │
│  │        │                                                 │  │
│  └────────┼─────────────────────────────────────────────────┘  │
│           ▼                                                     │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │                    .rivets/events.jsonl                   │  │
│  │                    (append-only event log)                │  │
│  └──────────────────────────────────────────────────────────┘  │
│                                                                 │
│                         Workspace                               │
└─────────────────────────────────────────────────────────────────┘
```

## Component Overview

### New Crates

| Crate | Purpose | Key Types |
|-------|---------|-----------|
| `rivets-events` | Event definitions, store trait, projections | `DomainEvent`, `EventStore`, `WorkspaceProjection` |
| `rivets-daemon` | axum server, REST handlers, WebSocket | `DaemonConfig`, `AppState`, route handlers |
| `rivets-client` | HTTP client SDK, IssueStorage implementation | `RivetsClient`, `DaemonManager` |

### Modified Crates

| Crate | Changes |
|-------|---------|
| `rivets` | Add `daemon` subcommand, CLI uses `rivets-client` instead of direct storage |
| `rivets-mcp` | Uses `rivets-client` instead of direct storage |

### Unchanged Crates

| Crate | Notes |
|-------|-------|
| `rivets-jsonl` | Low-level JSONL I/O, reused by `rivets-events` |

## Crate Dependency Graph

```
rivets-jsonl          (no internal deps)
     │
     ▼
rivets-events         (uses rivets-jsonl for event persistence)
     │
     ├──────────────────────┐
     ▼                      ▼
rivets-daemon         rivets-client
     │                      │
     │              ┌───────┴───────┐
     │              ▼               ▼
     │          rivets          rivets-mcp
     │          (CLI)           (MCP server)
     │              │
     └──────────────┘
         (daemon subcommand links rivets-daemon)
```

## Data Flow

### Write Operation (Create Issue)

```
1. CLI: rivets create "New feature"
         │
         ▼
2. Client: POST /api/v1/issues { title: "New feature" }
         │
         ▼
3. Daemon: Validate request
         │
         ▼
4. Daemon: Generate IssueCreated event
         │
         ├─────────────────────────────────┐
         ▼                                 ▼
5. EventStore: Append to events.jsonl   Projection: Apply event
         │                                 │
         ▼                                 ▼
6. Persist to disk                      Update in-memory state
         │
         ▼
7. Broadcast: Send event to WebSocket subscribers
         │
         ▼
8. Response: Return created issue to client
```

### Read Operation (List Issues)

```
1. CLI: rivets list --status open
         │
         ▼
2. Client: GET /api/v1/issues?status=open
         │
         ▼
3. Daemon: Query projection (in-memory state)
         │
         ▼
4. Response: Return filtered issues
```

### Real-time Sync (GUI)

```
1. GUI: Connect WebSocket to /api/v1/ws/events
         │
         ▼
2. Daemon: Send Connected { current_sequence }
         │
         ▼
3. GUI: Subscribe { from_sequence: last_seen }
         │
         ▼
4. Daemon: Replay missed events, then stream live
         │
         ▼
5. [Another client creates issue via REST]
         │
         ▼
6. Daemon: Broadcast IssueCreated to all WebSocket clients
         │
         ▼
7. GUI: Receives event, updates local view
```

## Daemon Lifecycle

### Per-Workspace Model

Each workspace gets its own daemon process:

```
~/.rivets/daemon/
├── a1b2c3d4.sock      # Socket for /home/user/project-a
├── e5f6g7h8.sock      # Socket for /home/user/project-b
└── i9j0k1l2.sock      # Socket for /tmp/test-workspace
```

Socket names are derived from workspace path hash (first 8 chars of SHA-256).

### Auto-Start

When a client needs the daemon:

```rust
// Pseudocode
fn ensure_daemon() -> Result<Client> {
    let socket = socket_path_for(workspace);

    if can_connect(socket) {
        return Client::connect(socket);
    }

    // Daemon not running, start it
    spawn_daemon(workspace, socket);
    wait_for_socket(socket, timeout: 5s);
    Client::connect(socket)
}
```

### Idle Shutdown

Daemon tracks last activity and shuts down after idle timeout:

```
┌─────────────────────────────────────────┐
│            Idle Tracker                 │
│                                         │
│  last_activity: Instant                 │
│  timeout: Duration (default: 1 hour)    │
│                                         │
│  on_request() → touch last_activity     │
│  check_loop() → if idle > timeout:      │
│                    graceful_shutdown()  │
└─────────────────────────────────────────┘
```

### Daemon Subcommand

```bash
# Start daemon for current workspace (foreground, for debugging)
rivets daemon start

# Start daemon for specific workspace
rivets daemon start --workspace /path/to/project

# Check daemon status
rivets daemon status

# Stop daemon
rivets daemon stop
```

Normal CLI commands auto-start the daemon as needed - users rarely interact with daemon commands directly.

## State Management

### On Startup

```
1. Load events.jsonl (if exists)
2. Replay all events through projection
3. Projection now holds current state
4. Start accepting connections
```

### During Operation

```
┌─────────────────────────────────────────────────────────────┐
│                       AppState                              │
│                                                             │
│  ┌─────────────────┐    ┌─────────────────────────────┐    │
│  │   EventStore    │    │      WorkspaceProjection    │    │
│  │                 │    │                             │    │
│  │  append()       │    │  issues: HashMap            │    │
│  │  read_from()    │    │  graph: DiGraph             │    │
│  │  subscribe()    │    │  last_sequence: u64         │    │
│  └─────────────────┘    └─────────────────────────────┘    │
│                                                             │
│  ┌─────────────────┐    ┌─────────────────────────────┐    │
│  │  Broadcast Hub  │    │      IdleTracker            │    │
│  │                 │    │                             │    │
│  │  subscribers    │    │  last_activity              │    │
│  │  send(event)    │    │  touch()                    │    │
│  └─────────────────┘    └─────────────────────────────┘    │
└─────────────────────────────────────────────────────────────┘
```

## Configuration

### Daemon Config

```rust
pub struct DaemonConfig {
    /// Workspace root directory
    pub workspace_root: PathBuf,

    /// Unix socket path
    pub socket_path: PathBuf,

    /// Event log path (.rivets/events.jsonl)
    pub events_path: PathBuf,

    /// Shutdown after this duration of inactivity
    pub idle_timeout: Duration,

    /// Optional TCP port for debugging (disabled by default)
    pub debug_port: Option<u16>,
}
```

### Default Paths

| Path | Purpose |
|------|---------|
| `{workspace}/.rivets/events.jsonl` | Event log (source of truth) |
| `~/.rivets/daemon/{hash}.sock` | Unix socket for IPC |
| `~/.rivets/daemon/{hash}.pid` | PID file for process management |

## Error Handling

### Client Connection Errors

| Error | Handling |
|-------|----------|
| Socket not found | Auto-start daemon |
| Connection refused | Remove stale socket, auto-start daemon |
| Timeout waiting for daemon | Return error with troubleshooting info |

### Daemon Errors

| Error | Handling |
|-------|----------|
| Event store I/O error | Return 500, log error, continue running |
| Invalid request | Return 400 with validation details |
| Workspace not initialized | Return 404 with init instructions |

## Security Considerations

### Local-Only Access

- Unix sockets provide file-system permission enforcement
- Socket created with user-only permissions (0600)
- No network exposure by default

### Future: Optional TCP

For remote GUI access (if needed later):
- Bind to localhost only by default
- Consider mTLS for LAN access
- Token-based authentication

## Performance Characteristics

| Operation | Complexity | Notes |
|-----------|------------|-------|
| Startup (replay) | O(n) events | One-time cost, typically fast |
| Create/Update | O(1) | Append to log, update projection |
| Query (list) | O(n) issues | In-memory scan with filter |
| WebSocket broadcast | O(k) clients | k = connected clients |

### Memory Usage

- Projection holds all issues in memory (same as current model)
- Event log is append-only on disk, not held in memory
- Typical workspace: <10MB memory for thousands of issues
