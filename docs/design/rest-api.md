# REST API Design

This document specifies the HTTP API exposed by the Rivets daemon.

## Overview

The daemon exposes a REST API over Unix socket for local clients (CLI, MCP, GUI). The API follows REST conventions with JSON payloads.

### Base Configuration

| Property | Value |
|----------|-------|
| Transport | Unix socket (`~/.rivets/daemon/{hash}.sock`) |
| Protocol | HTTP/1.1 |
| Content-Type | `application/json` |
| API Version | `/api/v1/` |

### Optional TCP (Debug Mode)

For debugging, the daemon can optionally bind to a TCP port:

```bash
rivets daemon start --debug-port 7878
```

This enables tools like `curl` and browser dev tools:

```bash
curl http://localhost:7878/api/v1/issues
```

## Endpoints

### Issues

#### List Issues

```http
GET /api/v1/issues
```

Query Parameters:

| Parameter | Type | Description |
|-----------|------|-------------|
| `status` | string | Filter by Workflow State: `open`, `in_progress`, `closed` |
| `priority` | integer | Filter by priority (1-5) |
| `issue_type` | string | Filter by type: `bug`, `feature`, `task`, `epic`, `chore` |
| `assignee` | string | Filter by assignee |
| `label` | string | Filter by label (can repeat for multiple) |
| `limit` | integer | Max results (default: 100) |
| `offset` | integer | Pagination offset |

Response:

```json
{
  "issues": [
    {
      "id": "rivets-001",
      "title": "Add daemon support",
      "description": "Implement event-sourced daemon...",
      "status": "in_progress",
      "priority": 1,
      "issue_type": "feature",
      "assignee": "alice",
      "labels": ["architecture", "v1"],
      "design": null,
      "acceptance_criteria": null,
      "notes": null,
      "external_ref": null,
      "created_at": "2024-01-15T10:30:00Z",
      "updated_at": "2024-01-15T14:00:00Z",
      "closed_at": null,
      "_links": {
        "self": "/api/v1/issues/rivets-001",
        "dependencies": "/api/v1/issues/rivets-001/dependencies"
      }
    }
  ],
  "total": 42,
  "limit": 100,
  "offset": 0
}
```

#### Get Issue

```http
GET /api/v1/issues/{id}
```

Response:

```json
{
  "id": "rivets-001",
  "title": "Add daemon support",
  "description": "...",
  "status": "in_progress",
  "priority": 1,
  "issue_type": "feature",
  "assignee": "alice",
  "labels": ["architecture"],
  "design": "Use event sourcing with JSONL...",
  "acceptance_criteria": "- Daemon starts automatically\n- CLI works as before",
  "notes": null,
  "external_ref": "https://github.com/org/repo/issues/42",
  "dependencies": [
    {
      "id": "rivets-002",
      "title": "Design event schema",
      "status": "closed",
      "dep_type": "blocks"
    }
  ],
  "dependents": [
    {
      "id": "rivets-003",
      "title": "Add GUI support",
      "status": "open",
      "dep_type": "blocks"
    }
  ],
  "created_at": "2024-01-15T10:30:00Z",
  "updated_at": "2024-01-15T14:00:00Z",
  "closed_at": null,
  "_links": {
    "self": "/api/v1/issues/rivets-001",
    "dependencies": "/api/v1/issues/rivets-001/dependencies"
  }
}
```

#### Create Issue

```http
POST /api/v1/issues
```

Request:

```json
{
  "title": "New feature request",
  "description": "Detailed description here",
  "priority": 2,
  "issue_type": "feature",
  "assignee": "bob",
  "labels": ["enhancement"],
  "design": null,
  "acceptance_criteria": "- Criteria 1\n- Criteria 2",
  "dependencies": ["rivets-001"]
}
```

| Field | Required | Default | Description |
|-------|----------|---------|-------------|
| `title` | Yes | - | Issue title (1-500 chars) |
| `description` | No | `""` | Detailed description |
| `priority` | No | `2` | Priority 1-5 (1 = highest) |
| `issue_type` | No | `"task"` | Type of issue |
| `assignee` | No | `null` | Assigned user |
| `labels` | No | `[]` | List of labels |
| `design` | No | `null` | Design notes |
| `acceptance_criteria` | No | `null` | Acceptance criteria |
| `dependencies` | No | `[]` | IDs of issues this depends on |

Response: `201 Created`

```json
{
  "id": "rivets-004",
  "title": "New feature request",
  ...
}
```

#### Update Issue

```http
PATCH /api/v1/issues/{id}
```

Request (all fields optional):

```json
{
  "title": "Updated title",
  "priority": 1,
  "assignee": null,
  "labels": ["urgent", "bug"]
}
```

Special handling:
- `assignee: null` clears the assignee
- `assignee` absent: no change to assignee
- `labels`: replaces entire label list

Response: `200 OK` with updated issue

#### Delete Issue

```http
DELETE /api/v1/issues/{id}
```

Response: `204 No Content`

### Issue Actions

#### Close Issue

```http
POST /api/v1/issues/{id}/close
```

Request:

```json
{
  "reason": "Completed as designed"
}
```

Response: `200 OK` with updated issue (status = closed)

#### Reopen Issue

```http
POST /api/v1/issues/{id}/reopen
```

Request:

```json
{
  "reason": "Need additional changes"
}
```

Response: `200 OK` with updated issue (status = open)

### Dependencies

#### List Dependencies

```http
GET /api/v1/issues/{id}/dependencies
```

Response:

```json
{
  "dependencies": [
    {
      "id": "rivets-002",
      "title": "Design event schema",
      "status": "closed",
      "dep_type": "blocks"
    }
  ],
  "dependents": [
    {
      "id": "rivets-003",
      "title": "Add GUI support",
      "status": "open",
      "dep_type": "blocks"
    }
  ]
}
```

#### Add Dependency

```http
POST /api/v1/issues/{id}/dependencies
```

Request:

```json
{
  "target_id": "rivets-002",
  "dep_type": "blocks"
}
```

| dep_type | Description |
|----------|-------------|
| `blocks` | Hard blocker - prevents work |
| `related` | Soft link - informational |
| `parent_child` | Hierarchical relationship |
| `discovered_from` | Found during work on another issue |

Response: `201 Created`

#### Remove Dependency

```http
DELETE /api/v1/issues/{id}/dependencies/{target_id}
```

Response: `204 No Content`

### Labels

#### List Issue Labels

```http
GET /api/v1/issues/{id}/labels
```

Response:

```json
{
  "labels": ["bug", "urgent", "v1"]
}
```

#### Add Label

```http
POST /api/v1/issues/{id}/labels/{label}
```

Response: `200 OK` with updated issue

#### Remove Label

```http
DELETE /api/v1/issues/{id}/labels/{label}
```

Response: `200 OK` with updated issue

#### List All Labels

```http
GET /api/v1/labels
```

Response:

```json
{
  "labels": [
    { "name": "bug", "count": 5 },
    { "name": "feature", "count": 12 },
    { "name": "urgent", "count": 2 }
  ]
}
```

### Queries

#### Ready to Work

```http
GET /api/v1/ready
```

Returns issues that are open with no blocking dependencies.

Query Parameters: Same as list issues (status filter ignored)

Response: Same format as list issues

#### Blocked Issues

```http
GET /api/v1/blocked
```

Response:

```json
{
  "blocked": [
    {
      "issue": {
        "id": "rivets-003",
        "title": "Add GUI support",
        ...
      },
      "blocked_by": [
        {
          "id": "rivets-001",
          "title": "Add daemon support",
          "status": "in_progress"
        }
      ]
    }
  ]
}
```

#### Stale Issues

```http
GET /api/v1/stale
```

Query Parameters:

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `days` | integer | 14 | Issues not updated in this many days |

Response: Same format as list issues

### Statistics

```http
GET /api/v1/stats
```

Response:

```json
{
  "total": 42,
  "by_status": {
    "open": 18,
    "in_progress": 8,
    "closed": 16
  },
  "by_priority": {
    "1": 5,
    "2": 20,
    "3": 12,
    "4": 5
  },
  "by_type": {
    "bug": 10,
    "feature": 20,
    "task": 8,
    "epic": 2,
    "chore": 2
  },
  "ready_count": 12,
  "blocked_by_dependencies": 3
}
```

### Daemon Management

#### Health Check

```http
GET /api/v1/health
```

Response:

```json
{
  "status": "ok",
  "version": "0.1.0",
  "workspace": "/home/user/project",
  "uptime_seconds": 3600,
  "event_count": 156,
  "issue_count": 42
}
```

#### Shutdown

```http
POST /api/v1/shutdown
```

Response: `202 Accepted`

Daemon initiates graceful shutdown.

## WebSocket API

### Connection

```http
GET /api/v1/ws/events
Upgrade: websocket
```

### Protocol

Messages are JSON objects with a `type` field.

#### Client → Server Messages

**Subscribe**

Subscribe to events, optionally replaying from a sequence number:

```json
{
  "type": "subscribe",
  "from_sequence": 100
}
```

If `from_sequence` is provided, server replays all events from that sequence before streaming live events. If omitted, only live events are streamed.

**Filter**

Filter events by type:

```json
{
  "type": "filter",
  "event_types": ["issue_created", "status_changed"]
}
```

**Watch**

Watch specific issues:

```json
{
  "type": "watch",
  "issue_ids": ["rivets-001", "rivets-002"]
}
```

**Ping**

```json
{
  "type": "ping"
}
```

#### Server → Client Messages

**Connected**

Sent immediately after connection:

```json
{
  "type": "connected",
  "daemon_version": "0.1.0",
  "workspace": "/home/user/project",
  "current_sequence": 156
}
```

**Event**

Event notification:

```json
{
  "type": "event",
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "sequence": 157,
  "timestamp": "2024-01-15T15:30:00Z",
  "event": {
    "type": "status_changed",
    "id": "rivets-001",
    "old_status": "in_progress",
    "new_status": "closed",
    "closed_at": "2024-01-15T15:30:00Z"
  }
}
```

**Subscribed**

Confirms subscription:

```json
{
  "type": "subscribed",
  "from_sequence": 100,
  "replaying": true
}
```

**Replay Complete**

Sent after replay finishes:

```json
{
  "type": "replay_complete",
  "events_replayed": 56
}
```

**Pong**

```json
{
  "type": "pong"
}
```

**Error**

```json
{
  "type": "error",
  "code": "invalid_sequence",
  "message": "Sequence 999 does not exist"
}
```

### Typical Flow

```
Client                              Server
   │                                   │
   │──── Connect ─────────────────────▶│
   │                                   │
   │◀──── Connected {seq: 156} ────────│
   │                                   │
   │──── Subscribe {from: 100} ───────▶│
   │                                   │
   │◀──── Subscribed {replaying} ──────│
   │◀──── Event {seq: 100} ────────────│
   │◀──── Event {seq: 101} ────────────│
   │      ... (replay) ...             │
   │◀──── Event {seq: 156} ────────────│
   │◀──── Replay Complete ─────────────│
   │                                   │
   │      ... (live events) ...        │
   │◀──── Event {seq: 157} ────────────│
   │◀──── Event {seq: 158} ────────────│
   │                                   │
```

## Error Responses

All errors follow a consistent format:

```json
{
  "error": {
    "code": "not_found",
    "message": "Issue 'rivets-999' not found",
    "details": null
  }
}
```

### Error Codes

| Code | HTTP Status | Description |
|------|-------------|-------------|
| `bad_request` | 400 | Malformed request body |
| `validation_error` | 400 | Request validation failed |
| `not_found` | 404 | Resource not found |
| `conflict` | 409 | Operation would create conflict (e.g., duplicate, cycle) |
| `internal_error` | 500 | Server error |

### Validation Errors

```json
{
  "error": {
    "code": "validation_error",
    "message": "Validation failed",
    "details": {
      "fields": {
        "title": "Title must be between 1 and 500 characters",
        "priority": "Priority must be between 1 and 5"
      }
    }
  }
}
```

### Conflict Errors

Dependency cycle:

```json
{
  "error": {
    "code": "conflict",
    "message": "Adding dependency would create a cycle",
    "details": {
      "cycle": ["rivets-001", "rivets-002", "rivets-003", "rivets-001"]
    }
  }
}
```

## Request/Response Types (Rust)

```rust
// Request types
#[derive(Debug, Deserialize)]
pub struct CreateIssueRequest {
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_priority")]
    pub priority: u8,
    #[serde(default = "default_issue_type")]
    pub issue_type: String,
    pub assignee: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
    pub design: Option<String>,
    pub acceptance_criteria: Option<String>,
    #[serde(default)]
    pub dependencies: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateIssueRequest {
    pub title: Option<String>,
    pub description: Option<String>,
    pub priority: Option<u8>,
    pub issue_type: Option<String>,
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub assignee: Option<Option<String>>,
    pub labels: Option<Vec<String>>,
    pub design: Option<String>,
    pub acceptance_criteria: Option<String>,
    pub notes: Option<String>,
    pub external_ref: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AddDependencyRequest {
    pub target_id: String,
    #[serde(default = "default_dep_type")]
    pub dep_type: String,
}

#[derive(Debug, Deserialize)]
pub struct CloseIssueRequest {
    pub reason: Option<String>,
}

// Response types
#[derive(Debug, Serialize)]
pub struct IssueResponse {
    pub id: String,
    pub title: String,
    pub description: String,
    pub status: String,
    pub priority: u8,
    pub issue_type: String,
    pub assignee: Option<String>,
    pub labels: Vec<String>,
    pub design: Option<String>,
    pub acceptance_criteria: Option<String>,
    pub notes: Option<String>,
    pub external_ref: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
    #[serde(rename = "_links")]
    pub links: Links,
}

#[derive(Debug, Serialize)]
pub struct Links {
    #[serde(rename = "self")]
    pub self_link: String,
    pub dependencies: String,
}

#[derive(Debug, Serialize)]
pub struct ListIssuesResponse {
    pub issues: Vec<IssueResponse>,
    pub total: usize,
    pub limit: usize,
    pub offset: usize,
}

#[derive(Debug, Serialize)]
pub struct ApiError {
    pub error: ApiErrorBody,
}

#[derive(Debug, Serialize)]
pub struct ApiErrorBody {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}
```

## axum Route Setup

```rust
use axum::{
    routing::{get, post, patch, delete},
    Router,
};

pub fn create_router(state: AppState) -> Router {
    Router::new()
        // Issues CRUD
        .route("/api/v1/issues", get(list_issues).post(create_issue))
        .route("/api/v1/issues/:id",
            get(get_issue).patch(update_issue).delete(delete_issue))

        // Issue actions
        .route("/api/v1/issues/:id/close", post(close_issue))
        .route("/api/v1/issues/:id/reopen", post(reopen_issue))

        // Dependencies
        .route("/api/v1/issues/:id/dependencies",
            get(list_dependencies).post(add_dependency))
        .route("/api/v1/issues/:id/dependencies/:target",
            delete(remove_dependency))

        // Labels
        .route("/api/v1/issues/:id/labels", get(list_issue_labels))
        .route("/api/v1/issues/:id/labels/:label",
            post(add_label).delete(remove_label))
        .route("/api/v1/labels", get(list_all_labels))

        // Queries
        .route("/api/v1/ready", get(ready_to_work))
        .route("/api/v1/blocked", get(blocked_issues))
        .route("/api/v1/stale", get(stale_issues))
        .route("/api/v1/stats", get(stats))

        // Management
        .route("/api/v1/health", get(health))
        .route("/api/v1/shutdown", post(shutdown))

        // WebSocket
        .route("/api/v1/ws/events", get(ws_events))

        .with_state(state)
}
```
