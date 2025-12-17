# rivets-mcp

MCP (Model Context Protocol) server for the Rivets issue tracking system. Enables AI assistants like Claude to manage issues directly through a standardized protocol.

## Features

- **10 MCP tools** for complete issue management
- **Multi-workspace support** - work with multiple projects in one session
- **Stdio transport** - works with any MCP-compatible client
- **Structured tracing** - debug with `RUST_LOG=debug`

## Installation

### From source

```bash
cargo install --path crates/rivets-mcp
```

### Build locally

```bash
cargo build --release -p rivets-mcp
```

Binary will be at `target/release/rivets-mcp`.

## Usage

### With Claude Code

Add to your Claude Code MCP configuration (`~/.config/claude-code/mcp.json` or project `.claude/mcp.json`):

```json
{
  "mcpServers": {
    "rivets": {
      "command": "rivets-mcp",
      "args": []
    }
  }
}
```

Or with an absolute path:

```json
{
  "mcpServers": {
    "rivets": {
      "command": "/path/to/rivets-mcp",
      "args": []
    }
  }
}
```

### Manual testing

```bash
# Run the server (communicates via stdin/stdout)
rivets-mcp

# With debug logging (logs go to stderr)
RUST_LOG=debug rivets-mcp
```

## Available Tools

### Context Management

| Tool | Description |
|------|-------------|
| `set_context` | Set workspace root directory (call first!) |
| `where_am_i` | Show current workspace and database path |

### Query Tools

| Tool | Description |
|------|-------------|
| `ready` | Find tasks with no blockers, ready to work on |
| `list` | List issues with optional filters (status, priority, type, assignee, label) |
| `show` | Show detailed information about a specific issue |
| `blocked` | Get blocked issues and what's blocking them |

### Modification Tools

| Tool | Description |
|------|-------------|
| `create` | Create a new issue (bug, feature, task, epic, chore) |
| `update` | Update an existing issue's fields |
| `close` | Close/complete an issue |
| `dep` | Add a dependency between issues |

## Tool Parameters

### set_context

```json
{
  "workspace_root": "/path/to/your/project"
}
```

### list

```json
{
  "status": "open",           // optional: open, in_progress, blocked, closed
  "priority": 1,              // optional: 0-4
  "issue_type": "bug",        // optional: bug, feature, task, epic, chore
  "assignee": "alice",        // optional
  "label": "urgent",          // optional
  "limit": 20,                // optional, default 100
  "workspace_root": "/path"   // optional, uses current context if omitted
}
```

### create

```json
{
  "title": "Fix login bug",           // required
  "description": "Users can't...",    // optional
  "priority": 1,                      // optional, default 2
  "issue_type": "bug",                // optional, default "task"
  "assignee": "bob",                  // optional
  "labels": ["urgent", "auth"],       // optional
  "design": "## Approach\n...",       // optional
  "acceptance_criteria": "- [ ] Tests pass",   // optional
  "workspace_root": "/path"           // optional
}
```

### update

```json
{
  "issue_id": "rivets-abc",           // required
  "title": "New title",               // optional
  "status": "in_progress",            // optional
  "priority": 0,                      // optional
  "assignee": "",                     // optional, empty string clears
  "workspace_root": "/path"           // optional
}
```

### dep

```json
{
  "issue_id": "rivets-abc",           // required: the blocked issue
  "depends_on_id": "rivets-xyz",      // required: the blocker
  "dep_type": "blocks",               // optional: blocks, related, parent-child, discovered-from
  "workspace_root": "/path"           // optional
}
```

## Workspace Parameter

Most tools accept an optional `workspace_root` parameter. This enables:

- **Multi-workspace workflows**: Switch between projects without calling `set_context`
- **Explicit targeting**: Specify exactly which project to query/modify
- **Fallback behavior**: If omitted, uses the current context set via `set_context`

## Debugging

Enable debug logging:

```bash
RUST_LOG=debug rivets-mcp
```

Log levels:
- `error` - Only errors
- `warn` - Warnings and errors
- `info` - Server start/stop (default)
- `debug` - Tool calls and operations
- `trace` - Detailed internal operations

Logs are written to stderr (stdout is reserved for MCP protocol).

## Example Workflow

1. **Set context** to your project:
   ```
   set_context(workspace_root: "/home/user/myproject")
   ```

2. **Find ready work**:
   ```
   ready(limit: 5, priority: 1)
   ```

3. **Claim a task**:
   ```
   update(issue_id: "rivets-abc", status: "in_progress", assignee: "me")
   ```

4. **Complete the task**:
   ```
   close(issue_id: "rivets-abc", reason: "Implemented in PR #42")
   ```

## Requirements

- A `.rivets/` directory in your project (created by `rivets init`)
- Issues stored in `.rivets/issues.jsonl`

## License

Apache-2.0 (same as rivets)
